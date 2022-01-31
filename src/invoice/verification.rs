use crate::invoice::Signed;

use super::signature::KeyRing;
use super::{Invoice, Signature, SignatureError, SignatureRole};
use ed25519_dalek::{PublicKey, Signature as EdSignature};
use tracing::{debug, info};

use std::borrow::{Borrow, BorrowMut};
use std::fmt::Debug;
use std::str::FromStr;

const GREEDY_VERIFICATION_ROLES: &[SignatureRole] = &[SignatureRole::Creator];
const CREATIVE_INTEGITY_ROLES: &[SignatureRole] = &[SignatureRole::Creator];
const AUTHORITATIVE_INTEGRITY_ROLES: &[SignatureRole] =
    &[SignatureRole::Creator, SignatureRole::Approver];
const EXHAUSTIVE_VERIFICATION_ROLES: &[SignatureRole] = &[
    SignatureRole::Creator,
    SignatureRole::Approver,
    SignatureRole::Host,
    SignatureRole::Proxy,
];

/// A marker trait indicating that an invoice has been verified
pub trait Verified: super::sealed::Sealed {}

/// This enumerates the verifications strategies described in the signing spec.
#[derive(Debug, Clone)]
pub enum VerificationStrategy {
    /// CreativeIntegrity verifies that (a) the key that signs as Creator is a known key,
    /// and that the signature is valid.
    CreativeIntegrity,
    /// AuthoritativeIntegrity verifies that at least one of the Creator or Approver keys
    /// is known and the signature is valid.
    AuthoritativeIntegrity,
    /// Verify that the Creator key is known and that all signatures are valid.
    ///
    /// This is subject to a DOS attack if a signer can generate intentionally bad signatures.
    GreedyVerification,
    /// Verify that every key on the invoice is known, and that every signature is valid.
    ExhaustiveVerification,
    /// Verifies that all signatures of the given roles are valid and signed by known keys.
    MultipleAttestation(Vec<SignatureRole>),
    /// Verifies that all signatures of the given roles are valid and signed by known keys. Will
    /// also validate unknown signers similar to GreedyVerification
    ///
    /// If the bool is true, unknown signers will also be validated. Be aware that doing so may make
    /// the validation subject to a special form of DOS attack in which someone can generate a
    /// known-bad signature.
    MultipleAttestationGreedy(Vec<SignatureRole>),
}

impl Default for VerificationStrategy {
    fn default() -> Self {
        VerificationStrategy::GreedyVerification
    }
}

/// This implementation will parse the strategy from a string. MultipleAttestation strategies should
/// be of the format `MultipleAttestation[Creator, Approver]`
impl FromStr for VerificationStrategy {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err("Cannot parse VerificationStrategy from an empty string");
        }
        // We are very flexible here, allowing for any casing and trimming leading or trailing
        // whitespace
        let normalized = s.trim().to_lowercase();
        let parts: Vec<&str> = normalized.splitn(2, '[').collect();
        // Safety: Shouldn't panic here because we checked for an empty string _and_ splitn on an
        // empty string still will return a vec with a length of 1
        match parts[0] {
            "creativeintegrity" => Ok(Self::CreativeIntegrity),
            "authoritativeintegrity" => Ok(Self::AuthoritativeIntegrity),
            "greedyverification" => Ok(Self::GreedyVerification),
            "exhaustiveverification" => Ok(Self::ExhaustiveVerification),
            "multipleattestation" => Ok(Self::MultipleAttestation(parse_roles(parts.get(1))?)),
            "multipleattestationgreedy" => {
                Ok(Self::MultipleAttestationGreedy(parse_roles(parts.get(1))?))
            }
            _ => Err("Unknown verification strategy"),
        }
    }
}

/// Manual implementation of deserialize due to TOML not supporting "newtype" enum variants. This
/// deserializes using the same parsing rules as `FromStr`
impl<'de> serde::Deserialize<'de> for VerificationStrategy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(StrategyVisitor)
    }
}

struct StrategyVisitor;

impl<'de> serde::de::Visitor<'de> for StrategyVisitor {
    type Value = VerificationStrategy;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a valid verification strategy value")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match v.parse::<VerificationStrategy>() {
            Ok(s) => Ok(s),
            Err(e) => Err(E::custom(e)),
        }
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }
}

