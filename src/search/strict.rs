//! A strict query engine implementation. It always expects a strict match of query terms

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use log::trace;
use tokio::sync::RwLock;

use crate::search::{Matches, Search, SearchOptions};

/// Implements strict query processing.
#[derive(Clone)]
pub struct StrictEngine {
    // A BTreeMap will keep the records in a predictable order, which makes the
    // search results predictable. This greatly simplifies the process of doing offsets
    // and limits.
    index: Arc<RwLock<BTreeMap<String, crate::Invoice>>>,
}

impl Default for StrictEngine {
    fn default() -> Self {
        StrictEngine {
            index: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl Search for StrictEngine {
    async fn query(
        &self,
        term: String,
        filter: String,
        options: SearchOptions,
    ) -> anyhow::Result<Matches> {
        trace!(
            "beginning search with term {}, version {}, and options {:?}",
            term,
            filter,
            options
        );
        let mut found: Vec<crate::Invoice> = self
            .index
            .read()
            .await
            .iter()
            .filter(|(_, i)| {
                // Term and version have to be exact matches.
                // TODO: Version should have matching turned on.
                i.bindle.id.name() == term && i.version_in_range(&filter)
            })
            .map(|(_, v)| (*v).clone())
            .collect();

        trace!("Found {} total matches", found.len());
        let mut matches = Matches::new(&options, term);
        matches.strict = true;
        matches.yanked = false;
        matches.total = found.len() as u64;

        if matches.offset >= matches.total {
            // We're past the end of the search results. Return an empty matches object.
            matches.more = false;
            return Ok(matches);
        }

        // Apply offset and limit
        let mut last_index = matches.offset + matches.limit as u64 - 1;
        if last_index >= matches.total {
            last_index = matches.total - 1;
        }

        matches.more = matches.total > last_index + 1;
        let range = RangeInclusive::new(matches.offset as usize, last_index as usize);
        matches.invoices = found.drain(range).collect();
        trace!("Returning {} found invoices", matches.invoices.len());

        Ok(matches)
    }

    async fn index(&self, invoice: &crate::Invoice) -> anyhow::Result<()> {
        self.index
            .write()
            .await
            .insert(invoice.name(), invoice.clone());
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Invoice;

    #[tokio::test]
    async fn strict_engine_should_index() {
        let inv = invoice_fixture("my/bindle".to_owned(), "1.2.3".to_owned());
        let inv2 = invoice_fixture("my/bindle".to_owned(), "1.3.0".to_owned());
        let searcher = StrictEngine::default();
        searcher
            .index(&inv)
            .await
            .expect("succesfully indexed my/bindle/1.2.3");
        searcher
            .index(&inv2)
            .await
            .expect("succesfully indexed my/bindle/1.3.0");
        assert_eq!(2, searcher.index.read().await.len());

        // Search for one result
        let matches = searcher
            .query(
                "my/bindle".to_owned(),
                "1.2.3".to_owned(),
                SearchOptions::default(),
            )
            .await
            .expect("found some matches");

        assert_eq!(1, matches.invoices.len());

        // Search for two results
        let matches = searcher
            .query(
                "my/bindle".to_owned(),
                "^1.2.3".to_owned(),
                SearchOptions::default(),
            )
            .await
            .expect("found some matches");

        assert_eq!(2, matches.invoices.len());

        // Search for non-existant bindle
        let matches = searcher
            .query(
                "my/bindle2".to_owned(),
                "1.2.3".to_owned(),
                SearchOptions::default(),
            )
            .await
            .expect("found some matches");
        assert!(matches.invoices.is_empty());

        // Search for non-existant version
        let matches = searcher
            .query(
                "my/bindle".to_owned(),
                "1.2.99".to_owned(),
                SearchOptions::default(),
            )
            .await
            .expect("found some matches");
        assert!(matches.invoices.is_empty());

        // TODO: Need to test yanked bindles
    }

    fn invoice_fixture(name: String, version: String) -> Invoice {
        let labels = vec![
            crate::Label {
                sha256: "abcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo.toml".to_owned(),
                size: 101,
                annotations: None,
            },
            crate::Label {
                sha256: "bbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo2.toml".to_owned(),
                size: 101,
                annotations: None,
            },
            crate::Label {
                sha256: "cbcdef1234567890987654321".to_owned(),
                media_type: "text/toml".to_owned(),
                name: "foo3.toml".to_owned(),
                size: 101,
                annotations: None,
            },
        ];

        Invoice {
            bindle_version: crate::BINDLE_VERSION_1.to_owned(),
            yanked: None,
            annotations: None,
            bindle: crate::BindleSpec {
                id: format!("{}/{}", name, version).parse().unwrap(),
                description: Some("bar".to_owned()),
                authors: Some(vec!["m butcher".to_owned()]),
            },
            parcel: Some(
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
