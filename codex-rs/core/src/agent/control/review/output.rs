use codex_utils_path_uri::PathUri;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use super::ReviewRun;
use codex_protocol::protocol::AgentStatus;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    status: Status,
    explanation: String,
    findings: Vec<Finding>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Complete,
    Incomplete,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    title: String,
    body: String,
    priority: u8,
    location: Location,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Location {
    Code {
        uri: PathUri,
        start_line: u32,
        end_line: u32,
    },
    Requirement {
        reference: String,
    },
}

impl ReviewRun {
    pub(crate) async fn finish(&self, status: AgentStatus) -> AgentStatus {
        let reason = match status {
            AgentStatus::Completed(Some(text)) => match self.store(&text).await {
                Ok(status) => return status,
                Err(error) => {
                    tracing::warn!(%error, "protected review result rejected");
                    "invalid or unavailable report"
                }
            },
            AgentStatus::Interrupted => "interrupted",
            AgentStatus::Completed(None) | AgentStatus::Errored(_) => "execution failed",
            other => return other,
        };
        AgentStatus::Errored(format!("Review incomplete: {reason}; see reviewer rollout"))
    }

    async fn store(&self, text: &str) -> anyhow::Result<AgentStatus> {
        let report: Report = serde_json::from_str(text)?;
        anyhow::ensure!(!report.explanation.trim().is_empty(), "missing explanation");
        let count = report.findings.len();
        let heading = match report.status {
            Status::Complete if count == 0 => "No findings.",
            Status::Complete => "Review complete.",
            Status::Incomplete => "Review incomplete.",
        };
        let mut markdown = format!("{heading}\n\n{}", report.explanation);
        for f in report.findings {
            let (priority, title, body) = (f.priority, &f.title, &f.body);
            anyhow::ensure!(
                priority <= 3 && !title.trim().is_empty() && !body.trim().is_empty(),
                "invalid finding"
            );
            let location = match f.location {
                Location::Code {
                    uri,
                    start_line,
                    end_line,
                } if start_line > 0 && start_line <= end_line => {
                    let path = uri.inferred_native_path_string();
                    format!("{path}:{start_line}-{end_line}")
                }
                Location::Requirement { reference } if !reference.trim().is_empty() => reference,
                _ => anyhow::bail!("invalid finding location"),
            };
            use std::fmt::Write;
            write!(
                markdown,
                "\n\n- [P{priority}] {title} — {location}\n\n  {body}"
            )?;
        }
        let relative = format!("reviewer-reports/{}.md", self.id);
        let directory = self.report_home.join("reviewer-reports");
        tokio::fs::create_dir_all(&directory).await?;
        anyhow::ensure!(
            tokio::fs::symlink_metadata(&directory).await?.is_dir(),
            "invalid report directory"
        );
        let mut options = tokio::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(self.report_home.join(&relative)).await?;
        file.write_all(markdown.as_bytes()).await?;
        file.sync_all().await?;
        if markdown.len() > 700 {
            markdown = format!(
                "{heading} {count} findings. Full review report: {relative} beneath CODEX_HOME."
            );
        }
        Ok(match report.status {
            Status::Complete => AgentStatus::Completed(Some(markdown)),
            Status::Incomplete => AgentStatus::Errored(markdown),
        })
    }
}
