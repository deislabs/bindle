use std::convert::Infallible;

use crate::storage::Storage;

// Currently these are all at the top level. As we increment API versions, we can encapsulate these in their own submodule

pub async fn list_invoices<S: Storage>(store: S) -> Result<impl warp::Reply, Infallible> {
    Ok(warp::reply::json(&"yay".to_string()))
}

pub async fn create_invoice<S: Storage>(store: S, inv: crate::Invoice) -> Result<impl warp::Reply, Infallible> {
    Ok(warp::reply::json(&"yay".to_string()))
}

pub async fn get_invoice<S: Storage>(id: String, store: S) -> Result<impl warp::Reply, Infallible> {
    Ok(warp::reply::json(&"yay".to_string()))
}

pub async fn yank_invoice<S: Storage>(id: String, store: S)  -> Result<impl warp::Reply, Infallible> {
    Ok(warp::reply::json(&"yay".to_string()))
}
