use super::filters;
use super::TOML_MIME_TYPE;
use crate::storage::Storage;

use warp::Filter;

pub mod v1 {
    use super::*;

    use crate::server::handlers::v1::*;

    pub fn list<S>(
        store: S,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
    where
        S: Storage + Clone + Send + Sync,
    {
        warp::path("_q")
            .and(warp::get())
            .and(with_store(store))
            .and_then(list_invoices)
    }

    pub fn create<S>(
        store: S,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
    where
        S: Storage + Clone + Send + Sync,
    {
        warp::path!("_i" / String)
            .and(warp::post())
            .and(with_store(store))
            .and(filters::toml())
            .and_then(create_invoice)
    }

    pub fn get<S>(
        store: S,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
    where
        S: Storage + Clone + Send + Sync,
    {
        // TODO: Figure out how to match arbitrarily pathy bindles (multiple string params)
        warp::path!("_i" / String)
            .and(warp::get())
            .and(warp::query::<filters::InvoiceQuery>())
            .and(with_store(store))
            .and_then(get_invoice)
    }

    pub fn head<S>(
        store: S,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
    where
        S: Storage + Clone + Send + Sync,
    {
        warp::path!("_i" / String)
            .and(warp::get())
            .and(warp::query::<filters::InvoiceQuery>())
            .and(with_store(store))
            .and_then(head_invoice)
    }

    pub fn yank<S>(
        store: S,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone
    where
        S: Storage + Clone + Send + Sync,
    {
        warp::path!("_i" / String)
            .and(warp::delete())
            .and(with_store(store))
            .and_then(yank_invoice)
    }
}

fn with_store<S>(store: S) -> impl Filter<Extract = (S,), Error = std::convert::Infallible> + Clone
where
    S: Storage + Clone + Send,
{
    // We have to clone for this to be Fn instead of FnOnce
    warp::any().map(move || store.clone())
}
