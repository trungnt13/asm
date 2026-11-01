//! Turn-local presentation folding. Original cells remain the source for raw and full transcripts.

use super::*;

#[derive(Debug)]
pub(crate) enum TurnActivityBoundary {
    Begin(String),
    LegacyAnswer {
        turn_id: Option<String>,
    },
    FinalAnswer {
        turn_id: Option<String>,
        elapsed_seconds: Option<u64>,
    },
}

impl HistoryCell for TurnActivityBoundary {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        Vec::new()
    }
}
