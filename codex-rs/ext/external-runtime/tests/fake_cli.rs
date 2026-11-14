//! Drives the runtimes against a scripted CLI. Each rule answers the next input frame of its
//! kind; `$<kind>` in a reply stands for the ID of the latest input frame of that kind.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use codex_external_runtime::*;
use serde_json::Value;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const FAKE_CLI: &str = r#"#!/usr/bin/env python3
import json, os, sys
rules = json.load(open(os.path.join(os.path.dirname(__file__), "rules.json")))
seen = {}
for line in sys.stdin:
    frame = json.loads(line)
    kind = frame.get("method") or frame.get("request", {}).get("subtype") or frame.get("type")
    seen[kind] = frame.get("id", frame.get("request_id", frame.get("uuid")))
    for reply in next((rules.pop(i)[1] for i, rule in enumerate(rules) if rule[0] == kind), []):
        text = json.dumps(reply)
        for key, value in seen.items():
            text = text.replace(json.dumps(f"${key}"), json.dumps(value))
        print(text, flush=True)
"#;
type Outcome = anyhow::Result<ExternalTurnOutcome>;
const SESSION: &str = "00000000-0000-4000-8000-000000000001";

fn fake_cli(rules: Value) -> anyhow::Result<TempDir> {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("rules.json"), rules.to_string())?;
    let cli = directory.path().join("cli");
    std::fs::write(&cli, FAKE_CLI)?;
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755))?;
    Ok(directory)
}

async fn claude(rules: Value) -> anyhow::Result<(TempDir, ClaudeRuntime)> {
    let directory = fake_cli(rules)?;
    let runtime = ClaudeRuntime::spawn(ClaudeOptions {
        executable: directory.path().join("cli"),
        cwd: directory.path().to_owned(),
        model: "fake".to_owned(),
        effort: None,
        auto_compact_window: None,
        role_instructions: None,
        session_kind: ClaudeSessionKind::NativeDefault,
        session: ClaudeSession::New {
            id: SESSION.to_owned(),
        },
        bridge_directory: None,
        disallowed_tools: Vec::new(),
        skill_overrides: Default::default(),
    })
    .await?;
    Ok((directory, runtime))
}

/// Runs one turn while draining its events; `interrupt` cancels it after the first event.
async fn drive(
    run: impl AsyncFnOnce(mpsc::Sender<Value>, CancellationToken) -> Outcome,
    interrupt: bool,
) -> Outcome {
    let (events, mut received) = mpsc::channel(/*buffer*/ 32);
    let cancel = CancellationToken::new();
    let (outcome, ()) = tokio::join!(run(events, cancel.clone()), async {
        if interrupt && received.recv().await.is_some() {
            cancel.cancel();
        }
        while received.recv().await.is_some() {}
    });
    outcome
}

async fn turn(runtime: &mut ClaudeRuntime, interrupt: bool) -> Outcome {
    let prompt = Prompt::Text("hello".to_owned());
    let run = async |events, cancel| runtime.run_turn(&prompt, events, cancel, None).await;
    drive(run, interrupt).await
}

fn init() -> Value {
    json!({"type": "system", "subtype": "init", "capabilities": ["interrupt_send_now_v1"]})
}

fn result(subtype: &str, api_error: Option<&str>) -> Value {
    let is_error = subtype != "success" || api_error.is_some();
    json!({"type": "result", "subtype": subtype, "is_error": is_error, "api_error": api_error,
        "session_id": SESSION, "user_message_uuid": "$user", "terminal_reason": "aborted_streaming"})
}

fn idle() -> Value {
    json!({"type": "system", "subtype": "session_state_changed", "state": "idle"})
}

