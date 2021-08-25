//! Various implementations and traits for loading and handling tokens. These are used in the
//! [`Client`](super::Client) to provide tokens when configured

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{serde::ts_seconds, DateTime, Utc};
use oauth2::reqwest::async_http_client;
use oauth2::{
    basic::*, devicecode::DeviceAuthorizationResponse, AuthUrl, Client as Oauth2Client, ClientId,
    RefreshToken, StandardRevocableToken, StandardTokenResponse, TokenResponse, TokenUrl,
};
use reqwest::{
    header::{HeaderValue, AUTHORIZATION},
    Client as HttpClient, RequestBuilder,
};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

use super::{ClientError, Result};

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct OidcTokenExtraFields {
    pub id_token: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    // TODO(thomastaylor312): Maybe the refresh endpoint should be through the api so we don't have
    // to pass configuration back?
    #[serde(default)]
    pub token_url: String,
}

impl oauth2::ExtraTokenFields for OidcTokenExtraFields {}

#[derive(serde::Deserialize, Debug)]
struct Claims {
    pub iss: String,
    #[serde(with = "ts_seconds")]
    pub exp: DateTime<Utc>,
}

/// A trait that can be implemented by anything that can provide a valid token for use in a client.
/// Implementors of this trait should ensure that any token refresh/validation is done as part of
/// applying the authentication header
#[async_trait::async_trait]
pub trait TokenManager {
    /// Adds the necessary header to the request, returning the newly updated request builder or an
    /// error if there was a problem generating the token
    async fn apply_auth_header(&self, builder: RequestBuilder) -> Result<RequestBuilder>;
}

/// A token manager that does nothing. For use when authentication is not enabled or anonymous auth
/// is desired
#[derive(Clone, Default)]
pub struct NoToken;

#[async_trait::async_trait]
impl TokenManager for NoToken {
    async fn apply_auth_header(&self, builder: RequestBuilder) -> Result<RequestBuilder> {
        Ok(builder)
    }
}

/// A token manager for long lived tokens (such as service account tokens or personal access
/// tokens). This will simply configure the request to always return the provided token
#[derive(Clone)]
pub struct LongLivedToken {
    token: String,
}

impl LongLivedToken {
    /// Create a new LongLivedToken with the given token value
    pub fn new(token: &str) -> Self {
        LongLivedToken {
            token: token.to_owned(),
        }
    }
}

#[async_trait::async_trait]
impl TokenManager for LongLivedToken {
    async fn apply_auth_header(&self, builder: RequestBuilder) -> Result<RequestBuilder> {
        let mut header_val = HeaderValue::from_str(&format!("Bearer {}", self.token))
            .map_err(|e| ClientError::Other(e.to_string()))?;
        header_val.set_sensitive(true);
        Ok(builder.header(AUTHORIZATION, header_val))
    }
}

#[derive(Clone)]
pub struct HttpBasic {
    username: String,
    password: String,
}

impl HttpBasic {
    pub fn new(username: &str, password: &str) -> Self {
        HttpBasic {
            username: username.to_owned(),
            password: password.to_owned(),
        }
    }
}

#[async_trait::async_trait]
impl TokenManager for HttpBasic {
    async fn apply_auth_header(&self, builder: RequestBuilder) -> Result<RequestBuilder> {
        let data = base64::encode(format!("{}:{}", self.username, self.password));
        let mut header_val = HeaderValue::from_str(&format!("Basic {}", data))
            .map_err(|e| ClientError::Other(e.to_string()))?;
        header_val.set_sensitive(true);
        Ok(builder.header(AUTHORIZATION, header_val))
    }
}

type LockData<T> = Arc<RwLock<T>>;

/// A token manager for JWTs issued by an OIDC provider. This token manager expects a refresh token
/// and will attempt to refresh the token when it is close to expiry.
///
/// Note that any clone of this token manager will reuse the same underlying tokens to ensure that
/// only one refresh is done as needed
#[derive(Clone)]
pub struct OidcToken {
    id_token: LockData<String>,
    refresh_token: LockData<RefreshToken>,
    expiry_time: LockData<DateTime<Utc>>,
    issuer: String,
    scopes: Vec<String>,
    client_id: String,
    token_url: String,
    token_file: Option<PathBuf>,
}

