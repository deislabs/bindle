use std::sync::Arc;

use warp::Filter;

use crate::{server::filters, signature::KeyRing};

/// A helper function that aggregates all routes into a complete API filter. If you only wish to
/// serve specific endpoints or versions, you can assemble them with the individual submodules
pub fn api<P, I, Authn, Authz, S>(
    store: P,
    index: I,
    authn: Authn,
    authz: Authz,
    secret_store: S,
    verification_strategy: crate::VerificationStrategy,
    keyring: KeyRing,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
where
    P: crate::provider::Provider + Clone + Send + Sync + 'static,
    I: crate::search::Search + Clone + Send + Sync + 'static,
    S: crate::invoice::signature::SecretKeyStorage + Clone + Send + Sync,
    Authn: crate::authn::Authenticator + Clone + Send + Sync + 'static,
    Authz: crate::authz::Authorizer + Clone + Send + Sync + 'static,
{
    // Use an Arc to avoid a possibly expensive clone of the keyring on every API call
    let wrapped_keyring = Arc::new(keyring);
    warp::path("v1").and(
        v1::auth::login(authn.client_id().to_owned())
            .with(warp::trace::request())
            .or(filters::authenticate_and_authorize(authn.clone(), authz)
                .untuple_one()
                .and(
                    v1::invoice::query(index)
                        .or(v1::invoice::create_toml(
                            store.clone(),
                            secret_store.clone(),
                            verification_strategy.clone(),
                            wrapped_keyring.clone(),
                        ))
                        .or(v1::invoice::create_json(
                            store.clone(),
                            secret_store,
                            verification_strategy,
                            wrapped_keyring,
                        ))
                        .or(v1::invoice::get(store.clone()))
                        .or(v1::invoice::head(store.clone()))
                        .or(v1::invoice::yank(store.clone()))
                        .or(v1::parcel::create(store.clone()))
                        .or(v1::parcel::get(store.clone()))
                        .or(v1::parcel::head(store.clone()))
                        .or(v1::relationships::get_missing_parcels(store)),
                )
                .recover(filters::handle_invalid_request_path)
                .recover(filters::handle_authn_rejection)
                .recover(filters::handle_authz_rejection)
                .with(warp::trace::request())),
    )
}

pub mod v1 {
    use crate::provider::Provider;
    use crate::search::Search;
    use crate::server::handlers::v1::*;
    use crate::server::{filters, routes::with_store};

    use warp::Filter;

    pub mod auth {
        use super::*;

        pub fn login(
            provider_client_id: String,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
            warp::path("login")
                .and(warp::get())
                .and(warp::query::<LoginParams>())
                .and(warp::any().map(move || provider_client_id.clone()))
                .and(warp::header::optional::<String>("accept"))
                .and_then(crate::server::handlers::v1::login)
        }
    }

    pub mod invoice {
        use crate::{
            server::routes::with_secret_store,
            signature::{KeyRing, SecretKeyStorage},
        };

        use super::*;

        use std::sync::Arc;

        pub fn query<S>(
            index: S,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            S: Search + Clone + Send + Sync,
        {
            warp::path("_q")
                .and(warp::get())
                .and(warp::query::<crate::QueryOptions>())
                .and(warp::any().map(move || index.clone()))
                .and(warp::header::optional::<String>("accept"))
                .and_then(query_invoices)
        }

        pub fn create_toml<P, S>(
            store: P,
            secret_store: S,
            verification_strategy: crate::VerificationStrategy,
            keyring: Arc<KeyRing>,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
            S: SecretKeyStorage + Clone + Send + Sync,
        {
            warp::path("_i")
                .and(warp::path::end())
                .and(warp::post())
                .and(with_store(store))
                .and(with_secret_store(secret_store))
                .and(warp::any().map(move || verification_strategy.clone()))
                .and(warp::any().map(move || keyring.clone()))
                .and(filters::toml())
                .and(warp::header::optional::<String>("accept"))
                .and_then(create_invoice)
                .recover(filters::handle_deserialize_rejection)
        }
        pub fn create_json<P, S>(
            store: P,
            secret_store: S,
            verification_strategy: crate::VerificationStrategy,
            keyring: Arc<KeyRing>,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
            S: SecretKeyStorage + Clone + Send + Sync,
        {
            warp::path("_i")
                .and(warp::path::end())
                .and(warp::post())
                .and(with_store(store))
                .and(with_secret_store(secret_store))
                .and(warp::any().map(move || verification_strategy.clone()))
                .and(warp::any().map(move || keyring.clone()))
                .and(warp::body::json())
                .and(warp::header::optional::<String>("accept"))
                .and_then(create_invoice)
                .recover(filters::handle_deserialize_rejection)
        }

        // The GET and HEAD endpoints handle both parcels and invoices through the request router function
        pub fn get<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            filters::invoice()
                .and(warp::get())
                .and(warp::query::<filters::InvoiceQuery>())
                .and(with_store(store))
                .and(warp::header::optional::<String>("accept"))
                .and_then(get_invoice)
        }

        pub fn head<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            filters::invoice()
                .and(warp::head())
                .and(warp::query::<filters::InvoiceQuery>())
                .and(with_store(store))
                .and(warp::header::optional::<String>("accept"))
                .and_then(head_invoice)
        }

        pub fn yank<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            warp::path("_i")
                .and(warp::path::tail())
                .and(warp::delete())
                .and(with_store(store))
                .and(warp::header::optional::<String>("accept"))
                .and_then(yank_invoice)
        }
    }

    pub mod parcel {
        use super::*;

        pub fn create<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            filters::parcel()
                .and(warp::post())
                .and(warp::body::stream())
                .and(with_store(store))
                .and(warp::header::optional::<String>("accept"))
                .and_then(create_parcel)
        }

        pub fn get<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            filters::parcel()
                .and(warp::get())
                .and(with_store(store))
                .and_then(get_parcel)
        }

        pub fn head<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            filters::parcel()
                .and(warp::head())
                .and(with_store(store))
                .and_then(head_parcel)
        }
    }

    pub mod relationships {
        use super::*;

        pub fn get_missing_parcels<P>(
            store: P,
        ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
        where
            P: Provider + Clone + Send + Sync,
        {
            // For some reason, using the `path!` macro here was causing matching problems
            warp::path("_r")
                .and(warp::path("missing"))
                .and(warp::path::tail())
                .and(warp::get())
                .and(with_store(store))
                .and(warp::header::optional::<String>("accept"))
                .and_then(get_missing)
        }
    }
}

pub(crate) fn with_store<P>(
    store: P,
) -> impl Filter<Extract = (P,), Error = std::convert::Infallible> + Clone
where
    P: crate::provider::Provider + Clone + Send,
{
    // We have to clone for this to be Fn instead of FnOnce
    warp::any().map(move || store.clone())
}

pub(crate) fn with_secret_store<P>(
    store: P,
) -> impl Filter<Extract = (P,), Error = std::convert::Infallible> + Clone
where
    P: crate::invoice::signature::SecretKeyStorage + Clone + Send,
{
    // We have to clone for this to be Fn instead of FnOnce
    warp::any().map(move || store.clone())
}