#[tokio::test]
async fn claude_session_survives_a_usage_limit_and_an_interrupt() -> anyhow::Result<()> {
    let tool = json!({"type": "tool_use", "id": "tool", "name": "Bash", "input": {}});
    let (_directory, mut runtime) = claude(json!([
        ["user", [
            init(),
            {"type": "assistant", "message": {"content": [tool]}},
            {"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "tool"}]}},
            result("success", Some("usage_limit_reached")),
            idle(),
        ]],
        ["user", [init()]],
        ["interrupt", [
            {"type": "control_response", "response": {"subtype": "success",
                "request_id": "$interrupt", "response": {"still_queued": []}}},
            result("error_during_execution", None),
            {"type": "command_lifecycle", "command_uuid": "$user", "state": "cancelled"},
            idle(),
        ]],
        ["user", [result("success", None), idle()]],
    ]))
    .await?;

    let limited = turn(&mut runtime, false).await?;
    assert_eq!(limited.failure, Some(ExternalTurnFailure::UsageLimit));
    assert!(turn(&mut runtime, true).await?.interrupted);
    assert_eq!(turn(&mut runtime, false).await?.failure, None);
    runtime.shutdown().await
}

#[tokio::test]
async fn claude_submits_input_queued_while_the_idle_frame_is_forwarded() -> anyhow::Result<()> {
    let done = json!({"type": "command_lifecycle", "command_uuid": "$user", "state": "completed"});
    let (_directory, mut runtime) = claude(json!([
        ["user", [init(), result("success", None), idle()]],
        ["user", [done, result("success", None), idle()]],
    ]))
    .await?;
    let prompt = Prompt::Text("hello".to_owned());
    // The consumer lags one frame behind, so the runtime holds `idle` when input arrives.
    let (events, mut received) = mpsc::channel(/*buffer*/ 1);
    let (queue, steering) = mpsc::channel(/*buffer*/ 1);

    let turn = runtime.run_turn(&prompt, events, CancellationToken::new(), Some(steering));
    let (outcome, ()) = tokio::join!(turn, async {
        received.recv().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let _ = queue.try_send(Prompt::Text("queued".to_owned()));
        while received.recv().await.is_some() {}
    });
    assert_eq!(outcome?.submitted_steering_count, 1);
    runtime.shutdown().await
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cursor_interrupt_is_clean_beside_an_unreadable_process() -> anyhow::Result<()> {
    let options = json!([{"id": "model", "currentValue": "auto", "options": [{"value": "auto"}]}]);
    let reply = |request: &str, result: Value| {
        let id = format!("${request}");
        json!([request, [{"jsonrpc": "2.0", "id": id, "result": result}]])
    };
    let directory = fake_cli(json!([
        reply("initialize", json!({"protocolVersion": 1,
            "agentCapabilities": {"sessionCapabilities": {"subagents": {}}}})),
        reply("session/new", json!({"sessionId": "cursor", "configOptions": options})),
        reply("session/set_config_option", json!({"configOptions": options})),
        ["session/prompt", [{"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": "cursor", "update": {"sessionUpdate": "agent_message_chunk"}}}]],
        ["session/cancel", [{"jsonrpc": "2.0", "id": "$session/prompt",
            "result": {"stopReason": "cancelled"}}]],
    ]))?;
    let mut runtime = CursorRuntime::spawn(CursorOptions {
        executable: directory.path().join("cli"),
        cwd: directory.path().to_owned(),
        model: "auto".to_owned(),
        effort: None,
        role_instructions: None,
        session: CursorSession::New,
        config_dir: directory.path().join("config"),
        bridge_directory: None,
    })
    .await?;
    // Like `sshd: user@notty`: same user, born after the CLI, with a hidden environment.
    let hide = "import ctypes, os, time; os.setsid(); ctypes.CDLL(None).prctl(4, 0, 0, 0, 0); time.sleep(30)";
    let mut bystander = std::process::Command::new("python3")
        .args(["-c", hide])
        .spawn()?;
    while std::fs::read(format!("/proc/{}/environ", bystander.id())).is_ok() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let prompt = Prompt::Text("hello".to_owned());
    let run = async |events, cancel| runtime.run_turn(&prompt, events, cancel).await;
    let stopped = drive(run, true).await;
    bystander.kill()?;
    bystander.wait()?;
    assert!(stopped?.interrupted);
    Ok(())
}
