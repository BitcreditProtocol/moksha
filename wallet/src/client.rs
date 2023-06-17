use std::collections::HashMap;

use async_trait::async_trait;
use cashurs_core::model::{
    BlindedMessage, CheckFeesRequest, CheckFeesResponse, Keysets, PaymentRequest, PostMeltRequest,
    PostMeltResponse, PostMintRequest, PostMintResponse, PostSplitRequest, PostSplitResponse,
    Proofs,
};

use reqwest::{
    header::{HeaderValue, CONTENT_TYPE},
    Response, StatusCode, Url,
};
use secp256k1::PublicKey;

use crate::error::CashuWalletError;
use dyn_clone::DynClone;

#[async_trait]
pub trait Client: Send + Sync + DynClone {
    async fn post_split_tokens(
        &self,
        mint_url: &Url,
        amount: u64,
        proofs: Proofs,
        output: Vec<BlindedMessage>,
    ) -> Result<PostSplitResponse, CashuWalletError>;

    async fn post_mint_payment_request(
        &self,
        mint_url: &Url,
        hash: String,
        blinded_messages: Vec<BlindedMessage>,
    ) -> Result<PostMintResponse, CashuWalletError>;

    async fn post_melt_tokens(
        &self,
        mint_url: &Url,
        proofs: Proofs,
        pr: String,
        outputs: Vec<BlindedMessage>,
    ) -> Result<PostMeltResponse, CashuWalletError>;

    async fn post_checkfees(
        &self,
        mint_url: &Url,
        pr: String,
    ) -> Result<CheckFeesResponse, CashuWalletError>;

    async fn get_mint_keys(
        &self,
        mint_url: &Url,
    ) -> Result<HashMap<u64, PublicKey>, CashuWalletError>;

    async fn get_mint_keysets(&self, mint_url: &Url) -> Result<Keysets, CashuWalletError>;

    async fn get_mint_payment_request(
        &self,
        mint_url: &Url,
        amount: u64,
    ) -> Result<PaymentRequest, CashuWalletError>;
}

#[derive(Debug, Clone)]
pub struct HttpClient {
    request_client: reqwest::Client,
}

#[derive(serde::Deserialize, Debug)]
struct CashuErrorResponse {
    code: u64,
    error: String,
}

impl HttpClient {
    pub fn new() -> Self {
        Self {
            request_client: reqwest::Client::new(),
        }
    }
}
impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Client for HttpClient {
    async fn post_split_tokens(
        &self,
        mint_url: &Url,
        amount: u64,
        proofs: Proofs,
        outputs: Vec<BlindedMessage>,
    ) -> Result<PostSplitResponse, CashuWalletError> {
        let body = serde_json::to_string(&PostSplitRequest {
            amount,
            proofs,
            outputs,
        })?;

        let resp = self
            .request_client
            .post(mint_url.join("split")?)
            .header(CONTENT_TYPE, HeaderValue::from_str("application/json")?)
            .body(body)
            .send()
            .await?;

        extract_response_data::<PostSplitResponse>(resp).await
    }

    async fn post_melt_tokens(
        &self,
        mint_url: &Url,
        proofs: Proofs,
        pr: String,
        outputs: Vec<BlindedMessage>,
    ) -> Result<PostMeltResponse, CashuWalletError> {
        let body = serde_json::to_string(&PostMeltRequest {
            pr,
            proofs,
            outputs,
        })?;

        let resp = self
            .request_client
            .post(mint_url.join("melt")?)
            .header(CONTENT_TYPE, HeaderValue::from_str("application/json")?)
            .body(body)
            .send()
            .await?;
        extract_response_data::<PostMeltResponse>(resp).await
    }

    async fn post_checkfees(
        &self,
        mint_url: &Url,
        pr: String,
    ) -> Result<CheckFeesResponse, CashuWalletError> {
        let body = serde_json::to_string(&CheckFeesRequest { pr })?;

        let resp = self
            .request_client
            .post(mint_url.join("checkfees")?)
            .header(CONTENT_TYPE, HeaderValue::from_str("application/json")?)
            .body(body)
            .send()
            .await?;

        extract_response_data::<CheckFeesResponse>(resp).await
    }

    async fn get_mint_keys(
        &self,
        mint_url: &Url,
    ) -> Result<HashMap<u64, PublicKey>, CashuWalletError> {
        let resp = self
            .request_client
            .get(mint_url.join("keys")?)
            .send()
            .await?;
        extract_response_data::<HashMap<u64, PublicKey>>(resp).await
    }

    async fn get_mint_keysets(&self, mint_url: &Url) -> Result<Keysets, CashuWalletError> {
        let resp = self
            .request_client
            .get(mint_url.join("keysets")?)
            .send()
            .await?;
        extract_response_data::<Keysets>(resp).await
    }

    async fn get_mint_payment_request(
        &self,
        mint_url: &Url,
        amount: u64,
    ) -> Result<PaymentRequest, CashuWalletError> {
        let url = mint_url.join(&format!("mint?amount={}", amount))?;
        let resp = self.request_client.get(url).send().await?;
        extract_response_data::<PaymentRequest>(resp).await
    }

    async fn post_mint_payment_request(
        &self,
        mint_url: &Url,
        hash: String,
        blinded_messages: Vec<BlindedMessage>,
    ) -> Result<PostMintResponse, CashuWalletError> {
        let url = mint_url.join(&format!("mint?hash={}", hash))?;
        let body = serde_json::to_string(&PostMintRequest {
            outputs: blinded_messages,
        })?;

        let resp = self
            .request_client
            .post(url)
            .header(CONTENT_TYPE, HeaderValue::from_str("application/json")?)
            .body(body)
            .send()
            .await?;
        extract_response_data::<PostMintResponse>(resp).await
    }
}

async fn extract_response_data<T: serde::de::DeserializeOwned>(
    response: Response,
) -> Result<T, CashuWalletError> {
    match response.status() {
        StatusCode::OK => {
            let response_text = response.text().await?;
            //println!("{}", &response_text);
            match serde_json::from_str::<T>(&response_text) {
                Ok(data) => Ok(data),
                Err(..) => Err(CashuWalletError::UnexpectedResponse(response_text)),
            }
        }
        _ => match &response.headers().get(CONTENT_TYPE) {
            Some(content_type) => {
                if *content_type == "application/json" {
                    let txt = response.text().await?;
                    let data = serde_json::from_str::<CashuErrorResponse>(&txt)
                        .map_err(|_| CashuWalletError::UnexpectedResponse(txt))
                        .unwrap();

                    // FIXME: use the error code to return a proper error
                    match data.error.as_str() {
                        "Lightning invoice not paid yet." => {
                            Err(CashuWalletError::InvoiceNotPaidYet(data.code, data.error))
                        }
                        _ => Err(CashuWalletError::MintError(data.error)),
                    }
                } else {
                    Err(CashuWalletError::UnexpectedResponse(response.text().await?))
                }
            }
            None => Err(CashuWalletError::UnexpectedResponse(response.text().await?)),
        },
    }
}
