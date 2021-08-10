use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
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
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("Bindle-Server"));
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
        // This is the raw auth data, so we need to chop off the "Bearer" part of the header data
        // with any starting whitespace. I am not using to_lowercase to avoid an extra string
        // allocation
        let trimmed = auth_data
            .trim_start_matches("Bearer")
            .trim_start_matches("bearer")
            .trim_start();
        let value = serde_json::json!({
            "access_token": trimmed,
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
            let status = resp.status();
            let body = String::from_utf8_lossy(resp.bytes().await.unwrap_or_default().as_ref())
                .to_string();
            tracing::info!(status_code = %status, %body, "Token validation failed");
            anyhow::bail!("Unauthorized")
        }
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }
}
