use clap::{App, Arg};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Seek, SeekFrom};
use std::path::Path;

const DESCRIPTION: &str = r#"
Transform an AssemblyScript project to a bindle.

This reads a package.json file and the build/ directory to construct a
Bindle from the data.

By default, it attempts to read the local package.json and build/ directory,
and then write the results to a bindir/ directory.

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
            Arg::with_name("src")
                .help("path to directory with package.json")
                .short("s")
                .long("src")
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
    let src_dir = app.value_of("src").unwrap_or("./");
    let bindle_dir = app.value_of("bindir").unwrap_or("./bindir");

    let src_path = Path::new(src_dir);
    let bindle_path = Path::new(bindle_dir);

    // Find and read package.json
    let package_json =
        fs::read_to_string(src_path.join("package.json")).expect("failed to read package.json");
    let package: Package =
        serde_json::from_str(package_json.as_str()).expect("failed to parse package.json");
    // Find target wasm
    let path = src_path.join("build/optimized.wasm");
    if !path.is_file() {
        panic!("no optimized.wasm found in build directory");
    }

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
        sha256: sha.to_owned(),
        size: Some(md.len() as i64),
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
    // Create invoice
    let mut invoice = bindle::Invoice {
        bindle_version: bindle::BINDLE_VERSION_1.to_owned(),
        yanked: None,
        bindle: bindle::BindleSpec {
            name: package.name,
            version: package.version,
            authors: None,
            description: package.description,
        },
        parcels: Some(vec![bindle::Parcel {
            label,
            conditions: None,
        }]),
        annotations: None,
        group: None,
    };

    if let Some(auth) = package.author {
        invoice.bindle.authors = Some(vec![auth])
    }

    // Write invoice
    let out = toml::to_string(&invoice).expect("Serialization of TOML data failed");
    println!("{}", out);
    let bindle_name = bindle::storage::file::canonical_invoice_name(&invoice);
    let invoice_path = bindle_path.join("invoices").join(bindle_name);
    fs::create_dir_all(invoice_path.clone()).expect("creating invoices dir failed");
    fs::write(invoice_path.join("invoice.toml"), out).expect("failed to write invoice");
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    author: Option<String>,
    description: Option<String>,
}
