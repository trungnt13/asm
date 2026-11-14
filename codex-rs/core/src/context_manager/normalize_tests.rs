use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn media_projection_preserves_plaintext_annotation_normalization() -> anyhow::Result<()> {
    for normalize in [
        strip_images_when_unsupported as fn(&[InputModality], &mut [ResponseItemEnvelope]),
        strip_audio_when_unsupported,
    ] {
        for metadata in [
            json!(null),
            json!({"content_item_kinds": ["user"]}),
            json!({"content_item_kinds": ["user", "user", "system"]}),
            json!({"content_item_kinds": ["user", "user"], "content_item_metadata": [{}, {}]}),
            json!({"content_item_kinds": ["user", "user"], "content_item_metadata": [ContentItemMetadata::harness()]}),
            json!({"content_item_kinds": ["user", "user"]}),
        ] {
            let original: ResponseItem = serde_json::from_value(json!({
                "type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "one"}, {"type": "input_text", "text": "two"}],
                "internal_chat_message_metadata_passthrough": metadata,
            }))?;
            let shared = Arc::new(original.clone());
            let mut expected = original.clone();
            let content = to_annotated_content(&mut expected).expect("message");
            set_annotated_content(&mut expected, content).expect("message");
            let mut actual = [ResponseItemEnvelope {
                item: Arc::clone(&shared),
                metadata: None,
            }];
            normalize(&[], &mut actual);
            assert_eq!(actual[0].item.as_ref(), &expected);
            assert_eq!(shared.as_ref(), &original);
            assert_eq!(Arc::ptr_eq(&actual[0].item, &shared), expected == original);
        }
    }
    Ok(())
}
