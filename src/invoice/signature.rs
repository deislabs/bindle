//! Contains the Signature type along with associated types and Roles

use ed25519_dalek::{Keypair, PublicKey, Signature as EdSignature, Signer};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::convert::TryInto;
use std::fmt::{Display, Formatter, Result as FmtResult};

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
}

/// The role of a signer in a signature block.
///
/// Signatories on a signature must have an associated role, as defined in the
/// specification.
#[derive(Serialize, Deserialize, Debug, Clone)]
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
                log::info!("Label was not signed. Skipping.");
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
}
