//! Helpers for loading bindles and associated objects from files

use std::io::Error;
use std::path::Path;

use crate::client::Result;

use tokio::fs::File;
use tokio::stream::Stream;
use tokio_util::codec::{BytesCodec, FramedRead};

/// Loads a file as an async `Stream`, returning the stream, and the SHA256 sum of the file. Used
/// primarily for streaming parcel data to a bindle server
pub async fn raw<P: AsRef<Path>>(
    file_path: P,
) -> Result<impl Stream<Item = std::result::Result<bytes::BytesMut, Error>>> {
    let file = File::open(file_path).await?;
    Ok(FramedRead::new(file, BytesCodec::new()))
}

/// Loads a file and deserializes it from TOML to an arbirary type. Turbofish may be required to
/// specify the type: `bindle::client::load::toml::<bindle::Label>("/my/path.toml").await;`
pub async fn toml<T>(file_path: impl AsRef<Path>) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let data = tokio::fs::read(file_path).await?;
    Ok(toml::from_slice::<T>(&data)?)
}
