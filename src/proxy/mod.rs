//! A proxy provider implementation that forwards all requests to another server using the Bindle
//! client. This requires the `client` feature to be enabled

use std::convert::TryInto;

use reqwest::StatusCode;
use tokio_stream::{Stream, StreamExt};

use crate::provider::{Provider, ProviderError, Result};
use crate::verification::Verified;
use crate::{
    client::{tokens::TokenManager, Client, ClientError},
    signature::SignatureRole,
    SecretKeyEntry,
};
use crate::{Id, Signed};

/// A proxy implementation that forwards requests to an upstream server as configured by a
/// [`Client`](crate::client::Client). The proxy implementation will verify and sign invoice create
/// operations and sign any fetched invoices
#[derive(Clone)]
pub struct Proxy<T> {
    client: Client<T>,
    secret_key: SecretKeyEntry,
}

impl<T> Proxy<T> {
    /// Returns a new proxy configured to connect to an upstream using the given client and verify
    /// and sign using the given secret key and keyring
    pub fn new(client: Client<T>, secret_key: SecretKeyEntry) -> Self {
        Proxy { client, secret_key }
    }
}

#[async_trait::async_trait]
impl<T: TokenManager + Send + Sync + 'static> Provider for Proxy<T> {
    /// Creates the invoice on the upstream server, signing the invoice as a proxy. The role and
    /// secret key parameters do not matter here
    async fn create_invoice<I>(&self, invoice: I) -> Result<(crate::Invoice, Vec<crate::Label>)>
    where
        I: Signed + Verified + Send + Sync,
    {
        let res = self.client.create_invoice(invoice.signed()).await?;
        Ok((res.invoice, res.missing.unwrap_or_default()))
    }

    async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let inv = self.client.get_yanked_invoice(parsed_id).await?;
        let signed = crate::sign(inv, vec![(SignatureRole::Proxy, &self.secret_key)])?;
        Ok(signed.signed())
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
