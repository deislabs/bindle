//! Client implementation for consuming a Bindle API. Although written in Rust, it is not specific
//! to the Rust implementation. It is meant to consume any spec-compliant bindle implementation.

mod error;
pub mod load;
pub mod tokens;

use std::convert::TryInto;
use std::path::Path;

use reqwest::header::{self, HeaderMap};
use reqwest::Client as HttpClient;
use reqwest::{Body, RequestBuilder, StatusCode};
use tokio_stream::{Stream, StreamExt};
use tracing::{debug, info, instrument, trace};
use url::Url;

use crate::provider::{Provider, ProviderError};
use crate::verification::Verified;
use crate::{Id, Signed};

pub use error::ClientError;

/// A shorthand `Result` type that always uses `ClientError` as its error variant
pub type Result<T> = std::result::Result<T, ClientError>;

pub const INVOICE_ENDPOINT: &str = "_i";
pub const QUERY_ENDPOINT: &str = "_q";
pub const RELATIONSHIP_ENDPOINT: &str = "_r";
pub const LOGIN_ENDPOINT: &str = "login";
const TOML_MIME_TYPE: &str = "application/toml";

/// A client type for interacting with a Bindle server
#[derive(Clone)]
pub struct Client<T> {
    client: HttpClient,
    base_url: Url,
    token_manager: T,
}

/// The operation being performed against a Bindle server.
enum Operation {
    Create,
    Yank,
    Get,
    Query,
    Login,
}

/// A builder for for setting up a `Client`. Created using `Client::builder`
#[derive(Default)]
pub struct ClientBuilder {
    http2_prior_knowledge: bool,
    danger_accept_invalid_certs: bool,
}

impl ClientBuilder {
    /// Controls whether the client assumes HTTP/2 or attempts to negotiate it. Defaults to false.
    pub fn http2_prior_knowledge(mut self, http2_prior_knowledge: bool) -> Self {
        self.http2_prior_knowledge = http2_prior_knowledge;
        self
    }

    /// Controls whether the client accepts invalid certificates. The default is to reject invalid
    /// certificates. It is sometimes necessary to set this option in dev-test situations where you
    /// may be working with self-signed certificates or the like. Defaults to false.
    pub fn danger_accept_invalid_certs(mut self, danger_accept_invalid_certs: bool) -> Self {
        self.danger_accept_invalid_certs = danger_accept_invalid_certs;
        self
    }

    /// Returns a new Client with the given URL and token manager, configured using the set options.
    ///
    /// This URL should be the FQDN plus any namespacing (like `v1`). So if you were running a
    /// bindle server mounted at the v1 endpoint, your URL would look something like
    /// `http://my.bindle.com/v1/`. Will return an error if the URL is not valid
    pub fn build<T>(self, base_url: &str, token_manager: T) -> Result<Client<T>> {
        let (base_parsed, headers) = base_url_and_headers(base_url)?;
        let client = HttpClient::builder()
            .and_if(self.http2_prior_knowledge, |b| b.http2_prior_knowledge())
            .and_if(self.danger_accept_invalid_certs, |b| {
                b.danger_accept_invalid_certs(true)
            })
            .default_headers(headers)
            .build()
            .map_err(|e| ClientError::Other(e.to_string()))?;

        Ok(Client {
            client,
            base_url: base_parsed,
            token_manager,
        })
    }
}

pub(crate) fn base_url_and_headers(base_url: &str) -> Result<(Url, HeaderMap)> {
    // Note that the trailing slash is important, otherwise the URL parser will treat is as a
    // "file" component of the URL. So we need to check that it is added before parsing
    let mut base = base_url.to_owned();
    if !base.ends_with('/') {
        info!("Provided base URL missing trailing slash, adding...");
        base.push('/');
    }
    let base_parsed = Url::parse(&base)?;
    let mut headers = header::HeaderMap::new();
    headers.insert(header::ACCEPT, TOML_MIME_TYPE.parse().unwrap());
    Ok((base_parsed, headers))
}

