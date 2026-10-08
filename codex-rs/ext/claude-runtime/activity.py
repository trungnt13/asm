"""Delivery and response-boundary checks for the owned Claude incarnation."""

import json
import subprocess
import time

from lifecycle import descendants
from observations import clipped


class Activity:
    def dispatch(self):
        if (
            not self.connected
            or self.active
            or self.writable_clients()
            or not self.delivery_allowed
        ):
            return
        for identifier, encoded in self.database.execute(
            "SELECT id,payload FROM mail WHERE state='queued' ORDER BY rowid"
        ):
            message = json.loads(encoded)
            if message["kind"] == "task":
                self.active, self.prompt, self.stop, self.idle_since = (
                    message,
                    None,
                    None,
                    None,
                )
                self.idle_without_stop_since = None
                self.activity_unavailable_since = None
                self.background_wait = False
                self.task_deadlines[identifier] = time.monotonic() + 300
                self.outbound.put_nowait(message)
                self.database.execute(
                    "UPDATE mail SET state='written' WHERE id=?", (identifier,)
                )
                self.database.commit()
                return

    def fail_owned_turn(self, reason, turn):
        self.settle_worker()
        for identifier, encoded in self.database.execute(
            "SELECT id,payload FROM mail WHERE state IN ('queued','written','acknowledged')"
        ).fetchall():
            message = json.loads(encoded)
            if message["kind"] == "task" and message["turn_id"] == turn:
                self.database.execute(
                    "UPDATE mail SET state='unknown' WHERE id=?", (identifier,)
                )
        self.database.commit()
        self.worker_alive, self.connected = False, False
        self.token = "revoked"
        self.active, self.prompt, self.stop, self.idle_since = None, None, None, None
        self.task_deadlines.clear()
        self.pending_prompts.clear()
        self.fragments.clear()
        self.unresolved_tools.clear()
        self.idle_without_stop_since = None
        self.activity_unavailable_since = None
        self.background_wait = False
        while not self.outbound.empty():
            self.outbound.get_nowait()
        self.emit("failed", reason, turn=turn)

    def poll(self):
        now = time.monotonic()
        if self.abort_reason:
            reason, self.abort_reason = self.abort_reason, None
            self.fail_owned_turn(reason, (self.active or {}).get("turn_id", ""))
            return
        if not self.worker_alive or now - self.last_poll < 1:
            return
        self.last_poll = now
        if self.pid not in descendants():
            queued = self.database.execute(
                "SELECT payload FROM mail WHERE state='queued'"
            ).fetchone()
            turn = (
                self.active["turn_id"]
                if self.active
                else (json.loads(queued[0])["turn_id"] if queued else "")
            )
            self.fail_owned_turn(
                "Claude interactive process exited; owned descendants settled", turn
            )
            return
        self.writable_clients()
        try:
            output = subprocess.check_output(
                [self.claude, "agents", "--json"],
                timeout=5,
                stderr=subprocess.PIPE,
                text=True,
            )
            entries = json.loads(output)
            worker = next(
                (
                    entry
                    for entry in entries
                    if entry.get("pid") == self.pid
                    and entry.get("sessionId") == self.session
                ),
                None,
            )
        except (OSError, subprocess.SubprocessError, ValueError, TypeError) as error:
            worker = None
            if not self.status_failure_noticed:
                self.emit(
                    "notice", "Claude activity cannot be confirmed: " + clipped(error)
                )
                self.status_failure_noticed = True
        expired = next(
            (
                identifier
                for identifier, deadline in self.task_deadlines.items()
                if now > deadline
            ),
            None,
        )
        if expired is not None:
            expired_mail = self.database.execute(
                "SELECT payload FROM mail WHERE id=?", (expired,)
            ).fetchone()
            turn = json.loads(expired_mail[0])["turn_id"]
            self.fail_owned_turn(
                "Claude task not acknowledged within 300 seconds; consent or hook/channel support missing",
                turn,
            )
            return
        known_activity = worker is not None and worker.get("status") in (
            "idle",
            "busy",
            "waiting",
        )
        if self.active and self.prompt and not known_activity:
            if self.activity_unavailable_since is None:
                self.activity_unavailable_since = now
            elif now - self.activity_unavailable_since >= 30:
                self.fail_owned_turn(
                    "Claude activity unavailable for 30 seconds; completion cannot be established",
                    self.active["turn_id"],
                )
                return
        else:
            self.activity_unavailable_since = None
        idle = known_activity and worker.get("status") == "idle"
        if (
            self.active
            and self.prompt
            and idle
            and not self.stop
            and not self.background_wait
        ):
            if self.idle_without_stop_since is None:
                self.idle_without_stop_since = now
            elif now - self.idle_without_stop_since >= 30:
                self.fail_owned_turn(
                    "Claude idle for 30 seconds without correlated Stop; hook delivery missing",
                    self.active["turn_id"],
                )
                return
        else:
            self.idle_without_stop_since = None
        if self.stop and idle and not self.writable_clients():
            if self.idle_since is None:
                self.idle_since = now
            elif now - self.idle_since >= 1:
                unacknowledged = self.database.execute(
                    "SELECT payload FROM mail WHERE state='written'"
                ).fetchall()
                if any(
                    json.loads(row[0])["turn_id"] == self.active["turn_id"]
                    for row in unacknowledged
                ):
                    self.fail_owned_turn(
                        "Claude stopped before acknowledging delivered work; outcome unknown",
                        self.active["turn_id"],
                    )
                    return
                for identifier in self.unresolved_tools:
                    self.emit(
                        "tool_finished",
                        {
                            "id": identifier,
                            "output": "",
                            "error": "Claude supplied no result; outcome unknown (possibly denied)",
                        },
                        event_id="tool-end:" + identifier,
                    )
                self.unresolved_tools.clear()
                queued = self.database.execute(
                    "SELECT payload FROM mail WHERE state='queued'"
                ).fetchall()
                pending_task = any(
                    json.loads(row[0])["kind"] == "task"
                    and json.loads(row[0])["turn_id"] == self.active["turn_id"]
                    for row in queued
                )
                if not pending_task:
                    self.emit(
                        "ready_to_finish",
                        {"text": clipped(self.stop.get("last_assistant_message", ""))},
                        event_id="finish:" + self.active["id"],
                    )
                for identifier, encoded in self.database.execute(
                    "SELECT id,payload FROM mail WHERE state='acknowledged'"
                ).fetchall():
                    message = json.loads(encoded)
                    if (
                        message["turn_id"] == self.active["turn_id"]
                        or message["kind"] == "message"
                    ):
                        self.database.execute(
                            "UPDATE mail SET state='finished' WHERE id=?", (identifier,)
                        )
                self.database.commit()
                self.active, self.prompt, self.stop, self.idle_since = (
                    None,
                    None,
                    None,
                    None,
                )
                self.dispatch()
        elif not idle:
            self.idle_since = None
        self.dispatch()