fn parse_roles(r: Option<&&str>) -> Result<Vec<SignatureRole>, &'static str> {
    let raw = r.ok_or("Multiple attestation strategy is missing roles")?;
    if !raw.ends_with(']') {
        return Err("Missing closing ']' on roles");
    }
    raw.trim_end_matches(']')
        .split(',')
        .map(|role| role.parse::<SignatureRole>())
        .collect::<Result<Vec<_>, _>>()
}

/// A strategy for verifying an invoice.
impl VerificationStrategy {
    fn verify_signature(&self, sig: &Signature, cleartext: &[u8]) -> Result<(), SignatureError> {
        let pk = base64::decode(sig.key.as_bytes())
            .map_err(|_| SignatureError::CorruptKey(sig.key.clone()))?;
        let sig_block = base64::decode(sig.signature.as_bytes())
            .map_err(|_| SignatureError::CorruptSignature(sig.key.clone()))?;

        let pubkey =
            PublicKey::from_bytes(&pk).map_err(|_| SignatureError::CorruptKey(sig.key.clone()))?;
        let ed_sig = EdSignature::try_from(sig_block.as_slice())
            .map_err(|_| SignatureError::CorruptSignature(sig.key.clone()))?;
        pubkey
            .verify_strict(cleartext, &ed_sig)
            .map_err(|_| SignatureError::Unverified(sig.key.clone()))
    }
    /// Verify that every signature on this invoice is correct.
    ///
    /// The verification strategy will determine how this verification is performed.
    /// Depending on the selected strategy, the `[[signature]]` blocks will be evaluated
    /// for the following:
    ///
    /// - Is the key in the keyring?
    /// - Can the signature be verified?
    ///
    /// Note that the purpose of the keyring is to ensure that we know about the
    /// entity that claims to have signed the invoice.
    ///
    /// If no signatures are on the invoice, this will succeed.
    ///
    /// A strategy will determine success or failure based on whether the signature is verified,
    /// whether the keys are known, whether the requisite number/roles are satisfied, and
    /// so on.
    pub fn verify<I>(
        &self,
        invoice: I,
        keyring: &KeyRing,
    ) -> Result<VerifiedInvoice<I>, SignatureError>
    where
        I: Borrow<Invoice> + Into<Invoice>,
    {
        let inv = invoice.borrow();
        let (roles, all_valid, all_verified, all_roles) = match self {
            VerificationStrategy::GreedyVerification => {
                (GREEDY_VERIFICATION_ROLES, true, true, true)
            }
            VerificationStrategy::CreativeIntegrity => (CREATIVE_INTEGITY_ROLES, false, true, true),
            VerificationStrategy::AuthoritativeIntegrity => {
                (AUTHORITATIVE_INTEGRITY_ROLES, false, false, false)
            }
            VerificationStrategy::ExhaustiveVerification => {
                (EXHAUSTIVE_VERIFICATION_ROLES, true, true, false)
            }
            VerificationStrategy::MultipleAttestation(a) => (a.as_slice(), false, true, true),
            VerificationStrategy::MultipleAttestationGreedy(a) => (a.as_slice(), true, true, true),
        };

        // Either the Creator or an Approver must be in the keyring
        match inv.signature.as_ref() {
            None => {
                info!(id = %inv.bindle.id, "No signatures on invoice");
                Ok(VerifiedInvoice(invoice))
            }
            Some(signatures) => {
                let mut known_key = false;
                let mut filled_roles: Vec<SignatureRole> = vec![];
                for s in signatures {
                    debug!(by = %s.by, "Checking signature");
                    let target_role = roles.contains(&s.role);

                    // If we're not validating all, and this role isn't one we're interested in,
                    // skip it.
                    if !all_valid && !target_role {
                        debug!("Not a target role, and not running all_valid");
                        continue;
                    }

                    let role = s.role.clone();
                    let cleartext = inv.cleartext(&s.by, &role);

                    // Verify the signature
                    // TODO: This would allow a trivial DOS attack in which an attacker
                    // would only need to attach a known-bad signature, and that would
                    // prevent the module from ever being usable. This is marginally
                    // better if we only verify signatures on known keys.
                    self.verify_signature(s, cleartext.as_bytes())?;
                    debug!("Signature verified");

                    if !target_role && !all_verified {
                        debug!("Not a target role, not checking for verification");
                        continue;
                    } else if all_roles {
                        filled_roles.push(role);
                    }
                    // See if the public key is known to us
                    let pubkey = base64::decode(&s.key)
                        .map_err(|_| SignatureError::CorruptKey(s.key.to_string()))?;
                    let pko = PublicKey::from_bytes(pubkey.as_slice())
                        .map_err(|_| SignatureError::CorruptKey(s.key.to_string()))?;

                    debug!("Looking for key");
                    // If the keyring contains PKO, then we are successful for this round.
                    if keyring.contains(&pko) {
                        debug!("Found key {}", s.by);
                        known_key = true;
                    } else if all_verified {
                        // If the keyring does not contain pko AND every key must be known,
                        // then we bail on error early.
                        return Err(SignatureError::Unverified(
                            "strategy requires that all signatures for role(s) must be verified"
                                .to_owned(),
                        ));
                    }
                }
                if !known_key {
                    debug!("No known key");
                    // If we get here, then the none of the signatures were created with
                    // a key from the keyring. This means the package is untrusted.
                    return Err(SignatureError::NoKnownKey);
                }
                // If we are supposed to make sure that all are valid, then we need to make sure
                // that at least one of each requested role is present.
                if all_roles {
                    for should_role in roles {
                        if !filled_roles.contains(should_role) {
                            return Err(SignatureError::Unverified(format!(
                                "No signature found for role {:?}",
                                should_role,
                            )));
                        }
                    }
                }
                Ok(VerifiedInvoice(invoice))
            }
        }
    }
}

