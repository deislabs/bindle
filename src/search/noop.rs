//! A no-op query engine implementation. Useful for use in storage engines being used in caches

use crate::search::{Matches, Search, SearchOptions};

/// A no-op query engine implementation. Its methods always returns `Ok` with an empty result
#[derive(Default, Clone)]
pub struct NoopEngine {}

#[async_trait::async_trait]
impl Search for NoopEngine {
    async fn query(
        &self,
        term: String,
        _filter: String,
        options: SearchOptions,
    ) -> anyhow::Result<Matches> {
        Ok(Matches::new(&options, term))
    }

    async fn index(&self, _: &crate::Invoice) -> anyhow::Result<()> {
        Ok(())
    }
}
