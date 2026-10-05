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
    let mut mixed_cells = vec![
        subagent("contact-1", SubAgentActivityKind::Interacted, "/root/a"),
        completed(
            "mixed-first",
            "glab api first",
            "first output",
            /*exit_code*/ 0,
        ),
        subagent("contact-2", SubAgentActivityKind::Interacted, "/root/a"),
        completed(
            "mixed-second",
            "curl endpoint",
            "second output",
            /*exit_code*/ 0,
        ),
        subagent("contact-3", SubAgentActivityKind::Interacted, "/root/b"),
    ];
    let mut mixed_view = TranscriptView::default();
    mixed_view.set_collapse_tool_calls(/*enabled*/ true);
    let mixed_collapsed = text(&render(
        &mut mixed_view,
        &mixed_cells,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(mixed_collapsed.contains("▸ Ran 5 tool calls (/root/a, glab, curl, /root/b)"));
    assert!(!mixed_collapsed.contains("Interacted with"));
    assert!(!mixed_collapsed.contains("first output"));
    mixed_view.handle_key(
        KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE),
        &mixed_cells,
    );
    mixed_view.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        &mixed_cells,
    );
    let mixed_expanded = text(&render(
        &mut mixed_view,
        &mixed_cells,
        /*width*/ 72,
        /*height*/ 20,
    ));
    let expected_previews = mixed_cells
        .iter()
        .flat_map(|member| member.compact_hyperlink_lines(/*width*/ 72))
        .map(|line| line.line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(mixed_expanded.contains(&expected_previews));
    assert_eq!(
        mixed_expanded.matches("Interacted with `/root/a`").count(),
        2
    );
    mixed_cells.push(subagent(
        "contact-4",
        SubAgentActivityKind::Interacted,
        "/root/b",
    ));
    let mixed_appended = text(&render(
        &mut mixed_view,
        &mixed_cells,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(mixed_appended.contains("▾ Ran 6 tool calls (/root/a, glab, curl, /root/b)"));
    assert_eq!(
        mixed_appended.matches("Interacted with `/root/b`").count(),
        2
    );
    mixed_view.handle_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        &mixed_cells,
    );
    let mixed_recollapsed = text(&render(
        &mut mixed_view,
        &mixed_cells,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(!mixed_recollapsed.contains("Interacted with"));
    assert!(mixed_recollapsed.contains("▸ Ran 6 tool calls (/root/a, glab, curl, /root/b)"));
    insta::assert_snapshot!(format!(
        "collapsed\n{collapsed}\n\nexpanded\n{expanded}\n\nappended\n{appended}\n\ncollapsed after append\n{recollapsed}\n\nmixed collapsed\n{mixed_collapsed}\n\nmixed expanded\n{mixed_expanded}\n\nmixed appended\n{mixed_appended}\n\nmixed recollapsed\n{mixed_recollapsed}"
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
        subagent("contact-1", SubAgentActivityKind::Interacted, "/root/a"),
        cell("Assistant message between tools"),
        completed(
            "failed",
            "false",
            "failure diagnostic",
            /*exit_code*/ 1,
        ),
        subagent("contact-2", SubAgentActivityKind::Interacted, "/root/b"),
        subagent("start-1", SubAgentActivityKind::Started, "/root/started"),
        subagent(
            "complete-1",
            SubAgentActivityKind::Completed,
            "/root/completed",
        ),
        subagent(
            "interrupt-1",
            SubAgentActivityKind::Interrupted,
            "/root/interrupted",
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
    assert!(rendered.contains("Started `/root/started`"));
    assert!(rendered.contains("Completed `/root/completed`"));
    assert!(rendered.contains("Interrupted `/root/interrupted`"));
    assert!(!rendered.contains("Interacted with"));
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
