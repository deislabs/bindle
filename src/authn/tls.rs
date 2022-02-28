use anyhow::bail;
use x509_parser::parse_x509_certificate;

use super::{AuthData, Authenticator};
use crate::authz::Authorizable;

/// An authenticator that checks for a (preauthenticated) client certificate
#[derive(Clone, Debug, Default)]
pub struct TlsAuthenticator(());

impl TlsAuthenticator {
    pub fn new() -> Self {
        Self(())
    }
}

#[async_trait::async_trait]
impl Authenticator for TlsAuthenticator {
    type Item = ClientCertInfo;

    async fn authenticate(&self, auth_data: &AuthData) -> anyhow::Result<Self::Item> {
        let peer_cert = match auth_data
            .peer_certs
            .as_ref()
            .and_then(|certs| certs.as_ref().last())
            .map(|cert| parse_x509_certificate(cert.as_ref()))
            .transpose()?
        {
            // Sanity check: should have been None instead of Some(<empty>)
            None => bail!("no certificate in peer_certificates!"),
            // Sanity check: valid Certificate should have just enough data
            Some((extra_data, _)) if !extra_data.is_empty() => {
                bail!("extra data in peer certificate!")
            }
            Some((_, cert)) => cert,
        };

        let common_name = match peer_cert.subject().iter_common_name().collect::<Vec<_>>().as_slice() {
            &[name] => name.as_str()?.to_string(),
            names => bail!("peer certificate has {} common names; expected 1", names.len())
        };

        // TODO(lann): Could populate groups from other subject parts

        Ok(ClientCertInfo { common_name })
    }
}

/// Represents authenticated client certificate info.
pub struct ClientCertInfo {
    common_name: String,
}

impl Authorizable for ClientCertInfo {
    fn principal(&self) -> &str {
        self.common_name.as_ref()
    }
}