use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bindle::search::StrictEngine;
use bindle::storage::file::FileStorage;

use tempfile::tempdir;
use tokio::stream::StreamExt;
use tokio::sync::RwLock;

const SCAFFOLD_DIR: &str = "../scaffolds";
const INVOICE_FILE: &str = "invoice.toml";
const PARCEL_DIR: &str = "parcels";
const PARCEL_EXTENSION: &str = "dat";
const LABEL_EXTENSION: &str = "toml";

/// A scaffold loaded from disk, containing the raw bytes for all files in the bindle. Both
/// `label_files` and `parcel_files` are the names of the files on disk with the extension removed.
/// This allows for easy lookups in both maps as they should have the same name
pub struct RawScaffold {
    pub invoice: Vec<u8>,
    pub label_files: HashMap<String, Vec<u8>>,
    pub parcel_files: HashMap<String, Vec<u8>>,
}

impl RawScaffold {
    /// Loads the raw scaffold files. Will panic if the scaffold doesn't exist. Returns a RawScaffold
    /// containing all the raw files
    pub async fn load(name: &str) -> RawScaffold {
        let dir = PathBuf::from(SCAFFOLD_DIR).join(name);

        if !dir.is_dir() {
            panic!("Path {} does not exist or isn't a directory", dir.display());
        }

        let invoice = tokio::fs::read(dir.join(INVOICE_FILE))
            .await
            .expect("unable to read invoice file");

        let mut parcel_files = HashMap::new();
        let mut label_files = HashMap::new();

        while let Some(file) = filter_files(&dir).await.next().await {
            let file_name = file
                .file_stem()
                .expect("got unrecognized file, this is likely a programmer error")
                .to_string_lossy()
                .to_string();
            let file_data = tokio::fs::read(&file)
                .await
                .expect("Unable to read parcel file");
            match file.extension().unwrap_or_default().to_str().unwrap() {
                PARCEL_EXTENSION => parcel_files.insert(file_name, file_data),
                LABEL_EXTENSION => label_files.insert(file_name, file_data),
                _ => panic!("Found unknown extension, this is likely a programmer error"),
            };
        }

        // Do a simple validation to make sure the scaffold has matching files
        if parcel_files.len() != label_files.len() {
            panic!("There are not an equal number of label files and parcel files")
        }

        RawScaffold {
            invoice,
            label_files,
            parcel_files,
        }
    }
}

/// A scaffold loaded from disk, containing the bindle object representations for all files in the
/// bindle (except for the parcels themselves, as they can be binary data). Both `labels and
/// `parcel_files` are the names of the files on disk with the extension removed. This allows for
/// easy lookups in both maps as they should have the same name
pub struct Scaffold {
    pub invoice: bindle::Invoice,
    pub labels: HashMap<String, bindle::Label>,
    pub parcel_files: HashMap<String, Vec<u8>>,
}

impl Scaffold {
    /// Loads the name scaffold from disk and deserializes them to actual bindle objects. Returns a
    /// Scaffold object containing the objects and raw parcel files
    pub async fn load(name: &str) -> Scaffold {
        let mut raw = RawScaffold::load(name).await;
        let invoice: bindle::Invoice =
            toml::from_slice(&raw.invoice).expect("Unable to deserialize invoice TOML");

        let labels = raw
            .label_files
            .drain()
            .map(|(k, v)| {
                (
                    k,
                    toml::from_slice(&v).expect("Unable to deserialize label TOML"),
                )
            })
            .collect();

        Scaffold {
            invoice,
            labels,
            parcel_files: raw.parcel_files,
        }
    }
}

/// Returns a file `Store` implementation configured with a temporary directory and strict Search
/// implementation for use in testing API endpoints
pub fn setup() -> FileStorage<StrictEngine> {
    let temp = tempdir().expect("unable to create tempdir");
    let index = Arc::new(RwLock::new(StrictEngine::default()));
    let store = FileStorage::new(temp.path().to_owned(), index);
    store
}

/// Loads all scaffolds in the scaffolds directory, returning them as a hashmap with the directory
/// name as the key and a `RawScaffold` as a value
pub async fn load_all_files() -> HashMap<String, RawScaffold> {
    let mut all = HashMap::new();
    while let Some(dir) = bindle_dirs().await.next().await {
        let dir_name = dir
            .file_name()
            .expect("got unrecognized directory, this is likely a programmer error")
            .to_string_lossy()
            .to_string();
        let raw = RawScaffold::load(&dir_name).await;
        all.insert(dir_name, raw);
    }
    all
}

/// Loads all scaffolds in the scaffolds directory, returning them as a hashmap with the directory
/// name as the key and a `Scaffold` as a value
pub async fn load_all() -> HashMap<String, Scaffold> {
    // I know this is almost identical to the other function, but to pull it out into a separate
    // function involves a bunch of boxing and extra code that seems overkill for a test harness
    let mut all = HashMap::new();
    while let Some(dir) = bindle_dirs().await.next().await {
        let dir_name = dir
            .file_name()
            .expect("got unrecognized directory, this is likely a programmer error")
            .to_string_lossy()
            .to_string();
        let raw = Scaffold::load(&dir_name).await;
        all.insert(dir_name, raw);
    }
    all
}

/// Filters all items in a parcel directory that do not match the proper extensions
async fn filter_files<P: AsRef<Path>>(root_path: P) -> impl tokio::stream::Stream<Item = PathBuf> {
    tokio::fs::read_dir(root_path.as_ref().join(PARCEL_DIR))
        .await
        .expect("unable to read parcel directory")
        .map(|entry| entry.expect("Error while reading parcel directory").path())
        .filter(|p| {
            let extension = p.extension().unwrap_or_default();
            extension == PARCEL_EXTENSION || extension == LABEL_EXTENSION
        })
}

/// Returns a stream of PathBufs pointing to all directories that are bindles. It does a simple
/// check to see if an item in the scaffolds directory is a directory and if that directory contains
/// an `invoice.toml` file
async fn bindle_dirs() -> impl tokio::stream::Stream<Item = PathBuf> {
    tokio::fs::read_dir(SCAFFOLD_DIR)
        .await
        .expect("unable to read scaffolds directory")
        .map(|entry| entry.expect("Error while reading parcel directory").path())
        .filter(|p| {
            let extension = p.extension().unwrap_or_default();
            (extension == PARCEL_EXTENSION || extension == LABEL_EXTENSION)
                && p.join("invoice.toml").is_file()
        })
}
