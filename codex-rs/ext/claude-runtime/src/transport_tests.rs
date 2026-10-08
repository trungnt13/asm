#![cfg(target_os = "linux")]

use std::process::Stdio;

use super::*;

#[tokio::test]
async fn launch_rejection_requires_explicit_settlement() -> Result<(), String> {
    for settled in [Some(true), Some(false), None] {
        let mut reply = json!({"ok": false, "error": "fixture startup cause"});
        if let Some(settled) = settled {
            reply["settled"] = json!(settled);
        }
        let child = tokio::process::Command::new("python3")
            .arg("-u")
            .arg("-c")
            .arg("import json,sys; command=json.loads(sys.stdin.readline()); reply=json.loads(sys.argv[1]); reply['reply_to']=command['id']; print(json.dumps(reply),flush=True)")
            .arg(reply.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn().map_err(|error| error.to_string())?;
        let (commands, exit) = Commands::start(child, Arc::new(Observations::default()))?;
        let result = commands.launch(Value::Null).await;
        commands.close();
        assert!(wait_for_exit(exit).await?.success());
        match (settled, result) {
            (Some(true), Err(ExternalAgentLaunchError::Rejected(error)))
            | (Some(false) | None, Err(ExternalAgentLaunchError::Unsettled(error))) => {
                assert_eq!(error, "fixture startup cause");
            }
            result => panic!("wrong launch settlement: {result:?}"),
        }
    }
    Ok(())
}

#[tokio::test]
async fn unexpected_error_stderr_is_drained_bounded_and_sanitized() -> Result<(), String> {
    let child = tokio::process::Command::new("python3")
        .arg("-u")
        .arg("-c")
        .arg("import json,signal,sys; signal.alarm(8); json.loads(sys.stdin.readline()); sys.stderr.buffer.write(b'x'*262144+b'\\x1b\\x07fixture tail\\n'); sys.stderr.flush(); raise RuntimeError('fixture unexpected error')")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().map_err(|error| error.to_string())?;
    let (commands, exit) = Commands::start(child, Arc::new(Observations::default()))?;
    let result = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 5),
        commands.launch(Value::Null),
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(matches!(
        result,
        Err(ExternalAgentLaunchError::Unsettled(_))
    ));
    assert!(!wait_for_exit(exit).await?.success());
    // Draining is independent of process-exit observation; wait for its final tail.
    let diagnostics = tokio::time::timeout(Duration::from_secs(/*secs*/ 2), async {
        loop {
            let diagnostics = commands.diagnostics().await;
            if diagnostics.contains("RuntimeError: fixture unexpected error") {
                break diagnostics;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    assert!(diagnostics.contains("fixture tail"));
    assert!(!diagnostics.contains('\x1b') && !diagnostics.contains('\x07'));
    assert!(diagnostics.len() <= 8192 + "; bridge stderr: ".len());
    Ok(())
}
