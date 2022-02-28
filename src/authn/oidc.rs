//! An authenticator that validates OIDC issued JWTs
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use openid::biscuit::jwk::{AlgorithmParameters, KeyType, PublicKeyUse};
use serde::Deserialize;
use tokio::sync::{RwLock, RwLockReadGuard};
use tokio::time::{Duration, Instant};
use url::Url;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::{Authenticator, AuthData};
use crate::authz::Authorizable;

const ONE_HOUR: Duration = Duration::from_secs(3600);
// Right now we only really support RSA algorithms. We _technically_ parse any JWKs that are EC
// keys, but we might have to change to dynamically creating this list by detecting the algorithm on
// the JWT
const SUPPORTED_ALGORITHMS: &[Algorithm] = &[Algorithm::RS256, Algorithm::RS384, Algorithm::RS512];

/// An authenticator that validates JWTs issued by OIDC providers.
#[derive(Clone)]
pub struct OidcAuthenticator {
    authority_url: Url,
    device_auth_url: String,
    client_id: String,
    token_url: String,
    // We are using RwLocks for interior mutability and so that if any cloned authenticator (like
    // what happens in warp) updates the data, it is reflected everywhere
    key_cache: Arc<RwLock<HashMap<String, DecodingKey>>>,
    last_refresh: Arc<RwLock<Instant>>,
    validator: Validation,
}

