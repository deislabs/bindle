mod test_util;

use std::convert::TryInto;

use test_util::TestController;

use bindle::provider::Provider;
use bindle::proxy::Proxy;
use bindle::signature::{KeyRing, SecretKeyEntry, SignatureRole};
use bindle::testing;
use bindle::VerificationStrategy;

#[tokio::test]
async fn test_proxy_get_signing() {
    let controller = TestController::new().await;

    // Create an initial invoice in the server
    let scaffold = testing::Scaffold::load("valid_v1").await;
    let id = scaffold.invoice.bindle.id.clone();
    controller
        .client
        .create_invoice(scaffold.invoice)
        .await
        .expect("unable to create invoice");

    let key_proxy = SecretKeyEntry::new("Test Proxy".to_owned(), vec![SignatureRole::Proxy]);
    let pubkey = key_proxy.clone().try_into().expect("convert to pubkey");

    // Setup the proxy
    let proxy = Proxy::new(controller.client.clone(), key_proxy, KeyRing::default());

    let inv = proxy
        .get_yanked_invoice(id)
        .await
        .expect("Should be able to fetch invoice");

    // Now verify that the invoice was signed by the proxy
    let keyring = KeyRing::new(vec![pubkey]);

    VerificationStrategy::MultipleAttestation(vec![SignatureRole::Proxy])
        .verify(&inv, &keyring)
        .expect("Invoice should be signed by the proxy")
}

// TODO: Uncomment this test to sign as a creator and check the validation once
// https://github.com/deislabs/bindle/issues/167 is done.
// #[tokio::test]
// async fn test_proxy_create_verification_and_signing() {
//     let controller = TestController::new().await;

//     // Create an initial invoice in the server
//     let scaffold = testing::Scaffold::load("valid_v1").await;
//     let mut inv = scaffold.invoice.clone();

//     let key_creator = SecretKeyEntry::new("Test Creator".to_owned(), vec![SignatureRole::Creator]);
//     let key_proxy = SecretKeyEntry::new("Test Proxy".to_owned(), vec![SignatureRole::Proxy]);
//     let keyring_keys = vec![
//         key_creator.clone().try_into().expect("convert to pubkey"),
//         key_proxy.clone().try_into().expect("convert to pubkey"),
//     ];

//     let keyring = KeyRing::new(keyring_keys);

//     // Setup the proxy
//     let proxy = Proxy::new(
//         controller.client.clone(),
//         key_proxy.clone(),
//         keyring.clone(),
//     );

//     // Add another signature to make sure that the proxy can validate, but don't add creator because
//     // the server doesn't have the public key available

//     inv.sign(SignatureRole::Creator, &key_host).unwrap();

//     // The role and secret key don't matter here
//     proxy
//         .create_invoice(
//             &mut inv,
//             SignatureRole::Proxy,
//             &key_proxy,
//             VerificationStrategy::CreativeIntegrity,
//         )
//         .await
//         .expect("Should be able to create invoice");

//     VerificationStrategy::MultipleAttestation(vec![SignatureRole::Proxy, SignatureRole::Creator])
//         .verify(&inv, &keyring)
//         .expect("Invoice should be signed by the proxy and creator keys");
// }
