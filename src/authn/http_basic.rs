use std::{collections::HashMap, path::Path};

use super::Authenticator;
use crate::authz::always::Anonymous;

/// HTTP header prefix
const HTTP_BASIC_PREFIX: &str = "Basic ";

/// An authenticator that simply returns an anonymous user
///
/// In basic auth, the auth_data will come in as 'Basic BASE64_STRING', where
/// the Base-64 string is the username and password separated by a colon.
///
/// This tool splits username and password, looks up the user in the database,
/// and then compares the hashed password to the hash returned by the database.
#[derive(Clone, Debug)]
pub struct HttpBasic {
    authmap: HashMap<String, String>,
}

impl HttpBasic {
    /// Read an htpasswd-formatted file.
    ///
    /// This only supports SHA1, though we should switch to bcrypt if there is a good lib.
    ///
    /// Example htpassword entry for a bcrypt hash:
    ///
    /// > myName:$2y$05$c4WoMPo3SXsafkva.HHa6uXQZWr7oboPiC2bT/r7q1BB8I2s0BRqC
    ///
    /// See https://httpd.apache.org/docs/2.4/misc/password_encryptions.html
    pub async fn from_file(authfile: impl AsRef<Path>) -> anyhow::Result<Self> {
        // Load the file
        let raw = tokio::fs::read_to_string(&authfile).await?;
        // Parse the records into a map
        let mut authmap = HashMap::new();
        for line in raw.split_terminator('\n') {
            let line = line.trim();
            // Each line is username:{hash}value
            let pair: Vec<&str> = line.splitn(2, ':').collect();
            if pair.len() == 2 {
                authmap.insert(pair[0].to_owned(), pair[1].to_owned());
            }
        }
        // Attach the map to the struct
        Ok(HttpBasic { authmap })
    }

    fn check_credentials(&self, username: String, password: String) -> bool {
        // Note that it is consider a security risk to leak any information about
        // why an auth failed. So returning a bool provides the minimal info necessary.
        match self.authmap.get(&username) {
            Some(ciphertext) => {
                if ciphertext.starts_with("$2y$") {
                    match bcrypt::verify(password, ciphertext) {
                        Err(e) => {
                            tracing::warn!(%e, "Error verifying bcrypted passwd");
                            false
                        }
                        Ok(res) => res,
                    }
                } else {
                    tracing::warn!("htpasswd has entries in the wrong format.");
                    false
                }
            }
            None => {
                // Intentionally waste time to prevent timing attacks from disclosing
                // the presence or absence of a user ID. The number of rounds ($07$) will
                // control how long this takes. Higher is longer.
                let _ = bcrypt::verify(
                    username,
                    "$2y$07$QCVM96JWmNWzx3k/7g1UXOLAO2y0imHGNjzEVkQoikrsV3gd4Xqk6",
                );
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl Authenticator for HttpBasic {
    // TODO: When Authz is plumbed in, we should be more specific.
    type Item = Anonymous;

    async fn authenticate(&self, auth_data: &str) -> anyhow::Result<Self::Item> {
        if auth_data.is_empty() {
            anyhow::bail!("Username and password are required")
        }

        let (username, password) = parse_basic(auth_data)?;
        match self.check_credentials(username, password) {
            true => Ok(Anonymous),
            false => anyhow::bail!("Authentication failed"),
        }
    }
}

fn parse_basic(auth_data: &str) -> anyhow::Result<(String, String)> {
    match auth_data.strip_prefix(HTTP_BASIC_PREFIX) {
        None => anyhow::bail!("Wrong auth type. Only Basic auth is supported"),
        Some(suffix) => {
            // suffix should be base64 string
            let decoded = String::from_utf8(base64::decode(suffix)?)?;
            let pair: Vec<&str> = decoded.splitn(2, ':').collect();
            if pair.len() != 2 {
                anyhow::bail!("Malformed Basic header")
            } else {
                Ok((pair[0].to_owned(), pair[1].to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_parse_basic() {
        let (name, pw) =
            super::parse_basic("Basic YWRtaW46c3cwcmRmMXNo").expect("Basic header should parse");
        assert_eq!("admin", name);
        assert_eq!("sw0rdf1sh", pw, "the password is always swordfish");

        super::parse_basic("NotBasic fadsfasdjkfhsadkjfhkashdfa").expect_err("Not a Basic header");
    }

    #[tokio::test]
    async fn test_load_and_auth() {
        let authfile = "test/data/htpasswd";
        let basic = super::HttpBasic::from_file(authfile)
            .await
            .expect("File should load");
        assert!(
            basic.check_credentials("admin".to_owned(), "sw0rdf1sh".to_owned()),
            "The password is always swordfish"
        );

        assert!(
            !basic.check_credentials("nope".to_owned(), "password".to_owned()),
            "should fail on nonexistent user"
        );
        assert!(
            !basic.check_credentials("admin".to_owned(), "swordfish".to_owned()),
            "The password is not swordfish"
        );
    }
}
