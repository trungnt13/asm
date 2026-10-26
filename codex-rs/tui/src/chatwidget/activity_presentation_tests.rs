//! Live tool summaries retain compact previews and leave raw/full output unchanged.

use super::*;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::history_cell::ActivityDisclosure;
use codex_app_server_protocol::CommandExecutionSource;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn live_tool_summary_preserves_compact_raw_and_full_presentations() {
    let (mut chat, _, _, _) = make_chatwidget_manual_with_sender().await;
    let preview = |chat: &ChatWidget, expanded| {
        chat.active_cell_owned_transcript_lines(/*width*/ 72, expanded)
            .unwrap()
    };
    let command = ["bash", "-lc", "glab api first"]
        .map(str::to_owned)
        .to_vec();
    let parsed = codex_shell_command::parse_command::parse_command(&command);
    let mut call = new_active_exec_command(
        "live".to_owned(),
        command,
        parsed,
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    chat.local_settings.tui.collapse_tool_calls = true;
    chat.transcript.active_cell = Some(Box::new(call));
    let running = preview(&chat, /*expanded*/ false);
    assert_eq!(running.activity.len(), 1);
    assert_eq!(
        running.activity[0].line.to_string(),
        "▸ Running 1 tool call (glab)"
    );
    call = new_active_exec_command(
        "live".to_owned(),
        vec!["glab".to_owned()],
        Vec::new(),
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    assert!(call.complete_call(
        "live",
        CommandOutput::new(
            /*exit_code*/ 0,
            "hidden-first\n2\n3\n4\n5\n6\nlast\n".to_owned()
        ),
        std::time::Duration::ZERO,
    ));
    let compact = call.compact_hyperlink_lines(/*width*/ 72);
    chat.transcript.active_cell = Some(Box::new(call));
    chat.local_settings.tui.collapse_tool_calls = false;
    let original = preview(&chat, /*expanded*/ false);
    assert_eq!(original.activity, compact);
    let full = chat.active_cell_transcript_hyperlink_lines(/*width*/ 72);
    chat.set_raw_output_mode(/*enabled*/ true);
    let raw = preview(&chat, /*expanded*/ false).activity;
    chat.set_raw_output_mode(/*enabled*/ false);
    chat.local_settings.tui.collapse_tool_calls = true;
    let collapsed = preview(&chat, /*expanded*/ false);
    assert_eq!(collapsed.activity.len(), 1);
    assert_eq!(collapsed.disclosure, Some(ActivityDisclosure::ToolGroup));
    let mode = chat.local_settings.transcript_mode;
    chat.local_settings.transcript_mode = crate::transcript_mode::TranscriptMode::Terminal;
    assert_eq!(preview(&chat, /*expanded*/ false).activity, compact);
    chat.local_settings.transcript_mode = mode;
    let expanded = preview(&chat, /*expanded*/ true);
    assert_eq!(&expanded.activity[1..], compact.as_slice());
    assert_eq!(
        chat.active_cell_transcript_hyperlink_lines(/*width*/ 72),
        full
    );
    chat.local_settings.tui.collapse_tool_calls_max_lines =
        std::num::NonZeroUsize::new(/*n*/ 2);
    let long_command = ["bash", "-lc", "alpha; bravo; charlie; delta; echo; foxtrot; golf; hotel; india; juliet; kilo; lima; mike; november"]
        .map(str::to_owned).to_vec();
    let parsed = codex_shell_command::parse_command::parse_command(&long_command);
    chat.transcript.active_cell = Some(Box::new(new_active_exec_command(
        "multi-line".to_owned(),
        long_command,
        parsed,
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    )));
    let wrapped = preview(&chat, /*expanded*/ false);
    assert_eq!(wrapped.activity.len(), 2);
    chat.local_settings.tui.collapse_tool_calls_max_lines = None;
    assert_eq!(preview(&chat, /*expanded*/ false).activity.len(), 1);
    chat.transcript.active_cell = None;
    // Restore the completed call to compare its original raw output.
    let mut call = new_active_exec_command(
        "live".to_owned(),
        vec!["glab".to_owned()],
        Vec::new(),
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    assert!(call.complete_call(
        "live",
        CommandOutput::new(
            /*exit_code*/ 0,
            "hidden-first\n2\n3\n4\n5\n6\nlast\n".to_owned()
        ),
        std::time::Duration::ZERO
    ));
    chat.transcript.active_cell = Some(Box::new(call));
    chat.set_raw_output_mode(/*enabled*/ true);
    assert_eq!(preview(&chat, /*expanded*/ false).activity, raw);
}
