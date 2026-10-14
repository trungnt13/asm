//! Optional tool grouping preserves original previews and unaffected transcript modes.

use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::transcript_view::tests::cell;
use crate::transcript_view::tests::render;
use crate::transcript_view::tests::text;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use std::time::Duration;

pub(super) fn completed(
    id: &str,
    script: &str,
    output: &str,
    exit_code: i32,
) -> Arc<dyn HistoryCell> {
    let command = vec!["bash".to_owned(), "-lc".to_owned(), script.to_owned()];
    let parsed = codex_shell_command::parse_command::parse_command(&command);
    let mut call = new_active_exec_command(
        id.to_owned(),
        command,
        parsed,
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    assert!(call.complete_call(
        id,
        CommandOutput::new(exit_code, output.to_owned()),
        Duration::ZERO,
    ));
    Arc::new(call)
}

pub(super) fn subagent(
    id: &str,
    kind: SubAgentActivityKind,
    agent_path: &str,
) -> Arc<dyn HistoryCell> {
    Arc::new(
        crate::multi_agents::sub_agent_activity_history_cell(&ThreadItem::SubAgentActivity {
            model: None,
            reasoning_effort: None,
            id: id.to_owned(),
            kind,
            agent_thread_id: "01912345-1234-7123-8123-123456789abc".to_owned(),
            agent_path: agent_path.to_owned(),
        })
        .unwrap(),
    )
}

#[test]
fn grouped_tools_expand_original_previews_and_refresh_after_append() {
    let output = (0..12)
        .map(|number| format!("output-{number:02}\n"))
        .collect::<String>();
    let mut cells = vec![
        completed("first", "glab api first", &output, /*exit_code*/ 0),
        completed(
            "second",
            "curl endpoint",
            "curl result",
            /*exit_code*/ 0,
        ),
        completed(
            "third",
            "glab api third",
            "third result",
            /*exit_code*/ 0,
        ),
    ];
    let mut view = TranscriptView::default();
    view.set_collapse_tool_calls(/*enabled*/ true, /*max_lines*/ 1);
    let collapsed = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(collapsed.contains("▸ Ran 3 tool calls (glab, curl)"));
    assert!(collapsed.contains("glab"));
    assert!(collapsed.contains("curl"));
    assert!(!collapsed.contains("output-11"));
    view.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE), &cells);
    assert!(view.is_activity_focused());
    view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &cells);
    let expanded = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(expanded.contains("output-11"));
    assert!(!expanded.contains("output-00"));
    assert!(expanded.contains("curl result"));
    assert!(expanded.contains("third result"));
    for member in &cells {
        for line in member.compact_hyperlink_lines(/*width*/ 72) {
            assert!(expanded.contains(line.line.to_string().trim()));
        }
    }
    cells.push(completed(
        "fourth",
        "jq .",
        "fourth result",
        /*exit_code*/ 0,
    ));
    let appended = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(appended.contains("fourth result"));
    view.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE), &cells);
    let recollapsed = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(recollapsed.contains("▸ Ran 4 tool calls (glab, curl, jq)"));
    assert!(recollapsed.contains("jq"));
    assert!(!recollapsed.contains("fourth result"));
    insta::assert_snapshot!(format!(
        "collapsed\n{collapsed}\n\nexpanded\n{expanded}\n\nappended\n{appended}\n\ncollapsed after append\n{recollapsed}"
    ));
    cells.extend([
        cell("Assistant message between tools"),
        completed(
            "failed",
            "false",
            "failure diagnostic",
            /*exit_code*/ 1,
        ),
        completed(
            "after-failure",
            "jq .",
            "hidden third",
            /*exit_code*/ 0,
        ),
    ]);
    let rendered = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(rendered.contains("Assistant message between tools"));
    assert!(rendered.contains("failure diagnostic"));
    assert!(rendered.contains("Ran 4 tool calls (glab, curl, jq)"));
    assert!(rendered.contains("Ran 1 tool call (jq)"));
    assert!(!rendered.contains("hidden third"));
}

#[test]
fn option_leaves_disabled_raw_and_detailed_views_unchanged() {
    let cells = vec![
        completed(
            "first",
            "glab api first",
            "first result",
            /*exit_code*/ 0,
        ),
        subagent("contact-1", SubAgentActivityKind::Interacted, "/root/a"),
        completed(
            "second",
            "curl endpoint",
            "second result",
            /*exit_code*/ 0,
        ),
    ];
    let mut original_cells = cells.clone();
    original_cells[1] = Arc::new(crate::history_cell::PlainHistoryCell::new(
        cells[1].display_lines(/*width*/ 72),
    ));
    for (enabled, detailed, mode) in [
        (false, false, HistoryRenderMode::Rich),
        (true, false, HistoryRenderMode::Raw),
        (true, true, HistoryRenderMode::Rich),
    ] {
        let mut baseline = TranscriptView::default();
        baseline.set_presentation(detailed, mode);
        let expected = render(
            &mut baseline,
            &original_cells,
            /*width*/ 72,
            /*height*/ 24,
        );
        let mut configured = TranscriptView::default();
        configured.set_collapse_tool_calls(enabled, /*max_lines*/ 1);
        configured.set_presentation(detailed, mode);
        assert_eq!(
            render(
                &mut configured,
                &cells,
                /*width*/ 72,
                /*height*/ 24
            ),
            expected,
        );
    }
}