impl<T: tokens::TokenManager> Client<T> {
    /// Returns a new Client with the given URL, configured using the default options.
    ///
    /// This URL should be the FQDN plus any namespacing (like `v1`). So if you were running a
    /// bindle server mounted at the v1 endpoint, your URL would look something like
    /// `http://my.bindle.com/v1/`. Will return an error if the URL is not valid
    pub fn new(base_url: &str, token_manager: T) -> Result<Self> {
        ClientBuilder::default().build(base_url, token_manager)
    }

    /// Returns a [`ClientBuilder`](ClientBuilder) configured with defaults
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Performs a raw request using the underlying HTTP client and returns the raw response. The
    /// path is just the path part of your URL. It will be joined with the configured base URL for
    /// the client.
    // TODO: Right now this is mainly used if you want to HEAD any of the endpoints, as a HEAD
    // requests is used to get the headers and status code, which is pretty much a raw response
    // anyway. But should we make those their own methods instead?
    #[instrument(level = "trace", skip(self, body))]
    pub async fn raw(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<impl Into<reqwest::Body>>,
    ) -> anyhow::Result<reqwest::Response> {
        let req = self.client.request(method, self.base_url.join(path)?);
        let req = self.token_manager.apply_auth_header(req).await?;
        let req = match body {
            Some(b) => req.body(b),
            None => req,
        };
        req.send().await.map_err(|e| e.into())
    }

    //////////////// Create Invoice ////////////////

    /// Creates the given invoice, returns a response containing the created invoice and a list of
    /// missing parcels (that have not yet been uploaded)
    #[instrument(level = "trace", skip(self, inv), fields(id = %inv.bindle.id))]
    pub async fn create_invoice(
        &self,
        inv: crate::Invoice,
    ) -> Result<crate::InvoiceCreateResponse> {
        let req = self
            .create_invoice_builder()
            .await?
            .body(toml::to_vec(&inv)?);
        self.create_invoice_request(req).await
    }

    /// Same as [`create_invoice`](Client::create_invoice), but takes a path to an invoice file
    /// instead. This will load the invoice file directly into the request, skipping serialization
    #[instrument(level = "trace", skip(self, file_path), fields(path = %file_path.as_ref().display()))]
    pub async fn create_invoice_from_file<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Result<crate::InvoiceCreateResponse> {
        // Create an owned version of the path to avoid worrying about lifetimes here for the stream
        let path = file_path.as_ref().to_owned();
        debug!("Loading invoice from file");
        let inv_stream = load::raw(path).await?;
        debug!("Successfully loaded invoice stream");
        let req = self
            .create_invoice_builder()
            .await?
            .body(Body::wrap_stream(inv_stream));
        self.create_invoice_request(req).await
    }

    async fn create_invoice_builder(&self) -> Result<RequestBuilder> {
        // We can unwrap here because any URL error would be programmers fault
        let req = self
            .client
            .post(self.base_url.join(INVOICE_ENDPOINT).unwrap())
            .header(header::CONTENT_TYPE, TOML_MIME_TYPE);
        self.token_manager.apply_auth_header(req).await
    }

    async fn create_invoice_request(
        &self,
        req: RequestBuilder,
    ) -> Result<crate::InvoiceCreateResponse> {
        trace!(?req);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice, Operation::Create).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Get Invoice ////////////////

