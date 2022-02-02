//! Contains the Signature type along with associated types and Roles

pub use ed25519_dalek::{Keypair, PublicKey, Signature as EdSignature, Signer};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tracing::error;

use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::Path;
use std::str::FromStr;

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
    #[error("no suitable key for signing data")]
    NoSuitableKey,
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

impl FromStr for SignatureRole {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "c" | "creator" => Ok(Self::Creator),
            "h" | "host" => Ok(Self::Host),
            "a" | "approver" => Ok(Self::Approver),
            "p" | "proxy" => Ok(Self::Proxy),
            _ => Err("Invalid SignatureRole, should be one of: Creator, Proxy, Host, Approver"),
        }
    }
}

// NOTE (thomastaylor312): These are basically glorified helper traits. We could theoretically
// define `KeyRing` as a set of traits and then implementors could only dynamically load what is
// necessary (as keychains could get large a big companies) and that could replace these helpers.
// This isn't an optimization we need now, but likely do in the future

/// Keyrings could be loaded from in any number of sources. This trait allows implementors to create
/// custom loader helpers for keyrings
#[async_trait::async_trait]
pub trait KeyRingLoader {
    /// Load the keyring from source, returning the KeyRing
    async fn load(&self) -> anyhow::Result<KeyRing>;
}

