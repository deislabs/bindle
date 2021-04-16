//! Client implementation for consuming a Bindle API. Although written in Rust, it is not specific
//! to the Rust implementation. It is meant to consume any spec-compliant bindle implementation.

mod error;
pub mod load;

use std::convert::TryInto;
use std::path::Path;

use reqwest::header;
use reqwest::Client as HttpClient;
use reqwest::{Body, RequestBuilder, StatusCode};
use tokio_stream::{Stream, StreamExt};
use tracing::log::{debug, info};
use url::Url;

use crate::Id;

pub use error::ClientError;

/// A shorthand `Result` type that always uses `ClientError` as its error variant
pub type Result<T> = std::result::Result<T, ClientError>;

pub const INVOICE_ENDPOINT: &str = "_i";
pub const QUERY_ENDPOINT: &str = "_q";
pub const RELATIONSHIP_ENDPOINT: &str = "_r";
const TOML_MIME_TYPE: &str = "application/toml";

/// A client type for interacting with a Bindle server
#[derive(Clone)]
pub struct Client {
    client: HttpClient,
    base_url: Url,
}

impl Client {
    /// Returns a new Client with the given URL. This URL should be the FQDN plus any namespacing
    /// (like `v1`). So if you were running a bindle server mounted at the v1 endpoint, your URL
    /// would look something like `http://my.bindle.com/v1/`. Will return an error if the URL is not
    /// valid
    pub fn new(base_url: &str) -> Result<Self> {
        // Note that the trailing slash is important, otherwise the URL parser will treat is as a
        // "file" component of the URL. So we need to check that it is added before parsing
        let mut base = base_url.to_owned();
        if !base.ends_with('/') {
            info!("Provided base URL missing trailing slash, adding...");
            base.push('/');
        }
        let base_parsed = Url::parse(&base)?;
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ACCEPT, "application/toml".parse().unwrap());
        // TODO: As this evolves, we might want to allow for setting time outs and accepting
        // self-signed certs
        let client = HttpClient::builder()
            .http2_prior_knowledge()
            .default_headers(headers)
            .build()
            .map_err(|e| ClientError::Other(e.to_string()))?;
        Ok(Client {
            client,
            base_url: base_parsed,
        })
    }

    /// Performs a raw request using the underlying HTTP client and returns the raw response. The
    /// path is just the path part of your URL. It will be joined with the configured base URL for
    /// the client.
    // TODO: Right now this is mainly used if you want to HEAD any of the endpoints, as a HEAD
    // requests is used to get the headers and status code, which is pretty much a raw response
    // anyway. But should we make those their own methods instead?
    pub async fn raw(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<impl Into<reqwest::Body>>,
    ) -> anyhow::Result<reqwest::Response> {
        let req = self.client.request(method, self.base_url.join(path)?);
        let req = match body {
            Some(b) => req.body(b),
            None => req,
        };
        req.send().await.map_err(|e| e.into())
    }

    //////////////// Create Invoice ////////////////

    /// Creates the given invoice, returns a response containing the created invoice and a list of
    /// missing parcels (that have not yet been uploaded)
    pub async fn create_invoice(
        &self,
        inv: crate::Invoice,
    ) -> Result<crate::InvoiceCreateResponse> {
        let req = self.create_invoice_builder().body(toml::to_vec(&inv)?);
        self.create_invoice_request(req).await
    }

    /// Same as [`create_invoice`](Client::create_invoice), but takes a path to an invoice file
    /// instead. This will load the invoice file directly into the request, skipping serialization
    pub async fn create_invoice_from_file<P: AsRef<Path>>(
        &self,
        file_path: P,
    ) -> Result<crate::InvoiceCreateResponse> {
        // Create an owned version of the path to avoid worrying about lifetimes here for the stream
        let path = file_path.as_ref().to_owned();
        debug!("Loading invoice from {}", path.display());
        let inv_stream = load::raw(path).await?;
        debug!("Successfully loaded invoice stream");
        let req = self
            .create_invoice_builder()
            .body(Body::wrap_stream(inv_stream));
        self.create_invoice_request(req).await
    }

    fn create_invoice_builder(&self) -> RequestBuilder {
        // We can unwrap here because any URL error would be programmers fault
        self.client
            .post(self.base_url.join(INVOICE_ENDPOINT).unwrap())
            .header(header::CONTENT_TYPE, TOML_MIME_TYPE)
    }

    async fn create_invoice_request(
        &self,
        req: RequestBuilder,
    ) -> Result<crate::InvoiceCreateResponse> {
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Get Invoice ////////////////

    /// Returns the requested invoice from the bindle server if it exists. This can take any form
    /// that can convert into the `Id` type, but generally speaking, this is the canonical name of
    /// the bindle (e.g. `example.com/foo/1.0.0`). If you want to fetch a yanked invoice, use the
    /// [`get_yanked_invoice`](Client::get_yanked_invoice) function
    pub async fn get_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        self.get_invoice_request(
            self.base_url
                .join(&format!("{}/{}", INVOICE_ENDPOINT, parsed_id))?,
        )
        .await
    }

    /// Same as `get_invoice` but allows you to fetch a yanked invoice
    pub async fn get_yanked_invoice<I>(&self, id: I) -> Result<crate::Invoice>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let mut url = self
            .base_url
            .join(&format!("{}/{}", INVOICE_ENDPOINT, parsed_id))?;
        url.set_query(Some("yanked=true"));
        self.get_invoice_request(url).await
    }

    async fn get_invoice_request(&self, url: Url) -> Result<crate::Invoice> {
        let req = self.client.get(url);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Query Invoice ////////////////

    /// Queries the bindle server for matching invoices as specified by the given query options
    pub async fn query_invoices(
        &self,
        query_opts: crate::QueryOptions,
    ) -> Result<crate::search::Matches> {
        let resp = self
            .client
            .get(self.base_url.join(QUERY_ENDPOINT).unwrap())
            .query(&query_opts)
            .send()
            .await?;
        let resp = unwrap_status(resp, Endpoint::Query).await?;
        Ok(toml::from_slice(&resp.bytes().await?)?)
    }

    //////////////// Yank Invoice ////////////////

    /// Yanks the invoice from availability on the bindle server. This can take any form that can
    /// convert into the `Id` type, but generally speaking, this is the canonical name of the bindle
    /// (e.g. `example.com/foo/1.0.0`)
    pub async fn yank_invoice<I>(&self, id: I) -> Result<()>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let req = self.client.delete(self.base_url.join(&format!(
            "{}/{}",
            INVOICE_ENDPOINT,
            parsed_id.to_string()
        ))?);
        let resp = req.send().await?;
        unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(())
    }

    //////////////// Create Parcel ////////////////

    /// Creates the given parcel using the SHA and the raw parcel data to upload to the server.
    ///
    /// Parcels are only accessible through a bindle, so the Bindle ID is required as well
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
        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .body(data),
        )
        .await
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes a path to the parcel
    /// file. This will be more efficient for large files as it will stream the data into the body
    /// rather than taking the intermediate step of loading the bytes into a `Vec`.
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
        debug!("Loading parcel data from {}", data.display());
        let stream = load::raw(data).await?;
        debug!("Successfully loaded parcel stream");
        let data_body = Body::wrap_stream(stream);

        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .body(data_body),
        )
        .await
    }

    /// Same as [`create_parcel`](Client::create_parcel), but takes a stream of parcel data as bytes
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
        let map = stream.map(|res| res.map(|mut b| b.copy_to_bytes(b.remaining())));
        let data_body = Body::wrap_stream(map);
        self.create_parcel_request(
            self.create_parcel_builder(&parsed_id, parcel_sha)
                .body(data_body),
        )
        .await
    }

    fn create_parcel_builder(&self, bindle_id: &Id, parcel_sha: &str) -> RequestBuilder {
        // We can unwrap here because any URL error would be programmers fault
        self.client.post(
            self.base_url
                .join(&format!(
                    "{}/{}@{}",
                    INVOICE_ENDPOINT, bindle_id, parcel_sha
                ))
                .unwrap(),
        )
    }

    async fn create_parcel_request(&self, req: RequestBuilder) -> Result<()> {
        // We can unwrap here because any URL error would be programmers fault
        let resp = req.send().await?;
        unwrap_status(resp, Endpoint::Parcel).await?;
        Ok(())
    }

    //////////////// Get Parcel ////////////////

    /// Returns the requested parcel (identified by its Bindle ID and SHA) as a vector of bytes
    pub async fn get_parcel<I>(&self, bindle_id: I, sha: &str) -> Result<Vec<u8>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = bindle_id.try_into().map_err(|e| e.into())?;
        let resp = self.get_parcel_request(&parsed_id, sha).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Returns the requested parcel (identified by its Bindle ID and SHA) as a stream of bytes.
    /// This is useful for when you don't want to read it into memory but are instead writing to a
    /// file or other location
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
        let resp = self.get_parcel_request(&parsed_id, sha).await?;
        Ok(resp.bytes_stream().map(|r| r.map_err(|e| e.into())))
    }

    async fn get_parcel_request(&self, bindle_id: &Id, sha: &str) -> Result<reqwest::Response> {
        // Override the default accept header
        let resp = self
            .client
            .get(
                self.base_url
                    .join(&format!("{}/{}@{}", INVOICE_ENDPOINT, bindle_id, sha))
                    .unwrap(),
            )
            .header(header::ACCEPT, "*/*")
            .send()
            .await?;
        unwrap_status(resp, Endpoint::Parcel).await
    }

    //////////////// Relationship Endpoints ////////////////

    /// Gets the labels of missing parcels, if any, of the specified bindle. If the bindle is
    /// yanked, this will fail
    pub async fn get_missing_parcels<I>(&self, id: I) -> Result<Vec<crate::Label>>
    where
        I: TryInto<Id>,
        I::Error: Into<ClientError>,
    {
        let parsed_id = id.try_into().map_err(|e| e.into())?;
        let req = self.client.get(self.base_url.join(&format!(
            "{}/{}/{}",
            RELATIONSHIP_ENDPOINT,
            "missing",
            parsed_id.to_string()
        ))?);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice).await?;
        Ok(toml::from_slice::<crate::MissingParcelsResponse>(&resp.bytes().await?)?.missing)
    }
}

// A helper function and related enum to make some reusable code for unwrapping a status code and returning the right error

enum Endpoint {
    Invoice,
    Parcel,
    Query,
}

async fn unwrap_status(resp: reqwest::Response, endpoint: Endpoint) -> Result<reqwest::Response> {
    match (resp.status(), endpoint) {
        (StatusCode::OK, _) => Ok(resp),
        (StatusCode::ACCEPTED, Endpoint::Invoice) => Ok(resp),
        (StatusCode::CREATED, Endpoint::Invoice) => Ok(resp),
        (StatusCode::NOT_FOUND, Endpoint::Invoice) | (StatusCode::FORBIDDEN, Endpoint::Invoice) => {
            Err(ClientError::InvoiceNotFound)
        }
        (StatusCode::NOT_FOUND, Endpoint::Parcel) => Err(ClientError::ParcelNotFound),
        (StatusCode::CONFLICT, Endpoint::Invoice) => Err(ClientError::InvoiceAlreadyExists),
        (StatusCode::CONFLICT, Endpoint::Parcel) => Err(ClientError::ParcelAlreadyExists),
        (StatusCode::UNAUTHORIZED, _) => Err(ClientError::Unauthorized),
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