    /// Returns the requested invoice from the bindle server if it exists. This can take any form
    /// that can convert into the `Id` type, but generally speaking, this is the canonical name of
    /// the bindle (e.g. `example.com/foo/1.0.0`). If you want to fetch a yanked invoice, use the
    /// [`get_yanked_invoice`](Client::get_yanked_invoice) function
    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    pub async fn get_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        self.get_invoice_request(
            self.base_url
                .join(&format!("{}/{}", INVOICE_ENDPOINT, parsed_id))?,
        )
        .await
    }

    /// Same as `get_invoice` but allows you to fetch a yanked invoice
    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    pub async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let mut url = self
            .base_url
            .join(&format!("{}/{}", INVOICE_ENDPOINT, parsed_id))?;
        url.set_query(Some("yanked=true"));
        self.get_invoice_request(url).await
    }

    async fn get_invoice_request(&self, url: Url) -> Result<crate::Invoice> {
        let req = self.client.get(url);
        let req = self.token_manager.apply_auth_header(req).await?;
        trace!(?req);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice, Operation::Get).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Query Invoice ////////////////

    /// Queries the bindle server for matching invoices as specified by the given query options
    #[instrument(level = "trace", skip(self))]
    pub async fn query_invoices(
        &self,
        query_opts: crate::QueryOptions,
    ) -> Result<crate::search::Matches> {
        let req = self
            .client
            .get(self.base_url.join(QUERY_ENDPOINT).unwrap())
            .query(&query_opts);
        let req = self.token_manager.apply_auth_header(req).await?;
        trace!(?req);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Query, Operation::Query).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Yank Invoice ////////////////

    /// Yanks the invoice from availability on the bindle server. This can take any form that can
    /// convert into the `Id` type, but generally speaking, this is the canonical name of the bindle
    /// (e.g. `example.com/foo/1.0.0`)
    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    pub async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let req = self.client.delete(
            self.base_url
                .join(&format!("{}/{}", INVOICE_ENDPOINT, parsed_id))?,
        );
        let req = self.token_manager.apply_auth_header(req).await?;
        trace!(?req);
        let resp = req.send().await?;
        unwrap_status(resp, Endpoint::Invoice, Operation::Yank).await?;
        Ok(())
    }

    //////////////// Create Parcel ////////////////

    /// Creates the given parcel using the SHA and the raw parcel data to upload to the server.
    ///
    /// Parcels are only accessible through a bindle, so the Bindle ID is required as well
    #[instrument(level = "trace", skip(self, bindle_id, data), fields(invoice_id, data_len = data.len()))]
    pub async fn create_parcel<I>(
        &self,
        bindle_id: I,
        parcel_sha: &str,
        data: Vec<u8>,
    ) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .await?
                .body(data),
        )
        .await
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes a path to the parcel
    /// file. This will be more efficient for large files as it will stream the data into the body
    /// rather than taking the intermediate step of loading the bytes into a `Vec`.
    #[instrument(level = "trace", skip(self, bindle_id, data_path), fields(invoice_id, path = %data_path.as_ref().display()))]
    pub async fn create_parcel_from_file<D, I>(
        &self,
        bindle_id: I,
        parcel_sha: &str,
        data_path: D,
    ) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
        D: AsRef<Path>,
    {
        // Copy the path to avoid lifetime issues
        let data = data_path.as_ref().to_owned();
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        debug!("Loading parcel data from file");
        let stream = load::raw(data).await?;
        debug!("Successfully loaded parcel stream");
        let data_body = Body::wrap_stream(stream);

        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .await?
                .body(data_body),
        )
        .await
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes a stream of parcel data as bytes
    #[instrument(level = "trace", skip(self, bindle_id, stream), fields(invoice_id))]
    pub async fn create_parcel_from_stream<I, S, B>(
        &self,
        bindle_id: I,
        parcel_sha: &str,
        stream: S,
    ) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
        S: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let map = stream.map(|res| res.map(|mut b| b.copy_to_bytes(b.remaining())));
        let data_body = Body::wrap_stream(map);
        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .await?
                .body(data_body),
        )
        .await
    }

    async fn create_parcel_builder(
        &self,
        bindle_id: &Id,
        parcel_sha: &str,
    ) -> Result<RequestBuilder> {
        // We can unwrap here because any URL error would be programmers fault
        let req = self.client.post(
            self.base_url
                .join(&format!(
                    "{}/{}@{}",
                    INVOICE_ENDPOINT, bindle_id, parcel_sha
                ))
                .unwrap(),
        );
        self.token_manager.apply_auth_header(req).await
    }

    async fn create_parcel_request(&self, req: RequestBuilder) -> Result<()> {
        // We can unwrap here because any URL error would be programmers fault
        trace!(?req);
        let resp = req.send().await?;
        unwrap_status(resp, Endpoint::Parcel, Operation::Create).await?;
        Ok(())
    }

    //////////////// Get Parcel ////////////////

    /// Returns the requested parcel (identified by its Bindle ID and SHA) as a vector of bytes
    #[instrument(level = "trace", skip(self, bindle_id), fields(invoice_id))]
    pub async fn get_parcel<I>(&self, bindle_id: I, sha: &str) -> Result<Vec<u8>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let resp = self.get_parcel_request(&parsed_id, sha).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Returns the requested parcel (identified by its Bindle ID and SHA) as a stream of bytes.
    /// This is useful for when you don't want to read it into memory but are instead writing to a
    /// file or other location
    #[instrument(level = "trace", skip(self, bindle_id), fields(invoice_id))]
    pub async fn get_parcel_stream<I>(
        &self,
        bindle_id: I,
        sha: &str,
    ) -> Result<impl Stream<Item = Result<bytes::Bytes>>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let resp = self.get_parcel_request(&parsed_id, sha).await?;
        Ok(resp.bytes_stream().map(|r| r.map_err(|e| e.into())))
    }

    async fn get_parcel_request(&self, bindle_id: &Id, sha: &str) -> Result<reqwest::Response> {
        // Override the default accept header
        let req = self
            .client
            .get(
                self.base_url
                    .join(&format!("{}/{}@{}", INVOICE_ENDPOINT, bindle_id, sha))
                    .unwrap(),
            )
            .header(header::ACCEPT, "*/*");
        let req = self.token_manager.apply_auth_header(req).await?;
        trace!(?req);
        let resp = req.send().await?;
        unwrap_status(resp, Endpoint::Parcel, Operation::Get).await
    }

    //////////////// Relationship Endpoints ////////////////

    /// Gets the labels of missing parcels, if any, of the specified bindle. If the bindle is
    /// yanked, this will fail
    #[instrument(level = "trace", skip(self, id), fields(invoice_id))]
    pub async fn get_missing_parcels<I>(&self, id: I) -> Result<Vec<crate::Label>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        tracing::span::Span::current().record("invoice_id", &tracing::field::display(&parsed_id));
        let req = self.client.get(self.base_url.join(&format!(
            "{}/{}/{}",
            RELATIONSHIP_ENDPOINT, "missing", parsed_id
        ))?);
        let req = self.token_manager.apply_auth_header(req).await?;
        trace!(?req);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice, Operation::Get).await?;
        Ok(toml::from_slice::<crate::MissingParcelsResponse>(&resp.bytes().await?)?.missing)
    }
}

