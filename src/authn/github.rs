use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::StatusCode;

use super::Authenticator;

#[derive(Clone)]
pub struct GithubAuthenticator {
    introspection_url: String,
    client_id: String,
    client: reqwest::Client,
}

impl GithubAuthenticator {
    pub fn new(client_id: &str, client_secret: &str) -> anyhow::Result<Self> {
        let introspection_url = format!("https://api.github.com/applications/{}/token", client_id);
        let mut auth_header = HeaderValue::from_str(&format!(
            "Basic {}",
            base64::encode(format!("{}:{}", client_id, client_secret))
        ))?;
        auth_header.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth_header);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github.v3+json"),
        );
        let client = reqwest::ClientBuilder::new()
            .default_headers(headers)
            .build()?;

        Ok(GithubAuthenticator {
            introspection_url,
            client_id: client_id.to_owned(),
            client,
        })
    }
}

#[async_trait::async_trait]
impl Authenticator for GithubAuthenticator {
    // TODO: Right now we have no authz stuff glued in. However, once we implement authz stuff, we
    // can add in the information from the token introspection request
    type Item = crate::authz::always::Anonymous;

    async fn authenticate(&self, auth_data: &str) -> anyhow::Result<Self::Item> {
        let value = serde_json::json!({
            "access_token": auth_data,
        });
        let raw = serde_json::to_vec(&value).map_err(|e| {
            // For security reasons, do not return the json error to the user, only log it
            tracing::error!(error = %e, "Unable to serialize access token data, possible malformed data sent");
            anyhow::anyhow!("Unauthorized")
        })?;
        let resp = self
            .client
            .post(&self.introspection_url)
            .body(raw)
            .send()
            .await
            .map_err(|e| {
                // For security reasons, do not return the client error
                tracing::error!(error = %e, "Unable to construct and send http request");
                anyhow::anyhow!("Unauthorized")
            })?;

        if resp.status() == StatusCode::OK {
            // TODO: Once we need to fetch data from the response, we can deserialize here
            Ok(crate::authz::always::Anonymous)
        } else {
            tracing::info!(status_code = %resp.status(), "Token validation failed");
            anyhow::bail!("Unauthorized")
        }
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }
}
