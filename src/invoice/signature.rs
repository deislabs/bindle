//! Contains the Signature type along with associated types and Roles

use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::fmt::{Display, Formatter, Result as FmtResult};

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
            }
        )
    }
}
