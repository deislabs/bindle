use crate::storage::Storage;

use warp::Filter;

pub mod v1 {
    use super::*;

    use crate::server::handlers::*;

    pub fn list<S: Storage + Clone + Send + Sync>(store: S) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("v1" / "invoices").and(warp::get()).and(with_store(store)).and_then(list_invoices)
    }
    
    pub fn create<S: Storage + Clone + Send + Sync>(store: S) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("v1" / "invoices").and(warp::post()).and(with_store(store)).and(warp::body::json()).and_then(create_invoice)
    }
    
    pub fn get<S: Storage + Clone + Send + Sync>(store: S) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("v1" / "invoice" / String).and(warp::get()).and(with_store(store)).and_then(get_invoice)
    }
    
    pub fn yank<S: Storage + Clone + Send + Sync>(store: S) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("v1" / "invoice" / String).and(warp::delete()).and(with_store(store)).and_then(yank_invoice)
    }
}

fn with_store<S: Storage + Clone + Send>(store: S) -> impl Filter<Extract = (S,), Error = std::convert::Infallible> + Clone {
    // We have to clone for this to Fn instead of FnOnce
    warp::any().map(move || store.clone())
}

