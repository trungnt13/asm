use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn navigation_discards_temporary_side_only_when_returning_to_its_parent() {
    let mut app = Box::pin(super::super::test_support::make_test_app()).await;
    let parent = ThreadId::new();
    let side = ThreadId::new();
    let elsewhere = ThreadId::new();
    app.side_threads.insert(side, SideThreadState::new(parent));
    for (active, target, expected) in [
        (side, side, None),
        (side, parent, Some(side)),
        (parent, elsewhere, None),
        (parent, parent, None),
        (parent, side, None),
        (elsewhere, parent, None),
    ] {
        app.active_thread_id = Some(active);
        assert_eq!(app.side_thread_to_discard_after_switch(target), expected);
    }
}
