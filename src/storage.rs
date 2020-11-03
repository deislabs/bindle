use sha2::{Digest, Sha256};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// The folder name for the invoices directory
const INVOICE_DIRECTORY: &str = "invoices";
/// The folder name for the parcels directory
const PARCEL_DIRECTORY: &str = "parcels";
const INVOICE_TOML: &str = "invoice.toml";
const PARCEL_DAT: &str = "parcel.dat";
const LABEL_TOML: &str = "label.toml";

pub trait Storage {
    /// This takes an invoice and creates it in storage.
    /// It must verify that each referenced box is present in storage. Any box that
    /// is not present must be returned in the list of IDs.
    fn create_invoice(&mut self, inv: &super::Invoice) -> Result<Vec<super::Label>, StorageError>;
    /// Load an invoice and return it
    ///
    /// This will return an invoice if the bindle exists and is not yanked
    fn get_invoice(&self, id: String) -> Result<super::Invoice, StorageError>;
    /// Load an invoice, even if it is yanked.
    fn get_yanked_invoice(&self, id: String) -> Result<super::Invoice, StorageError>;
    /// Remove an invoice
    ///
    /// Because invoices are not necessarily stored using just one field on the invoice,
    /// the entire invoice must be passed to the deletion command.
    fn yank_invoice(&mut self, inv: &mut super::Invoice) -> Result<(), StorageError>;
    fn create_parcel(
        &self,
        label: &super::Label,
        data: &mut std::fs::File,
    ) -> Result<(), StorageError>;
    fn get_parcel(&self, label: &crate::Label) -> Result<std::fs::File, StorageError>;
    /// Get the label for a parcel
    ///
    /// This reads the label from storage and then parses it into a Label object.
    fn get_label(&self, parcel_id: &str) -> Result<crate::Label, StorageError>;
}

