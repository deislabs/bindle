//! Common types and traits for use in implementing query functionality for a Bindle server. Note
//! that this functionality is quite likely to change
use serde::{Deserialize, Serialize};

mod noop;
mod strict;

pub use noop::NoopEngine;
pub use strict::StrictEngine;

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

/// Describes the matches that are returned from a query
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
/// query support.
///
/// Implementors of this trait should handle any locking of the internal index in their
/// implementation Please note that due to this being an `async_trait`, the types might look
/// complicated. Look at the code directly to see the simpler function signatures for implementation
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
