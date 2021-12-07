//! Server implementation of the [Bindle Protocol
//! Spec](https://github.com/deislabs/bindle/blob/master/docs/protocol-spec.md), with associated
//! HTTP handlers and functions

pub(crate) mod filters;
mod handlers;
pub(crate) mod reply;

mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;

use tracing::debug;

use super::provider::Provider;
use crate::events::EventSink;
use crate::signature::KeyRing;
use crate::{search::Search, signature::SecretKeyStorage};

pub(crate) const TOML_MIME_TYPE: &str = "application/toml";
pub(crate) const JSON_MIME_TYPE: &str = "application/json";

/// The configuration required for running with TLS enabled
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

/// Returns a future that runs a server until it receives a SIGINT to stop. If optional TLS
/// configuration is given, the server will be configured to use TLS. Otherwise it will use plain
/// HTTP
#[allow(clippy::too_many_arguments)]
pub async fn server<P, I, Authn, Authz, S, E>(
    store: P,
    index: I,

    authn: Authn,
    authz: Authz,
    addr: impl Into<SocketAddr> + 'static,
    tls: Option<TlsConfig>,
    keystore: S,
    verification_strategy: crate::VerificationStrategy,
    keyring: KeyRing,
    eventsink: E,
) -> anyhow::Result<()>
where
    P: Provider + Clone + Send + Sync + 'static,
    I: Search + Clone + Send + Sync + 'static,
    S: SecretKeyStorage + Clone + Send + Sync + 'static,
    Authn: crate::authn::Authenticator + Clone + Send + Sync + 'static,
    Authz: crate::authz::Authorizer + Clone + Send + Sync + 'static,
    E: EventSink + Clone + Send + Sync + 'static,
{
    // V1 API paths, currently the only version
    let api = routes::api(
        store,
        index,
        authn,
        authz,
        keystore,
        verification_strategy,
        keyring,
        eventsink,
    );

    let server = warp::serve(api);
    match tls {
        None => {
            debug!("No TLS config found, starting server in HTTP mode");
            server
                .try_bind_with_graceful_shutdown(addr, shutdown_signal())?
                .1
                .await
        }
        Some(config) => {
            debug!(
                ?config.key_path,
                ?config.cert_path, "Got TLS config, starting server in HTTPS mode",
            );
            server
                .tls()
                .key_path(config.key_path)
                .cert_path(config.cert_path)
                .bind_with_graceful_shutdown(addr, shutdown_signal())
                .1
                .await
        }
    };
    Ok(())
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to setup signal handler");
}

#[cfg(test)]
mod test {
    use async_std::fs::File;
    use async_std::prelude::*;
    use chrono::naive::serde::ts_milliseconds::deserialize;
    use std::convert::TryInto;
    use std::path::PathBuf;

    use crate::authn::always::AlwaysAuthenticate;
    use crate::authz::always::AlwaysAuthorize;
    use crate::events::{defaultsink::DefaultEventSink, filesink::FileEventSink};
    use crate::invoice::{
        signature::{KeyRing, SecretKeyEntry},
        SignatureRole, VerificationStrategy,
    };
    use crate::provider::Provider;
    use crate::search::StrictEngine;
    use crate::testing::{
        self, BindleEvent, InvoiceCreatedEventData, InvoiceYankedEventData, MissingParcelEventData,
        MockKeyStore, ParcelCreatedEventData,
    };

    use rstest::rstest;
    use tempfile::tempdir;
    use testing::Scaffold;
    use tokio_util::codec::{BytesCodec, FramedRead};

    #[rstest]
    #[tokio::test]
    async fn test_successful_workflow<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let bindles = testing::load_all_files().await;
        let (store, index, ks, _) = provider_setup.await;
        let temp = tempdir().expect("unable to create tempdir");
        let directory = PathBuf::from(temp.path());
        let filename = directory.join(crate::events::filesink::EVENT_SINK_FILE_NAME);
        let es = FileEventSink::new(directory);

        let api = super::routes::api(
            store,
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );

        // Now that we can't upload parcels before invoices exist, we need to create a bindle that shares some parcels

        let valid_v1 = bindles.get("valid_v1").expect("Missing scaffold");
        // Create an invoice pointing to those parcels and make sure the correct response is returned
        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&valid_v1.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::ACCEPTED,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let create_res: crate::InvoiceCreateResponse =
            toml::from_slice(res.body()).expect("should be valid invoice response TOML");

