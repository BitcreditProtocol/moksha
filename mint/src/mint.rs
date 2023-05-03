use std::sync::Arc;

use cashurs_core::{
    crypto,
    dhke::Dhke,
    model::{split_amount, BlindedMessage, BlindedSignature, MintKeyset, Proofs},
};

use crate::{database::Database, error::CashuMintError, lightning::Lightning, model::Invoice};

#[derive(Clone)]
pub struct Mint {
    pub lightning: Arc<dyn Lightning + Send + Sync>,
    pub keyset: MintKeyset,
    pub db: Arc<dyn Database + Send + Sync>,
    pub dhke: Dhke,
}

impl Mint {
    pub fn new(
        secret: String,
        derivation_path: String,
        lightning: Arc<dyn Lightning + Send + Sync>,
        db: Arc<dyn Database + Send + Sync>,
    ) -> Self {
        Self {
            lightning,
            keyset: MintKeyset::new(secret, derivation_path),
            db,
            dhke: Dhke::new(),
        }
    }

    pub async fn create_blinded_signatures(
        &self,
        blinded_messages: Vec<BlindedMessage>,
    ) -> Result<Vec<BlindedSignature>, CashuMintError> {
        let promises = blinded_messages
            .iter()
            .map(|blinded_msg| {
                let private_key = self.keyset.private_keys.get(&blinded_msg.amount).unwrap(); // FIXME unwrap
                let blinded_sig = self.dhke.step2_bob(blinded_msg.b_, private_key).unwrap(); // FIXME unwrap
                BlindedSignature {
                    id: Some(self.keyset.keyset_id.clone()),
                    amount: blinded_msg.amount,
                    c_: blinded_sig,
                }
            })
            .collect::<Vec<BlindedSignature>>();
        Ok(promises)
    }

    pub async fn create_invoice(&self, amount: u64) -> Result<(String, String), CashuMintError> {
        let pr = self.lightning.create_invoice(amount).await.payment_request;
        let key = crypto::generate_hash();
        self.db
            .add_pending_invoice(key.clone(), Invoice::new(amount, pr.clone()))?;
        Ok((pr, key))
    }

    pub async fn mint_tokens(
        &self,
        invoice_hash: String,
        outputs: Vec<BlindedMessage>,
    ) -> Result<Vec<BlindedSignature>, CashuMintError> {
        let invoice = self.db.get_pending_invoice(invoice_hash.clone())?;

        let is_paid = self
            .lightning
            .is_invoice_paid(invoice.payment_request.clone())
            .await?;

        if !is_paid {
            return Ok(vec![]);
        }

        self.db.remove_pending_invoice(invoice_hash)?;
        self.create_blinded_signatures(outputs).await
    }

    pub async fn split(
        &self,
        amount: u64,
        proofs: Proofs,
        blinded_messages: Vec<BlindedMessage>,
    ) -> Result<(Vec<BlindedSignature>, Vec<BlindedSignature>), CashuMintError> {
        self.check_used_proofs(&proofs)?;

        let sum_proofs = proofs.get_total_amount();
        let sum_first = split_amount(sum_proofs - amount).len();

        // TODO check: "split amount is higher than the total sum."
        // TODO check: "duplicate promises."
        // TODO check: "split of promises is not as expected."

        let first_slice = blinded_messages[0..sum_first].to_vec();
        let first_sigs = self.create_blinded_signatures(first_slice).await?;
        let second_slice = blinded_messages[sum_first..blinded_messages.len()].to_vec();
        let second_sigs = self.create_blinded_signatures(second_slice).await?;

        let amount_first = self.get_amount(&first_sigs);
        let amount_second = self.get_amount(&second_sigs);

        if sum_proofs != (amount_first + amount_second) {
            return Err(CashuMintError::SplitAmountMismatch(format!(
                "Split amount mismatch: {} != {} + {}",
                sum_proofs, amount_first, amount_second
            )));
        }

        self.db.add_used_proofs(proofs)?;

        Ok((second_sigs, first_sigs))
    }

    fn get_amount(&self, blinded_sigs: &[BlindedSignature]) -> u64 {
        blinded_sigs
            .iter()
            .map(|blinded_sig| blinded_sig.amount)
            .sum()
    }

