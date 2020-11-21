use clap::{App, Arg};
use sha2::{Digest, Sha256};

use std::fs;
use std::io::{Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

const DESCRIPTION: &str = r#"
Transform a Cargo-based WebAssembly project to a bindle.

This reads a Cargo.toml file and the targets/ directory to construct a
Bindle from the data.

By default, it attempts to read the local Cargo.toml and targets/ directory, and then
write the results to a bindir/ directory.

This will create a directory structure like this (where 'bindir' is the value
of -b/--bindle):

bindir
├── invoices
│   └── 2b535f67f3ba68dee98490aba87bc2568aaa42c76f4d03b030eb76ab7a496b0b
│       └── invoice.toml
└── parcels
    └── 47a3286c12385212d6c1f5d188cbaba402181bf94530d59a8054ee9257514cf6
        ├── label.toml
        └── parcel.dat

This on-disk layout is the same as is used by the Bindle server's file storage backend.
"#;

fn main() {
    let app = App::new("cargo2bindle")
        .version("0.1.0")
        .author("DeisLabs at Microsoft Azure")
        .about(DESCRIPTION)
        .arg(
            Arg::with_name("cargo")
                .help("path to directory with cargo.toml")
                .short("c")
                .long("cargo")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bindir")
                .help("path to bindle directory.")
                .short("b")
                .long("bindle")
                .takes_value(true),
        )
        .get_matches();

    let cargo_dir = app.value_of("cargo").unwrap_or("./");
    let bindle_dir = app.value_of("bindir").unwrap_or("./bindir");

    let cargo_toml = Path::new(cargo_dir).join("Cargo.toml");
    let bindle_path = Path::new(bindle_dir);

    // Find and parse Cargo.toml
    let cargo_raw = fs::read_to_string(cargo_toml).expect("Cargo.toml failed to load");
    let cargo: Cargo = toml::from_str(cargo_raw.as_str()).expect("Cargo.toml failed to parse");

    // Find target wasm
    let wasm_release_dir = Path::new(cargo_dir).join("target/wasm32-wasi/release");
    if !wasm_release_dir.is_dir() {
        panic!("not a directory");
    }

    // Create label and parcel
    let parcels: Vec<bindle::Parcel> = fs::read_dir(wasm_release_dir)
        .unwrap()
        .filter(|entry| {
            // We only want non-directory items with the extension '.wasm'
            let path = entry.as_ref().unwrap().path();
            !path.is_dir() && path.extension().unwrap_or_else(|| std::ffi::OsStr::new("")) == "wasm"
        })
        .map(|entry| {
            // TODO: This is admittedly way to much to do in one closure.
            let path = entry.unwrap().path();
            // Calculate the hash of the WASM file
            let mut file = std::fs::File::open(&path).expect("file cannot be opened");
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher).expect("hashing file failed");
            let sha = format!("{:x}", hasher.finalize());

            // Create the label object
            let md = path.metadata().expect("failed to stat");
            let label = bindle::Label {
                name: format!("{}", path.file_name().unwrap().to_string_lossy()),
                media_type: "application/wasm".to_owned(),
                sha256: sha,
                size: md.len() as u64,
                annotations: None,
            };

            // Write the parcel
            let parcel_path = bindle_path.join("parcels").join(label.sha256.clone());
            let parcel_dat = parcel_path.join("parcel.dat");
            let label_path = parcel_path.join("label.toml");

            fs::create_dir_all(parcel_path).expect("creating parcel dir failed");
            let label_toml = toml::to_string(&label).expect("failed to serialize label.toml");
            fs::write(label_path, label_toml).expect("failed to write label.toml");
            let mut dat = fs::File::create(parcel_dat).expect("failed to create parcel.dat");
            file.seek(SeekFrom::Start(0))
                .expect("failed to seek to begining of WASM file");
            std::io::copy(&mut file, &mut dat).expect("failed to copy data");

            // Return the parcel section to be added to the invoice.
            bindle::Parcel {
                label,
                conditions: None,
            }
        })
        .collect();

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
        parcels: None,
        annotations: None,
        group: None,
    };

    if !parcels.is_empty() {
        invoice.parcels = Some(parcels);
    }

    // Write invoice
    let out = toml::to_string(&invoice).expect("Serialization of TOML data failed");
    println!("{}", out);
    let bindle_name = invoice.canonical_name();
    let invoice_path = bindle_path.join("invoices").join(bindle_name);
    fs::create_dir_all(invoice_path.clone()).expect("creating invoices dir failed");
    fs::write(invoice_path.join("invoice.toml"), out).expect("failed to write invoice");
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
