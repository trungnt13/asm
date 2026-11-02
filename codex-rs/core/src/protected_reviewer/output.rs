//! Defines the protected review result contract, without changing slash-review events.

use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewerOutput {
    pub(super) status: ReviewerStatus,
    pub(super) findings: Vec<ReviewerFinding>,
    pub(super) explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewerStatus {
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewerFinding {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) priority: u8,
    pub(super) location: ReviewerLocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ReviewerLocation {
    Code {
        uri: PathUri,
        start_line: u32,
        end_line: u32,
    },
    Requirement {
        reference: String,
    },
}

impl ReviewerOutput {
    /// Rejects malformed results instead of converting failures into empty findings.
    pub(super) fn parse(text: &str) -> Result<Self, &'static str> {
        let output: Self = serde_json::from_str(text).map_err(|_| "invalid reviewer JSON")?;
        if output.explanation.trim().is_empty() {
            return Err("explanation must not be blank");
        }
        for finding in &output.findings {
            let valid_location = match &finding.location {
                ReviewerLocation::Code {
                    start_line,
                    end_line,
                    ..
                } => *start_line > 0 && end_line >= start_line,
                ReviewerLocation::Requirement { reference } => !reference.trim().is_empty(),
            };
            if finding.title.trim().is_empty()
                || finding.body.trim().is_empty()
                || finding.priority > 3
                || !valid_location
            {
                return Err("invalid reviewer finding");
            }
        }
        Ok(output)
    }

    /// Renders findings without asking another model to rewrite the reviewer's conclusions.
    pub(super) fn render_markdown(&self) -> String {
        let status = match self.status {
            ReviewerStatus::Complete if self.findings.is_empty() => "No findings.",
            ReviewerStatus::Complete => "Review complete.",
            ReviewerStatus::Incomplete => "Review incomplete.",
        };
        let mut sections = vec![status.to_string(), self.explanation.clone()];
        for finding in &self.findings {
            let location = match &finding.location {
                ReviewerLocation::Code {
                    uri,
                    start_line,
                    end_line,
                } => {
                    format!(
                        "{}:{start_line}-{end_line}",
                        uri.inferred_native_path_string()
                    )
                }
                ReviewerLocation::Requirement { reference } => reference.clone(),
            };
            sections.push(format!(
                "- [P{}] {} — {}\n\n  {}",
                finding.priority, finding.title, location, finding.body
            ));
        }
        sections.join("\n\n")
    }
}
