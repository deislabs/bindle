//! Contains the main invoice object definition, its implementation, and all related subobject (such
//! as `Parcel`s and `Label`s)

mod api;
mod bindle_spec;
mod condition;
mod group;
mod label;
mod parcel;
mod sealed;
pub mod signature;
pub mod verification;

#[doc(inline)]
pub use api::{ErrorResponse, InvoiceCreateResponse, MissingParcelsResponse, QueryOptions};
#[doc(inline)]
pub use bindle_spec::BindleSpec;
#[doc(inline)]
pub use condition::Condition;
#[doc(inline)]
pub use group::Group;
#[doc(inline)]
pub use label::Label;
#[doc(inline)]
pub use parcel::Parcel;
#[doc(inline)]
pub use signature::{SecretKeyEntry, Signature, SignatureError, SignatureRole};
#[doc(inline)]
pub use verification::VerificationStrategy;

use ed25519_dalek::{Signature as EdSignature, Signer};
use semver::{Compat, Version, VersionReq};
use serde::{Deserialize, Serialize};
use tracing::info;

use std::borrow::{Borrow, BorrowMut};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::time::{SystemTime, UNIX_EPOCH};

use self::verification::Verified;
use crate::BINDLE_VERSION_1;

/// Alias for feature map in an Invoice's parcel
pub type FeatureMap = BTreeMap<String, BTreeMap<String, String>>;

/// Alias for annotations map
pub type AnnotationMap = BTreeMap<String, String>;

/// A sealed trait used to mark that an invoice has been signed. This trait cannot be implemented by
/// consumers of the bindle crate
pub trait Signed {
    /// Consumes the object, returning the signed invoice
    fn signed(self) -> Invoice;
}

/// The main structure for a Bindle invoice.
///
/// The invoice describes a specific version of a bindle. For example, the bindle
/// `foo/bar/1.0.0` would be represented as an Invoice with the `BindleSpec` name
/// set to `foo/bar` and version set to `1.0.0`.
///
/// Most fields on this struct are singular to best represent the specification. There,
/// fields like `group` and `parcel` are singular due to the conventions of TOML.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Invoice {
    pub bindle_version: String,
    pub yanked: Option<bool>,
    pub yanked_signature: Option<Vec<Signature>>,
    pub bindle: BindleSpec,
    pub annotations: Option<AnnotationMap>,
    pub parcel: Option<Vec<Parcel>>,
    pub group: Option<Vec<Group>>,
    pub signature: Option<Vec<Signature>>,
}

impl Invoice {
    /// Create a new Invoice with a bindle specification.
    ///
    /// The returned bindle will have no parcels, annotations, signatures, or groups.
    pub fn new(spec: BindleSpec) -> Self {
        Invoice {
            bindle_version: BINDLE_VERSION_1.to_owned(),
            bindle: spec,
            parcel: None,
            yanked: None,
            yanked_signature: None,
            annotations: None,
            signature: None,
            group: None,
        }
    }

    /// produce a slash-delimited "invoice name"
    ///
    /// For example, an invoice with the bindle name "hello" and the bindle version
    /// "1.2.3" will produce "hello/1.2.3"
    pub fn name(&self) -> String {
        format!("{}/{}", self.bindle.id.name(), self.bindle.id.version())
    }

    /// Creates a standard name for an invoice
    ///
    /// This is designed to create a repeatable opaque name for the invoice
    /// We don't typically want to have a bindle ID using its name and version number. This
    /// would impose both naming constraints on the bindle and security issues on the
    /// storage layout. So this function hashes the name/version data (which together
    /// MUST be unique in the system) and uses the resulting hash as the canonical
    /// name. The hash is guaranteed to be in the character set [a-zA-Z0-9].
    pub fn canonical_name(&self) -> String {
        self.bindle.id.sha()
    }

    /// Compare a SemVer "requirement" string to the version on this bindle
    ///
    /// An empty range matches anything.
    ///
    /// A range that fails to parse matches nothing.
    ///
    /// An empty version matches nothing (unless the requirement is empty)
    ///
    /// A version that fails to parse matches nothing (unless the requirement is empty).
    ///
    /// In all other cases, if the version satisfies the requirement, this returns true.
    /// And if it fails to satisfy the requirement, this returns false.
    pub(crate) fn version_in_range(&self, requirement: &str) -> bool {
        version_compare(self.bindle.id.version(), requirement)
    }