impl OidcAuthenticator {
    /// Constructs a new OIDC authenticator using the given URL and the client_id of the OIDC
    /// application. This URL should be the issuer of the tokens being validated and the server
    /// where the OIDC discovery information can be found
    pub async fn new(
        oidc_authority_url: &str,
        device_auth_url: &str,
        client_id: &str,
    ) -> anyhow::Result<Self> {
        // Trim off any trailing slash
        let authority_url: Url = oidc_authority_url
            .trim_end_matches('/')
            .to_owned()
            .parse()?;

        let discovery = openid::DiscoveredClient::discover(
            client_id.to_owned(),
            String::new(),
            None,
            authority_url.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Unable to fetch discovery data: {}", e))?;

        let mut issuers = HashSet::with_capacity(1);
        issuers.insert(authority_url.to_string());
        let mut validator = Validation::default();
        validator.validate_nbf = true;
        validator.iss = Some(issuers);
        validator.algorithms = SUPPORTED_ALGORITHMS.to_owned();
        let me = OidcAuthenticator {
            authority_url: authority_url.clone(),
            device_auth_url: device_auth_url.to_owned(),
            client_id: client_id.to_owned(),
            token_url: discovery.config().token_endpoint.to_string(),
            key_cache: Arc::new(RwLock::new(HashMap::new())),
            last_refresh: Arc::new(RwLock::new(Instant::now())),
            validator,
        };

        // Do the initial discovery of keys
        me.update_keys(discovery).await?;
        Ok(me)
    }

    /// Finds the given key id from well known keys, updating the keys if not found.
    async fn find_key(&self, kid: &str) -> anyhow::Result<RwLockReadGuard<'_, DecodingKey>> {
        // 1. Check if key exists
        // 2. If key is not found and refresh timer is expired, fetch well-known keys and merge.
        //    Then reset timer
        // 3. Check again

        if let Ok(k) = self.lookup_key(kid).await {
            return Ok(k);
        }

        if self.last_refresh.read().await.elapsed() >= ONE_HOUR {
            let discovery = openid::DiscoveredClient::discover(
                self.client_id.clone(),
                String::new(),
                None,
                self.authority_url.clone(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Unable to fetch key set from issuer on update: {}", e))?;
            self.update_keys(discovery).await?;
        } else {
            // If it isn't update time, just return an error as we know we won't find the key
            anyhow::bail!("A key with a key_id of {} was not found", kid)
        }

        self.lookup_key(kid).await
    }

    /// Looks up if a key with the given ID exists
    async fn lookup_key(&self, kid: &str) -> anyhow::Result<RwLockReadGuard<'_, DecodingKey>> {
        let keys = self.key_cache.read().await;
        RwLockReadGuard::try_map(keys, |k| k.get(kid))
            .map_err(|_| anyhow::anyhow!("A key with a key_id of {} was not found", kid))
    }

    /// Updates the keys with the latest set from the issuer. This takes a write lock on the key cache
    async fn update_keys(&self, discovery: openid::DiscoveredClient) -> anyhow::Result<()> {
        let mut key_cache = self.key_cache.write().await;

        *key_cache = discovery
            .jwks
            .ok_or_else(|| anyhow::anyhow!("Issuer has no keys available"))?
            .keys
            .into_iter()
            .filter_map(|k| {
                // Right now we are just filtering out anything that isn't for signatures, because
                // that is all we care about here
                if !k
                    .common
                    .public_key_use
                    .map_or(false, |u| matches!(u, PublicKeyUse::Signature))
                {
                    return None;
                }

                let key = match k.algorithm.key_type() {
                    KeyType::EllipticCurve => {
                        // TODO(thomastaylor312): This method of using the cert didn't seem to work
                        // for RSA (and technically the x5c parameter is optional according to the
                        // spec), so I don't know if it works here. But there isn't a method on
                        // decoding key to use the `x` and `y` coordinates to create the key.
                        // However, I think most keys I've seen in the wild are using RSA right now
                        // and some of the handling around JWTs is improved in the forthcoming
                        // version of the `jsonwebtokens` crate. So we can revisit things at that
                        // point

                        // The first certificate in the chain should always be the one used to validate. It
                        // is base64 encoded per the spec, so we need to decode it here. If for some reason
                        // that key is malformed (which should be a rare instance because it would mean
                        // misconfiguration on the OIDC provider's part), just skip it
                        let raw =
                            base64::decode(k.common.x509_chain.unwrap_or_default().pop()?).ok()?;
                        DecodingKey::from_ec_der(&raw)
                    }
                    KeyType::RSA => {
                        if let AlgorithmParameters::RSA(rsa) = k.algorithm {
                            // NOTE: jsonwebtoken expects a base64 encoded component (big endian) so
                            // we are reencoding it here
                            DecodingKey::from_rsa_components(
                                &base64::encode_config(
                                    rsa.n.to_bytes_be(),
                                    base64::URL_SAFE_NO_PAD,
                                ),
                                &base64::encode_config(
                                    rsa.e.to_bytes_be(),
                                    base64::URL_SAFE_NO_PAD,
                                ),
                            ).map_or_else(|e| {
                                tracing::error!(error = %e, "Unable to parse decoding key from discovery client, skipping");
                                None
                            }, Some)?
                        } else {
                            return None;
                        }
                    }
                    // We don't support octet here
                    _ => return None,
                };
                Some((k.common.key_id.unwrap_or_default(), key))
            })
            .collect();
        // If the update was successful, update the last refresh time to now
        let mut last_refresh = self.last_refresh.write().await;
        *last_refresh = Instant::now();
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct Claims {
    preferred_username: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    sub: String,
    iss: String,
    groups: Option<Vec<String>>,
}

#[async_trait::async_trait]
impl Authenticator for OidcAuthenticator {
    type Item = OidcUser;

    async fn authenticate(&self, auth_data: &AuthData) -> anyhow::Result<Self::Item> {
        let auth_header = auth_data.auth_header.as_deref().unwrap_or_default();
        // This is the raw auth data, so we need to chop off the "Bearer" part of the header data
        // with any starting whitespace. I am not using to_lowercase to avoid an extra string
        // allocation
        let raw_token = auth_header
            .trim_start_matches("Bearer")
            .trim_start_matches("bearer")
            .trim();

        // Get the header so we can fetch the desired key
        let header = jsonwebtoken::decode_header(raw_token)?;
        let key = self.find_key(&header.kid.unwrap_or_default()).await?;
        let token = jsonwebtoken::decode::<Claims>(raw_token, &key, &self.validator)?;

        Ok(token.claims.into())
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn auth_url(&self) -> &str {
        &self.device_auth_url
    }

    fn token_url(&self) -> &str {
        &self.token_url
    }
}

/// An [`Authorizable`] user generated from a valid JWT.
///
/// The principal of the user will be one of the following values from the JWT claims (in priority
/// order): `preferred_username`, `email` (if `email_verified` is `true`), and then `sub`. This will
/// be combined with the `iss` claim to ensure the username is unique (e.g.
/// foobar@https://service.com). This is to prevent bad actors from creating an account with another
/// provider with the same username and then gaining access to resources that are only scoped to
/// specific usernames.
///
/// The groups list will be populated from the token if the `groups` claim exists on the JWT.
/// Otherwise, it will be empty
pub struct OidcUser {
    principal: String,
    groups: Vec<String>,
}

impl Authorizable for OidcUser {
    fn principal(&self) -> &str {
        self.principal.as_ref()
    }

    fn groups(&self) -> &[String] {
        self.groups.as_ref()
    }
}

impl From<Claims> for OidcUser {
    fn from(c: Claims) -> Self {
        let username = match (c.preferred_username, c.email) {
            (Some(u), _) => u,
            (None, Some(e)) if c.email_verified.unwrap_or_default() => e,
            _ => c.sub,
        };

        OidcUser {
            principal: format!("{}@{}", username, c.iss),
            groups: c.groups.unwrap_or_default(),
        }
    }
}
