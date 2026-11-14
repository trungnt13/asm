-- Rebuild display-only history with typed spawn arguments instead of the old
-- text suffix. Canonical rollouts and model history remain untouched.
DELETE FROM thread_items;
DELETE FROM thread_turns;
DELETE FROM thread_history_projection_state;