/// An invoice whose signatures have been verified. Can be converted borrowed as a plain [`Invoice`]
pub struct VerifiedInvoice<T: Into<crate::Invoice>>(T);

impl<T: Into<crate::Invoice>> Verified for VerifiedInvoice<T> {}

impl<T: Into<crate::Invoice>> super::sealed::Sealed for VerifiedInvoice<T> {}

impl<T> Signed for VerifiedInvoice<T>
where
    T: Signed + Into<crate::Invoice>,
{
    fn signed(self) -> crate::Invoice {
        self.0.signed()
    }
}

impl<T> Borrow<crate::Invoice> for VerifiedInvoice<T>
where
    T: Into<crate::Invoice> + Borrow<crate::Invoice>,
{
    fn borrow(&self) -> &crate::Invoice {
        self.0.borrow()
    }
}

impl<T> BorrowMut<crate::Invoice> for VerifiedInvoice<T>
where
    T: Into<crate::Invoice> + BorrowMut<crate::Invoice>,
{
    fn borrow_mut(&mut self) -> &mut crate::Invoice {
        self.0.borrow_mut()
    }
}

/// An internal only type that implementes `Verified` for use in caches and other passthroughs
pub(crate) struct NoopVerified<T: Into<crate::Invoice>>(pub(crate) T);

impl<T: Into<crate::Invoice>> Verified for NoopVerified<T> {}

impl<T: Into<crate::Invoice>> super::sealed::Sealed for NoopVerified<T> {}

impl<T> Signed for NoopVerified<T>
where
    T: Signed + Into<crate::Invoice>,
{
    fn signed(self) -> crate::Invoice {
        self.0.signed()
    }
}

// Need the into implementation so that it can be passed into NoopSigned
#[allow(clippy::from_over_into)]
impl<T: Into<crate::Invoice>> Into<crate::Invoice> for NoopVerified<T> {
    fn into(self) -> crate::Invoice {
        self.0.into()
    }
}

// Only implementing this in one direction (e.g. no `From`) because we only want to be able to
// convert into an invoice
#[allow(clippy::from_over_into)]
impl<T: Into<crate::Invoice>> Into<crate::Invoice> for VerifiedInvoice<T> {
    fn into(self) -> crate::Invoice {
        self.0.into()
    }
}

