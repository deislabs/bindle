// Tests for the CLI
mod test_util;
use std::ffi::OsStr;
use std::path::Path;

use bindle::signature::KeyRingLoader;
use test_util::*;

use bindle::client::{tokens::TokenManager, Client};
use bindle::{testing, SignatureRole};

const ENV_BINDLE_KEYRING: &str = "BINDLE_KEYRING";
const KEYRING_FILE: &str = "keyring.toml";
const SECRETS_FILE: &str = "secret_keys.toml";
const TEST_LABEL: &str = "Benjamin Sisko <thesisko@bajor.com>";

// Inserts data into the test server for fetching
async fn setup_data<T: TokenManager>(client: &Client<T>) {
    // For now let's just put in a simple manifest and one with a lot of parcels
    for name in &["valid_v1", "lotsa_parcels"] {
        let scaffold = testing::Scaffold::load(name).await;
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
        .args([
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
async fn test_push_tarball() {
    let controller = TestController::new(BINARY_NAME).await;
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let path = std::path::PathBuf::from(root).join("test/data/standalone");
    // TODO: Figure out how to dedup these outputs. I tried doing something but `args` returns an `&mut` which complicates things
    let output = std::process::Command::new("cargo")
        .args([
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
        .args([
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
        .args([
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
        .args([
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
        .env(ENV_BINDLE_KEYRING, &controller.keyring_path)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get an invoice");

    // This is a sanity check test to make sure a second call triggers the code path for successfully fetching from the cache
    let output = std::process::Command::new("cargo")
        .args([
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
        .env(ENV_BINDLE_KEYRING, &controller.keyring_path)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get an invoice");

    // Now try and export to a test directory and make sure it is there
    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args([
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
        .env(ENV_BINDLE_KEYRING, &controller.keyring_path)
        .output()
        .expect("Should be able to run command");

    assert_status(output, "Should be able to get a bindle");
    assert!(
        tokio::fs::metadata(
            tempdir
                .path()
                .join("1927aefa8fdc8327499e918300e2e49ecb271321530cc5881fcd069ca8372dcd.tar.gz")
        )
        .await
        .expect("Unable to read exported bindle")
        .is_file(),
        "Expected exported bindle tarball"
    )
}

#[tokio::test]
async fn test_get_invoice() {
    let controller = TestController::new(BINARY_NAME).await;
    setup_data(&controller.client).await;

    let tempdir = tempfile::tempdir().expect("Unable to set up tempdir");
    let cachedir = tempfile::tempdir().expect("Unable to set up tempdir");
    let output = std::process::Command::new("cargo")
        .args([
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
        .env(ENV_BINDLE_KEYRING, &controller.keyring_path)
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
            "run --features cli --bin bindle -- keys create testkey -f {}",
            tempdir.path().join("testkey.toml").to_str().unwrap()
        );
        let output = std::process::Command::new("cargo")
            .args(cmd.split(' '))
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
        .args([
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
        .args([
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

#[tokio::test]
async fn test_no_bindles() {
    let controller = TestController::new(BINARY_NAME).await;
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "search",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .output()
        .expect("Should be able to run command");
    assert!(
        output.status.success(),
        "Should be able to search for bindles"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .ends_with("No matching bindles were found"),
        "Should get no bindles found message"
    );
}

#[tokio::test]
async fn test_package() {
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let path = std::path::PathBuf::from(root).join("test/data/standalone");
    // TODO: Figure out how to dedup these outputs. I tried doing something but `args` returns an `&mut` which complicates things
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "package",
            "-p",
            path.to_str().unwrap(),
            "-e",
            tempdir.path().to_str().unwrap(),
            "enterprise.com/warpcore/1.0.0",
        ])
        .env(ENV_BINDLE_URL, "https://127.0.0.1:8080/v1/")
        .output()
        .expect("Should be able to run command");
    assert_status(
        output,
        "Should be able to package a standalone bindle directory",
    );

    assert!(
        tokio::fs::metadata(
            tempdir
                .path()
                .join("1927aefa8fdc8327499e918300e2e49ecb271321530cc5881fcd069ca8372dcd.tar.gz")
        )
        .await
        .expect("Unable to read packaged bindle")
        .is_file(),
        "Expected packaged bindle tarball"
    )
}

#[tokio::test]
async fn test_key_create_and_keyring() {
    // Tempdir for keyring and secret-file
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let secrets_file = tempdir.path().join(SECRETS_FILE);
    let keyring_file = tempdir.path().join(KEYRING_FILE);
    create_key(&keyring_file, &secrets_file, TEST_LABEL, false);

    assert!(
        tokio::fs::read_to_string(&secrets_file)
            .await
            .expect("Unable to read secrets file")
            .contains(TEST_LABEL),
        "Newly created key should be saved in file"
    );
    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert_eq!(
        keyring.key.len(),
        1,
        "Only 1 entry should exist in the keyring"
    );

    assert_eq!(
        keyring.key[0].label, TEST_LABEL,
        "The keyring key should be from the newly created key"
    );

    // Create one more secret and make sure the keyring now has 2 entries
    let second_label = "Kira Nerys <kira@bajor.mil>";
    create_key(&keyring_file, &secrets_file, second_label, false);

    assert!(
        tokio::fs::read_to_string(&secrets_file)
            .await
            .expect("Unable to read secrets file")
            .contains(second_label),
        "Newly created key should be saved in file"
    );
    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert_eq!(keyring.key.len(), 2, "Should have 2 entries in keyring");

    assert_eq!(
        keyring.key[0].label, TEST_LABEL,
        "Previously created key should still exist"
    );
    assert_eq!(
        keyring.key[1].label, second_label,
        "Newly created key should exist in keyring"
    );

    // Create a third and skip the keyring
    let third_label = "Jadzia Dax <dax@trill.com>";

    create_key(&keyring_file, &secrets_file, third_label, true);

    assert!(
        tokio::fs::read_to_string(&secrets_file)
            .await
            .expect("Unable to read secrets file")
            .contains(third_label),
        "Newly created key should be saved in file"
    );
    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert_eq!(
        keyring.key.len(),
        2,
        "Newly created key should not have been added to keyring"
    );

    for key in keyring.key.iter() {
        assert_ne!(
            key.label, third_label,
            "Newly created key should not exist in keyring"
        );
    }
}

#[tokio::test]
async fn test_keyring_add() {
    // Tempdir for keyring
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let keyring_file = tempdir.path().join(KEYRING_FILE);

    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "keys",
            "add",
            TEST_LABEL,
            "approver",
            "XbhLeOX4BtvUnT+o7xyi2waw5WXGOl3H/l3b5h97Dk4=",
        ])
        .env(ENV_BINDLE_KEYRING, &keyring_file)
        .output()
        .expect("Should be able to run command");

    assert!(output.status.success(), "Should be able to add a key");

    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert_eq!(keyring.key.len(), 1, "Should have 1 entry in keyring");

    assert_eq!(
        keyring.key[0].label, TEST_LABEL,
        "Correct label should have been added"
    );
    assert_eq!(keyring.key[0].roles.len(), 1, "Should only have 1 role");
    assert_eq!(
        keyring.key[0].roles[0],
        SignatureRole::Approver,
        "Should have approver role"
    );
    assert_eq!(
        keyring.key[0].key, "XbhLeOX4BtvUnT+o7xyi2waw5WXGOl3H/l3b5h97Dk4=",
        "Should have correct key"
    );
}

#[tokio::test]
async fn test_keyring_add_to_existing() {
    // Tempdir for keyring
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let secrets_file = tempdir.path().join(SECRETS_FILE);
    let keyring_file = tempdir.path().join(KEYRING_FILE);

    // Create a key to make sure one exists first
    create_key(&keyring_file, &secrets_file, TEST_LABEL, false);

    let second_label = "Miles O'Brien <everyman@ufp.com>";
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "keys",
            "add",
            second_label,
            "approver",
            "XbhLeOX4BtvUnT+o7xyi2waw5WXGOl3H/l3b5h97Dk4=",
        ])
        .env(ENV_BINDLE_KEYRING, &keyring_file)
        .output()
        .expect("Should be able to run command");

    assert!(output.status.success(), "Should be able to add a key");

    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert_eq!(keyring.key.len(), 2, "Should have 2 entries in keyring");

    assert_eq!(
        keyring.key[1].label, second_label,
        "New key should have been added"
    );
}

#[tokio::test]
async fn test_fetch_host_keys() {
    // Tempdir for keyring
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let keyring_file = tempdir.path().join(KEYRING_FILE);

    let controller = TestController::new(BINARY_NAME).await;
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "keys",
            "fetch",
        ])
        .env(ENV_BINDLE_URL, &controller.base_url)
        .env(ENV_BINDLE_KEYRING, &keyring_file)
        .output()
        .expect("Should be able to run command");
    assert!(
        output.status.success(),
        "Should be able to fetch host keys Stdout: {}\n Stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert!(
        !keyring.key.is_empty(),
        "Should have at least 1 entry in keyring"
    );
}

#[tokio::test]
async fn test_fetch_host_keys_from_specific_host() {
    // Tempdir for keyring
    let tempdir = tempfile::tempdir().expect("Unable to create tempdir");
    let keyring_file = tempdir.path().join(KEYRING_FILE);

    let controller = TestController::new(BINARY_NAME).await;
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--features",
            "cli",
            "--bin",
            "bindle",
            "--",
            "keys",
            "fetch",
            "--key-server",
            &format!("{}bindle-keys", controller.base_url),
        ])
        .env(ENV_BINDLE_URL, "http://not-real.com")
        .env(ENV_BINDLE_KEYRING, &keyring_file)
        .output()
        .expect("Should be able to run command");
    assert!(
        output.status.success(),
        "Should be able to fetch host keys Stdout: {}\n Stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let keyring = keyring_file
        .load()
        .await
        .expect("Should be able to load keyring file");

    assert!(
        !keyring.key.is_empty(),
        "Should have at least 1 entry in keyring"
    );
}

fn create_key(
    keyring_file: impl AsRef<OsStr>,
    secrets_file: &Path,
    label: &str,
    skip_keyring: bool,
) {
    let mut args = vec![
        "run",
        "--features",
        "cli",
        "--bin",
        "bindle",
        "--",
        "keys",
        "create",
        "--secrets-file",
        secrets_file.to_str().unwrap(),
        "--roles",
        "creator,approver",
    ];

    if skip_keyring {
        args.push("--skip-keyring");
    }

    args.push(label);
    let output = std::process::Command::new("cargo")
        .args(args)
        .env(ENV_BINDLE_KEYRING, keyring_file)
        .output()
        .expect("Should be able to run command");
    assert!(output.status.success(), "Should be able to create a key");
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
