-- Thread history is a derived cache. Replay canonical rollouts so existing
-- plaintext inter-agent messages receive the new display projection.
DELETE FROM thread_items;
DELETE FROM thread_turns;
DELETE FROM thread_history_projection_state;
