//! Logical size of a fresh, single-writer shadow board. Count each stored copy of
//! serialized evidence, excluding SQLite metadata, indexes and search text.
use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;

const DEFAULT_BOARD_LIMIT_MIB: u32 = 32;

#[derive(Debug)]
pub(super) struct PayloadLimit;

impl fmt::Display for PayloadLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Guardian shadow board payload limit exceeded")
    }
}

impl std::error::Error for PayloadLimit {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BoardSize {
    payload_bytes: u64,
    limit_bytes: u64,
}

impl BoardSize {
    pub(super) fn new(limit_mib: Option<NonZeroU32>) -> Self {
        Self {
            payload_bytes: 0,
            limit_bytes: u64::from(limit_mib.map_or(DEFAULT_BOARD_LIMIT_MIB, NonZeroU32::get))
                * 1024
                * 1024,
        }
    }

    pub(super) fn check_capacity(&self, bytes: u64) -> Result<(), PayloadLimit> {
        if bytes > self.limit_bytes - self.payload_bytes {
            return Err(PayloadLimit);
        }
        Ok(())
    }

    // Preflight the entire validated batch before creating channels or appending posts.
    // The sampler can select fewer sources than were serialized.
    pub(super) fn with_batch(
        mut self,
        routes: &[Vec<String>],
        texts: &[String],
    ) -> Result<Self, PayloadLimit> {
        for (destinations, text) in routes.iter().zip(texts) {
            let copies = destinations.iter().collect::<BTreeSet<_>>().len();
            let bytes = (text.len() as u64).saturating_mul(copies as u64);
            self.check_capacity(bytes)?;
            self.payload_bytes += bytes;
        }
        Ok(self)
    }
}

#[cfg(test)]
#[path = "shadow_limit_tests.rs"]
mod tests;
