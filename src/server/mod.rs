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
use crate::search::Search;

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
pub async fn server<P, I, Authn, Authz>(
    store: P,
    index: I,
    authn: Authn,
    authz: Authz,
    addr: impl Into<SocketAddr> + 'static,
    tls: Option<TlsConfig>,
) -> anyhow::Result<()>
where
    P: Provider + Clone + Send + Sync + 'static,
    I: Search + Clone + Send + Sync + 'static,
    Authn: crate::authn::Authenticator + Clone + Send + Sync + 'static,
    Authz: crate::authz::Authorizer + Clone + Send + Sync + 'static,
{
    // V1 API paths, currently the only version
    let api = routes::api(store, index, authn, authz);

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
    use std::convert::TryInto;

    use crate::authn::always::AlwaysAuthenticate;
    use crate::authz::always::AlwaysAuthorize;
    use crate::provider::Provider;
    use crate::testing;

    use testing::Scaffold;
    use tokio_util::codec::{BytesCodec, FramedRead};

    #[tokio::test]
    async fn test_successful_workflow() {
        let bindles = testing::load_all_files().await;
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store, index, AlwaysAuthenticate, AlwaysAuthorize);

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
            "Invoice should not have missing parcels"
        );

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

    #[tokio::test]
    async fn test_yank() {
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store.clone(), index, AlwaysAuthenticate, AlwaysAuthorize);
        // Insert an invoice
        let scaffold = testing::Scaffold::load("incomplete").await;
        store
            .create_invoice(&scaffold.invoice)
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

    #[tokio::test]
    // This isn't meant to test all of the possible validation failures (that should be done in a unit
    // test for storage), just the main validation failures from the API
    async fn test_invoice_validation() {
        let bindles = testing::load_all_files().await;
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store.clone(), index, AlwaysAuthenticate, AlwaysAuthorize);
        let valid_raw = bindles.get("valid_v1").expect("Missing scaffold");
        let valid = testing::Scaffold::from(valid_raw.clone());
        store
            .create_invoice(&valid.invoice)
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

    #[tokio::test]
    // This isn't meant to test all of the possible validation failures (that should be done in a unit
    // test for storage), just the main validation failures from the API
    async fn test_parcel_validation() {
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store.clone(), index, AlwaysAuthenticate, AlwaysAuthorize);
        // Insert a parcel
        let scaffold = testing::Scaffold::load("valid_v1").await;
        let parcel = scaffold.parcel_files.get("parcel").expect("Missing parcel");
        let data = std::io::Cursor::new(parcel.data.clone());
        store
            .create_invoice(&scaffold.invoice)
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

        // Create invoice first
        store
            .create_invoice(&scaffold.invoice)
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

    #[tokio::test]
    // Once again, this isn't meant to exercise all of the query functionality, just that the API
    // functions properly
    async fn test_queries() {
        // Insert data into store
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store.clone(), index, AlwaysAuthenticate, AlwaysAuthorize);
        let bindles_to_insert = vec!["incomplete", "valid_v1", "valid_v2"];

        for b in bindles_to_insert.into_iter() {
            let current = testing::Scaffold::load(b).await;
            store
                .create_invoice(&current.invoice)
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

        // NOTE: This is currently broken in the code. It is being fixed and this should be uncommented
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

    #[tokio::test]
    async fn test_missing() {
        let (store, index) = testing::setup().await;

        let api = super::routes::api(store.clone(), index, AlwaysAuthenticate, AlwaysAuthorize);

        let scaffold = testing::Scaffold::load("lotsa_parcels").await;
        store
            .create_invoice(&scaffold.invoice)
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
}
