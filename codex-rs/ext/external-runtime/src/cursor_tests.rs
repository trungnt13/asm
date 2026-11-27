use super::*;
use std::process;
use tempfile::tempdir;

#[tokio::test]
async fn cursor_interrupt_is_clean_beside_an_unreadable_process() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let sdk = r#"import json, sys
for line in sys.stdin:
    kind = json.loads(line)["type"]
    if kind == "init":
        frame = {"type": "ready", "id": "cursor", "model": "auto", "effort": None, "baselineEffort": None}
    elif kind == "prompt":
        frame = {"type": "assistant_delta", "text": "working"}
    elif kind == "cancel":
        frame = {"type": "result", "status": "cancelled", "uuid": "turn"}
    else:
        sys.exit(1)
    print(json.dumps(frame), flush=True)
"#;
    let mut command = Command::new("python3");
    command.args(["-c", sdk]);
    let helper = ExternalProcess::spawn(command, FramePolicy::CursorSdk, StderrPolicy::Discard)?;
    let mut runtime = CursorRuntime::initialize(
        helper,
        CursorOptions {
            cwd: directory.path().to_owned(),
            model: "auto".to_owned(),
            effort: None,
            system_instructions: String::new(),
            bridge_directory: directory.path().to_owned(),
        },
        /*api_key*/ "not-a-real-key",
    )
    .await?;
    // Like sshd, the same-user bystander hides its environment.
    let hide =
        "import ctypes, os, time; os.setsid(); ctypes.CDLL(None).prctl(4, 0, 0, 0); time.sleep(30)";
    let mut bystander = process::Command::new("python3")
        .args(["-c", hide])
        .spawn()?;
    while std::fs::read(format!("/proc/{}/environ", bystander.id())).is_ok() {
        tokio::time::sleep(Duration::from_millis(/*millis*/ 20)).await;
    }

    let (events, mut received) = mpsc::channel(/*buffer*/ 32);
    let cancel = CancellationToken::new();
    let prompt = Prompt::Text("hello".to_owned());
    let (stopped, ()) = tokio::join!(runtime.run_turn(&prompt, events, cancel.clone()), async {
        if received.recv().await.is_some() {
            cancel.cancel();
        }
        while received.recv().await.is_some() {}
    });
    bystander.kill()?;
    bystander.wait()?;
    assert!(stopped?.interrupted);
    Ok(())
}
