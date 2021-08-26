//! Client implementation for consuming a Bindle API. Although written in Rust, it is not specific
//! to the Rust implementation. It is meant to consume any spec-compliant bindle implementation.

mod error;
pub mod load;

use std::convert::TryInto;
use std::path::Path;
use std::time::Duration;

use hyper::header::{HeaderMap, HeaderValue};
use oauth2::TokenResponse;
use oauth2::{
    basic::BasicTokenResponse,
    devicecode::{DeviceAuthorizationResponse, DeviceCodeErrorResponseType},
    StandardErrorResponse,
};
use reqwest::header;
use reqwest::Client as HttpClient;
use reqwest::{Body, RequestBuilder, StatusCode};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
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
// TODO: Uncomment once Github is fixed
// const GITHUB_AUTH_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

/// A client type for interacting with a Bindle server
#[derive(Clone)]
pub struct Client {
    client: HttpClient,
    base_url: Url,
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
pub struct ClientBuilder {
    http2_prior_knowledge: bool,
    danger_accept_invalid_certs: bool,
    auth_token: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            http2_prior_knowledge: false,
            danger_accept_invalid_certs: false,
            auth_token: None,
            username: None,
            password: None,
        }
    }
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

    /// Sets the bearer token to use for authentication with the bindle server, if one already
    /// exists.
    ///
    /// If you need to log in, you can set the token after constructing the `Client` with
    /// [`set_auth_token`](Client::set_auth_token)
    pub fn auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    pub fn user_password(mut self, user: String, password: String) -> Self {
        self.username = Some(user);
        self.password = Some(password);
        self
    }

    /// Returns a new Client with the given URL, configured using the set options. If you want to
    /// build a client while logging into a server, use [`ClientBuilder::login`]
    ///
    /// This URL should be the FQDN plus any namespacing (like `v1`). So if you were running a
    /// bindle server mounted at the v1 endpoint, your URL would look something like
    /// `http://my.bindle.com/v1/`. Will return an error if the URL is not valid
    pub fn build(self, base_url: &str) -> Result<Client> {
        let (base_parsed, mut headers) = base_url_and_headers(base_url)?;

        // Token auth overrides user auth
        if let Some(token) = self.auth_token {
            let mut header_val = HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|e| ClientError::Other(e.to_string()))?;
            header_val.set_sensitive(true);
            headers.insert(header::AUTHORIZATION, header_val);
        } else if let Some(username) = self.username {
            let pw = self.password.unwrap_or_default();
            let data = base64::encode(format!("{}:{}", username, pw));
            let mut header_val = HeaderValue::from_str(&format!("Basic {}", data))
                .map_err(|e| ClientError::Other(e.to_string()))?;
            header_val.set_sensitive(true);
            headers.insert(header::AUTHORIZATION, header_val);
        }

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
        })
    }

    /// Logs in to a bindle server, saving the access_token at the configured path (it will
    /// overwrite any existing data at the path), and then returning a new client configured with
    /// that token.
    ///
    /// Please note that this function requires user interaction. It will request a device code for
    /// logging in to the CLI, which requires a user to do so in a browser. This information will be
    /// printed to STDOUT
    pub async fn login(mut self, base_url: &str, token_path: impl AsRef<Path>) -> Result<Client> {
        let (base_parsed, headers) = base_url_and_headers(base_url)?;

        let client = HttpClient::builder()
            .and_if(self.http2_prior_knowledge, |b| b.http2_prior_knowledge())
            .and_if(self.danger_accept_invalid_certs, |b| {
                b.danger_accept_invalid_certs(true)
            })
            .default_headers(headers.clone())
            .build()
            .map_err(|e| ClientError::Other(e.to_string()))?;
        let login_resp = client
            .get(base_parsed.join(LOGIN_ENDPOINT).unwrap())
            .query(&crate::LoginParams {
                provider: crate::LoginProvider::Github,
            })
            .send()
            .await?;
        let login_resp = unwrap_status(login_resp, Endpoint::Login, Operation::Login).await?;
        let device_code_details: DeviceAuthorizationResponse<
            crate::DeviceAuthorizationExtraFields,
        > = toml::from_slice(&login_resp.bytes().await?)?;

        println!(
            "Open this URL in your browser:\n{}\nand then enter the code when prompted: {}",
            device_code_details.verification_uri().to_string(),
            device_code_details.user_code().secret().to_string()
        );

        // HACK(thomastaylor312): Please note that this is how things should work (for pretty much
        // any device flow), but Github's is in beta and it doesn't return the proper status code
        // (400) when the user hasn't logged in yet per RFC 8628 section 3.5 and RFC 6749 section
        // 5.2. So the code completely chokes. Instead we are doing the check manually

        // let oauth_client = BasicClient::new(
        //     ClientId::new(device_code_details.extra_fields().client_id.clone()),
        //     None,
        //     AuthUrl::new(GITHUB_AUTH_URL.to_owned()).unwrap(),
        //     Some(TokenUrl::new(GITHUB_TOKEN_URL.to_owned()).unwrap()),
        // );

        // let token_res = match oauth_client
        //     .exchange_device_access_token(&device_code_details)
        //     .request_async(async_http_client, tokio::time::sleep, None)
        //     .await
        // {
        //     Ok(t) => t,
        //     Err(e) => {
        //         if let oauth2::RequestTokenError::Parse(err, d) = &e {
        //             println!("Data: {}", String::from_utf8_lossy(&d));
        //         }
        //         return Err(ClientError::Other(format!("{:?}", e)));
        //     }
        // };

        // let token = token_res.access_token();

        let token = wait_for_auth(device_code_details).await?;

        tracing::info!(path = %token_path.as_ref().display(), "Writing access token to file");

        #[cfg(not(target_family = "windows"))]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .open(token_path)
            .await?;

        // TODO: Figure out the proper permission for a token on disk for windows
        #[cfg(target_family = "windows")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(token_path)
            .await?;

        file.write_all(token.as_bytes()).await?;

        self.auth_token = Some(token);

        // Because we can't consume the client and reuse the config, we just need to reconfigure the
        // whole thing
        self.build(base_parsed.as_str())
    }
}

