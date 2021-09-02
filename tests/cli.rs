// Tests for the CLI
mod test_util;
use test_util::TestController;

use bindle::client::{tokens::TokenManager, Client};
use bindle::testing;

const ENV_BINDLE_URL: &str = "BINDLE_URL";
const BINARY_NAME: &str = "bindle-server";

// Inserts data into the test server for fetching
async fn setup_data<T: TokenManager>(client: &Client<T>) {
    // For now let's just put in a simple manifest and one with a lot of parcels
    for name in &["valid_v1", "lotsa_parcels"] {
        let scaffold = testing::Scaffold::load(*name).await;
        client
            .create_invoice(scaffold.invoice.clone())
            .await
            .expect("Unable to insert invoice");
        for parcel in scaffold.parcel_files.values() {
            match client
                .create_parcel(
                    &scaffold.invoice.bindle.id,
                    &parcel.sha,
                    parcel.data.clone(),
                )
                .await
            {
                Ok(_) => continue,
                Err(e) if matches!(e, bindle::client::ClientError::ParcelAlreadyExists) => continue,
                Err(e) => panic!("Unable to insert parcel: {}", e),
            };
        }
    }
}

#[tokio::test]
async fn test_push() {
    let controller = TestController::new(BINARY_NAME).await;
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let path = std::path::PathBuf::from(root).join("test/data/standalone");
    // TODO: Figure out how to dedup these outputs. I tried doing something but `args` returns an `&mut` which complicates things
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "push",
            "-p",
            path.to_str().unwrap(),
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");
    assert_status(output, "Should be able to push a full bindle");
}

#[tokio::test]
async fn test_push_invoice_and_file() {
    let controller = TestController::new(BINARY_NAME).await;
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let base = std::path::PathBuf::from(root).join("tests/scaffolds/valid_v1");
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "push-invoice",
            base.join("invoice.toml").to_str().unwrap(),
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to push an invoice");

    // Now try to push a file from the bindle
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "push-file",
            "enterprise.com/warpcore/1.0.0",
            base.join("parcels/parcel.dat").to_str().unwrap(),
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");
    assert_status(output, "Should be able to push a file");
}

#[tokio::test]
async fn test_get() {
    let controller = TestController::new(BINARY_NAME).await;
    setup_data(&controller.client).await;
    let cachedir = tempfile::tempdir().expect("unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "-d",
            cachedir.path().to_str().unwrap(),
            "get",
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get an invoice");

    // This is a sanity check test to make sure a second call triggers the code path for successfully fetching from the cache
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "-d",
            cachedir.path().to_str().unwrap(),
            "get",
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get an invoice");

    // Now try and export to a test directory and make sure it is there
    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "-d",
            cachedir.path().to_str().unwrap(),
            "get",
            "-e",
            tempdir.path().to_str().unwrap(),
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get a bindle");
    assert!(
        tokio::fs::metadata(
            tempdir
                .path()
                .join("1927aefa8fdc8327499e918300e2e49ecb271321530cc5881fcd069ca8372dcd")
        )
        .await
        .expect("Unable to read exported bindle")
        .is_dir(),
        "Expected exported bindle directory"
    )
}

#[tokio::test]
async fn test_get_invoice() {
    let controller = TestController::new(BINARY_NAME).await;
    setup_data(&controller.client).await;

    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    let cachedir = tempfile::tempdir().expect("Unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "-d",
            cachedir.path().to_str().unwrap(),
            "get-invoice",
            "-o",
            tempdir.path().join("invoice.toml").to_str().unwrap(),
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get an invoice");
    assert!(
        tokio::fs::metadata(tempdir.path().join("invoice.toml"))
            .await
            .expect("Unable to read exported invoice")
            .is_file(),
        "Expected invoice file"
    )
}

#[tokio::test]
async fn test_create_key_and_sign_invoice() {
    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    // Create a signing key
    {
        let cmd = format!(
            "run --features cli --bin bindle -- create-key testkey -f {}",
            tempdir.path().join("testkey.toml").to_str().unwrap()
        );
        let output = std::process::Command::new("cargo")
            .args(cmd.split(' '))
            .env(ENV_BINDLE_URL, "localhost:8080")
            .output()
            .expect("Key should get created");
        assert_status(output, "Key should be generated");
        assert!(
            tokio::fs::metadata(tempdir.path().join("testkey.toml"))
                .await
                .expect("Unable to read keyfile")
                .is_file(),
            "Expected key file"
        );
    }
    // Use the new key to sign an invoice
    {
        let cmd = format!(
            "run --features cli --bin bindle -- sign-invoice ./test/data/simple-invoice.toml -o {} -f {}",
            tempdir.path().join("signed-invoice.toml").to_str().unwrap(),
            tempdir.path().join("testkey.toml").to_str().unwrap()
        );
        let output = std::process::Command::new("cargo")
            .args(cmd.split(' '))
            .env(ENV_BINDLE_URL, "localhost:8080")
            .output()
            .expect("Invoice should get signed");
        assert_status(output, "Invoice should get signed");
        assert!(
            tokio::fs::metadata(tempdir.path().join("signed-invoice.toml"))
                .await
                .expect("Unable to read invoice")
                .is_file(),
            "Expected signed invoice"
        )
    }
}

#[tokio::test]
async fn test_get_parcel() {
    let controller = TestController::new(BINARY_NAME).await;
    setup_data(&controller.client).await;

    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    let cachedir = tempfile::tempdir().expect("Unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "-d",
            cachedir.path().to_str().unwrap(),
            "get-parcel",
            "-o",
            tempdir.path().join("parcel.dat").to_str().unwrap(),
            "enterprise.com/warpcore/1.0.0",
            "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get a parcel");
    assert!(
        tokio::fs::metadata(tempdir.path().join("parcel.dat"))
            .await
            .expect("Unable to read exported invoice")
            .is_file(),
        "Expected parcel file"
    )
}

#[tokio::test]
async fn test_yank() {
    let controller = TestController::new(BINARY_NAME).await;
    setup_data(&controller.client).await;

    let output = std::process::Command::new("cargo")
        .args(&[
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "yank",
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to yank a bindle");
}

fn assert_status(output: std::process::Output, message: &str) {
    assert!(
        output.status.success(),
        "{}:\nStdout:\n {}\nStderr:\n{}",
        message,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
