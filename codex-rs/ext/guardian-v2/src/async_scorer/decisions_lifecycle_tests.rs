use super::*;
use crate::async_scorer::conversation::ConversationBackend;

#[tokio::test]
async fn guardian_budget_rejects_baseline_before_sending() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let fixture =
        GuardianFailureFixture::with_config("[features.guardianv2]\nenabled = true\n").await?;
    let server = responses::start_mock_server().await;
    let store = fixture.test.codex.thread_extension_data();
    let metrics = Arc::new(RecordingMetrics::default());
    store.insert(GuardianV2ScoreProgress::new(Some(metrics.clone())));
    // Replace the conversation backend to force the budget failure.
    let mut sampler_config = crate::async_scorer::sampler::tests::sampler_config(server.uri());
    sampler_config.max_input_tokens = 256;
    store.insert(LunaSampler::new(sampler_config));
    store.insert(ConversationBackend::new(
        store.get::<LunaSampler>().unwrap(),
    ));
    fixture.score_tool(ToolName::plain("read_file")).await;
    fixture.assert_fails_closed("elevated_risk").await?;
    tokio::time::timeout(ASYNC_TEST_TIMEOUT, async {
        loop {
            let completed = {
                let samples = metrics.0.lock().unwrap();
                samples.iter().any(|sample| {
                    matches!(sample,
                    RecordedMetric::Counter(name, _, _) if name == CLASSIFICATION_METRIC)
                })
            };
            if completed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(server.received_requests().await.unwrap().is_empty());
    Ok(())
}