    pub async fn melt(
        &self,
        payment_request: String,
        proofs: Proofs,
    ) -> Result<(bool, String, Vec<BlindedSignature>), CashuMintError> {
        let invoice = self
            .lightning
            .decode_invoice(payment_request.clone())
            .await?;

        let proofs_amount = proofs.get_total_amount();

        // TODO verify proofs

        self.check_used_proofs(&proofs)?;

        // TODO check for fees
        let amount_msat = invoice
            .amount_milli_satoshis()
            .expect("Invoice amount is missing");

        if amount_msat < (proofs_amount / 1000) {
            return Err(CashuMintError::InvoiceAmountTooLow(format!(
                "Invoice amount is too low: {}",
                amount_msat
            )));
        }

        self.db.add_used_proofs(proofs)?;
        // TODO check invoice

        let result = self.lightning.pay_invoice(payment_request).await?;

        Ok((true, result.payment_hash, vec![]))
    }

    pub fn check_used_proofs(&self, proofs: &Proofs) -> Result<(), CashuMintError> {
        let used_proofs = self.db.get_used_proofs()?.get_proofs();
        for used_proof in used_proofs {
            if proofs.get_proofs().contains(&used_proof) {
                return Err(CashuMintError::ProofAlreadyUsed(format!("{used_proof:?}")));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{database::MockDatabase, error::CashuMintError, lightning::Lightning, Mint};
    use async_trait::async_trait;
    use cashurs_core::dhke;
    use cashurs_core::model::{BlindedMessage, TotalAmount};
    use cashurs_core::model::{PostSplitRequest, Proofs};
    use lnbits_rust::api::invoice::{CreateInvoiceResult, PayInvoiceResult};

    pub struct LightningMock {}

    #[async_trait]
    impl Lightning for LightningMock {
        async fn is_invoice_paid(&self, _invoice: String) -> Result<bool, CashuMintError> {
            Ok(true)
        }

        async fn create_invoice(&self, _amount: u64) -> CreateInvoiceResult {
            CreateInvoiceResult {
                payment_hash: "test".to_string(),
                payment_request: "test".to_string(),
            }
        }

        async fn pay_invoice(
            &self,
            _payment_request: String,
        ) -> Result<PayInvoiceResult, CashuMintError> {
            Ok(PayInvoiceResult {
                payment_hash: "test".to_string(),
            })
        }
    }

    fn create_mint_from_mocks() -> Mint {
        let mock_db = MockDatabase::new();
        let db = Arc::new(mock_db);
        let lightning = Arc::new(LightningMock {});
        Mint::new(
            "TEST_PRIVATE_KEY".to_string(),
            "0/0/0/0".to_string(),
            lightning,
            db,
        )
    }

    #[tokio::test]
    async fn test_create_blindsignatures() -> anyhow::Result<()> {
        let mint = create_mint_from_mocks();

        let blinded_messages = vec![BlindedMessage {
            amount: 8,
            b_: dhke::public_key_from_hex(
                "02634a2c2b34bec9e8a4aba4361f6bf202d7fa2365379b0840afe249a7a9d71239",
            ),
        }];

        let result = mint.create_blinded_signatures(blinded_messages).await?;

        assert_eq!(1, result.len());
        assert_eq!(8, result[0].amount);
        assert_eq!(
            dhke::public_key_from_hex(
                "037074c4f53e326ee14ed67125f387d160e0e729351471b69ad41f7d5d21071e15"
            ),
            result[0].c_
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_split_zero() -> anyhow::Result<()> {
        let blinded_messages = vec![];

        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_used_proofs()
            .returning(|| Ok(Proofs::empty()));
        mock_db.expect_add_used_proofs().returning(|_| Ok(()));

        let db = Arc::new(mock_db);

        let lightning = Arc::new(LightningMock {});
        let mint = Mint::new(
            "superprivatesecretkey".to_string(),
            "".to_string(),
            lightning,
            db,
        );

        let proofs = Proofs::empty();
        let (first, second) = mint.split(0, proofs, blinded_messages).await?;

        assert_eq!(first.len(), 0);
        assert_eq!(second.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_split_64_in_20() -> anyhow::Result<()> {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_used_proofs()
            .returning(|| Ok(Proofs::empty()));
        mock_db.expect_add_used_proofs().returning(|_| Ok(()));

        let db = Arc::new(mock_db);

        let base_dir = std::env::var("CARGO_MANIFEST_DIR")?;
        let raw_token = std::fs::read_to_string(format!(
            "{base_dir}/src/fixtures/post_split_request_64_20.json"
        ))?;

        let request = serde_json::from_str::<PostSplitRequest>(&raw_token)?;

        let lightning = Arc::new(LightningMock {});
        let mint = Mint::new(
            "superprivatesecretkey".to_string(),
            "".to_string(),
            lightning,
            db,
        );

        let (first, second) = mint.split(20, request.proofs, request.outputs).await?;

        first.total_amount();
        assert_eq!(first.total_amount(), 20);
        assert_eq!(second.total_amount(), 44);
        Ok(())
    }
}
