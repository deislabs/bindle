mod test_util;
use test_util::*;

use bindle::testing;

use std::collections::HashMap;
use std::io::Cursor;

use bindle::standalone::{StandaloneRead, StandaloneWrite, INVOICE_FILE, PARCEL_DIR};

use tokio_stream::StreamExt;
use tokio_util::codec::{BytesCodec, FramedRead};

#[tokio::test]
async fn test_successful_write() {
    let tempdir = tempfile::tempdir().expect("unable to create tempdir");

    let scaffold = testing::Scaffold::load("lotsa_parcels").await;

    let standalone = StandaloneWrite::new(&tempdir, &scaffold.invoice.bindle.id)
        .await
        .expect("Unable to create new standalone write");

    let expected_len = scaffold.parcel_files.len();
    let id = scaffold.invoice.bindle.id.clone();

    standalone
        .write(
            scaffold.invoice,
            scaffold
                .parcel_files
                .into_iter()
                .map(|(_, parcel)| (parcel.sha, Cursor::new(parcel.data)))
                .collect(),
        )
        .await
        .expect("write shouldn't error");

    // We should have the same amount of labels as files, so use that here
    validate_write(id, tempdir.path().to_owned(), expected_len).await;
}

#[tokio::test]
async fn test_successful_write_stream() {
    let tempdir = tempfile::tempdir().expect("unable to create tempdir");

    let scaffold = testing::Scaffold::load("lotsa_parcels").await;

    let standalone = StandaloneWrite::new(&tempdir, &scaffold.invoice.bindle.id)
        .await
        .expect("Unable to create new standalone write");

    let expected_len = scaffold.parcel_files.len();
    let id = scaffold.invoice.bindle.id.clone();

    let parcels = scaffold
        .parcel_files
        .into_iter()
        .map(|(_, parcel)| {
            (
                parcel.sha,
                FramedRead::new(std::io::Cursor::new(parcel.data), BytesCodec::default())
                    .map(|res| res.map(|b| b.freeze())),
            )
        })
        .collect();
    standalone
        .write_stream(scaffold.invoice, parcels)
        .await
        .expect("write shouldn't error");

    // We should have the same amount of labels as files, so use that here
    validate_write(id, tempdir.path().to_owned(), expected_len).await;
}

async fn validate_write(id: bindle::Id, tempdir: std::path::PathBuf, expected_files: usize) {
    // TODO: Do we want to validate more than this (things like file contents)?
    let base_path = tempdir.join(id.sha());

    let metadata = tokio::fs::metadata(base_path.join(INVOICE_FILE))
        .await
        .expect("Should be able to read file metadata for invoice");
    assert!(metadata.is_file(), "Invoice is not a file");

    let mut count: usize = 0;
    let mut stream = tokio::fs::read_dir(base_path.join(PARCEL_DIR))
        .await
        .expect("unable to read parcel directory");
    while let Some(entry) = stream
        .next_entry()
        .await
        .expect("unable to read parcel directory")
    {
        count += 1;
        assert!(
            entry
                .metadata()
                .await
                .expect("Unable to read file metadata")
                .is_file(),
            "Item {:?} in parcel directory is not a file",
            entry.path().file_name()
        );
    }

    assert_eq!(
        count, expected_files,
        "Expected {} parcel files, found {}",
        expected_files, count
    );

    // Sanity check: make sure that our own read standalone can read what we write
    let read = StandaloneRead::new(tempdir, id)
        .await
        .expect("Should be able to read what we wrote");
    assert_eq!(
        read.parcels.len(),
        expected_files,
        "Expected standalone read to find {} parcel files, found {}",
        expected_files,
        read.parcels.len(),
    );
}

#[tokio::test]
async fn test_invalid_standalone_write() {
    let tempdir = tempfile::tempdir().expect("unable to create tempdir");

    let scaffold = testing::Scaffold::load("lotsa_parcels").await;

    let standalone = StandaloneWrite::new(&tempdir, &scaffold.invoice.bindle.id)
        .await
        .expect("Unable to create new standalone write");

    let mut parcels: HashMap<String, Cursor<Vec<u8>>> = scaffold
        .parcel_files
        .into_iter()
        .map(|(_, parcel)| (parcel.sha, Cursor::new(parcel.data)))
        .collect();

    parcels.insert(
        "123456789abcdef".to_string(),
        Cursor::new(b"a parcel that shouldn't be here".to_vec()),
    );

    standalone
        .write(scaffold.invoice, parcels)
        .await
        .expect_err("write shouldn't succeed");
}

#[tokio::test]
async fn test_push() {
    let controller = TestController::new(BINARY_NAME).await;

    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let path = std::path::PathBuf::from(root).join("test/data/standalone");
    let read = StandaloneRead::new(path, "enterprise.com/warpcore/1.0.0")
        .await
        .expect("Should be able to read standalone bindle");

    read.push(&controller.client)
        .await
        .expect("Should be able to push successfully");

    // TODO: Future improvements here could be how we check that things were pushed properly, but
    // this is enough of a sanity check for now
}

#[tokio::test]
async fn test_push_tarball() {
    let controller = TestController::new(BINARY_NAME).await;

    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let path = std::path::PathBuf::from(root)
        .join("test/data/standalone/1927aefa8fdc8327499e918300e2e49ecb271321530cc5881fcd069ca8372dcd.tar.gz");
    let read = StandaloneRead::new_from_tarball(path)
        .await
        .expect("Should be able to read standalone bindle");

    read.push(&controller.client)
        .await
        .expect("Should be able to push successfully");

    // TODO: Future improvements here could be how we check that things were pushed properly, but
    // this is enough of a sanity check for now
}