impl OidcToken {
    /// Create a new OidcToken from an ID token, refresh token, client ID, token url, and a set of
    /// scopes. Only use this method if you do not have a token file available. Because there is no
    /// token file, refreshed tokens will not be saved on disk
    pub async fn new_from_parts(
        id_token: &str,
        refresh_token: &str,
        client_id: &str,
        token_url: &str,
        scopes: Vec<String>,
    ) -> Result<Self> {
        let (expiry_time, issuer) = data_from_token(id_token)?;
        let me = OidcToken {
            id_token: Arc::new(RwLock::new(id_token.to_owned())),
            refresh_token: Arc::new(RwLock::new(RefreshToken::new(refresh_token.to_owned()))),
            expiry_time: Arc::new(RwLock::new(expiry_time)),
            issuer,
            scopes,
            client_id: client_id.to_owned(),
            token_url: token_url.to_owned(),
            token_file: None,
        };
        // Make sure we don't need a refresh
        me.ensure_token().await?;
        Ok(me)
    }

    /// Create a new OidcToken by loading a token file from the path. This token file is what is
    /// generated from the `login` method
    pub async fn new_from_file(token_file: impl AsRef<Path>) -> Result<Self> {
        let path = token_file.as_ref().to_owned();
        let raw = tokio::fs::read(&path).await?;
        let token_res: StandardTokenResponse<OidcTokenExtraFields, BasicTokenType> =
            toml::from_slice(&raw)?;
        let mut me = Self::new_from_parts(
            &token_res.extra_fields().id_token,
            token_res
                .refresh_token()
                .ok_or_else(|| {
                    ClientError::TokenError(
                        "Token response does not contain a refresh token".into(),
                    )
                })?
                .secret(),
            &token_res.extra_fields().client_id,
            &token_res.extra_fields().token_url,
            token_res
                .scopes()
                .map(|s| s.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
        )
        .await?;
        me.token_file = Some(path);
        Ok(me)
    }

    /// Creates a new OidcToken by logging in with the given bindle server base URL (e.g.
    /// https:://my.bindle.com/v1) and then saving the resulting token at the given path
    ///
    /// The token file is the OAuth2 response body from the authorization flow serialized to disk as
    /// TOML
    ///
    /// NOTE: This function requires user interaction and will print to stdout
    pub async fn login(bindle_base_url: &str, token_file: impl AsRef<Path>) -> Result<Self> {
        let (base_url, headers) = super::base_url_and_headers(bindle_base_url)?;
        let login_resp = HttpClient::builder()
            .build()?
            .get(base_url.join(super::LOGIN_ENDPOINT).unwrap())
            .query(&crate::LoginParams {
                provider: "nothing".into(), // TODO: this will matter once we allow multiple kinds of auth
            })
            .headers(headers)
            .send()
            .await?;
        let login_resp =
            super::unwrap_status(login_resp, super::Endpoint::Login, super::Operation::Login)
                .await?;
        let device_code_details: DeviceAuthorizationResponse<
            crate::DeviceAuthorizationExtraFields,
        > = toml::from_slice(&login_resp.bytes().await?)?;

        println!(
            "Open this URL in your browser:\n{}\nand then enter the code when prompted: {}",
            device_code_details.verification_uri().to_string(),
            device_code_details.user_code().secret().to_string()
        );

        let oauth_client: Oauth2Client<
            BasicErrorResponse,
            StandardTokenResponse<OidcTokenExtraFields, BasicTokenType>,
            BasicTokenType,
            BasicTokenIntrospectionResponse,
            StandardRevocableToken,
            BasicRevocationErrorResponse,
        > = Oauth2Client::new(
            ClientId::new(device_code_details.extra_fields().client_id.clone()),
            None,
            AuthUrl::new("https://not.needed.com".into()).unwrap(),
            Some(TokenUrl::new(device_code_details.extra_fields().token_url.clone()).unwrap()),
        )
        .set_auth_type(oauth2::AuthType::RequestBody);

        let token_res = match oauth_client
            .exchange_device_access_token(&device_code_details)
            .request_async(async_http_client, tokio::time::sleep, None)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                return Err(ClientError::Other(format!("{:?}", e)));
            }
        };

        let (expiry_time, issuer) = data_from_token(&token_res.extra_fields().id_token)?;

        let me = OidcToken {
            id_token: Arc::new(RwLock::new(token_res.extra_fields().id_token.to_owned())),
            refresh_token: Arc::new(RwLock::new(RefreshToken::new(
                token_res
                    .refresh_token()
                    .ok_or_else(|| {
                        ClientError::TokenError(
                            "Token response does not contain a refresh token".into(),
                        )
                    })?
                    .secret()
                    .to_owned(),
            ))),
            expiry_time: Arc::new(RwLock::new(expiry_time)),
            issuer,
            scopes: token_res
                .scopes()
                .map(|s| s.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            client_id: device_code_details.extra_fields().client_id.clone(),
            token_url: device_code_details.extra_fields().token_url.clone(),
            token_file: Some(token_file.as_ref().to_owned()),
        };

        me.write_token_file(token_res).await?;

        Ok(me)
    }