    /// Check whether a group by this name is present.
    pub fn has_group(&self, name: &str) -> bool {
        let empty = Vec::with_capacity(0);
        self.group
            .as_ref()
            .unwrap_or(&empty)
            .iter()
            .any(|g| g.name == name)
    }

    /// Get all of the parcels on the given group.
    pub fn group_members(&self, name: &str) -> Vec<Parcel> {
        // If there is no such group, return early.
        if !self.has_group(name) {
            info!(name, "no such group");
            return vec![];
        }

        let zero_vec = Vec::with_capacity(0);
        self.parcel
            .as_ref()
            .unwrap_or(&zero_vec)
            .iter()
            .filter(|p| p.member_of(name))
            .cloned()
            .collect()
    }

    fn cleartext(&self, by: &str, role: &SignatureRole) -> String {
        let mut buf = vec![
            by.to_owned(),
            self.bindle.id.name().to_owned(),
            self.bindle.id.version_string(),
            role.to_string(),
            '~'.to_string(),
        ];

        // Add bindles
        if let Some(list) = self.parcel.as_ref() {
            list.iter().for_each(|p| {
                buf.push(p.label.sha256.clone());
            })
        }

        buf.join("\n")
    }

    /// Sign the parcels on the current package.
    ///
    /// Note that this signature will be invalidated if any parcels are
    /// added after this signature.
    ///
    /// In the current version of the spec, a signature is generated by combining the
    /// signer's ID, the invoice version, and a list of parcels, and then performing
    /// a cryptographic signature on those fields. The result is then stored in
    /// a `[[signature]]` block on the invoice. Multiple signatures can be attached
    /// to any invoice.
    pub fn sign(
        &mut self,
        signer_role: SignatureRole,
        keyfile: &SecretKeyEntry,
    ) -> Result<(), SignatureError> {
        let signer_name = keyfile.label.clone();
        let key = keyfile.key()?;
        // The spec says it is illegal for the a single key to sign the same invoice
        // more than once.
        let encoded_key = base64::encode(key.public.to_bytes());
        if let Some(sigs) = self.signature.as_ref() {
            for s in sigs {
                if s.key == encoded_key {
                    return Err(SignatureError::DuplicateSignature);
                }
            }
        }

        let cleartext = self.cleartext(&signer_name, &signer_role);
        let signature: EdSignature = key.sign(cleartext.as_bytes());

        // Timestamp should be generated at this moment.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SignatureError::SigningFailed)?;

        let signature_entry = Signature {
            by: signer_name,
            key: encoded_key,
            signature: base64::encode(signature.to_bytes()),
            role: signer_role,
            at: ts.as_secs(),
        };

        match self.signature.as_mut() {
            Some(signatures) => signatures.push(signature_entry),
            None => self.signature = Some(vec![signature_entry]),
        };

        Ok(())
    }
}

/// Sign the parcels in the invoice using the given list of roles and keys. This is a list of tuples
/// containing a [`SignatureRole`] and [`SecretKeyEntry`] in that order. Returns a [`SignedInvoice`]
///
/// Note that this signature will be invalidated if any parcels are added after this signature.
///
/// In the current version of the spec, a signature is generated by combining the signer's ID, the
/// invoice version, and a list of parcels, and then performing a cryptographic signature on those
/// fields. The result is then stored in a `[[signature]]` block on the invoice. Multiple signatures
/// can be attached to any invoice.
pub fn sign<I>(
    mut invoice: I,
    sign_with: Vec<(SignatureRole, &SecretKeyEntry)>,
) -> Result<SignedInvoice<I>, SignatureError>
where
    I: BorrowMut<Invoice> + Into<crate::Invoice>,
{
    let mut inv = invoice.borrow_mut();
    for (role, key) in sign_with {
        sign_one(&mut inv, role, key)?;
    }

    Ok(SignedInvoice(invoice))
}

