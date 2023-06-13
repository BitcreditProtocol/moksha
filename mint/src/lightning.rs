use async_trait::async_trait;

use crate::{
    error::CashuMintError,
    lnbits::{CreateInvoiceParams, CreateInvoiceResult, LNBitsClient, PayInvoiceResult},
};

use lightning_invoice::Invoice as LNInvoice;

#[cfg(test)]
use mockall::automock;
use std::str::FromStr;

#[derive(Clone)]
pub struct LnbitsLightning {
    pub client: LNBitsClient,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait Lightning: Send + Sync {
    async fn is_invoice_paid(&self, invoice: String) -> Result<bool, CashuMintError>;
    async fn create_invoice(&self, amount: u64) -> Result<CreateInvoiceResult, CashuMintError>;
    async fn pay_invoice(
        &self,
        payment_request: String,
    ) -> Result<PayInvoiceResult, CashuMintError>;

    async fn decode_invoice(&self, payment_request: String) -> Result<LNInvoice, CashuMintError> {
        LNInvoice::from_str(&payment_request)
            .map_err(|err| CashuMintError::DecodeInvoice(payment_request, err))
    }
}

impl LnbitsLightning {
    pub fn new(admin_key: String, url: String) -> Self {
        Self {
            client: LNBitsClient::new(&admin_key, &url, None)
                .expect("Can not create Lnbits client"),
        }
    }
}

#[async_trait]
impl Lightning for LnbitsLightning {
    async fn is_invoice_paid(&self, invoice: String) -> Result<bool, CashuMintError> {
        let decoded_invoice = self.decode_invoice(invoice).await?;
        Ok(self
            .client
            .is_invoice_paid(&decoded_invoice.payment_hash().to_string())
            .await?)
    }

    async fn create_invoice(&self, amount: u64) -> Result<CreateInvoiceResult, CashuMintError> {
        Ok(self
            .client
            .create_invoice(&CreateInvoiceParams {
                amount,
                unit: "sat".to_string(),
                memo: None,
                expiry: Some(10000),
                webhook: None,
                internal: None,
            })
            .await?)
    }

    async fn pay_invoice(
        &self,
        payment_request: String,
    ) -> Result<PayInvoiceResult, CashuMintError> {
        self.client
            .pay_invoice(&payment_request)
            .await
            .map_err(|err| CashuMintError::PayInvoice(payment_request, err))
    }
}