        assert!(
            create_res.missing.is_some(),
            "Invoice should have missing parcels"
        );

        // Check that we got a Invoice Created event and a missing parcel event.

        let mut file = File::open(filename).await.unwrap();
        let mut buf = [0u8; 4096];
        let bytes = file.read(&mut buf).await.unwrap();
        assert!(
            bytes < buf.len(),
            "Should have read less than the buffer size"
        );
        let json_string = String::from_utf8_lossy(&buf);
        let deserializer = serde_json::Deserializer::from_str(&json_string);
        let mut iterator = deserializer.into_iter::<serde_json::Value>();

        let inv = iterator.next().unwrap().unwrap();
        let event: BindleEvent<InvoiceCreatedEventData> = serde_json::from_value(inv).unwrap();
        assert_eq!(
            "enterprise.com/warpcore",
            event.event_data.invoice_created.bindle.name
        );
        assert_eq!("1.0.0", event.event_data.invoice_created.bindle.version);

        let missing = iterator.next().unwrap().unwrap();
        let event: BindleEvent<MissingParcelEventData> = serde_json::from_value(missing).unwrap();
        assert_eq!(
            "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5",
            event.event_data.missing_parcel.sha256
        );
        assert_eq!("text/plain", event.event_data.missing_parcel.media_type);
        assert_eq!("isolinear_chip.txt", event.event_data.missing_parcel.name);

        // Upload the parcels for one of the invoices

        for file in valid_v1.parcel_files.values() {
            let res = warp::test::request()
                .method("POST")
                .path(&format!(
                    "/v1/_i/{}@{}",
                    create_res.invoice.bindle.id, file.sha
                ))
                .body(file.data.clone())
                .reply(&api)
                .await;
            assert_eq!(
                res.status(),
                warp::http::StatusCode::OK,
                "Body: {}",
                String::from_utf8_lossy(res.body())
            );
        }

        // Check that we got a parcel created event for the parcel.

        let bytes = file.read(&mut buf).await.unwrap();
        assert!(
            bytes < buf.len(),
            "Should have read less than the buffer size"
        );
        let json_string = String::from_utf8_lossy(&buf);
        let deserializer = serde_json::Deserializer::from_str(&json_string);
        let mut iterator = deserializer.into_iter::<serde_json::Value>();

        let parcel = iterator.next().unwrap().unwrap();
        let event: BindleEvent<ParcelCreatedEventData> = serde_json::from_value(parcel).unwrap();
        assert_eq!(
            "enterprise.com/warpcore/1.0.0",
            event.event_data.parcel_created[0]
        );
        assert_eq!(
            "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5",
            event.event_data.parcel_created[1]
        );

        // Now create another invoice that references the same parcel and validated that it returns
        // the proper response

