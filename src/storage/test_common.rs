use std::io::SeekFrom;

use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub async fn parcel_fixture(content: &str) -> (crate::Label, tokio::fs::File) {
    let data = tempfile::tempfile().unwrap();
    let sha = format!("{:x}", Sha256::digest(content.as_bytes()));
    let mut data = File::from_std(data);
    data.write_all(content.as_bytes())
        .await
        .expect("unable to write test data");
    data.flush().await.expect("unable to flush the test file");
    data.seek(SeekFrom::Start(0))
        .await
        .expect("unable to reset read pointer to head");
    (
        crate::Label {
            sha256: sha.to_owned(),
            media_type: "text/toml".to_owned(),
            name: "foo.toml".to_owned(),
            size: Some(6),
            annotations: None,
        },
        data,
    )
}

pub fn invoice_fixture() -> crate::Invoice {
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

    crate::Invoice {
        bindle_version: crate::BINDLE_VERSION_1.to_owned(),
        yanked: None,
        annotations: None,
        bindle: crate::BindleSpec {
            id: "foo/1.2.3".parse().unwrap(),
            description: Some("bar".to_owned()),
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
