//! Group disclosure keeps mouse coordinates and pinned history stable.

use super::*;
use crate::transcript_view::tests::cell;
use crate::transcript_view::tests::render;
use crate::transcript_view::tests::text;
use crate::transcript_view::tool_groups::tests::completed;
use crate::transcript_view::tool_groups::tests::subagent;
use codex_app_server_protocol::SubAgentActivityKind;
use crossterm::event::KeyCode;
use pretty_assertions::assert_eq;

#[test]
fn group_header_mouse_coordinates_and_pagination_preserve_reading() {
    let mut cells = vec![
        cell("message"),
        completed("first", "git status", "first output", /*exit_code*/ 0),
        completed(
            "second",
            "glab api second",
            "second output",
            /*exit_code*/ 0,
        ),
    ];
    let mut view = TranscriptView::default();
    view.set_collapse_tool_calls(/*enabled*/ true);
    render(&mut view, &cells, /*width*/ 72, /*height*/ 20);
    let layout = view.current_layout(&cells, /*index*/ 1).unwrap();
    let offset = layout.text().find("git").unwrap();
    assert_eq!(layout.row_for_offset(offset), 1);
    assert!(layout.column_for_offset(offset) > 0);
    let y = view
        .visible
        .iter()
        .position(|row| row.index == 1 && row.row == 1)
        .unwrap() as u16;
    assert!(view.toggle_disclosure_at(&cells, /*column*/ 0, y));
    let opened = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 20,
    ));
    assert!(opened.contains("first output"));
    assert!(opened.contains("second output"));
    // Hold an existing group while an older history page adds a command before its leader.
    cells.remove(/*index*/ 0);
    render(&mut view, &cells, /*width*/ 72, /*height*/ 20);
    view.held_reading = Some(view.capture_snapshot(&cells));
    let before = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 20,
    ));
    cells.insert(
        /*index*/ 0,
        completed("older", "curl older", "older output", /*exit_code*/ 0),
    );
    view.history_loaded(&cells, 0..1);
    let after = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 20,
    ));
    assert_eq!(
        after.matches("first output").count(),
        before.matches("first output").count()
    );
    assert_eq!(
        after.matches("second output").count(),
        before.matches("second output").count()
    );
    assert_eq!(after.matches("Ran 2 tool calls").count(), 1);
    view.jump_to_latest();
    let latest = text(&render(
        &mut view, &cells, /*width*/ 72, /*height*/ 20,
    ));
    assert!(latest.contains("Ran 3 tool calls (curl, git, glab)"));

    let mut contacts = vec![
        subagent("contact-1", SubAgentActivityKind::Interacted, "/root/a"),
        subagent("contact-2", SubAgentActivityKind::Interacted, "/root/b"),
    ];
    let mut contact_view = TranscriptView::default();
    contact_view.set_collapse_tool_calls(/*enabled*/ true);
    let collapsed = text(&render(
        &mut contact_view,
        &contacts,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(collapsed.contains("Ran 2 tool calls (/root/a, /root/b)"));
    assert!(!collapsed.contains("Interacted with"));
    let y = contact_view
        .visible
        .iter()
        .position(|row| row.index == 0 && row.row == 0)
        .unwrap() as u16;
    assert!(contact_view.toggle_disclosure_at(&contacts, /*column*/ 0, y));
    let opened = text(&render(
        &mut contact_view,
        &contacts,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(opened.contains("Interacted with `/root/a`"));
    assert!(opened.contains("Interacted with `/root/b`"));
    contact_view.held_reading = Some(contact_view.capture_snapshot(&contacts));
    contacts.insert(
        /*index*/ 0,
        subagent(
            "older-contact",
            SubAgentActivityKind::Interacted,
            "/root/older",
        ),
    );
    contact_view.history_loaded(&contacts, 0..1);
    let pinned = text(&render(
        &mut contact_view,
        &contacts,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert_eq!(pinned.matches("Ran 2 tool calls").count(), 1);
    assert_eq!(pinned.matches("Interacted with `/root/a`").count(), 1);
    assert_eq!(pinned.matches("Interacted with `/root/b`").count(), 1);
    contact_view.jump_to_latest();
    let latest = text(&render(
        &mut contact_view,
        &contacts,
        /*width*/ 72,
        /*height*/ 20,
    ));
    assert!(latest.contains("Ran 3 tool calls (/root/older, /root/a, /root/b)"));
    assert!(latest.contains("Interacted with `/root/older`"));
}

#[test]
fn find_reveals_hidden_group_follower_without_changing_group_disclosure() {
    let output = format!(
        "{}hidden needle\n{}",
        "head\n".repeat(/*n*/ 20),
        "tail\n".repeat(/*n*/ 20)
    );
    let cells = vec![
        completed("first", "git status", "first output", /*exit_code*/ 0),
        completed("second", "curl endpoint", &output, /*exit_code*/ 0),
    ];
    for manually_expanded in [false, true] {
        let mut view = TranscriptView::default();
        view.set_collapse_tool_calls(/*enabled*/ true);
        render(&mut view, &cells, /*width*/ 80, /*height*/ 24);
        if manually_expanded {
            view.handle_key(KeyCode::F(4).into(), &cells);
            view.handle_key(KeyCode::Enter.into(), &cells);
        }
        view.jump_to_latest();
        let before = render(&mut view, &cells, /*width*/ 80, /*height*/ 24);
        assert!(!text(&before).contains("hidden needle"));
        assert!(text(&before).contains("Ran 2 tool calls"));
        let disclosure = view.disclosure.expanded.clone();

        view.begin_search();
        view.paste_search("hidden needle");
        for _ in 0..512 {
            if !view.advance_search(&cells) {
                break;
            }
        }
        assert_eq!(
            view.search.match_anchor().map(|anchor| anchor.index),
            Some(1)
        );
        let found = render(&mut view, &cells, /*width*/ 80, /*height*/ 24);
        assert!(text(&found).contains("hidden needle"));
        if !manually_expanded {
            insta::assert_snapshot!("find_hidden_group_follower", text(&found));
        }
        let snapshot = view.capture_snapshot(&cells);
        assert_eq!(snapshot.pinned[&EntryKey::cell(&cells[1])].text(), "",);
        assert_eq!(view.disclosure.expanded, disclosure);

        view.cancel_search();
        let restored = render(&mut view, &cells, /*width*/ 80, /*height*/ 24);
        assert_eq!(restored, before);
        assert_eq!(view.disclosure.expanded, disclosure);
    }
}
