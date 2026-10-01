use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn navigation_discards_only_temporary_sides_after_leaving_the_pair() {
    let mut app = Box::pin(super::super::test_support::make_test_app()).await;
    let parent = ThreadId::new();
    let side = ThreadId::new();
    let elsewhere = ThreadId::new();
    for (kind, expected_discard) in [
        (CompanionKind::Side, Some(side)),
        (CompanionKind::Parallel, None),
    ] {
        let mut state = SideThreadState::new(parent);
        state.kind = kind;
        app.side_threads.insert(side, state);
        for (active, target, expected) in [
            (side, side, None),
            (side, parent, expected_discard),
            (parent, elsewhere, expected_discard),
            (parent, parent, None),
            (parent, side, None),
            (elsewhere, parent, None),
        ] {
            app.active_thread_id = Some(active);
            assert_eq!(app.side_thread_to_discard_after_switch(target), expected);
        }
    }
}
