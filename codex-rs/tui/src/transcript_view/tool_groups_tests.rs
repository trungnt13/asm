//! Optional tool grouping preserves original previews and unaffected transcript modes.

use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::transcript_view::tests::cell;
use crate::transcript_view::tests::render;
use crate::transcript_view::tests::text;
use codex_app_server_protocol::CommandExecutionSource;
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
    view.set_collapse_tool_calls(/*enabled*/ true);
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
        completed(
            "second",
            "curl endpoint",
            "second result",
            /*exit_code*/ 0,
        ),
    ];
    for (enabled, detailed, mode) in [
        (false, false, HistoryRenderMode::Rich),
        (true, false, HistoryRenderMode::Raw),
        (true, true, HistoryRenderMode::Rich),
    ] {
        let mut baseline = TranscriptView::default();
        baseline.set_presentation(detailed, mode);
        let expected = render(&mut baseline, &cells, /*width*/ 72, /*height*/ 24);
        let mut configured = TranscriptView::default();
        configured.set_collapse_tool_calls(enabled);
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

#[test]
fn failures_and_messages_break_groups_and_narrow_headers_remain_bounded() {
    let cells = vec![
        completed(
            "first",
            "glab api first",
            "hidden first",
            /*exit_code*/ 0,
        ),
        completed(
            "second",
            "curl endpoint",
            "hidden second",
            /*exit_code*/ 0,
        ),
        cell("Assistant message between tools"),
        completed(
            "failed",
            "false",
            "failure diagnostic",
            /*exit_code*/ 1,
        ),
        completed("third", "jq .", "hidden third", /*exit_code*/ 0),
        completed(
            "fourth",
            "git status",
            "hidden fourth",
            /*exit_code*/ 0,
        ),
    ];
    let mut view = TranscriptView::default();
    view.set_collapse_tool_calls(/*enabled*/ true);
    let rendered = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 28,
    ));
    assert!(rendered.contains("Assistant message between tools"));
    assert!(rendered.contains("failure diagnostic"));
    assert!(!rendered.contains("hidden first"));
    assert!(!rendered.contains("hidden fourth"));
    let names = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
    let narrow_cells = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            completed(
                &format!("narrow-{index}"),
                name,
                "hidden",
                /*exit_code*/ 0,
            )
        })
        .collect::<Vec<_>>();
    let mut narrow = TranscriptView::default();
    narrow.set_collapse_tool_calls(/*enabled*/ true);
    let narrow_rendered = text(&render(
        &mut narrow,
        &narrow_cells,
        /*width*/ 32,
        /*height*/ 8,
    ));
    assert!(narrow_rendered.contains("more"));
    assert!(!narrow_rendered.contains("hidden"));
    insta::assert_snapshot!(format!("barriers\n{rendered}\n\nnarrow\n{narrow_rendered}"));
}