impl<T> Debug for VerifiedInvoice<T>
where
    T: Debug + Into<crate::Invoice>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::invoice::*;
    use std::convert::TryInto;

    #[test]
    fn test_parse_verification_strategies() {
        // Happy path
        let strat = "CreativeIntegrity"
            .parse::<VerificationStrategy>()
            .expect("should parse");
        assert!(
            matches!(strat, VerificationStrategy::CreativeIntegrity),
            "Should parse to the correct type"
        );
        let strat = "AuthoritativeIntegrity"
            .parse::<VerificationStrategy>()
            .expect("should parse");
        assert!(
            matches!(strat, VerificationStrategy::AuthoritativeIntegrity),
            "Should parse to the correct type"
        );
        let strat = "GreedyVerification"
            .parse::<VerificationStrategy>()
            .expect("should parse");
        assert!(
            matches!(strat, VerificationStrategy::GreedyVerification),
            "Should parse to the correct type"
        );
        let strat = "ExhaustiveVerification"
            .parse::<VerificationStrategy>()
            .expect("should parse");
        assert!(
            matches!(strat, VerificationStrategy::ExhaustiveVerification),
            "Should parse to the correct type"
        );
        let strat = "MultipleAttestation[Creator, Host, Approver]"
            .parse::<VerificationStrategy>()
            .expect("should parse");
        assert!(
            matches!(strat, VerificationStrategy::MultipleAttestation(_)),
            "Should parse to the correct type"
        );
        let strat = "MultipleAttestationGreedy[Creator, Host, Approver]"
            .parse::<VerificationStrategy>()
            .expect("should parse");

        // Validate everything parsed to the right type as a sanity check
        match strat {
            VerificationStrategy::MultipleAttestationGreedy(roles) => {
                assert!(
                    roles.contains(&SignatureRole::Creator),
                    "Roles should contain creator"
                );
                assert!(
                    roles.contains(&SignatureRole::Host),
                    "Roles should contain host"
                );
                assert!(
                    roles.contains(&SignatureRole::Approver),
                    "Roles should contain approver"
                );
            }
            _ => panic!("Wrong type returned"),
        }

        // Odd formatting
        let strat = "CrEaTiVeInTeGrItY"
            .parse::<VerificationStrategy>()
            .expect("mixed case should parse");
        assert!(
            matches!(strat, VerificationStrategy::CreativeIntegrity),
            "Should parse to the correct type"
        );
        let strat = "  multipleAttestAtion[Creator, Host, Approver] "
            .parse::<VerificationStrategy>()
            .expect("extra spaces should parse");
        assert!(
            matches!(strat, VerificationStrategy::MultipleAttestation(_)),
            "Should parse to the correct type"
        );

        // Unhappy path
        "nopenopenope"
            .parse::<VerificationStrategy>()
            .expect_err("non-existent strategy shouldn't parse");
        "Creative Integrity"
            .parse::<VerificationStrategy>()
            .expect_err("spacing in the middle shouldn't parse");
        "MultipleAttestationCreator, Host, Approver]"
            .parse::<VerificationStrategy>()
            .expect_err("missing start brace shouldn't parse");
        "MultipleAttestation[Creator, Host, Approver"
            .parse::<VerificationStrategy>()
            .expect_err("missing end brace shouldn't parse");
        "MultipleAttestation[Blah, Host, Approver]"
            .parse::<VerificationStrategy>()
            .expect_err("Invalid role shouldn't parse");
    }

    #[test]
    fn test_strategy_deserialize() {
        #[derive(serde::Deserialize)]
        struct StrategyMock {
            verification_strategy: VerificationStrategy,
        }

        let toml_value = r#"
        verification_strategy = "MultipleAttestation[Creator, Host, Approver]"
        "#;

        let mock: StrategyMock = toml::from_str(toml_value).expect("toml should parse");

        assert!(
            matches!(
                mock.verification_strategy,
                VerificationStrategy::MultipleAttestation(_)
            ),
            "Should parse to the correct type"
        );

        // Sanity check: A non-array variant

        let toml_value = r#"
        verification_strategy = "CreativeIntegrity"
        "#;

        let mock: StrategyMock = toml::from_str(toml_value).expect("toml should parse");

        assert!(
            matches!(
                mock.verification_strategy,
                VerificationStrategy::CreativeIntegrity
            ),
            "Should parse to the correct type"
        );

        // Now check JSON
        let json_value = r#"
        {
            "verification_strategy": "MultipleAttestation[Creator, Host, Approver]"
        }
        "#;

        let mock: StrategyMock = serde_json::from_str(json_value).expect("json should parse");

        assert!(
            matches!(
                mock.verification_strategy,
                VerificationStrategy::MultipleAttestation(_)
            ),
            "Should parse to the correct type"
        );
    }

    #[test]
    fn test_verification_strategies() {
        let invoice = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "arecebo"
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

        let key_creator =
            SecretKeyEntry::new("Test Creator".to_owned(), vec![SignatureRole::Creator]);
        let key_approver =
            SecretKeyEntry::new("Test Approver".to_owned(), vec![SignatureRole::Approver]);
        let key_host = SecretKeyEntry::new("Test Host".to_owned(), vec![SignatureRole::Host]);
        let key_proxy = SecretKeyEntry::new("Test Proxy".to_owned(), vec![SignatureRole::Proxy]);
        let keyring_keys = vec![
            key_approver.clone().try_into().expect("convert to pubkey"),
            key_host.clone().try_into().expect("convert to pubkey"),
            key_creator.clone().try_into().expect("convert to pubkey"),
            key_proxy.clone().try_into().expect("convert to pubkey"),
        ];
        let keyring = KeyRing::new(keyring_keys);

        // Only signed by host
        {
            let mut inv = invoice.clone();
            inv.sign(SignatureRole::Host, &key_host)
                .expect("signed as host");
            // This should fail
            VerificationStrategy::CreativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect_err("inv should not pass: Requires creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect_err("inv should not pass: Requires creator or approver");
            VerificationStrategy::GreedyVerification
                .verify(inv.clone(), &keyring)
                .expect_err("inv should not pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(inv.clone(), &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(inv, &keyring)
                .expect("inv should not pass: Requires that all signatures must be verified");
        }
        // Signed by creator and host
        {
            let mut inv = invoice.clone();
            inv.sign(SignatureRole::Host, &key_host)
                .expect("signed as host");
            inv.sign(SignatureRole::Creator, &key_creator)
                .expect("signed as creator");
            // This should fail
            VerificationStrategy::CreativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(inv.clone(), &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(inv, &keyring)
                .expect("inv should not pass: Requires that all signatures must be verified");
        }
        // Signed by approver and host
        {
            let mut inv = invoice.clone();
            inv.sign(SignatureRole::Host, &key_host)
                .expect("signed as host");
            inv.sign(SignatureRole::Approver, &key_approver)
                .expect("signed as approver");
            // This should fail
            VerificationStrategy::CreativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect_err("inv should not pass: not signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Signed by approver");
            VerificationStrategy::GreedyVerification
                .verify(inv.clone(), &keyring)
                .expect_err("inv should not pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(inv.clone(), &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(inv, &keyring)
                .expect("inv should not pass: Requires that all signatures must be verified");
        }
        // Signed by creator, proxy, and host
        {
            let mut inv = invoice.clone();
            inv.sign(SignatureRole::Host, &key_host)
                .expect("signed as host");
            inv.sign(SignatureRole::Creator, &key_creator)
                .expect("signed as creator");
            inv.sign(SignatureRole::Proxy, &key_proxy)
                .expect("signed as proxy");
            // This should fail
            VerificationStrategy::CreativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(inv.clone(), &keyring)
            .expect("inv should pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(inv, &keyring)
                .expect("inv should pass: Requires that all signatures must be verified");
        }
        println!("Signed by creator, host, and unknown key");
        {
            let mut inv = invoice;
            inv.sign(SignatureRole::Host, &key_host)
                .expect("signed as host");
            inv.sign(SignatureRole::Creator, &key_creator)
                .expect("signed as creator");

            // Mock an unknown key and don't add it to keyring
            let key_anon =
                SecretKeyEntry::new("Unknown key".to_owned(), vec![SignatureRole::Approver]);
            inv.sign(SignatureRole::Approver, &key_anon)
                .expect("signed with unknown key");

            // This should fail
            VerificationStrategy::CreativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(inv.clone(), &keyring)
                .expect_err(
                    "inv should not pass: Requires creator and all known, anon is not known",
                );
            VerificationStrategy::MultipleAttestation(vec![SignatureRole::Host])
                .verify(inv.clone(), &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Approver,
            ])
            .verify(inv.clone(), &keyring)
            .expect_err("inv should not pass: Requires approver to be known");

            VerificationStrategy::ExhaustiveVerification
                .verify(inv, &keyring)
                .expect_err("inv should not pass: Requires that all signatures must be verified");
        }
    }
}