/// Keyrings could be saved to any number of sources. This trait allows implementors to create
/// custom saving helpers for keyrings
#[async_trait::async_trait]
pub trait KeyRingSaver {
    /// Save the keyring to the given source
    async fn save(&self, keyring: &KeyRing) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl<T: AsRef<Path> + Sync> KeyRingLoader for T {
    async fn load(&self) -> anyhow::Result<KeyRing> {
        let raw_data = tokio::fs::read(self).await.map_err(|e| {
            anyhow::anyhow!(
                "failed to read TOML file {}: {}",
                self.as_ref().display(),
                e
            )
        })?;
        let res: KeyRing = toml::from_slice(&raw_data)?;
        Ok(res)
    }
}

#[async_trait::async_trait]
impl<T: AsRef<Path> + Sync> KeyRingSaver for T {
    async fn save(&self, keyring: &KeyRing) -> anyhow::Result<()> {
        #[cfg(target_family = "unix")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true) // Overwrite all data
            .mode(0o600)
            .open(self)
            .await?;

        // TODO(thomastaylor312): Figure out what the proper permissions are on windows (probably
        // creator/owner with read/write permissions and everything else excluded) and figure out
        // how to set those
        #[cfg(target_family = "windows")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true) // Overwrite all data
            .open(self)
            .await?;

        file.write_all(&toml::to_vec(keyring)?).await?;
        Ok(())
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

impl KeyRing {
    pub fn new(keys: Vec<KeyEntry>) -> Self {
        KeyRing {
            version: KEY_RING_VERSION.to_owned(),
            key: keys,
        }
    }

    pub fn add_entry(&mut self, entry: KeyEntry) {
        self.key.push(entry)
    }

    pub fn contains(&self, key: &PublicKey) -> bool {
        // This could definitely be optimized.
        for k in self.key.iter() {
            // Note that we are skipping malformed keys because they don't matter
            // when doing a contains(). If they key is malformed, it definitely
            // is not the key we are looking for.
            match k.public_key() {
                Err(e) => tracing::warn!(%e, "Error parsing key"),
                Ok(pk) if pk == *key => return true,
                _ => {}
            }

            tracing::debug!("No match. Moving on.");
        }
        tracing::debug!("No more keys to check");
        false
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
    /// Create a new KeyEntry from a public key and related information.
    ///
    /// In most cases, it is fine to construct a KeyEntry struct manually. This
    /// constructor merely encapsulates the logic to store the public key in its
    /// canonical encoded format (as a String).
    pub fn new(label: &str, roles: Vec<SignatureRole>, public_key: PublicKey) -> Self {
        let key = base64::encode(public_key.to_bytes());
        KeyEntry {
            label: label.to_owned(),
            roles,
            key,
            label_signature: None,
        }
    }
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
                let sig = EdSignature::try_from(decoded_txt.as_slice())?;
                key.verify_strict(self.label.as_bytes(), &sig)?;
                Ok(())
            }
        }
    }
    pub(crate) fn public_key(&self) -> Result<PublicKey, SignatureError> {
        let rawbytes = base64::decode(&self.key).map_err(|_e| {
            // We swallow the source error because it could disclose information about
            // the secret key.
            SignatureError::CorruptKey("Base64 decoding of the public key failed".to_owned())
        })?;
        let pk = PublicKey::from_bytes(rawbytes.as_slice()).map_err(|e| {
            error!(%e, "Error loading public key");
            // Don't leak information about the key, because this could be sent to
            // a remote. A generic error is all the user should see.
            SignatureError::CorruptKey("Could not load keypair".to_owned())
        })?;
        Ok(pk)
    }
}

/// Convert a secret key to a public key.
impl TryFrom<SecretKeyEntry> for KeyEntry {
    type Error = SignatureError;
    fn try_from(secret: SecretKeyEntry) -> std::result::Result<Self, SignatureError> {
        let skey = secret.key()?;
        let mut s = Self {
            label: secret.label,
            roles: secret.roles,
            key: base64::encode(skey.public.to_bytes()),
            label_signature: None,
        };
        s.sign_label(skey);
        Ok(s)
    }
}

impl TryFrom<&SecretKeyEntry> for KeyEntry {
    type Error = SignatureError;
    fn try_from(secret: &SecretKeyEntry) -> std::result::Result<Self, SignatureError> {
        let skey = secret.key()?;
        let mut s = Self {
            label: secret.label.clone(),
            roles: secret.roles.clone(),
            key: base64::encode(skey.public.to_bytes()),
            label_signature: None,
        };
        s.sign_label(skey);
        Ok(s)
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

    pub(crate) fn key(&self) -> Result<Keypair, SignatureError> {
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

/// Storage for secret keys
///
/// Any possible number of key storage systems may be used for key storage, but
/// all of them must provide a way for the system to fetch a key matching the
/// desired role.
pub trait SecretKeyStorage {
    /// Get a key appropriate for signing with the given role.
    ///
    /// If no key is found, this will return a None.
    /// In general, if multiple keys match, the implementation chooses the "best fit"
    /// and returns that key.
    fn get_first_matching(&self, role: &SignatureRole) -> Option<&SecretKeyEntry>;
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
    pub async fn load_file(path: impl AsRef<Path>) -> anyhow::Result<SecretKeyFile> {
        let raw = tokio::fs::read(path).await?;
        let t = toml::from_slice(&raw)?;
        Ok(t)
    }

    /// Save the present keyfile to the named path.
    pub async fn save_file(&self, dest: impl AsRef<Path>) -> anyhow::Result<()> {
        let out = toml::to_vec(self)?;
        #[cfg(target_family = "unix")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .open(dest)
            .await?;

        // TODO(thomastaylor312): Figure out what the proper permissions are on windows (probably
        // creator/owner with read/write permissions and everything else excluded) and figure out
        // how to set those
        #[cfg(target_family = "windows")]
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(dest)
            .await?;

        file.write_all(&out).await?;
        file.flush().await?;
        Ok(())
    }
}

impl SecretKeyStorage for SecretKeyFile {
    fn get_first_matching(&self, role: &SignatureRole) -> Option<&SecretKeyEntry> {
        self.key.iter().find(|k| k.roles.contains(role))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ed25519_dalek::Keypair;

    #[test]
    fn test_parse_role() {
        // Happy path
        "Creator".parse::<SignatureRole>().expect("should parse");
        "Proxy".parse::<SignatureRole>().expect("should parse");
        "Host".parse::<SignatureRole>().expect("should parse");
        "Approver".parse::<SignatureRole>().expect("should parse");

        // Odd formatting
        "CrEaToR"
            .parse::<SignatureRole>()
            .expect("mixed case should parse");
        "  ProxY "
            .parse::<SignatureRole>()
            .expect("extra spacing should parse");

        // Unhappy path
        "yipyipyip"
            .parse::<SignatureRole>()
            .expect_err("non-existent shouldn't parse");
    }

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

        kr.save_file(&dest)
            .await
            .expect("Should write new key to file");
        #[cfg(target_family = "unix")]
        {
            use std::os::unix::fs::PermissionsExt;

            let metadata = tokio::fs::metadata(&dest).await.unwrap();
            // This masks out the bits we don't care about
            assert_eq!(
                metadata.permissions().mode() & 0o00600,
                0o600,
                "Permissions of saved key should be 0600"
            )
        }
        let newfile = SecretKeyFile::load_file(dest)
            .await
            .expect("Should load key from file");
        assert_eq!(newfile.key.len(), 1);
    }
}
