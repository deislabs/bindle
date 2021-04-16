//! Contains the Signature type along with associated types and Roles

pub use ed25519_dalek::{Keypair, PublicKey, Signature as EdSignature, Signer};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::convert::TryInto;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::PathBuf;

/// The latest key ring version supported by this library.
pub const KEY_RING_VERSION: &str = "1.0";

/// A signature describes a cryptographic signature of the parcel list.
///
/// In the current implementation, a signature signs the list of parcels that belong on
/// an invoice. The signature, in the current implementation, is an Ed25519 signature
/// and is signed by the private counterpart of the given public key.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Signature {
    // The cleartext name of the user who signed
    pub by: String,
    // The signature block, encoded as hex chars
    pub signature: String,
    // The public key, encoded as hex chars
    pub key: String,
    // The role of the signer
    pub role: SignatureRole,
    // The UNIX timestamp, expressed as an unsigned 64-bit integer
    pub at: u64,
}

/// Wrap errors related to signing
///
/// These errors are designed to tell what failed and how, but not necessarily why.
/// This is to avoid leaking sensitive data back to a user agent.
/// Where possible, the error is linked to the signing key that failed. That key can
/// be cross-referenced with the invoice to determine which block failed or which key
/// is not correctly represented in the keyring.
#[derive(Error, Debug)]
pub enum SignatureError {
    #[error("signatures `{0}` cannot be verified")]
    Unverified(String),
    #[error("failed to sign the invoice with the given key")]
    SigningFailed,
    #[error("key is corrupt for `{0}`")]
    CorruptKey(String),
    #[error("signature block is corrupt for key {0}")]
    CorruptSignature(String),
    #[error("unknown signing key {0}")]
    UnknownSigningKey(String),
    #[error("none of the signatures are made with a known key")]
    NoKnownKey,
    #[error("cannot sign the data again with a key that has already signed the data")]
    DuplicateSignature,
}

/// The role of a signer in a signature block.
///
/// Signatories on a signature must have an associated role, as defined in the
/// specification.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SignatureRole {
    Creator,
    Proxy,
    Host,
    Approver,
}

impl Display for SignatureRole {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(
            f,
            "{}",
            match self {
                Self::Creator => "creator",
                Self::Proxy => "proxy",
                Self::Host => "host",
                Self::Approver => "approver",
            }
        )
    }
}

/// A KeyRing contains a list of public keys.
///
/// The purpose of this keyring is to validate signatures. The keyring NEVER
/// contains private keys.
///
/// The concepts are described in the signing-spec.md document for Bindle.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KeyRing {
    pub version: String,
    pub key: Vec<KeyEntry>,
}

impl Default for KeyRing {
    fn default() -> Self {
        Self {
            version: KEY_RING_VERSION.to_owned(),
            key: vec![],
        }
    }
}

/// A KeyEntry describes an entry on a keyring.
///
/// An entry has a key, an identifying label, a list of roles, and an optional signature of this data.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KeyEntry {
    /// The human-friendly name of the key.
    ///
    /// This has no particular security importance. It is just for convenience and
    /// can be changed by the user who owns this keyring.
    pub label: String,
    /// The list of roles, where a role is one of the signature roles.
    pub roles: Vec<SignatureRole>,
    /// The public key, encoded as the platform dictates.
    ///
    /// As of this writing, a key is a base64-encoded Ed25519 public key.
    pub key: String,
    /// A signed version of the label
    ///
    /// The specification provides an optional field for signing the label with a known
    /// private key as a way of protecting labels against tampering.
    pub label_signature: Option<String>,
}

impl KeyEntry {
    pub fn sign_label(&mut self, key: Keypair) {
        let sig = key.sign(self.label.as_bytes());
        self.label_signature = Some(base64::encode(sig.to_bytes()));
    }
    pub fn verify_label(self, key: PublicKey) -> anyhow::Result<()> {
        match self.label_signature {
            None => {
                tracing::log::info!("Label was not signed. Skipping.");
                Ok(())
            }
            Some(txt) => {
                let decoded_txt = base64::decode(txt)?;
                let sig = EdSignature::new(decoded_txt.as_slice().try_into()?);
                key.verify_strict(self.label.as_bytes(), &sig)?;
                Ok(())
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyEntry {
    /// A label for this key.
    ///
    /// This is intended for human consumption
    pub label: String,
    /// Base64-encoded Ed25519 key
    pub keypair: String,
    /// The roles this key should be used for.
    /// The default should be SignatureRole::Creator
    pub roles: Vec<SignatureRole>,
}

impl SecretKeyEntry {
    pub fn new(label: String, roles: Vec<SignatureRole>) -> Self {
        let mut rng = rand::rngs::OsRng {};
        let rawkey = Keypair::generate(&mut rng);
        let keypair = base64::encode(rawkey.to_bytes());
        Self {
            label,
            keypair,
            roles,
        }
    }

    pub fn key(&self) -> Result<Keypair, SignatureError> {
        let rawbytes = base64::decode(&self.keypair).map_err(|_e| {
            // We swallow the source error because it could disclose information about
            // the secret key.
            SignatureError::CorruptKey("Base64 decoding of the keypair failed".to_owned())
        })?;
        let keypair = Keypair::from_bytes(&rawbytes).map_err(|e| {
            tracing::log::error!("Error loading key: {}", e);
            // Don't leak information about the key, because this could be sent to
            // a remote. A generic error is all the user should see.
            SignatureError::CorruptKey("Could not load keypair".to_owned())
        })?;
        Ok(keypair)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyFile {
    pub version: String,
    pub key: Vec<SecretKeyEntry>,
}

impl Default for SecretKeyFile {
    fn default() -> Self {
        Self {
            version: KEY_RING_VERSION.to_owned(),
            key: vec![],
        }
    }
}

impl SecretKeyFile {
    pub async fn load_file(path: PathBuf) -> anyhow::Result<SecretKeyFile> {
        let s = tokio::fs::read_to_string(path).await?;
        let t = toml::from_str(s.as_str())?;
        Ok(t)
    }

    /// Save the present keyfile to the named path.
    pub async fn save_file(&self, dest: PathBuf) -> anyhow::Result<()> {
        let out = toml::to_vec(self)?;
        tokio::fs::write(dest, out).await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ed25519_dalek::Keypair;

    #[test]
    fn test_sign_label() {
        let mut rng = rand::rngs::OsRng {};
        let keypair = Keypair::generate(&mut rng);

        let mut ke = KeyEntry {
            label: "Matt Butcher <matt@example.com>".to_owned(),
            key: "jTtZIzQCfZh8xy6st40xxLwxVw++cf0C0cMH3nJBF+c=".to_owned(),
            roles: vec![SignatureRole::Host],
            label_signature: None,
        };

        let pubkey = keypair.public;
        ke.sign_label(keypair);

        assert!(ke.label_signature.is_some());

        ke.verify_label(pubkey).expect("verification failed");
    }

    #[tokio::test]
    async fn test_secret_keys() {
        let mut kr = SecretKeyFile::default();
        assert_eq!(kr.key.len(), 0);
        kr.key.push(SecretKeyEntry::new(
            "test".to_owned(),
            vec![SignatureRole::Proxy],
        ));
        assert_eq!(kr.key.len(), 1);

        let outdir = tempfile::tempdir().expect("created a temp dir");
        let dest = outdir.path().join("testkey.toml");

        kr.save_file(dest.clone())
            .await
            .expect("Should write new key to file");
        let newfile = SecretKeyFile::load_file(dest)
            .await
            .expect("Should load key from file");
        assert_eq!(newfile.key.len(), 1);
    }
}
