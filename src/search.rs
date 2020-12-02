use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use log::trace;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug)]
/// The search options for performing this query and returning results
pub struct SearchOptions {
    /// The offset from the last search results
    pub offset: u64,
    /// The maximum number of results to return
    pub limit: u8,
    /// Whether to use strict mode (if there are multiple modes supported)
    pub strict: bool,
    /// Whether to return yanked bindles
    pub yanked: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            offset: 0,
            limit: 50,
            strict: false,
            yanked: false,
        }
    }
}

/// Describes the matches that are returned
#[derive(Debug, Serialize, Deserialize)]
pub struct Matches {
    /// The query used to find this match set
    pub query: String,
    /// Whether the search engine used strict mode
    pub strict: bool,
    /// The offset of the first result in the matches
    pub offset: u64,
    /// The maximum number of results this query would have returned
    pub limit: u8,
    /// The total number of matches the search engine located
    ///
    /// In many cases, this will not match the number of results returned on this query
    pub total: u64,
    /// Whether there are more results than the ones returned here
    pub more: bool,
    /// Whether this list includes potentially yanked invoices
    pub yanked: bool,
    /// The list of invoices returned as this part of the query
    ///
    /// The length of this Vec will be less than or equal to the limit.
    // This needs to go at the bottom otherwise the table serialization in TOML gets weird. See
    // https://github.com/alexcrichton/toml-rs/issues/258
    pub invoices: Vec<crate::Invoice>,
}

impl Matches {
    fn new(opts: &SearchOptions, query: String) -> Self {
        Matches {
            // Assume options are definitive.
            query,
            strict: opts.strict,
            offset: opts.offset,
            limit: opts.limit,
            yanked: opts.yanked,

            // Defaults
            invoices: vec![],
            more: false,
            total: 0,
        }
    }
}

/// This trait describes the minimal set of features a Bindle provider must implement to provide
/// query support. Implementors of this trait should handle any locking of the internal index in
/// their implementation
#[async_trait::async_trait]
pub trait Search {
    /// A high-level function that can take raw search strings (queries and filters) and options.
    ///
    /// This will parse the terms and filters according to its internal rules, and return
    /// a set of matches.
    ///
    /// An error is returned if either there is something incorrect in the terms/filters,
    /// or if the search engine itself fails to process the query.
    async fn query(
        &self,
        term: String,
        filter: String,
        options: SearchOptions,
    ) -> anyhow::Result<Matches>;

    /// Given an invoice, extract information from it that will be useful for searching.
    ///
    /// This high-level feature does not provide any guarantees about how it will
    /// process the invoice. But it may implement Strict and/or Standard modes
    /// described in the protocol specification.
    ///
    /// If the index function is given an invoice it has already indexed, it treats
    /// the call as an update. Otherwise, it adds a new entry to the index.
    ///
    /// As a special note, if an invoice is yanked, the index function will mark it
    /// as such, following the protocol specification's requirements for yanked
    /// invoices.
    async fn index(&self, document: &crate::Invoice) -> anyhow::Result<()>;
}

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

    /// Given an invoice, extract information from it that will be useful for searching.
    ///
    /// This high-level feature does not provide any guarantees about how it will
    /// process the invoice. But it may implement Strict and/or Standard modes
    /// described in the protocol specification.
    ///
    /// If the index function is given an invoice it has already indexed, it treats
    /// the call as an update. Otherwise, it adds a new entry to the index.
    ///
    /// As a special note, if an invoice is yanked, the index function will mark it
    /// as such, following the protocol specification's requirements for yanked
    /// invoices.
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