    /// Ensures that the current token is valid, refreshing if necessary and writing to the token
    /// file (if one was used)
    async fn ensure_token(&self) -> Result<()> {
        // Magic number: wiggle room of 1 minute. If we are under 1 minute of expiring, then we
        // should refresh. Also, we are locking inline here so the read lock is dropped as soon as
        // possible
        let is_expired =
            Utc::now() - chrono::Duration::minutes(1) >= *self.expiry_time.read().await;
        if is_expired {
            tracing::debug!("Token has expired, attempting to refresh token");
            let oauth_client: Oauth2Client<
                BasicErrorResponse,
                StandardTokenResponse<OidcTokenExtraFields, BasicTokenType>,
                BasicTokenType,
                BasicTokenIntrospectionResponse,
                StandardRevocableToken,
                BasicRevocationErrorResponse,
            > =
                Oauth2Client::new(
                    ClientId::new(self.client_id.clone()),
                    None,
                    AuthUrl::new("https://not.needed.com".into()).unwrap(),
                    Some(TokenUrl::new(self.token_url.clone()).map_err(|e| {
                        ClientError::TokenError(format!("Invalid token url: {}", e))
                    })?),
                )
                .set_auth_type(oauth2::AuthType::RequestBody);

            // Block for holding the write locks as short as possible
            let token_res = {
                // We are taking a write lock here because we are going to overwrite it
                let mut refresh_token = self.refresh_token.write().await;
                let token_res = match oauth_client
                    .exchange_refresh_token(&refresh_token)
                    .request_async(async_http_client)
                    .await
                {
                    Ok(t) => t,
                    Err(e) => {
                        return Err(ClientError::TokenError(format!(
                            "Unable to refresh token {:?}",
                            e
                        )));
                    }
                };

                let (expiry, _) = data_from_token(&token_res.extra_fields().id_token)?;
                let mut expiry_time = self.expiry_time.write().await;
                let mut id_token = self.id_token.write().await;
                *expiry_time = expiry;
                *id_token = token_res.extra_fields().id_token.clone();
                *refresh_token = RefreshToken::new(
                    token_res
                        .refresh_token()
                        .ok_or_else(|| {
                            ClientError::TokenError(
                                "Token response does not contain a refresh token".into(),
                            )
                        })?
                        .secret()
                        .to_owned(),
                );
                token_res
            };

            if let Some(p) = self.token_file.as_ref() {
                tracing::trace!(path = %p.display(), "Token refreshed and token file is set. Updating with token data");
                self.write_token_file(token_res).await?;
            }
        }
        Ok(())
    }

    async fn write_token_file(
        &self,
        mut token_res: StandardTokenResponse<OidcTokenExtraFields, BasicTokenType>,
    ) -> Result<()> {
        let token_file = match self.token_file.as_ref() {
            Some(p) => p,
            // If the file is not set, just return
            None => return Ok(()),
        };
        // Parse the issuer from the id token so it can be stored for refresh. We aren't worrying
        // about validating the token here as we just need the issuer
        let mut extra = token_res.extra_fields().to_owned();
        let (_, issuer) = data_from_token(&token_res.extra_fields().id_token)?;
        extra.issuer = issuer.clone();
        extra.client_id = self.client_id.clone();
        extra.token_url = self.token_url.clone();
        token_res.set_extra_fields(extra);

        tracing::info!(path = %token_file.display(), "Writing access token to file");

        #[cfg(not(target_family = "windows"))]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .truncate(true)
            .open(token_file)
            .await?;

        // TODO: Figure out the proper permission for a token on disk for windows
        #[cfg(target_family = "windows")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(token_file)
            .await?;

        file.write_all(&toml::to_vec(&token_res)?).await?;
        // Make sure everything is flushed out to disk
        file.flush().await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl TokenManager for OidcToken {
    async fn apply_auth_header(&self, builder: RequestBuilder) -> Result<RequestBuilder> {
        self.ensure_token().await?;

        let mut header_val =
            HeaderValue::from_str(&format!("Bearer {}", (*self.id_token.read().await).clone()))
                .map_err(|e| ClientError::Other(e.to_string()))?;
        header_val.set_sensitive(true);
        Ok(builder.header(AUTHORIZATION, header_val))
    }
}

fn data_from_token(token: &str) -> Result<(DateTime<Utc>, String)> {
    let parsed_token = jsonwebtoken::dangerous_insecure_decode::<Claims>(token)
        .map_err(|e| ClientError::TokenError(format!("Invalid token data: {}", e)))?;

    Ok((parsed_token.claims.exp, parsed_token.claims.iss))
}
