use super::*;
use codex_history::CodexHarnessMetadata;
use codex_protocol::models::FunctionCallOutputPayload;
use pretty_assertions::assert_eq;

fn message(
    role: &str,
    content: Vec<ContentItem>,
    metadata: Option<CodexHarnessMetadata>,
) -> ResponseItemEnvelope {
    ResponseItemEnvelope {
        item: ResponseItem::Message {
            id: None,
            role: role.to_string(),
            encrypted_content: None,
            status: None,
            content,
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
        metadata,
    }
}

#[test]
fn basic_evidence_includes_task_and_conversation_history() {
    // Basic: retain the original task, steering, and readable user/assistant/tool history.
    let input = |text: &str| TurnInput::UserInput {
        content: vec![ResolvedUserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        client_id: None,
        metadata: Default::default(),
    };
    let history = vec![
        message(
            "user",
            vec![ContentItem::InputText {
                text: "Review performance.".to_string(),
            }],
            Some(CodexHarnessMetadata {
                user_input_order: Some(0),
                ..Default::default()
            }),
        ),
        message(
            "assistant",
            vec![ContentItem::OutputText {
                text: "I will check the measurements.".to_string(),
                annotations: None,
                logprobs: None,
            }],
            /*metadata*/ None,
        ),
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: Some("measurements".to_string()),
            name: None,
            namespace: None,
            output: FunctionCallOutputPayload::from_text("Latency: 100 ms.".to_string()),
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    ];
    let mut evidence = System1Evidence::default();
    evidence.ingest(
        &input("Review performance."),
        /*limit*/ 8192,
        /*include_images*/ false,
    );
    evidence.ingest(
        &input("Review documentation only."),
        /*limit*/ 8192,
        /*include_images*/ false,
    );
    assert_eq!(
        evidence.render(&history, /*limit*/ 8192),
        "Latest user message:\nReview documentation only.\nOriginal task:\nReview performance.\nRecent evidence (newest first):\n\ntool: Latency: 100 ms.\nassistant: I will check the measurements.",
    );
    evidence.ingest(
        &input("Review performance."),
        /*limit*/ 8192,
        /*include_images*/ false,
    );
    assert_eq!(
        evidence.render(&history, /*limit*/ 8192),
        "Latest user message:\nReview performance.\nRecent evidence (newest first):\n\ntool: Latency: 100 ms.\nassistant: I will check the measurements.",
    );
    assert_eq!(
        evidence.original.as_ref().unwrap().text,
        "Review performance."
    );
    assert!(!evidence.encrypted_task_unavailable);
}

#[test]
fn basic_evidence_respects_configured_context_budget() {
    // Basic: the configured budget bounds Unicode text and reaches older genuine history.
    let input = TurnInput::UserInput {
        content: vec![ResolvedUserInput::Text {
            text: "界".repeat(/*n*/ 32768),
            text_elements: Vec::new(),
        }],
        client_id: None,
        metadata: Default::default(),
    };
    let history = vec![message(
        "assistant",
        vec![ContentItem::OutputText {
            text: "é".repeat(/*n*/ 32768),
            annotations: None,
            logprobs: None,
        }],
        /*metadata*/ None,
    )];
    let mut evidence = System1Evidence::default();
    evidence.ingest(&input, /*limit*/ 32768, /*include_images*/ false);
    let text = evidence.render(&history, /*limit*/ 32768);
    assert!(text.len() <= 32768);
    assert!(text.starts_with("Latest user message:\n界"));
    assert!(text.contains("\nassistant: é"));

    let user_metadata = Some(CodexHarnessMetadata {
        user_input_order: Some(0),
        ..Default::default()
    });
    let mut history = vec![message(
        "user",
        vec![ContentItem::InputText {
            text: "EARLIER_USER_CONSTRAINT".to_string(),
        }],
        user_metadata.clone(),
    )];
    history.extend((0..140).map(|_| {
        message(
            "assistant",
            vec![ContentItem::OutputText {
                text: "Routine evidence.".to_string(),
                annotations: None,
                logprobs: None,
            }],
            /*metadata*/ None,
        )
    }));
    history.push(message(
        "assistant",
        (0..40)
            .map(|part| ContentItem::OutputText {
                text: if part == 39 {
                    "LATE_ASSISTANT_PART".to_string()
                } else {
                    "Short part. ".to_string()
                },
                annotations: None,
                logprobs: None,
            })
            .collect(),
        /*metadata*/ None,
    ));
    history.push(message(
        "user",
        vec![ContentItem::InputText {
            text: "INJECTED_INSTRUCTIONS".to_string(),
        }],
        /*metadata*/ None,
    ));
    history.push(message(
        "user",
        vec![ContentItem::InputText {
            text: "COMPACTION_OUTPUT".to_string(),
        }],
        Some(CodexHarnessMetadata {
            compaction_output: true,
            ..user_metadata.unwrap()
        }),
    ));
    let evidence = System1Evidence {
        original: Some(EvidenceMessage {
            text: "Current task.".to_string(),
            order: None,
            user: true,
        }),
        ..Default::default()
    };
    let default_text = evidence.render(&history, /*limit*/ 8192);
    assert!(default_text.len() <= 8192);
    assert!(!default_text.contains("EARLIER_USER_CONSTRAINT"));
    assert!(!default_text.contains("LATE_ASSISTANT_PART"));
    let expanded_text = evidence.render(&history, /*limit*/ 32768);
    assert!(expanded_text.len() <= 32768);
    assert!(expanded_text.contains("\nuser: EARLIER_USER_CONSTRAINT"));
    assert!(expanded_text.contains("LATE_ASSISTANT_PART"));
    assert!(!expanded_text.contains("INJECTED_INSTRUCTIONS"));
    assert!(!expanded_text.contains("COMPACTION_OUTPUT"));
}