fn sign_one(
    inv: &mut Invoice,
    signer_role: SignatureRole,
    keyfile: &SecretKeyEntry,
) -> Result<(), SignatureError> {
    let signer_name = keyfile.label.clone();
    let key = keyfile.key()?;
    // The spec says it is illegal for the a single key to sign the same invoice
    // more than once.
    let encoded_key = base64::encode(key.public.to_bytes());
    if let Some(sigs) = inv.signature.as_ref() {
        for s in sigs {
            if s.key == encoded_key {
                return Err(SignatureError::DuplicateSignature);
            }
        }
    }

    let cleartext = inv.cleartext(&signer_name, &signer_role);
    let signature: EdSignature = key.sign(cleartext.as_bytes());

    // Timestamp should be generated at this moment.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SignatureError::SigningFailed)?;

    let signature_entry = Signature {
        by: signer_name,
        key: encoded_key,
        signature: base64::encode(signature.to_bytes()),
        role: signer_role,
        at: ts.as_secs(),
    };

    match inv.signature.as_mut() {
        Some(signatures) => signatures.push(signature_entry),
        None => inv.signature = Some(vec![signature_entry]),
    };

    Ok(())
}

/// An invoice that has been signed and can no longer be modified unless converted back into a
/// normal invoice with the `signed` method
pub struct SignedInvoice<T: Into<Invoice>>(T);

impl<T: Into<Invoice>> Signed for SignedInvoice<T> {
    fn signed(self) -> Invoice {
        self.0.into()
    }
}

impl<T: Into<Invoice>> sealed::Sealed for SignedInvoice<T> {}

impl<T> Verified for SignedInvoice<T> where T: Verified + Into<Invoice> {}

impl<T> Borrow<Invoice> for SignedInvoice<T>
where
    T: Into<Invoice> + Borrow<Invoice>,
{
    fn borrow(&self) -> &Invoice {
        self.0.borrow()
    }
}

impl<T> Debug for SignedInvoice<T>
where
    T: Debug + Into<Invoice>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

/// A struct that fakes signing. Purely for internal usage with things such as caches and
/// passthrough implementations (like the client)
pub(crate) struct NoopSigned<T: Into<Invoice>>(pub(crate) T);

impl<T: Into<Invoice>> Signed for NoopSigned<T> {
    fn signed(self) -> Invoice {
        self.0.into()
    }
}

impl<T> Verified for NoopSigned<T> where T: Verified + Into<Invoice> {}

impl<T: Into<Invoice>> sealed::Sealed for NoopSigned<T> {}

impl<T> Borrow<Invoice> for NoopSigned<T>
where
    T: Into<Invoice> + Borrow<Invoice>,
{
    fn borrow(&self) -> &Invoice {
        self.0.borrow()
    }
}

impl<T> Debug for NoopSigned<T>
where
    T: Debug + Into<Invoice>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

