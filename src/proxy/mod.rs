//! A proxy provider implementation that forwards all requests to another server using the Bindle
//! client. This requires the `client` feature to be enabled

use std::convert::TryInto;

use reqwest::StatusCode;
use tokio_stream::{Stream, StreamExt};

use crate::provider::{Provider, ProviderError, Result};
use crate::Id;
use crate::{
    client::{Client, ClientError},
    signature::{KeyRing, SignatureRole},
    SecretKeyEntry, VerificationStrategy,
};

#[derive(Clone)]
pub struct Proxy {
    client: Client,
}

impl Proxy {
    pub fn new(client: Client) -> Self {
        Proxy { client }
    }
}

#[async_trait::async_trait]
impl Provider for Proxy {
    async fn create_invoice(
        &self,
        inv: &mut crate::Invoice,
        _role: SignatureRole,
        secret_key: &SecretKeyEntry,
        _strategy: VerificationStrategy,
    ) -> Result<Vec<crate::Label>> {
        // TODO: When we add proxy support as part of #141, we need to also add
        // proxy verification here. We'll need to get the keyring and then do
        // the verification using the official keyring. But we might need to
        // add logic to ensure that the upstream proxy is remote, because if it is
        // local we don't need to verify here.
        // let keyring = KeyRing::default();
        // strategy.verify(inv, &keyring)?;

        let mut inv2 = inv.to_owned();
        self.sign_invoice(&mut inv2, SignatureRole::Proxy, secret_key)?;

        let res = self.client.create_invoice(inv.to_owned()).await?;
        Ok(res.missing.unwrap_or_default())
    }

    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        self.client
            .get_yanked_invoice(parsed_id)
            .await
            .map_err(|e| e.into())
    }

    async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        self.client
            .yank_invoice(parsed_id)
            .await
            .map_err(|e| e.into())
    }

    async fn create_parcel<I, R, B>(&self, bindle_id: I, parcel_id: &str, data: R) -> Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        self.client
            .create_parcel_from_stream(parsed_id, parcel_id, data)
            .await
            .map_err(|e| e.into())
    }

    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> Result<Box<dyn Stream<Item = Result<bytes::Bytes>> + Unpin + Send + Sync>>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        let stream = self.client.get_parcel_stream(parsed_id, parcel_id).await?;
        Ok(Box::new(stream.map(|res| res.map_err(|e| e.into()))))
    }

    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        let resp = self
            .client
            .raw(
                reqwest::Method::HEAD,
                &format!(
                    "{}/{}@{}",
                    crate::client::INVOICE_ENDPOINT,
                    parsed_id,
                    parcel_id,
                ),
                None::<reqwest::Body>,
            )
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        match resp.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(ProviderError::ProxyError(ClientError::InvalidRequest {
                status_code: resp.status(),
                message: None,
            })),
        }
    }
}
