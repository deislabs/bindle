use clap::{App, Arg};
use sha2::Digest;
use tokio::fs;
use tokio::stream::StreamExt;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

const DESCRIPTION: &str = r#"
Transform a Cargo-based WebAssembly project to a bindle.

This reads a Cargo.toml file and the targets/ directory to construct a
Bindle from the data.

By default, it attempts to read the local Cargo.toml and targets/ directory, and then
write the results to a bindir/ directory as a standalone bindle.
"#;

#[tokio::main]
async fn main() {
    let app = App::new("cargo2bindle")
        .version("0.1.0")
        .author("DeisLabs at Microsoft Azure")
        .about(DESCRIPTION)
        .arg(
            Arg::new("cargo")
                .about("path to directory with cargo.toml")
                .short('c')
                .long("cargo")
                .takes_value(true),
        )
        .arg(
            Arg::new("bindir")
                .about("path to bindle directory")
                .short('b')
                .long("bindle")
                .takes_value(true),
        )
        .get_matches();

    let cargo_dir = app.value_of("cargo").unwrap_or("./");
    let bindle_dir = app.value_of("bindir").unwrap_or("./");

    let cargo_toml = Path::new(cargo_dir).join("Cargo.toml");

    // Find and parse Cargo.toml
    let cargo_raw = fs::read_to_string(cargo_toml)
        .await
        .expect("Cargo.toml failed to load");
    let cargo: Cargo = toml::from_str(cargo_raw.as_str()).expect("Cargo.toml failed to parse");

    // Find target wasm
    let wasm_release_dir = Path::new(cargo_dir).join("target/wasm32-wasi/release");
    if !wasm_release_dir.is_dir() {
        panic!("not a directory");
    }

    let parcel_map = Arc::new(Mutex::new(HashMap::new()));
    let stream = fs::read_dir(wasm_release_dir)
        .await
        .expect("unable to read wasm release directory");

    // Create label and parcel
    let paths: Vec<PathBuf> = stream
        .filter(|entry| {
            // We only want non-directory items with the extension '.wasm'
            let path = entry.as_ref().unwrap().path();
            !path.is_dir() && path.extension().unwrap_or_else(|| std::ffi::OsStr::new("")) == "wasm"
        })
        .map(|entry| entry.unwrap().path())
        .collect()
        .await;

    let parcel_futures =
        paths
            .into_iter()
            .map(|p| (p, parcel_map.clone()))
            .map(|(path, parcel_map)| async move {
                // Calculate the hash of the WASM file
                let mut file = tokio::fs::File::open(&path)
                    .await
                    .expect("file cannot be opened");
                let mut hasher = bindle::async_util::AsyncSha256::new();
                tokio::io::copy(&mut file, &mut hasher)
                    .await
                    .expect("hashing file failed");
                let sha = format!("{:x}", hasher.into_inner().unwrap().finalize());
                file.seek(SeekFrom::Start(0))
                    .await
                    .expect("failed to seek to begining of WASM file");
                parcel_map.lock().await.insert(sha.clone(), file);

                // Create the label object
                let md = tokio::fs::metadata(&path).await.expect("failed to stat");
                let label = bindle::Label {
                    name: format!("{}", path.file_name().unwrap().to_string_lossy()),
                    media_type: "application/wasm".to_owned(),
                    sha256: sha,
                    size: md.len() as u64,
                    ..bindle::Label::default()
                };

                // Return the parcel section to be added to the invoice.
                bindle::Parcel {
                    label,
                    conditions: None,
                }
            });

    let parcels: Vec<bindle::Parcel> = futures::future::join_all(parcel_futures).await;

    // Create invoice
    let mut invoice = bindle::Invoice {
        bindle_version: bindle::BINDLE_VERSION_1.to_owned(),
        yanked: None,
        bindle: bindle::BindleSpec {
            id: format!("{}/{}", cargo.package.name, cargo.package.version)
                .parse()
                .expect("Missing name or version information"),
            authors: cargo.package.authors,
            description: cargo.package.description,
        },
        parcel: None,
        annotations: None,
        group: None,
    };

    if !parcels.is_empty() {
        invoice.parcel = Some(parcels);
    }

    // Write invoice
    let standalone = bindle::standalone::StandaloneWrite::new(bindle_dir, &invoice.bindle.id)
        .expect("Invalid invoice");
    standalone
        .write(invoice, Arc::try_unwrap(parcel_map).unwrap().into_inner())
        .await
        .expect("unable to write data to standalone bindle");
    println!("Wrote bindle to {}", standalone.path().display());
}

/* This is very similar to a BindleSpec, but BindleSpec has deny_unknown_fields
[package]
name = "bindle"
version = "0.1.0"
authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
edition = "2018"
*/

#[derive(Deserialize)]
struct Cargo {
    package: Package,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    authors: Option<Vec<String>>,
    description: Option<String>,
}
