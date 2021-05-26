//! Some helpful utilities for testing. This module is only available if the `test-tools` feature is
//! enabled. Its main feature is allowing the use of prebuilt scaffolds for testing.
//!
//! See the [README](https://github.com/deislabs/bindle/blob/master/tests/scaffolds/README.md) in
//! the testing directory of bindle for more information on scaffolding structure. All loading
//! functions will load scaffolds by default from `$CARGO_MANIFEST_DIR/tests/scaffolds`. However,
//! this directory can be configured by setting the `BINDLE_SCAFFOLD_DIR` environment variable to
//! your desired path. All functions will panic if they encounter an error to make it easier on
//! users (so they don't have to handle the errors in their tests in the exact same way)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::invoice::signature::{KeyRing, SecretKeyEntry, SecretKeyStorage, SignatureRole};
use crate::provider::file::FileProvider;
use crate::search::StrictEngine;

use sha2::{Digest, Sha256};
use tempfile::tempdir;

const SCAFFOLD_DIR: &str = "tests/scaffolds";
const INVOICE_FILE: &str = "invoice.toml";
const PARCEL_DIR: &str = "parcels";
const PARCEL_EXTENSION: &str = "dat";

/// The environment variable name used for setting the scaffolds directory
pub const SCAFFOLD_DIR_ENV: &str = "BINDLE_SCAFFOLD_DIR";

fn default_scaffold_dir() -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let mut path = PathBuf::from(root);
    path.push(SCAFFOLD_DIR);
    path
}

fn scaffold_dir() -> PathBuf {
    std::env::var(SCAFFOLD_DIR_ENV)
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(default_scaffold_dir)
}

/// A struct containing the SHA of the data and the data as bytes
#[derive(Clone, Debug)]
pub struct ParcelInfo {
    pub sha: String,
    pub data: Vec<u8>,
}

/// A scaffold loaded from disk, containing the raw bytes for all files in the bindle.
#[derive(Clone, Debug)]
pub struct RawScaffold {
    pub invoice: Vec<u8>,
    pub parcel_files: HashMap<String, ParcelInfo>,
}

impl RawScaffold {
    /// Loads the raw scaffold files. Will panic if the scaffold doesn't exist. Returns a RawScaffold
    /// containing all the raw files.
    pub async fn load(name: &str) -> RawScaffold {
        let dir = scaffold_dir().join(name);

        if !dir.is_dir() {
            panic!("Path {} does not exist or isn't a directory", dir.display());
        }

        let invoice = tokio::fs::read(dir.join(INVOICE_FILE))
            .await
            .expect("unable to read invoice file");

        let files = match filter_files(&dir).await {
            Some(s) => s,
            None => {
                return RawScaffold {
                    invoice,
                    parcel_files: HashMap::new(),
                }
            }
        };

        let file_futures = files.into_iter().map(|file| async move {
            let file_name = file
                .file_stem()
                .expect("got unrecognized file, this is likely a programmer error")
                .to_string_lossy()
                .to_string();
            let file_data = tokio::fs::read(&file)
                .await
                .expect("Unable to read parcel file");
            match file.extension().unwrap_or_default().to_str().unwrap() {
                PARCEL_EXTENSION => (
                    file_name,
                    ParcelInfo {
                        sha: format!("{:x}", Sha256::digest(&file_data)),
                        data: file_data,
                    },
                ),
                _ => panic!("Found unknown extension, this is likely a programmer error"),
            }
        });

        RawScaffold {
            invoice,
            parcel_files: futures::future::join_all(file_futures)
                .await
                .into_iter()
                .collect(),
        }
    }
}

// This shouldn't fail as we built it from deserializing them
impl From<Scaffold> for RawScaffold {
    fn from(s: Scaffold) -> RawScaffold {
        let invoice = toml::to_vec(&s.invoice).expect("Reserialization shouldn't fail");

        RawScaffold {
            invoice,
            parcel_files: s.parcel_files,
        }
    }
}