/// Check whether the given version is within the legal range.
///
/// An empty range matches anything.
///
/// A range that fails to parse matches nothing.
///
/// An empty version matches nothing (unless the requirement is empty)
///
/// A version that fails to parse matches nothing (unless the requirement is empty).
///
/// In all other cases, if the version satisfies the requirement, this returns true.
/// And if it fails to satisfy the requirement, this returns false.
fn version_compare(version: &Version, requirement: &str) -> bool {
    if requirement.is_empty() {
        return true;
    }

    // Setting Compat::Npm follows the rules here:
    // https://www.npmjs.com/package/semver
    //
    // Most importantly, the requirement "1.2.3" is treated as "= 1.2.3".
    // Without the compat mode, "1.2.3" is treated as "^1.2.3".
    match VersionReq::parse_compat(requirement, Compat::Npm) {
        Ok(req) => {
            return req.matches(version);
        }
        Err(e) => {
            tracing::log::error!("SemVer range could not parse: {}", e);
        }
    }
    false
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::invoice::signature::{KeyEntry, KeyRing};
    use std::convert::TryInto;
    use std::fs::read;
    use std::path::Path;

    #[test]
    fn test_version_comparisons() {
        // Do not need an exhaustive list of matches -- just a sampling to make sure
        // the outer logic is correct.
        let reqs = vec!["= 1.2.3", "1.2.3", "1.2.3", "^1.1", "~1.2", ""];
        let version = Version::parse("1.2.3").unwrap();

        reqs.iter().for_each(|r| {
            if !version_compare(&version, r) {
                panic!("Should have passed: {}", r)
            }
        });

        // Again, we do not need to test the SemVer crate -- just make sure some
        // outliers and obvious cases are covered.
        let reqs = vec!["2", "%^&%^&%"];
        reqs.iter()
            .for_each(|r| assert!(!version_compare(&version, r)));
    }

    #[test]
    fn signing_and_verifying() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        
        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");

        // Base case: No signature, no keyring should pass.
        assert!(invoice.signature.is_none());
        let nokeys = KeyRing::default();
        let verified = VerificationStrategy::default()
            .verify(invoice, &nokeys)
            .expect("If no signature, then this should verify fine");

        // Create two signing keys.
        let signer_name1 = "Matt Butcher <matt@example.com>".to_owned();
        let signer_name2 = "Not Matt Butcher <not.matt@example.com>".to_owned();

        let keypair1 = SecretKeyEntry::new(signer_name1, vec![SignatureRole::Creator]);
        let keypair2 = SecretKeyEntry::new(signer_name2, vec![SignatureRole::Proxy]);

        // Put one of the two keys on the keyring. Since the proxy key is not used in
        // CreativeIntegrity, it can be omitted from keyring.
        let keyring = KeyRing::new(vec![(&keypair1).try_into().expect("convert to public key")]);

        // Add two signatures
        let signed = sign(
            verified,
            vec![
                (SignatureRole::Creator, &keypair1),
                (SignatureRole::Proxy, &keypair2),
            ],
        )
        .expect("Sign the parcels");

        let invoice: Invoice = signed.signed();

        // Should not be able to sign the same invoice again with the same key, even with a different role
        sign(invoice.clone(), vec![(SignatureRole::Host, &keypair2)])
            .expect_err("Should not be able to sign again with the same key");

        // There should be two signature blocks
        assert_eq!(2, invoice.signature.as_ref().unwrap().len());

        // With the keyring, the signature should work
        VerificationStrategy::CreativeIntegrity
            .verify(invoice.clone(), &keyring)
            .expect("with keys on the keyring, this should pass");

        // If we switch the keys in the keyring, this should fail because the Creator
        // key is not on the ring.
        let keyring = KeyRing::new(vec![keypair2.try_into().expect("convert to public key")]);
        VerificationStrategy::CreativeIntegrity
            .verify(invoice, &keyring)
            .expect_err("missing the creator key, so verification should fail");
    }
    #[test]
    fn invalid_signatures_should_fail() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[signature]]
        by = "Matt Butcher <matt@example.com>"
        signature = "T0JWSU9VU0xZIEZBS0UK" # echo "OBVIOUSLY FAKE" | base64
        key = "jTtZIzQCfZh8xy6st40xxLwxVw++cf0C0cMH3nJBF+c="
        role = "creator"
        at = 1611960337

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        
        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");

        // Parse the key from the above example, and put it into the keyring.
        let pubkey = KeyEntry {
            key: "jTtZIzQCfZh8xy6st40xxLwxVw++cf0C0cMH3nJBF+c=".to_owned(),
            label: "Test Key".to_owned(),
            roles: vec![SignatureRole::Host],
            label_signature: None,
        };

        // Set up a keyring
        let keyring = KeyRing::new(vec![pubkey]);

        match VerificationStrategy::default().verify(invoice, &keyring) {
            Err(SignatureError::CorruptSignature(s)) => {
                assert_eq!("jTtZIzQCfZh8xy6st40xxLwxVw++cf0C0cMH3nJBF+c=", s)
            }
            Err(e) => panic!("Unexpected error {:?}", e),
            Ok(_) => panic!("Verification should have failed"),
        }
    }

    #[test]
    fn invalid_key_should_fail() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[signature]]
        by = "Matt Butcher <matt@example.com>"
        signature = "x6sI2Qme4xf6IRtHGaoMqMRL0vjvVHLq3ZCaKVkHNr3oCw+kvTrxek7RbuajIgS71zUQew4/vVT4Do0xa49+CQ=="
        key = "T0JWSU9VU0xZIEZBS0UK" # echo "OBVIOUSLY FAKE" | base64
        role = "creator"
        at = 1611960337

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        
        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");

        // Set up a keyring
        let keyring = KeyRing::new(vec![KeyEntry {
            key: "jTtZIzQCfZh8xy6st40xxLwxVw++cf0C0cMH3nJBF+c=".to_owned(),
            label: "Test Key".to_owned(),
            roles: vec![SignatureRole::Creator],
            label_signature: None,
        }]);

        match VerificationStrategy::default().verify(invoice, &keyring) {
            Err(SignatureError::CorruptKey(s)) => assert_eq!("T0JWSU9VU0xZIEZBS0UK", s),
            Err(e) => panic!("Unexpected error {:?}", e),
            Ok(_) => panic!("Verification should have failed"),
        }
    }

    #[test]
    fn test_invoice_should_serialize() {
        let label = Label {
            sha256: "abcdef1234567890987654321".to_owned(),
            media_type: "text/toml".to_owned(),
            name: "foo.toml".to_owned(),
            size: 101,
            annotations: None,
            feature: None,
            origin: None,
        };
        let parcel = Parcel {
            label,
            conditions: None,
        };
        let parcels = Some(vec![parcel]);
        let inv = Invoice {
            bindle_version: crate::BINDLE_VERSION_1.to_owned(),
            bindle: BindleSpec {
                id: "foo/1.2.3".parse().unwrap(),
                description: Some("bar".to_owned()),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcel: parcels,
            yanked: None,
            yanked_signature: None,
            annotations: None,
            group: None,
            signature: None,
        };

        let res = toml::to_string(&inv).unwrap();
        let inv2 = toml::from_str::<Invoice>(res.as_str()).unwrap();

        let b = inv2.bindle;
        assert_eq!(b.id.name(), "foo".to_owned());
        assert_eq!(b.id.version_string(), "1.2.3");
        assert_eq!(b.description.unwrap().as_str(), "bar");
        assert_eq!(b.authors.unwrap()[0], "m butcher".to_owned());

        let parcels = inv2.parcel.unwrap();

        assert_eq!(parcels.len(), 1);

        let par = &parcels[0];
        let lab = &par.label;
        assert_eq!(lab.name, "foo.toml".to_owned());
        assert_eq!(lab.media_type, "text/toml".to_owned());
        assert_eq!(lab.sha256, "abcdef1234567890987654321".to_owned());
        assert_eq!(lab.size, 101);
    }

    #[test]
    fn test_examples_in_spec_parse() {
        let test_files = vec![
            "test/data/simple-invoice.toml",
            "test/data/full-invoice.toml",
            "test/data/alt-format-invoice.toml",
        ];
        test_files.iter().for_each(|file| test_parsing_a_file(file));
    }

    fn test_parsing_a_file(filename: &str) {
        let invoice_path = Path::new(filename);
        let raw = read(invoice_path).expect("read file contents");

        let invoice = toml::from_slice::<Invoice>(&raw).expect("clean parse of invoice");

        // Now we serialize it and compare it to the original version
        let _raw2 = toml::to_string_pretty(&invoice).expect("clean serialization of TOML");
        // FIXME: Do we care about this detail?
        //assert_eq!(raw, raw2);
    }

    #[test]
    fn parcel_no_groups() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[group]]
        name = "images"

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        [parcel.conditions]
        memberOf = ["telescopes"]

        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");
        let parcels = invoice.parcel.expect("expected some parcels");

        let img = &parcels[0];
        let txt = &parcels[1];

        assert!(img.member_of("telescopes"));
        assert!(!img.is_global_group());

        assert!(txt.is_global_group());
        assert!(!txt.member_of("telescopes"));
    }

    #[test]
    fn test_group_members() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "aricebo"
        version = "1.2.3"

        [[group]]
        name = "telescopes"

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeefff"
        name = "telescope.gif"
        mediaType = "image/gif"
        size = 123_456
        [parcel.conditions]
        memberOf = ["telescopes"]

        [[parcel]]
        [parcel.label]
        sha256 = "aaabbbcccdddeeeggg"
        name = "telescope2.gif"
        mediaType = "image/gif"
        size = 123_456
        [parcel.conditions]
        memberOf = ["telescopes"]

        [[parcel]]
        [parcel.label]
        sha256 = "111aaabbbcccdddeee"
        name = "telescope.txt"
        mediaType = "text/plain"
        size = 123_456
        "#;

        let invoice: crate::Invoice = toml::from_str(invoice).expect("a nice clean parse");
        let members = invoice.group_members("telescopes");
        assert_eq!(2, members.len());
    }
}