async fn wait_for_auth(
    device_code_details: DeviceAuthorizationResponse<crate::DeviceAuthorizationExtraFields>,
) -> Result<String> {
    // Add a little wiggle room on the interval
    let mut interval =
        tokio::time::interval(device_code_details.interval() + Duration::from_secs(1));
    // Just skip if we miss a tick so we don't get slow down responses
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Get the maximum time we should wait from the details. We are subtracting the interval from
    // the max duration so that we have a buffer of time at the end rather than triggering a code
    // expired error on the last tick
    let timeout = device_code_details.expires_in() + device_code_details.interval();

    let mut headers = header::HeaderMap::new();
    headers.insert(header::ACCEPT, "application/json".parse().unwrap());
    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());

    let client = reqwest::ClientBuilder::new()
        .default_headers(headers)
        .build()?;

    loop {
        let elapsed = interval.tick().await;
        if elapsed.elapsed() >= timeout {
            return Err(ClientError::Other(
                "Timeout reached for device code auth. Please login again".into(),
            ));
        }

        tracing::debug!("Checking for access token");

        let res = client
            .post(GITHUB_TOKEN_URL)
            .body(
                serde_json::to_vec(&serde_json::json!({
                    "client_id": device_code_details.extra_fields().client_id,
                    "device_code": device_code_details.device_code().secret(),
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                }))
                .expect("Unable to serialize auth token body. This is programmer error"),
            )
            .send()
            .await?;

        match parse_response(res).await? {
            either::Left(e) => {
                match e.error() {
                    DeviceCodeErrorResponseType::AuthorizationPending => {
                        tracing::debug!(retry_interval = ?device_code_details.interval(), "Device authorization still pending, will retry");
                        continue
                    }
                    DeviceCodeErrorResponseType::SlowDown => tracing::warn!("Token polling operation occurred too quickly, will retry"),
                    DeviceCodeErrorResponseType::ExpiredToken => return Err(ClientError::Other(e.error_description().map(|s| s.to_owned()).unwrap_or_else(|| "Device login token has expired".into()))),
                    _ => return Err(ClientError::Other(format!("Unable to continue with device authentication, got error of type {} with description '{}' and error_uri of '{}'", e.error(), e.error_description().map(|s| s.to_owned()).unwrap_or_default(), e.error_uri().map(|s| s.to_owned()).unwrap_or_default())))
                }
            }
            either::Right(s) => {
                return Ok(s.access_token().secret().to_owned())
            }
        }
    }
}

async fn parse_response(
    resp: reqwest::Response,
) -> Result<either::Either<StandardErrorResponse<DeviceCodeErrorResponseType>, BasicTokenResponse>>
{
    let status = resp.status();
    let raw = resp.bytes().await?;
    if !status.is_success() {
        return Err(ClientError::Other(format!(
            "Got non-success response from Github (status code {}) with the following body: {}",
            status,
            String::from_utf8_lossy(&raw)
        )));
    }

    // First attempt to parse as an error, if that fails, then parse it as a success
    match serde_json::from_slice::<StandardErrorResponse<DeviceCodeErrorResponseType>>(&raw) {
        Ok(r) => return Ok(either::Left(r)),
        // This means it was a parse error so it isn't an error response
        Err(e) if e.is_syntax() || e.is_data() => (),
        Err(_) => {
            return Err(ClientError::Other(format!(
                "Invalid data received from Github: {}",
                String::from_utf8_lossy(&raw)
            )))
        }
    }

    // If we got here, it is probably a success response
    Ok(either::Right(serde_json::from_slice(&raw).map_err(
        |_| {
            ClientError::Other(format!(
                "Invalid success response received from Github: {}",
                String::from_utf8_lossy(&raw)
            ))
        },
    )?))
}

fn base_url_and_headers(base_url: &str) -> Result<(Url, HeaderMap)> {
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

impl Client {
    /// Returns a new Client with the given URL, configured using the default options.
    ///
    /// This URL should be the FQDN plus any namespacing (like `v1`). So if you were running a
    /// bindle server mounted at the v1 endpoint, your URL would look something like
    /// `http://my.bindle.com/v1/`. Will return an error if the URL is not valid
    pub fn new(base_url: &str) -> Result<Self> {
        ClientBuilder::default().build(base_url)
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
        let req = self.create_invoice_builder().body(toml::to_vec(&inv)?);
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
        let req = self.client.delete(self.base_url.join(&format!(
            "{}/{}",
            INVOICE_ENDPOINT,
            parsed_id.to_string()
        ))?);
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
            RELATIONSHIP_ENDPOINT,
            "missing",
            parsed_id.to_string()
        ))?);
        trace!(?req);
        let resp = req.send().await?;
        let resp = unwrap_status(resp, Endpoint::Invoice, Operation::Get).await?;
        Ok(toml::from_slice::<crate::MissingParcelsResponse>(&resp.bytes().await?)?.missing)
    }
}

// We implement provider for client because often times (such as in the CLI) we are composing the
// client in to a provider cache. This implementation does not verify or sign anything
#[async_trait::async_trait]
impl Provider for Client {
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
