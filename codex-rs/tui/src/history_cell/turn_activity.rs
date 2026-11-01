//! Turn-local presentation folding. Original cells remain the source for raw and full transcripts.

use super::*;
use std::sync::Arc;

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

impl TurnActivityBoundary {
    pub(crate) fn is_activity(cell: &dyn HistoryCell) -> bool {
        cell.as_any().is::<AgentMarkdownCell>()
            || cell.as_any().is::<AgentMessageCell>()
            || cell.as_any().is::<ReasoningSummaryCell>()
            || cell.tool_call_summary().is_some()
    }

    pub(crate) fn position_header(cells: &mut [Arc<dyn HistoryCell>]) {
        let Some(last) = cells.len().checked_sub(1) else {
            return;
        };
        if !Self::is_activity(cells[last].as_ref()) {
            return;
        }
        for index in (0..last).rev() {
            if let Some(boundary) = cells[index].as_any().downcast_ref::<Self>() {
                if matches!(boundary, Self::Begin(_)) {
                    // Move only the source-free marker, leaving prompts and notices in order.
                    cells[index..last].rotate_left(/*mid*/ 1);
                }
                break;
            }
            if Self::is_activity(cells[index].as_ref())
                || cells[index].as_any().is::<FinalMessageSeparator>()
            {
                break;
            }
        }
    }
}
