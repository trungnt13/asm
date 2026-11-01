//! Defines the protected review result contract, without changing slash-review events.

use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerOutput {
    pub status: ReviewerStatus,
    pub findings: Vec<ReviewerFinding>,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerStatus {
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerFinding {
    pub title: String,
    pub body: String,
    pub priority: u8,
    pub location: ReviewerLocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReviewerLocation {
    Code {
        uri: PathUri,
        start_line: u32,
        end_line: u32,
    },
    Requirement {
        reference: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewerOutputError {
    #[error("invalid reviewer JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid reviewer result: {0}")]
    Invalid(&'static str),
}

impl ReviewerOutput {
    /// Rejects malformed results instead of converting failures into empty findings.
    pub fn parse(text: &str) -> Result<Self, ReviewerOutputError> {
        let output: Self = serde_json::from_str(text)?;
        if output.explanation.trim().is_empty() {
            return Err(ReviewerOutputError::Invalid(
                "explanation must not be blank",
            ));
        }
        for finding in &output.findings {
            if finding.title.trim().is_empty() || finding.body.trim().is_empty() {
                return Err(ReviewerOutputError::Invalid(
                    "finding text must not be blank",
                ));
            }
            if finding.priority > 3 {
                return Err(ReviewerOutputError::Invalid(
                    "priority must be between 0 and 3",
                ));
            }
            match &finding.location {
                ReviewerLocation::Code {
                    start_line,
                    end_line,
                    ..
                } => {
                    if *start_line == 0 || end_line < start_line {
                        return Err(ReviewerOutputError::Invalid("invalid inclusive line range"));
                    }
                }
                ReviewerLocation::Requirement { reference } => {
                    if reference.trim().is_empty() {
                        return Err(ReviewerOutputError::Invalid(
                            "requirement reference must not be blank",
                        ));
                    }
                }
            }
        }
        Ok(output)
    }

    /// Renders findings without asking another model to rewrite the reviewer's conclusions.
    pub fn render_markdown(&self) -> String {
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
