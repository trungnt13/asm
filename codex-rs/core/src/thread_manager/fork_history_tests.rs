use super::super::ForkSnapshot;
use super::super::fork_history_from_snapshot;
use crate::context_manager::ContextManager;
use crate::tasks::InterruptedTurnHistoryMarker;
use codex_attachment_store::InlineAttachmentStore;
use codex_history::CodexHarnessMetadata;
use codex_history::InitialHistory;
use codex_history::ResponseItemEnvelope;
use codex_history::ResumedHistory;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::InputModality;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use test_case::test_case;

#[test_case("custom_tool_call"; "custom")]
#[test_case("function_call"; "function")]
#[test_case("local_shell_call"; "local_shell")]
#[test_case("tool_search_call"; "tool_search")]
#[tokio::test]
async fn interrupted_snapshot_closes_only_child_pending_calls(kind: &str) {
    let mut call = json!({"type": kind, "id": "source-call", "call_id": "pending"});
    match kind {
        "custom_tool_call" => {
            call["name"] = json!("code_mode");
            call["input"] = json!("sleep(25)");
        }
        "function_call" => {
            call["name"] = json!("exec_command");
            call["arguments"] = json!("{}");
        }
        "local_shell_call" => {
            call["status"] = json!("in_progress");
            call["action"] = json!({"type": "exec", "command": ["sleep", "25"]});
        }
        "tool_search_call" => {
            call["status"] = json!("in_progress");
            call["execution"] = json!("client");
            call["arguments"] = json!({"query": "sleep"});
        }
        _ => unreachable!("test call kind"),
    }
    let source = Arc::new(vec![
        serde_json::from_value(json!({
            "type": "event_msg",
            "payload": {"type": "task_started", "turn_id": "main-turn"}
        }))
        .expect("turn boundary"),
        RolloutItem::ResponseItem(
            serde_json::from_value::<ResponseItem>(json!({
                "type": "custom_tool_call", "call_id": "finished", "name": "code_mode",
                "input": "done"
            }))
            .unwrap()
            .into(),
        ),
        RolloutItem::ResponseItem(
            serde_json::from_value::<ResponseItem>(json!({
                "type": "custom_tool_call_output", "call_id": "finished", "output": "success"
            }))
            .unwrap()
            .into(),
        ),
        RolloutItem::ResponseItem(ResponseItemEnvelope {
            item: serde_json::from_value(call).expect("pending call"),
            metadata: Some(CodexHarnessMetadata {
                client_authored: true,
                user_input_order: Some(7),
                ..Default::default()
            }),
        }),
    ]);
    let original = serde_json::to_value(source.as_ref()).expect("source history");
    let fork = fork_history_from_snapshot(
        ForkSnapshot::Interrupted,
        InitialHistory::Resumed(ResumedHistory {
            conversation_id: ThreadId::new(),
            history: Arc::clone(&source),
            history_revision: None,
            rollout_path: None,
        }),
        InterruptedTurnHistoryMarker::Disabled,
    );
    let child = fork.get_rollout_items();
    assert_eq!(serde_json::to_value(source.as_ref()).unwrap(), original);
    assert_eq!(
        serde_json::to_value(&child[..source.len()]).unwrap(),
        original
    );
    assert_eq!(child.len(), source.len() + 2);
    let RolloutItem::ResponseItem(output) = &child[source.len()] else {
        panic!("the child's suffix must close the inherited call before the abort boundary");
    };
    let expected_type = match kind {
        "custom_tool_call" => "custom_tool_call_output",
        "function_call" | "local_shell_call" => "function_call_output",
        "tool_search_call" => "tool_search_output",
        _ => unreachable!("test call kind"),
    };
    let output_json = serde_json::to_value(&output.item).unwrap();
    assert_eq!(output_json["type"], expected_type);
    assert_eq!(output_json["call_id"], "pending");
    assert!(output_json["id"].as_str().is_some());
    if kind != "tool_search_call" {
        assert_eq!(output_json["output"], "aborted");
    }

    // Reconstruct from persisted data, including the unchanged referenced ancestor prefix.
    let resumed: InitialHistory =
        serde_json::from_value(serde_json::to_value(&fork).unwrap()).unwrap();
    let envelopes: Vec<_> = resumed
        .get_rollout_items()
        .iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope) => Some(envelope.clone()),
            _ => None,
        })
        .collect();
    let mut history = ContextManager::default();
    history.replace_annotated(envelopes.clone());
    assert_eq!(
        history
            .for_prompt_annotated(&[InputModality::Text], &InlineAttachmentStore)
            .await,
        envelopes,
    );
    assert_eq!(
        serde_json::to_value(fork_history_from_snapshot(
            ForkSnapshot::Interrupted,
            resumed,
            InterruptedTurnHistoryMarker::Disabled,
        ))
        .unwrap(),
        serde_json::to_value(fork).unwrap(),
    );
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "Custom tool call output is missing")]
fn live_custom_call_without_output_still_violates_history_invariant() {
    let call: ResponseItem = serde_json::from_value(json!({
        "type": "custom_tool_call", "call_id": "live", "name": "code_mode", "input": ""
    }))
    .unwrap();
    let mut history = ContextManager::default();
    history.replace(vec![call]);
    history.normalize_history(&[InputModality::Text]);
}
