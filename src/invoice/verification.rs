use super::signature::KeyRing;
use super::{Invoice, Signature, SignatureError, SignatureRole};
use ed25519_dalek::{PublicKey, Signature as EdSignature};
use tracing::{debug, info};

use std::convert::TryInto;

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

/// This enumerates the verifications strategies described in the signing spec.
#[derive(Debug)]
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

/// A strategy for verifying an invoice.
impl VerificationStrategy {
    fn verify_signature(&self, sig: &Signature, cleartext: &[u8]) -> Result<(), SignatureError> {
        let pk = base64::decode(sig.key.as_bytes())
            .map_err(|_| SignatureError::CorruptKey(sig.key.clone()))?;
        let sig_block = base64::decode(sig.signature.as_bytes())
            .map_err(|_| SignatureError::CorruptSignature(sig.key.clone()))?;

        let pubkey =
            PublicKey::from_bytes(&pk).map_err(|_| SignatureError::CorruptKey(sig.key.clone()))?;
        let ed_sig = EdSignature::new(
            sig_block
                .as_slice()
                .try_into()
                .map_err(|_| SignatureError::CorruptSignature(sig.key.clone()))?,
        );
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
    pub fn verify(&self, inv: &Invoice, keyring: &KeyRing) -> Result<(), SignatureError> {
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
                Ok(())
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
                    self.verify_signature(&s, cleartext.as_bytes())?;
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
                        if !filled_roles.contains(&should_role) {
                            return Err(SignatureError::Unverified(format!(
                                "No signature found for role {:?}",
                                should_role,
                            )));
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::invoice::*;

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
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: Requires creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: Requires creator or approver");
            VerificationStrategy::GreedyVerification
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(&inv, &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(&inv, &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(&inv, &keyring)
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
                .verify(&inv, &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(&inv, &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(&inv, &keyring)
                .expect("inv should pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(&inv, &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(&inv, &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(&inv, &keyring)
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
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: not signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(&inv, &keyring)
                .expect("inv should pass: Signed by approver");
            VerificationStrategy::GreedyVerification
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(&inv, &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(&inv, &keyring)
            .expect_err("inv should not pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(&inv, &keyring)
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
                .verify(&inv, &keyring)
                .expect("inv should pass: signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(&inv, &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(&inv, &keyring)
                .expect("inv should pass: Requires creator and all valid");
            VerificationStrategy::MultipleAttestationGreedy(vec![SignatureRole::Host])
                .verify(&inv, &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Proxy,
            ])
            .verify(&inv, &keyring)
            .expect("inv should pass: Requires proxy");

            VerificationStrategy::ExhaustiveVerification
                .verify(&inv, &keyring)
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
                .verify(&inv, &keyring)
                .expect("inv should pass: signed by creator");
            VerificationStrategy::AuthoritativeIntegrity
                .verify(&inv, &keyring)
                .expect("inv should pass: Signed by creator");
            VerificationStrategy::GreedyVerification
                .verify(&inv, &keyring)
                .expect_err(
                    "inv should not pass: Requires creator and all known, anon is not known",
                );
            VerificationStrategy::MultipleAttestation(vec![SignatureRole::Host])
                .verify(&inv, &keyring)
                .expect("inv should pass: Only requires host");
            VerificationStrategy::MultipleAttestationGreedy(vec![
                SignatureRole::Host,
                SignatureRole::Approver,
            ])
            .verify(&inv, &keyring)
            .expect_err("inv should not pass: Requires approver to be known");

            VerificationStrategy::ExhaustiveVerification
                .verify(&inv, &keyring)
                .expect_err("inv should not pass: Requires that all signatures must be verified");
        }
    }
}