/// StorageError describes the possible error states when storing and retrieving bindles.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("bindle is yanked")]
    Yanked,
    #[error("resource not found")]
    NotFound,
    #[error("resource could not be loaded")]
    IO(#[from] std::io::Error),
    #[error("resource already exists")]
    Exists,

    // TODO: Investigate how to make this more helpful
    #[error("resource is malformed")]
    Malformed(#[from] toml::de::Error),
    #[error("resource cannot be stored")]
    Unserializable(#[from] toml::ser::Error),
}

/// A file system backend for storing and retriving bindles and parcles.
///
/// Given a root directory, FileStorage brings its own storage layout for keeping track
/// of Bindles.
///
/// A FileStorage needs a search engine implementation. When invoices are created or yanked,
/// the index will be updated.
pub struct FileStorage<T: crate::search::Search> {
    root: String, // TODO: this should be a path
    index: T,
}

impl<T: crate::search::Search> FileStorage<T> {
    /// Create a standard name for an invoice
    ///
    /// This is designed to create a repeatable opaque name when given an invoice.
    fn canonical_invoice_name(&self, inv: &crate::Invoice) -> String {
        self.canonical_invoice_name_strings(inv.bindle.name.as_str(), inv.bindle.version.as_str())
    }

    /// Given a name and a version, this returns a repeatable name for an on-disk location.
    ///
    /// We don't typically want to store a bindle with its name and version number. This
    /// would impose both naming constraints on the bindle and security issues on the
    /// storage layout. So this function hashes the name/version data (which together
    /// MUST be unique in the system) and uses the resulting hash as the canonical
    /// name. The hash is guaranteed to be in the character set [a-zA-Z0-9].
    fn canonical_invoice_name_strings(&self, name: &str, version: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(name.as_bytes());
        hasher.update(version.as_bytes());
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// Return the path to the invoice directory for a particular bindle.
    fn invoice_path(&self, invoice_id: &str) -> PathBuf {
        Path::new(self.root.as_str())
            .join(INVOICE_DIRECTORY)
            .join(invoice_id)
    }
    /// Return the path for an invoice.toml for a particular bindle.
    fn invoice_toml_path(&self, invoice_id: &str) -> PathBuf {
        self.invoice_path(invoice_id).join(INVOICE_TOML)
    }
    /// Return the parcel-specific path for storing a parcel.
    fn parcel_path(&self, parcel_id: &str) -> PathBuf {
        Path::new(self.root.as_str())
            .join(PARCEL_DIRECTORY)
            .join(parcel_id)
    }
    /// Return the path to a parcel.toml for a specific parcel.
    fn label_toml_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_path(parcel_id).join(LABEL_TOML)
    }
    /// Return the path to the parcel.dat file for the given box ID
    fn parcel_data_path(&self, parcel_id: &str) -> PathBuf {
        self.parcel_path(parcel_id).join(PARCEL_DAT)
    }
}

impl<T: crate::search::Search> Storage for FileStorage<T> {
    fn create_invoice(&mut self, inv: &super::Invoice) -> Result<Vec<super::Label>, StorageError> {
        // It is illegal to create a yanked invoice.
        if inv.yanked.unwrap_or(false) {
            return Err(StorageError::Yanked);
        }

        let invoice_cname = self.canonical_invoice_name(inv);
        let invoice_id = invoice_cname.as_str();

        // Create the base path if necessary
        let inv_path = self.invoice_path(invoice_id);
        if !inv_path.is_dir() {
            // If it exists and is a regular file, we have a problem
            if inv_path.is_file() {
                return Err(StorageError::Exists);
            }
            create_dir_all(inv_path)?;
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(invoice_id);
        let mut out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(dest)?;

        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        out.write_all(data.as_slice())?;

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv) {
            eprintln!("Error indexing {}: {}", invoice_id, e);
        }

        // if there are no parcels, bail early
        if inv.parcels.is_none() {
            return Ok(vec![]);
        }

        // Loop through the boxes and see what exists
        let missing = inv.parcels.as_ref().unwrap().iter().filter(|k| {
            let parcel_path = self.parcel_path(k.label.name.as_str());
            // Stat k to see if it exists. If it does not exist, add it.
            match std::fs::metadata(parcel_path) {
                Ok(stat) => !stat.is_dir(),
                Err(_e) => true,
            }
        });

        Ok(missing.map(|p| p.label.clone()).collect())
    }
    fn get_invoice(&self, id: String) -> Result<super::Invoice, StorageError> {
        match self.get_yanked_invoice(id) {
            Ok(inv) if !inv.yanked.unwrap_or(false) => Ok(inv),
            Err(e) => Err(e),
            _ => Err(StorageError::Yanked),
        }
    }
    fn get_yanked_invoice(&self, id: String) -> Result<super::Invoice, StorageError> {
        // TODO: Parse the id into an invoice name and version.
        let id_path = Path::new(&id);
        let parent = id_path.parent();
        if parent.is_none() {
            return Err(StorageError::NotFound);
        }

        let name = parent.unwrap().to_str().unwrap();

        let version_part = id_path.file_name();
        if version_part.is_none() {
            return Err(StorageError::NotFound);
        }
        let version = version_part.unwrap().to_str().unwrap();

        let invoice_cname = self.canonical_invoice_name_strings(name, version);
        let invoice_id = invoice_cname.as_str();

        // Now construct a path and read it
        let invoice_path = self.invoice_toml_path(invoice_id);

        // Open file
        let inv_toml = std::fs::read_to_string(invoice_path)?;

        // Parse
        let invoice: crate::Invoice = toml::from_str(inv_toml.as_str())?;

        // Return object
        Ok(invoice)
    }
    fn yank_invoice(&mut self, inv: &mut super::Invoice) -> Result<(), StorageError> {
        let invoice_cname = self.canonical_invoice_name(inv);
        let invoice_id = invoice_cname.as_str();
        // Load the invoice and mark it as yanked.
        inv.yanked = Some(true);

        // Attempt to update the index. Right now, we log an error if the index update
        // fails.
        if let Err(e) = self.index.index(&inv) {
            eprintln!("Error indexing {}: {}", invoice_id, e);
        }

        // Open the destination or error out if it already exists.
        let dest = self.invoice_toml_path(invoice_id);

        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        // NOTE: Right now, this just force-overwites the existing invoice. We are assuming
        // that the bindle has already been confirmed to be present. However, we have not
        // ensured that here. So it is theoretically possible (if get_invoice was not used)
        // to build the invoice) that this could _create_ a new file. We could probably change
        // this behavior with OpenOptions.

        std::fs::write(dest, data)?;
        Ok(())
    }
    fn create_parcel(
        &self,
        label: &super::Label,
        data: &mut std::fs::File,
    ) -> Result<(), StorageError> {
        let sha = label.sha256.as_str();
        // Test if a dir with that SHA exists. If so, this is an error.
        let par_path = self.parcel_path(sha);
        if par_path.is_file() {
            return Err(StorageError::Exists);
        }
        // Create box dir
        create_dir_all(par_path)?;

        // Write data
        {
            let data_file = self.parcel_data_path(sha);
            let mut out = OpenOptions::new()
                .create_new(true)
                .write(true)
                .read(true)
                .open(data_file)?;

            std::io::copy(data, &mut out)?;
        }

        // Write label
        {
            let dest = self.label_toml_path(sha);
            let mut out = OpenOptions::new()
                .create_new(true)
                .write(true)
                .read(true)
                .open(dest)?;

            let data = toml::to_vec(label)?;
            out.write_all(data.as_slice())?;
        }
        Ok(())
    }
    fn get_parcel(&self, label: &crate::Label) -> Result<std::fs::File, StorageError> {
        let name = self.parcel_data_path(label.sha256.as_str());
        let reader = File::open(name)?;
        Ok(reader)
    }
    fn get_label(&self, parcel_id: &str) -> Result<crate::Label, StorageError> {
        let label_path = self.label_toml_path(parcel_id);
        let label_toml = std::fs::read_to_string(label_path)?;
        let label: crate::Label = toml::from_str(label_toml.as_str())?;

        // Return object
        Ok(label)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Invoice;
    use tempfile::tempdir;

    #[test]
    fn test_should_generate_paths() {
        let f = FileStorage {
            root: "test".to_owned(),
            index: crate::search::StrictEngine::default(),
        };
        assert_eq!("test/invoices/123", f.invoice_path("123").to_str().unwrap());
        assert_eq!(
            "test/invoices/123/invoice.toml",
            f.invoice_toml_path("123").to_str().unwrap()
        );
        assert_eq!(
            "test/parcels/123".to_owned(),
            f.parcel_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/parcels/123/label.toml".to_owned(),
            f.label_toml_path("123").to_string_lossy()
        );
        assert_eq!(
            "test/parcels/123/parcel.dat".to_owned(),
            f.parcel_data_path("123").to_string_lossy()
        );
    }

    #[test]
    fn test_should_create_yank_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let mut inv = invoice_fixture();
        let mut store = FileStorage {
            root: root.path().to_str().unwrap().to_owned(),
            index: crate::search::StrictEngine::default(),
        };
        let inv_cname = store.canonical_invoice_name(&inv);
        let inv_name = inv_cname.as_str();
        // Create an file
        let missing = store.create_invoice(&inv).unwrap();
        assert_eq!(3, missing.len());

        // Out-of-band read the invoice
        assert!(store.invoice_toml_path(inv_name).exists());

        // Yank the invoice
        store.yank_invoice(&mut inv).unwrap();

        // Make sure the invoice is yanked
        let inv2 = store
            .get_yanked_invoice(crate::invoice_to_name(&inv))
            .unwrap();
        assert!(inv2.yanked.unwrap_or(false));

        // Sanity check that this produces an error
        assert!(store.get_invoice(crate::invoice_to_name(&inv)).is_err());

        // Drop the temporary directory
        assert!(root.close().is_ok());
    }

    #[test]
    fn test_should_reject_yanked_invoice() {
        // Create a temporary directory
        let root = tempdir().unwrap();
        let mut inv = invoice_fixture();
        inv.yanked = Some(true);
        let mut store = FileStorage {
            root: root.path().to_str().unwrap().to_owned(),
            index: crate::search::StrictEngine::default(),
        };
        // Create an file
        assert!(store.create_invoice(&inv).is_err());
        assert!(root.close().is_ok());
    }

    #[test]
    fn test_should_write_read_parcel() {
        let id = "abcdef1234567890987654321";
        let (label, mut data) = parcel_fixture(id);
        let root = tempdir().expect("create tempdir");
        let store = FileStorage {
            root: root.path().to_str().expect("root path").to_owned(),
            index: crate::search::StrictEngine::default(),
        };

        store
            .create_parcel(&label, &mut data)
            .expect("create parcel");

        // Now attempt to read just the label

        let label2 = store.get_label(id).expect("fetch label after saving");
        let mut data = String::new();
        store
            .get_parcel(&label2)
            .expect("load parcel data")
            .read_to_string(&mut data)
            .expect("read file into string");
        assert_eq!(data, "hello\n");
    }

    #[test]
    fn test_should_store_and_retrieve_bindle() {
        let root = tempdir().expect("create tempdir");
        let mut store = FileStorage {
            root: root.path().to_str().expect("root path").to_owned(),
            index: crate::search::StrictEngine::default(),
        };

        // Store a parcel
        let id = "abcdef1234567890987654321";
        let (label, mut data) = parcel_fixture(id);
        let mut invoice = invoice_fixture();
        let inv_name = crate::invoice_to_name(&invoice);

        let parcel = crate::Parcel {
            label: label.clone(),
            conditions: None,
        };
        invoice.parcels = Some(vec![parcel]);

        store
            .create_parcel(&label, &mut data)
            .expect("stored the parcel");

        // Store an invoice that points to that parcel

        store.create_invoice(&invoice).expect("create parcel");

        // Get the bindle
        let inv = store
            .get_invoice(inv_name)
            .expect("get the invoice we just stored");

        let first_parcel = inv
            .parcels
            .expect("parsel vector")
            .pop()
            .expect("got a parcel");
        assert_eq!(first_parcel.label.name, "foo.toml".to_owned())
    }

    fn parcel_fixture(id: &str) -> (crate::Label, std::fs::File) {
        let mut data = tempfile::tempfile().unwrap();
        writeln!(data, "hello").expect("data written");
        data.flush().expect("flush the file");
        data.seek(SeekFrom::Start(0))
            .expect("reset read pointer to head");
        (
            crate::Label {
                sha256: id.to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo.toml".to_owned(),
                size: Some(6),
                annotations: None,
            },
            data,
        )
    }

    fn invoice_fixture() -> Invoice {
        let labels = vec![
            crate::Label {
                sha256: "abcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo.toml".to_owned(),
                size: Some(101),
                annotations: None,
            },
            crate::Label {
                sha256: "bbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo2.toml".to_owned(),
                size: Some(101),
                annotations: None,
            },
            crate::Label {
                sha256: "cbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo3.toml".to_owned(),
                size: Some(101),
                annotations: None,
            },
        ];

        Invoice {
            bindle_version: crate::BINDLE_VERSION_1.to_owned(),
            yanked: None,
            annotations: None,
            bindle: crate::BindleSpec {
                name: "foo".to_owned(),
                description: Some("bar".to_owned()),
                version: "v1.2.3".to_owned(),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcels: Some(
                labels
                    .iter()
                    .map(|l| crate::Parcel {
                        label: l.clone(),
                        conditions: None,
                    })
                    .collect(),
            ),
            group: None,
        }
    }
}
