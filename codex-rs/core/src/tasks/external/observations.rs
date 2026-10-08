//! Projects external observations without invoking native tool executors.

use std::collections::HashMap;
use std::collections::HashSet;

use codex_extension_api::ExternalObservation;
use codex_protocol::ResponseItemId;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem;
use codex_protocol::error::CodexErr;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::DynamicToolCallItem;
use codex_protocol::items::DynamicToolCallStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

use super::invalid;
use super::validate_fragment;

pub(super) struct ObservedItems {
    recorded: HashSet<String>,
    tools: HashMap<String, DynamicToolCallItem>,
    last_message: Option<String>,
}

impl ObservedItems {
    #[tracing::instrument(level = "trace", skip_all)]
    pub(super) async fn new(session: &Session) -> Self {
        Self {
            recorded: session
                .clone_history()
                .await
                .raw_items()
                .filter_map(|item| item.id().map(ToString::to_string))
                .collect(),
            tools: HashMap::new(),
            last_message: None,
        }
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(super) async fn publish(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        observation: ExternalObservation,
    ) -> Result<(), CodexErr> {
        match observation {
            ExternalObservation::AssistantMessage { id, text } => {
                self.message(session, turn, &id, text, /*phase*/ None)
                    .await?;
            }
            ExternalObservation::ToolStarted {
                id,
                name,
                arguments,
            } => {
                validate_identifier(&id)?;
                validate_identifier(&name)?;
                validate_fragment(&arguments.to_string())?;
                let canonical = ResponseItemId::with_suffix(
                    "external_tool",
                    format!("{}.{}.{}", session.thread_id, turn.sub_id, id),
                )
                .to_string();
                if self.tools.contains_key(&id) {
                    return Ok(());
                }
                let namespace = turn
                    .config
                    .external_agent
                    .as_ref()
                    .ok_or_else(|| invalid("external descriptor is missing"))?
                    .backend_id
                    .clone();
                let tool = DynamicToolCallItem {
                    id: canonical.clone(),
                    namespace: Some(namespace.clone()),
                    tool: name.clone(),
                    arguments: arguments.clone(),
                    status: DynamicToolCallStatus::InProgress,
                    content_items: None,
                    success: None,
                    error: None,
                    duration: None,
                };
                let item = ResponseItem::CustomToolCall {
                    id: Some(ResponseItemId::from_server(canonical.clone())),
                    status: Some("in_progress".to_string()),
                    call_id: canonical.clone(),
                    name,
                    namespace: Some(namespace),
                    input: arguments.to_string(),
                    internal_chat_message_metadata_passthrough: None,
                };
                if self.recorded.contains(&canonical) {
                    self.tools.insert(id, tool);
                    return Ok(());
                }
                session
                    .record_conversation_items(turn, &turn.capture_current_model_info(), &[item])
                    .await;
                session
                    .emit_turn_item_started(turn, &TurnItem::DynamicToolCall(tool.clone()))
                    .await;
                self.recorded.insert(canonical);
                self.tools.insert(id, tool);
            }
            ExternalObservation::ToolFinished { id, output, error } => {
                validate_fragment(&output)?;
                if let Some(error) = &error {
                    validate_fragment(error)?;
                }
                let Some(mut tool) = self.tools.remove(&id) else {
                    return Err(invalid("external tool completion has no matching start"));
                };
                let success = error.is_none();
                tool.status = if success {
                    DynamicToolCallStatus::Completed
                } else {
                    DynamicToolCallStatus::Failed
                };
                tool.success = Some(success);
                tool.error = error.clone();
                let mut content_items =
                    vec![DynamicToolCallOutputContentItem::InputText { text: output }];
                if let Some(error) = error {
                    content_items.push(DynamicToolCallOutputContentItem::InputText {
                        text: "External tool error:".to_string(),
                    });
                    content_items.push(DynamicToolCallOutputContentItem::InputText { text: error });
                }
                tool.content_items = Some(content_items.clone());
                let output_id = ResponseItemId::with_suffix("external_tool_output", &tool.id);
                if self.recorded.contains(output_id.as_str()) {
                    return Ok(());
                }
                let item = ResponseItem::CustomToolCallOutput {
                    id: Some(output_id.clone()),
                    call_id: tool.id.clone(),
                    name: Some(tool.tool.clone()),
                    output: FunctionCallOutputPayload::from_content_items(
                        content_items.into_iter().map(Into::into).collect(),
                    ),
                    internal_chat_message_metadata_passthrough: None,
                };
                session
                    .record_conversation_items(turn, &turn.capture_current_model_info(), &[item])
                    .await;
                session
                    .emit_turn_item_completed(turn, TurnItem::DynamicToolCall(tool))
                    .await;
                self.recorded.insert(output_id.to_string());
            }
            ExternalObservation::ReadyToFinish { .. }
            | ExternalObservation::Usage(_)
            | ExternalObservation::Notice(_)
            | ExternalObservation::Failed(_)
            | ExternalObservation::Closed => {
                return Err(invalid(
                    "external control event entered observation projection",
                ));
            }
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub(super) async fn final_message(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        text: String,
    ) -> Result<(), CodexErr> {
        if !self.tools.is_empty() {
            return Err(invalid("external runtime completed with unfinished tools"));
        }
        if self.last_message.as_deref() != Some(text.as_str()) {
            let id = ResponseItemId::new("external_final");
            self.message(
                session,
                turn,
                id.as_str(),
                text,
                Some(MessagePhase::FinalAnswer),
            )
            .await?;
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    async fn message(
        &mut self,
        session: &Session,
        turn: &TurnContext,
        id: &str,
        text: String,
        phase: Option<MessagePhase>,
    ) -> Result<(), CodexErr> {
        validate_identifier(id)?;
        validate_fragment(&text)?;
        let id = ResponseItemId::with_suffix(
            "external_message",
            format!("{}.{}.{}", session.thread_id, turn.sub_id, id),
        );
        if !self.recorded.insert(id.to_string()) {
            return Ok(());
        }
        let message = ResponseItem::Message {
            id: Some(id.clone()),
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText { text: text.clone() }],
            phase: phase.clone(),
            internal_chat_message_metadata_passthrough: None,
        };
        let item = TurnItem::AgentMessage(AgentMessageItem {
            id: id.to_string(),
            content: vec![AgentMessageContent::Text { text: text.clone() }],
            phase,
            memory_citation: None,
            delivery: None,
            questions: None,
        });
        session.emit_turn_item_started(turn, &item).await;
        session
            .record_conversation_items(turn, &turn.capture_current_model_info(), &[message])
            .await;
        session.emit_turn_item_completed(turn, item).await;
        self.last_message = Some(text);
        Ok(())
    }
}

fn validate_identifier(id: &str) -> Result<(), CodexErr> {
    if id.is_empty() || id.len() > 512 {
        return Err(invalid("external item identifier is empty or too long"));
    }
    Ok(())
}
