use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bindle::search::StrictEngine;
use bindle::storage::file::FileStorage;

use multipart::client::lazy::Multipart;
use tempfile::tempdir;
use tokio::stream::StreamExt;
use tokio::sync::RwLock;

const SCAFFOLD_DIR: &str = "tests/scaffolds";
const INVOICE_FILE: &str = "invoice.toml";
const PARCEL_DIR: &str = "parcels";
const PARCEL_EXTENSION: &str = "dat";
const LABEL_EXTENSION: &str = "toml";

fn scaffold_dir() -> PathBuf {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Unable to get project directory");
    let mut path = PathBuf::from(root);
    path.push(SCAFFOLD_DIR);
    path
}

/// A scaffold loaded from disk, containing the raw bytes for all files in the bindle. Both
/// `label_files` and `parcel_files` are the names of the files on disk with the extension removed.
/// This allows for easy lookups in both maps as they should have the same name
#[derive(Clone, Debug)]
pub struct RawScaffold {
    pub invoice: Vec<u8>,
    pub label_files: HashMap<String, Vec<u8>>,
    pub parcel_files: HashMap<String, Vec<u8>>,
}

impl RawScaffold {
    /// Loads the raw scaffold files. Will panic if the scaffold doesn't exist. Returns a RawScaffold
    /// containing all the raw files
    pub async fn load(name: &str) -> RawScaffold {
        let dir = scaffold_dir().join(name);

        if !dir.is_dir() {
            panic!("Path {} does not exist or isn't a directory", dir.display());
        }

        let invoice = tokio::fs::read(dir.join(INVOICE_FILE))
            .await
            .expect("unable to read invoice file");

        let mut parcel_files = HashMap::new();
        let mut label_files = HashMap::new();

        let mut files = match filter_files(&dir).await {
            Some(s) => s,
            None => {
                return RawScaffold {
                    invoice,
                    label_files,
                    parcel_files,
                }
            }
        };
        while let Some(file) = files.next().await {
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

    /// Prepares a raw multipart body for the given parcel name for sending in a request and returns
    /// a warp test `RequestBuilder` with the body and `Content-Type` header set
    pub fn parcel_body(&self, parcel_name: &str) -> warp::test::RequestBuilder {
        let label = self
            .label_files
            .get(parcel_name)
            .expect("non existent parcel in scaffold");
        let data = self
            .parcel_files
            .get(parcel_name)
            .expect("non existent parcel in scaffold");
        // This behaves like a stack, so put on the label last
        let mut mp = Multipart::new()
            .add_stream(
                format!("{}.{}", parcel_name, PARCEL_EXTENSION),
                std::io::Cursor::new(data.clone()),
                None::<&str>,
                None,
            )
            .add_stream(
                format!("{}.{}", parcel_name, LABEL_EXTENSION),
                std::io::Cursor::new(label.clone()),
                None::<&str>,
                Some("application/toml".parse().unwrap()),
            )
            .prepare()
            .expect("valid multipart body");

        let mut body = Vec::new();
        mp.read_to_end(&mut body).expect("Unable to read out body");
        warp::test::request()
            .header(
                "Content-Type",
                format!("multipart/form-data;boundary={}", mp.boundary().to_owned()),
            )
            .body(body)
    }
}

// This shouldn't fail as we built it from deserializing them
impl From<Scaffold> for RawScaffold {
    fn from(mut s: Scaffold) -> RawScaffold {
        let mut label_files = HashMap::new();
        for (k, v) in s.labels.drain() {
            label_files.insert(k, toml::to_vec(&v).expect("Reserialization shouldn't fail"));
        }
        let invoice = toml::to_vec(&s.invoice).expect("Reserialization shouldn't fail");

        RawScaffold {
            invoice,
            label_files,
            parcel_files: s.parcel_files,
        }
    }
}

/// A scaffold loaded from disk, containing the bindle object representations for all files in the
/// bindle (except for the parcels themselves, as they can be binary data). Both `labels and
/// `parcel_files` are the names of the files on disk with the extension removed. This allows for
/// easy lookups in both maps as they should have the same name
#[derive(Clone, Debug)]
pub struct Scaffold {
    pub invoice: bindle::Invoice,
    pub labels: HashMap<String, bindle::Label>,
    pub parcel_files: HashMap<String, Vec<u8>>,
}

impl Scaffold {
    /// Loads the name scaffold from disk and deserializes them to actual bindle objects. Returns a
    /// Scaffold object containing the objects and raw parcel files
    pub async fn load(name: &str) -> Scaffold {
        let raw = RawScaffold::load(name).await;
        raw.into()
    }
}

// Because this is a test, just panicing if conversion fails
impl From<RawScaffold> for Scaffold {
    fn from(mut raw: RawScaffold) -> Scaffold {
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
pub async fn setup() -> (FileStorage<StrictEngine>, Arc<RwLock<StrictEngine>>) {
    let temp = tempdir().expect("unable to create tempdir");
    let index = Arc::new(RwLock::new(StrictEngine::default()));
    let store = FileStorage::new(temp.path().to_owned(), index.clone()).await;
    (store, index)
}

/// Loads all scaffolds in the scaffolds directory, returning them as a hashmap with the directory
/// name as the key and a `RawScaffold` as a value. There is not an equivalent for loading all
/// scaffolds as a `Scaffold` object, because some of them may be invalid on will not deserialize
/// properly
pub async fn load_all_files() -> HashMap<String, RawScaffold> {
    let mut all = HashMap::new();
    let mut dirs = bindle_dirs().await;
    while let Some(dir) = dirs.next().await {
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

/// Filters all items in a parcel directory that do not match the proper extensions. Returns None if there isn't a parcel directory
async fn filter_files<P: AsRef<Path>>(
    root_path: P,
) -> Option<impl tokio::stream::Stream<Item = PathBuf>> {
    let readdir = match tokio::fs::read_dir(root_path.as_ref().join(PARCEL_DIR)).await {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("unable to read parcel directory: {:?}", e),
    };
    Some(
        readdir
            .map(|entry| entry.expect("Error while reading parcel directory").path())
            .filter(|p| {
                let extension = p.extension().unwrap_or_default();
                extension == PARCEL_EXTENSION || extension == LABEL_EXTENSION
            }),
    )
}

/// Returns a stream of PathBufs pointing to all directories that are bindles. It does a simple
/// check to see if an item in the scaffolds directory is a directory and if that directory contains
/// an `invoice.toml` file
async fn bindle_dirs() -> impl tokio::stream::Stream<Item = PathBuf> {
    tokio::fs::read_dir(scaffold_dir())
        .await
        .expect("unable to read scaffolds directory")
        .map(|entry| entry.expect("Error while reading parcel directory").path())
        .filter(|p| p.is_dir() && p.join("invoice.toml").is_file())
}