// We implement provider for client because often times (such as in the CLI) we are composing the
// client in to a provider cache. This implementation does not verify or sign anything
#[async_trait::async_trait]
impl<T: tokens::TokenManager + Send + Sync + 'static> Provider for Client<T> {
    async fn create_invoice<I>(
        &self,
        invoice: I,
    ) -> crate::provider::Result<(crate::Invoice, Vec<crate::Label>)>
    where
        I: Signed + Verified + Send + Sync,
    {
        let res = self.create_invoice(invoice.signed()).await?;
        Ok((res.invoice, res.missing.unwrap_or_default()))
    }

    async fn get_yanked_invoice<I>(&self, id: I) -> crate::provider::Result<crate::Invoice>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        self.get_yanked_invoice(parsed_id)
            .await
            .map_err(|e| e.into())
    }

    async fn yank_invoice<I>(&self, id: I) -> crate::provider::Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        self.yank_invoice(parsed_id).await.map_err(|e| e.into())
    }

    async fn create_parcel<I, R, B>(
        &self,
        bindle_id: I,
        parcel_id: &str,
        data: R,
    ) -> crate::provider::Result<()>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
        R: Stream<Item = std::io::Result<B>> + Unpin + Send + Sync + 'static,
        B: bytes::Buf,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        self.create_parcel_from_stream(parsed_id, parcel_id, data)
            .await
            .map_err(|e| e.into())
    }

    async fn get_parcel<I>(
        &self,
        bindle_id: I,
        parcel_id: &str,
    ) -> crate::provider::Result<
        Box<dyn Stream<Item = crate::provider::Result<bytes::Bytes>> + Unpin + Send + Sync>,
    >
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        // Parse the ID now because the error type constraint doesn't match that of the client
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        let stream = self.get_parcel_stream(parsed_id, parcel_id).await?;
        Ok(Box::new(stream.map(|res| res.map_err(|e| e.into()))))
    }

    async fn parcel_exists<I>(&self, bindle_id: I, parcel_id: &str) -> crate::provider::Result<bool>
    where
        I: TryInto<Id> + Send,
        I::Error: Into<ProviderError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        let resp = self
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