        let mut inv = Scaffold::from(valid_v1.to_owned()).invoice;
        inv.bindle.id = "another.com/bindle/1.0.0".try_into().unwrap();
        let inv = toml::to_vec(&inv).expect("serialization shouldn't fail");

        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&inv)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::CREATED,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let create_res: crate::InvoiceCreateResponse =
            toml::from_slice(res.body()).expect("should be valid invoice response TOML");

        assert!(
            create_res.missing.is_none(),
            "Invoice should not have missing parcels"
        );

        // Check that we got a Invoice Created event.

        let bytes = file.read(&mut buf).await.unwrap();
        assert!(
            bytes < buf.len(),
            "Should have read less than the buffer size"
        );
        let json_string = String::from_utf8_lossy(&buf);
        let deserializer = serde_json::Deserializer::from_str(&json_string);
        let mut iterator = deserializer.into_iter::<serde_json::Value>();

        let inv = iterator.next().unwrap().unwrap();
        let event: BindleEvent<InvoiceCreatedEventData> = serde_json::from_value(inv).unwrap();
        assert_eq!(
            "another.com/bindle",
            event.event_data.invoice_created.bindle.name
        );
        assert_eq!("1.0.0", event.event_data.invoice_created.bindle.version);

        // Create a second version of the same invoice with some missing and already existing
        // parcels and make sure the correct response is returned
        let valid_v2 = bindles.get("valid_v2").expect("Missing scaffold");

        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&valid_v2.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::ACCEPTED,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let create_res: crate::InvoiceCreateResponse =
            toml::from_slice(res.body()).expect("should be valid invoice response TOML");

        assert_eq!(
            create_res
                .missing
                .expect("Should have missing parcels")
                .len(),
            1,
            "Invoice should have missing parcels"
        );

        // Check that we got a Invoice Created event and a missing parcel event.

        let bytes = file.read(&mut buf).await.unwrap();
        assert!(
            bytes < buf.len(),
            "Should have read less than the buffer size"
        );
        let json_string = String::from_utf8_lossy(&buf);
        let deserializer = serde_json::Deserializer::from_str(&json_string);
        let mut iterator = deserializer.into_iter::<serde_json::Value>();

        let inv = iterator.next().unwrap().unwrap();
        let event: BindleEvent<InvoiceCreatedEventData> = serde_json::from_value(inv).unwrap();
        assert_eq!(
            "enterprise.com/warpcore",
            event.event_data.invoice_created.bindle.name
        );
        assert_eq!("2.0.0", event.event_data.invoice_created.bindle.version);

        // Get an invoice
        let res = warp::test::request()
            .path("/v1/_i/enterprise.com/warpcore/1.0.0")
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let inv: crate::Invoice =
            toml::from_slice(res.body()).expect("should be valid invoice TOML");

        // Get a parcel
        let parcel = &inv.parcel.expect("Should have parcels")[0];
        let res = warp::test::request()
            .path(&format!(
                "/v1/_i/enterprise.com/warpcore/1.0.0@{}",
                parcel.label.sha256
            ))
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        assert_eq!(
            res.body().as_ref(),
            valid_v1.parcel_files.get("parcel").unwrap().data.as_slice()
        );
        assert_eq!(
            res.headers()
                .get("Content-Type")
                .expect("No content type header found")
                .to_str()
                .unwrap(),
            parcel.label.media_type
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_yank<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let (store, index, ks, _) = provider_setup.await;
        let temp = tempdir().expect("unable to create tempdir");
        let directory = PathBuf::from(temp.path());
        let filename = directory.join(crate::events::filesink::EVENT_SINK_FILE_NAME);
        let es = FileEventSink::new(directory);

        let api = super::routes::api(
            store.clone(),
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );

        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);

        // Insert an invoice
        let scaffold = testing::Scaffold::load("incomplete").await;
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(scaffold.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();
        store
            .create_invoice(signed)
            .await
            .expect("Should be able to insert invoice");

        let inv_path = format!("/v1/_i/{}", scaffold.invoice.name());
        // Yank the invoice
        let res = warp::test::request()
            .method("DELETE")
            .path(&inv_path)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        // Check that we got a Invoice Yanked Event.

        let mut file = File::open(filename).await.unwrap();
        let mut buf = [0u8; 4096];
        let bytes = file.read(&mut buf).await.unwrap();
        assert!(
            bytes < buf.len(),
            "Should have read less than the buffer size"
        );
        let json_string = String::from_utf8_lossy(&buf);
        let deserializer = serde_json::Deserializer::from_str(&json_string);
        let mut iterator = deserializer.into_iter::<serde_json::Value>();

        let inv = iterator.next().unwrap().unwrap();
        let event: BindleEvent<InvoiceYankedEventData> = serde_json::from_value(inv).unwrap();
        assert_eq!(
            "enterprise.com/bridge/1.0.0",
            event.event_data.invoice_yanked
        );

        // Attempt to fetch the invoice and make sure it doesn't return
        let res = warp::test::request().path(&inv_path).reply(&api).await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::FORBIDDEN,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        // Set yanked to true and attempt to fetch again
        let res = warp::test::request()
            .path(&format!("{}?yanked=true", inv_path))
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        toml::from_slice::<crate::Invoice>(res.body()).expect("should be valid invoice TOML");
    }

    #[rstest]
    #[tokio::test]
    // This isn't meant to test all of the possible validation failures (that should be done in a unit
    // test for storage), just the main validation failures from the API
    async fn test_invoice_validation<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let bindles = testing::load_all_files().await;
        let (store, index, ks, es) = provider_setup.await;

        let api = super::routes::api(
            store.clone(),
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );
        let valid_raw = bindles.get("valid_v1").expect("Missing scaffold");
        let valid = testing::Scaffold::from(valid_raw.clone());
        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(valid.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();
        store
            .create_invoice(signed)
            .await
            .expect("Invoice create failure");

        // Already created invoice
        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&valid_raw.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::CONFLICT,
            "Trying to upload existing invoice should fail"
        );
    }

    #[rstest]
    #[tokio::test]
    // This isn't meant to test all of the possible validation failures (that should be done in a unit
    // test for storage), just the main validation failures from the API
    async fn test_parcel_validation<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let (store, index, keystore, es) = provider_setup.await;

        let api = super::routes::api(
            store.clone(),
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            keystore.clone(),
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );
        // Insert a parcel
        let scaffold = testing::Scaffold::load("valid_v1").await;
        let parcel = scaffold.parcel_files.get("parcel").expect("Missing parcel");
        let data = std::io::Cursor::new(parcel.data.clone());
        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(scaffold.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();
        store
            .create_invoice(signed)
            .await
            .expect("Unable to insert invoice into store");
        store
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(data, BytesCodec::default()),
            )
            .await
            .expect("Unable to create parcel");

        // Already created parcel
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/v1/_i/{}@{}",
                scaffold.invoice.bindle.id, &parcel.sha
            ))
            .body(parcel.data.clone())
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::CONFLICT,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        let scaffold = testing::Scaffold::load("invalid").await;

        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(scaffold.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();

        // Create invoice first
        store
            .create_invoice(signed)
            .await
            .expect("Unable to create invoice");

        // Incorrect SHA
        let parcel = scaffold
            .parcel_files
            .get("invalid_sha")
            .expect("Missing parcel");
        let res = warp::test::request()
            .method("POST")
            .path(&format!(
                "/v1/_i/{}@{}",
                scaffold.invoice.bindle.id, &parcel.sha
            ))
            .body(parcel.data.clone())
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::BAD_REQUEST,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
    }

    #[rstest]
    #[tokio::test]
    // Once again, this isn't meant to exercise all of the query functionality, just that the API
    // functions properly
    async fn test_queries<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        // Insert data into store
        let (store, index, ks, es) = provider_setup.await;

        let api = super::routes::api(
            store.clone(),
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );
        let bindles_to_insert = vec!["incomplete", "valid_v1", "valid_v2"];

        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);

        for b in bindles_to_insert.into_iter() {
            let current = testing::Scaffold::load(b).await;
            let verified = VerificationStrategy::MultipleAttestation(vec![])
                .verify(current.invoice.clone(), &KeyRing::default())
                .unwrap();
            let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();
            store
                .create_invoice(signed)
                .await
                .expect("Unable to create invoice");
        }

        // Test empty query (don't think this works yet, so commented out)
        // let res = warp::test::request().path("/v1/_q").reply(&api).await;
        // assert_eq!(
        //     res.status(),
        //     warp::http::StatusCode::OK,
        //     "Body: {}",
        //     String::from_utf8_lossy(res.body())
        // );
        // let matches: crate::Matches =
        //     toml::from_slice(res.body()).expect("Unable to deserialize response");

        // assert_eq!(
        //     matches.invoices.len(),
        //     3,
        //     "Expected to get 3 invoice matches"
        // );

        // Test query term filter
        let res = warp::test::request()
            .path("/v1/_q?q=enterprise.com/warpcore")
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let matches: crate::Matches =
            toml::from_slice(res.body()).expect("Unable to deserialize response");

        assert_eq!(
            matches.invoices.len(),
            2,
            "Expected to get multiple invoice matches"
        );

        // Make sure the query was set
        assert_eq!(
            matches.query, "enterprise.com/warpcore",
            "Response did not contain the query data"
        );

        for inv in matches.invoices.into_iter() {
            assert_eq!(
                inv.bindle.id.name(),
                "enterprise.com/warpcore",
                "Didn't get the correct bindle"
            );
        }

        // Test loose query term filter (e.g. example.com/), this also doesn't work yet

        // Non existent query should be empty
        let res = warp::test::request()
            .path("/v1/_q?q=non/existent")
            .reply(&api)
            .await;
        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let matches: crate::Matches =
            toml::from_slice(res.body()).expect("Unable to deserialize response");
        assert!(
            matches.invoices.is_empty(),
            "Expected to get no invoice matches"
        );

        // Test version queries (also broken for the same reason as other tests here)

        // Test yank

        // Test limit/offset
    }

    #[rstest]
    #[tokio::test]
    async fn test_missing<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let (store, index, ks, es) = provider_setup.await;

        let api = super::routes::api(
            store.clone(),
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );

        let scaffold = testing::Scaffold::load("lotsa_parcels").await;
        let sk = SecretKeyEntry::new("test".to_owned(), vec![SignatureRole::Host]);
        let verified = VerificationStrategy::MultipleAttestation(vec![])
            .verify(scaffold.invoice.clone(), &KeyRing::default())
            .unwrap();
        let signed = crate::sign(verified, vec![(SignatureRole::Host, &sk)]).unwrap();
        store
            .create_invoice(signed)
            .await
            .expect("Unable to load in invoice");
        let parcel = scaffold
            .parcel_files
            .get("parcel")
            .expect("parcel doesn't exist");
        let parcel_data = std::io::Cursor::new(parcel.data.clone());
        store
            .create_parcel(
                &scaffold.invoice.bindle.id,
                &parcel.sha,
                FramedRead::new(parcel_data, BytesCodec::default()),
            )
            .await
            .expect("Unable to create parcel");

        let res = warp::test::request()
            .method("GET")
            .path(&format!("/v1/_r/missing/{}", scaffold.invoice.bindle.id))
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        let resp: crate::MissingParcelsResponse =
            toml::from_slice(res.body()).expect("should be valid invoice response TOML");

        assert_eq!(
            resp.missing.len(),
            2,
            "Expected 2 missing parcels, got {}",
            resp.missing.len()
        );

        assert!(
            resp.missing.iter().any(|l| l.name.contains("crate")),
            "Missing labels does not contain correct data: {:?}",
            resp.missing
        );
        assert!(
            resp.missing.iter().any(|l| l.name.contains("barrel")),
            "Missing labels does not contain correct data: {:?}",
            resp.missing
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_host_signed<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let (store, index, ks, es) = provider_setup.await;

        let api = super::routes::api(
            store,
            index,
            AlwaysAuthenticate,
            AlwaysAuthorize,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );

        let scaffold = testing::RawScaffold::load("valid_v1").await;
        // Create a valid invoice and make sure the returned invoice is signed
        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&scaffold.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::ACCEPTED,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
        let create_res: crate::InvoiceCreateResponse =
            toml::from_slice(res.body()).expect("should be valid invoice response TOML");

        assert!(
            create_res
                .invoice
                .signature
                .unwrap_or_default()
                .into_iter()
                .any(|sig| matches!(sig.role, crate::SignatureRole::Host)),
            "Newly created invoice should be signed by the host"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_anonymous_get<T>(
        #[values(testing::setup(), testing::setup_embedded())]
        #[future]
        provider_setup: (T, StrictEngine, MockKeyStore, DefaultEventSink),
    ) where
        T: Provider + Clone + Send + Sync + 'static,
    {
        let (store, index, ks, es) = provider_setup.await;

        let api = super::routes::api(
            store,
            index,
            crate::authn::http_basic::HttpBasic::from_file("test/data/htpasswd")
                .await
                .expect("Unable to load htpasswd file"),
            crate::authz::anonymous_get::AnonymousGet,
            ks,
            VerificationStrategy::default(),
            KeyRing::default(),
            es.clone(),
        );

        let scaffold = testing::RawScaffold::load("valid_v1").await;

        // Creating the invoice without a token should fail
        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .path("/v1/_i")
            .body(&scaffold.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::FORBIDDEN,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        // Creating with a token should succeed
        let res = warp::test::request()
            .method("POST")
            .header("Content-Type", "application/toml")
            .header(
                "Authorization",
                format!("Basic {}", base64::encode(b"admin:sw0rdf1sh")),
            )
            .path("/v1/_i")
            .body(&scaffold.invoice)
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::ACCEPTED,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        let scaffold: testing::Scaffold = scaffold.into();

        // Fetching with a token should work
        let res = warp::test::request()
            .method("GET")
            .header(
                "Authorization",
                format!("Basic {}", base64::encode(b"admin:sw0rdf1sh")),
            )
            .path(&format!("/v1/_i/{}", scaffold.invoice.bindle.id))
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );

        // Fetching without a token should work
        let res = warp::test::request()
            .method("GET")
            .path(&format!("/v1/_i/{}", scaffold.invoice.bindle.id))
            .reply(&api)
            .await;

        assert_eq!(
            res.status(),
            warp::http::StatusCode::OK,
            "Body: {}",
            String::from_utf8_lossy(res.body())
        );
    }
}