/// A scaffold loaded from disk, containing the bindle object representations for all files in the
/// bindle (except for the parcels themselves, as they can be binary data).
#[derive(Clone, Debug)]
pub struct Scaffold {
    pub invoice: crate::Invoice,
    pub parcel_files: HashMap<String, ParcelInfo>,
}

impl Scaffold {
    /// Loads the name scaffold from disk and deserializes them to actual bindle objects. Returns a
    /// Scaffold object containing the objects and raw parcel files
    pub async fn load(name: &str) -> Scaffold {
        let raw = RawScaffold::load(name).await;
        raw.into()
    }
}

// Because this is a test, just panic if conversion fails
impl From<RawScaffold> for Scaffold {
    fn from(raw: RawScaffold) -> Scaffold {
        let invoice: crate::Invoice =
            toml::from_slice(&raw.invoice).expect("Unable to deserialize invoice TOML");

        Scaffold {
            invoice,
            parcel_files: raw.parcel_files,
        }
    }
}

/// Returns a file `Store` implementation configured with a temporary directory and strict Search
/// implementation for use in testing API endpoints
pub async fn setup() -> (FileProvider<StrictEngine>, StrictEngine, MockKeyStore) {
    let temp = tempdir().expect("unable to create tempdir");
    let index = StrictEngine::default();
    let store = FileProvider::new(temp.path().to_owned(), index.clone(), KeyRing::default()).await;
    let kstore = MockKeyStore::new();
    (store, index, kstore)
}

/// Loads all scaffolds in the scaffolds directory, returning them as a hashmap with the directory
/// name as the key and a `RawScaffold` as a value. There is not an equivalent for loading all
/// scaffolds as a `Scaffold` object, because some of them may be invalid on will not deserialize
/// properly
pub async fn load_all_files() -> HashMap<String, RawScaffold> {
    let dirs = bindle_dirs().await;
    let dir_futures = dirs.into_iter().map(|dir| async move {
        let dir_name = dir
            .file_name()
            .expect("got unrecognized directory, this is likely a programmer error")
            .to_string_lossy()
            .to_string();
        let raw = RawScaffold::load(&dir_name).await;
        (dir_name, raw)
    });
    futures::future::join_all(dir_futures)
        .await
        .into_iter()
        .collect()
}

/// Filters all items in a parcel directory that do not match the proper extensions. Returns None if
/// there isn't a parcel directory
async fn filter_files<P: AsRef<Path>>(root_path: P) -> Option<Vec<PathBuf>> {
    let mut readdir = match tokio::fs::read_dir(root_path.as_ref().join(PARCEL_DIR)).await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("unable to read parcel directory: {:?}", e),
    };
    let mut files = Vec::new();
    while let Some(entry) = readdir
        .next_entry()
        .await
        .expect("unable to read parcel directory")
    {
        let path = entry.path();
        if path.extension().unwrap_or_default() == PARCEL_EXTENSION {
            files.push(path);
        }
    }
    Some(files)
}

/// Returns a Vec of PathBufs pointing to all directories that are bindles. It does a simple check
/// to see if an item in the scaffolds directory is a directory and if that directory contains an
/// `invoice.toml` file
async fn bindle_dirs() -> Vec<PathBuf> {
    let mut readdir = tokio::fs::read_dir(scaffold_dir())
        .await
        .expect("unable to read scaffolds directory");

    let mut directories = Vec::new();
    while let Some(entry) = readdir
        .next_entry()
        .await
        .expect("unable to read scaffold directory")
    {
        let path = entry.path();
        if path.is_dir() && path.join("invoice.toml").is_file() {
            directories.push(path);
        }
    }

    directories
}

#[derive(Clone)]
pub struct MockKeyStore {
    mock_secret_key: SecretKeyEntry,
}

impl MockKeyStore {
    pub fn new() -> Self {
        MockKeyStore {
            mock_secret_key: SecretKeyEntry::new(
                "Test <test@example.com>".to_owned(),
                vec![SignatureRole::Host],
            ),
        }
    }
}

impl SecretKeyStorage for MockKeyStore {
    fn get_first_matching(&self, _role: &SignatureRole) -> Option<&SecretKeyEntry> {
        Some(&self.mock_secret_key)
    }
}