// A helper function and related enum to make some reusable code for unwrapping a status code and returning the right error

enum Endpoint {
    Invoice,
    Parcel,
    Query,
    // NOTE: This endpoint currently does nothing, but if we need more specific errors, we can use
    // this down the line
    Login,
}

async fn unwrap_status(
    resp: reqwest::Response,
    endpoint: Endpoint,
    operation: Operation,
) -> Result<reqwest::Response> {
    match (resp.status(), endpoint) {
        (StatusCode::OK, _) => Ok(resp),
        (StatusCode::ACCEPTED, Endpoint::Invoice) => Ok(resp),
        (StatusCode::CREATED, Endpoint::Invoice) => Ok(resp),
        (StatusCode::NOT_FOUND, Endpoint::Invoice) | (StatusCode::FORBIDDEN, Endpoint::Invoice) => {
            match operation {
                Operation::Get => Err(ClientError::InvoiceNotFound),
                _ => Err(ClientError::ResourceNotFound),
            }
        }
        (StatusCode::NOT_FOUND, Endpoint::Parcel) => match operation {
            Operation::Get => Err(ClientError::ParcelNotFound),
            _ => Err(ClientError::ResourceNotFound),
        },
        (StatusCode::CONFLICT, Endpoint::Invoice) => Err(ClientError::InvoiceAlreadyExists),
        (StatusCode::CONFLICT, Endpoint::Parcel) => Err(ClientError::ParcelAlreadyExists),
        (StatusCode::UNAUTHORIZED, _) => Err(ClientError::Unauthorized),
        (StatusCode::BAD_REQUEST, _) => Err(ClientError::ServerError(Some(
            "The request could not be handled by the server. Verify your Bindle server URL"
                .to_owned(),
        ))),
        // You can't range match on u16 so we use a guard
        (_, _) if resp.status().is_server_error() => {
            Err(ClientError::ServerError(parse_error_from_body(resp).await))
        }
        (_, _) if resp.status().is_client_error() => Err(ClientError::InvalidRequest {
            status_code: resp.status(),
            message: parse_error_from_body(resp).await,
        }),
        _ => Err(ClientError::Other(format!(
            "Unknown error: {}",
            parse_error_from_body(resp).await.unwrap_or_default()
        ))),
    }
}

async fn parse_error_from_body(resp: reqwest::Response) -> Option<String> {
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return None,
    };

    match toml::from_slice::<crate::ErrorResponse>(&bytes) {
        Ok(e) => Some(e.error),
        Err(_) => None,
    }
}

trait ConditionalBuilder {
    fn and_if(self, condition: bool, build_method: impl Fn(Self) -> Self) -> Self
    where
        Self: Sized,
    {
        if condition {
            build_method(self)
        } else {
            self
        }
    }
}

impl ConditionalBuilder for reqwest::ClientBuilder {}
