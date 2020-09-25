use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

trait Storage {
    /// This takes an invoice and creates it in storage.
    /// It must verify that each referenced box is present in storage. Any box that
    /// is not present must be returned in the list of IDs.
    fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<String>, anyhow::Error>;
    fn get_invoice(&self);
    fn delete_invoice();
    fn create_box();
    fn get_box();
    fn cleanup();
}

struct FileStorage {
    root: Path, // TODO: this should be a path
}

impl FileStorage {
    fn invoice_path(&self) -> PathBuf {
        self.root.join(Path::new("invoice"))
    }
    fn box_path(&self) -> PathBuf {
        self.root.join(Path::new("invoice"))
    }
}

impl Storage for FileStorage {
    fn create_invoice(&self, inv: &super::Invoice) -> Result<Vec<String>, anyhow::Error> {
        let dest = self
            .invoice_path()
            .join(Path::new(inv.bindle.name.as_str()));

        // Open the destination or error out if it already exists.
        let mut out = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(dest)?;

        // Encode the invoice into a TOML object
        let data = toml::to_vec(inv)?;
        out.write_all(data.as_slice())?;

        // Loop through the boxes and see what exists
        let mut missing = Vec::new();

        inv.boxes.iter().for_each(|k| {
            let boxpath = self.box_path().join(Path::new(k.0));
            // Stat k to see if it exists. If it does not exist, add it.
            match std::fs::metadata(boxpath) {
                Ok(stat) => {
                    if !stat.is_dir() {
                        missing.push(k.0.to_owned())
                    }
                }
                Err(_e) => missing.push(k.0.to_owned()),
            }
        });

        Ok(missing)
    }
    fn get_invoice(&self) {}
    fn delete_invoice() {}
    fn create_box() {}
    fn get_box() {}
    fn cleanup() {}
}

#[cfg(test)]
mod test {}
