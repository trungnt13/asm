use super::*;
use pretty_assertions::assert_eq;

#[test]
fn adaptive_evidence_tracks_steering_back_to_the_original_task() {
    let mut evidence = AdaptiveEvidence::default();
    for (input, steer) in [
        ("Review performance.", ""),
        (
            "Review documentation only.",
            "\nLatest user steer:\nReview documentation only.",
        ),
        ("Review performance.", ""),
    ] {
        let text = evidence.collect(
            &[TurnInput::UserInput {
                content: vec![UserInput::Text {
                    text: input.to_string(),
                    text_elements: Vec::new(),
                }],
                client_id: None,
                metadata: Default::default(),
            }],
            &[],
            /*limit*/ 8192,
        );
        assert_eq!(
            text,
            format!(
                "Current user task:\nReview performance.{steer}\nRecent evidence (newest first):\n"
            ),
        );
    }
}

#[test]
fn adaptive_evidence_bounds_multibyte_task_and_recent_output() {
    let task = "界".repeat(/*n*/ 4096);
    let output = "é".repeat(/*n*/ 4096);
    let history = vec![
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText { text: output }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    ];
    for limit in [128, 8192] {
        let text = AdaptiveEvidence::default().collect(
            &[TurnInput::UserInput {
                content: vec![UserInput::Text {
                    text: task.clone(),
                    text_elements: Vec::new(),
                }],
                client_id: None,
                metadata: Default::default(),
            }],
            &history,
            limit,
        );
        assert!(text.len() <= limit);
        assert!(text.starts_with("Current user task:\n界"));
        assert!(text.contains("\nassistant: é"));
    }
}
