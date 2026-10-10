use super::*;
use codex_protocol::AgentPath;
use codex_protocol::protocol::InterAgentCommunication;
use pretty_assertions::assert_eq;

#[test]
fn adaptive_evidence_tracks_steering_back_to_the_original_task() {
    for agent_input in [false, true] {
        let mut evidence = AdaptiveEvidence::default();
        for (input, steer) in [
            ("Review performance.", ""),
            (
                "Review documentation only.",
                "\nLatest user steer:\nReview documentation only.",
            ),
            ("Review performance.", ""),
        ] {
            let input = if agent_input {
                TurnInput::InterAgentCommunication(InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::try_from("/root/worker").unwrap(),
                    Vec::new(),
                    input.to_string(),
                    /*trigger_turn*/ true,
                ))
            } else {
                TurnInput::UserInput {
                    content: vec![UserInput::Text {
                        text: input.to_string(),
                        text_elements: Vec::new(),
                    }],
                    client_id: None,
                    metadata: Default::default(),
                }
            };
            let text = evidence.collect(&[input], &[], /*limit*/ 8192);
            assert_eq!(
                text,
                format!(
                    "Current user task:\nReview performance.{steer}\nRecent evidence (newest first):\n"
                ),
            );
            assert!(!evidence.encrypted_task_unavailable);
        }
    }
    let mut encrypted = InterAgentCommunication::new_encrypted(
        AgentPath::root(),
        AgentPath::try_from("/root/worker").unwrap(),
        Vec::new(),
        "synthetic encrypted payload".to_string(),
        /*trigger_turn*/ true,
    );
    encrypted.content = "not plaintext when encrypted_content is present".to_string();
    let mut evidence = AdaptiveEvidence::default();
    assert_eq!(
        evidence.collect(
            &[TurnInput::InterAgentCommunication(encrypted)],
            &[],
            /*limit*/ 8192
        ),
        "Current user task:\n\nRecent evidence (newest first):\n"
    );
    assert_eq!(evidence.task, "");
    assert!(evidence.encrypted_task_unavailable);
    let history = vec![ResponseItemEnvelope {
        item: ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            encrypted_content: None,
            status: None,
            content: vec![ContentItem::InputText {
                text: "Inherited parent task.".to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        },
        metadata: Some(codex_history::CodexHarnessMetadata {
            user_input_order: Some(0),
            ..Default::default()
        }),
    }];
    let text = evidence.collect(&[], &history, /*limit*/ 8192);
    assert_eq!(evidence.task, "Inherited parent task.");
    assert!(evidence.encrypted_task_unavailable);
    assert!(!text.contains("synthetic encrypted payload"));
    assert!(!text.contains("not plaintext when encrypted_content is present"));
    let mut known_task = AdaptiveEvidence::default();
    known_task.collect(&[], &history, /*limit*/ 8192);
    let encrypted = InterAgentCommunication::new_encrypted(
        AgentPath::root(),
        AgentPath::try_from("/root/worker").unwrap(),
        Vec::new(),
        "synthetic encrypted payload".to_string(),
        /*trigger_turn*/ true,
    );
    known_task.collect(
        &[TurnInput::InterAgentCommunication(encrypted)],
        &history,
        /*limit*/ 8192,
    );
    assert_eq!(known_task.task, "Inherited parent task.");
    assert!(known_task.encrypted_task_unavailable);
    evidence.task.clear();
    let plaintext = InterAgentCommunication::new(
        AgentPath::root(),
        AgentPath::try_from("/root/worker").unwrap(),
        Vec::new(),
        "Review performance.".to_string(),
        /*trigger_turn*/ true,
    );
    assert_eq!(
        evidence.collect(
            &[TurnInput::InterAgentCommunication(plaintext)],
            &[],
            /*limit*/ 8192
        ),
        "Current user task:\nReview performance.\nRecent evidence (newest first):\n",
    );
    assert!(!evidence.encrypted_task_unavailable);
}

#[test]
fn adaptive_evidence_bounds_multibyte_task_and_recent_output() {
    let task = "界".repeat(/*n*/ 4096);
    let output = "é".repeat(/*n*/ 4096);
    let history = vec![
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            encrypted_content: None,
            status: None,
            content: vec![ContentItem::OutputText {
                text: output,
                annotations: None,
                logprobs: None,
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    ];
    for limit in [128, 8192] {
        for agent_input in [false, true] {
            let input = if agent_input {
                TurnInput::InterAgentCommunication(InterAgentCommunication::new(
                    AgentPath::root(),
                    AgentPath::try_from("/root/worker").unwrap(),
                    Vec::new(),
                    task.clone(),
                    /*trigger_turn*/ false,
                ))
            } else {
                TurnInput::UserInput {
                    content: vec![UserInput::Text {
                        text: task.clone(),
                        text_elements: Vec::new(),
                    }],
                    client_id: None,
                    metadata: Default::default(),
                }
            };
            let text = AdaptiveEvidence::default().collect(&[input], &history, limit);
            assert!(text.len() <= limit);
            assert!(text.starts_with("Current user task:\n界"));
            assert!(text.contains("\nassistant: é"));
        }
    }
}
