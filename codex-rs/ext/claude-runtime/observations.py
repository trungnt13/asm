"""Prompt-correlated hooks and optional provider usage observations."""

import json
import time

TEXT_LIMIT = 6000


def clipped(text):
    encoded = str(text).encode()
    return (
        encoded.decode()
        if len(encoded) <= TEXT_LIMIT
        else encoded[:TEXT_LIMIT].decode(errors="ignore")
        + "\n[Claude bridge: text truncated]"
    )


class Observations:
    def hook(self, payload):
        if payload.get("session_id") != self.session or payload.get("agent_id"):
            return {}
        name = payload["hook_event_name"]
        prompt = payload.get("prompt_id")
        control_ack = payload.get("tool_name") == "mcp__asm_bridge__ack"
        control_tool = payload.get("tool_name") in (
            "mcp__asm_bridge__ack",
            "mcp__asm_bridge__read_instructions",
        )
        if payload.get("tool_name") == "mcp__asm_bridge__read_instructions":
            self.observe_instruction_read(payload)
        if (
            name == "PreToolUse"
            and self.active
            and not control_tool
            and (not self.initial_acknowledged or not prompt or self.prompt != prompt)
        ):
            return {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "Read all ASM instruction pages and acknowledge the active task before ordinary tools",
                }
            }
        if control_tool and name in ("PreToolUse", "PostToolUse", "PostToolUseFailure"):
            self.stop, self.idle_since = None, None
        if name == "PreToolUse" and control_ack:
            identifier = payload.get("tool_input", {}).get("message_id")
            if self.active and identifier and prompt:
                self.pending_prompts[identifier] = prompt
        if (
            name == "UserPromptSubmit"
            and self.active
            and (self.writable_clients() or self.prompt and prompt != self.prompt)
        ):
            self.abort_reason = "An unowned Claude prompt arrived during ASM work; completion invalidated"
            self.stop = None
            return {
                "decision": "block",
                "reason": "ASM owns the active task; use native mailbox/followup instead",
            }
        correlated = bool(prompt and self.prompt == prompt and self.active)
        if correlated and name in (
            "MessageDisplay",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
        ):
            self.idle_without_stop_since = None
            self.background_wait = False
        if name == "MessageDisplay" and correlated:
            self.stop, self.idle_since = None, None
            identifier = payload["message_id"]
            if self.database.execute(
                "SELECT 1 FROM events WHERE id=?", ("assistant:" + identifier,)
            ).fetchone():
                return {}
            if identifier not in self.fragments:
                if len(self.fragments) >= 16:
                    raise ValueError("Claude display buffer capacity exceeded")
                self.fragments[identifier] = {
                    "parts": {},
                    "next": 0,
                    "text": b"",
                    "final": None,
                    "truncated": False,
                }
            fragments = self.fragments[identifier]
            index = payload["index"]
            if index >= fragments["next"]:
                if len(fragments["parts"]) >= 32:
                    raise ValueError(
                        "Claude out-of-order display buffer capacity exceeded"
                    )
                encoded = payload["delta"].encode()
                fragments["truncated"] |= len(encoded) > TEXT_LIMIT
                fragments["parts"][index] = encoded[:TEXT_LIMIT]
            if payload["final"]:
                fragments["final"] = index
            while fragments["next"] in fragments["parts"]:
                combined = fragments["text"] + fragments["parts"].pop(fragments["next"])
                fragments["truncated"] |= len(combined) > TEXT_LIMIT
                fragments["text"] = combined[:TEXT_LIMIT]
                fragments["next"] += 1
            if (
                fragments["final"] is not None
                and fragments["next"] > fragments["final"]
            ):
                text = fragments["text"].decode(errors="ignore")
                if fragments["truncated"]:
                    text += "\n[Claude bridge: text truncated]"
                self.emit(
                    "assistant_message",
                    {"id": identifier, "text": text},
                    event_id="assistant:" + identifier,
                )
                self.fragments.pop(identifier)
        elif (
            name in ("PreToolUse", "PostToolUse", "PostToolUseFailure")
            and correlated
            and not control_tool
        ):
            self.stop, self.idle_since = None, None
            identifier = payload.get("tool_use_id", "")
            if name == "PreToolUse":
                if (
                    identifier not in self.unresolved_tools
                    and len(self.unresolved_tools) >= 128
                ):
                    raise ValueError("Claude unresolved tool capacity exceeded")
                self.unresolved_tools[identifier] = payload["tool_name"]
                arguments = payload.get("tool_input", {})
                if len(json.dumps(arguments).encode()) > TEXT_LIMIT:
                    arguments = {
                        "truncated": True,
                        "preview": clipped(json.dumps(arguments)),
                    }
                self.emit(
                    "tool_started",
                    {
                        "id": identifier,
                        "name": payload["tool_name"],
                        "arguments": arguments,
                    },
                    event_id="tool-start:" + identifier,
                )
            else:
                self.unresolved_tools.pop(identifier, None)
                self.emit(
                    "tool_finished",
                    {
                        "id": identifier,
                        "output": clipped(json.dumps(payload.get("tool_response", ""))),
                        "error": clipped(payload["error"])
                        if "error" in payload
                        else None,
                    },
                    event_id="tool-end:" + identifier,
                )
        elif name == "Stop" and correlated:
            self.idle_without_stop_since = None
            self.background_wait = bool(
                payload.get("background_tasks") or payload.get("session_crons")
            )
            if "background_tasks" not in payload or "session_crons" not in payload:
                self.abort_reason = "Claude Stop lacks required activity snapshot; unsupported completion capability"
            elif not payload["background_tasks"] and not payload["session_crons"]:
                self.stop, self.idle_since = payload, None
        elif name == "StopFailure" and correlated:
            self.abort_reason = clipped(
                payload.get("last_assistant_message")
                or payload.get("error", "Claude API error")
            )
        elif name == "PermissionRequest":
            self.emit(
                "notice",
                "Claude requests permission; attach and decide in Claude's own terminal",
            )
        elif name == "SessionEnd":
            self.emit(
                "notice", "Claude session ended: " + payload.get("reason", "unknown")
            )
        if (
            name in ("PreToolUse", "PostToolUse", "Stop")
            and correlated
            and not self.writable_clients()
        ):
            messages = []
            for identifier, encoded in self.database.execute(
                "SELECT id,payload FROM mail WHERE state='queued' ORDER BY rowid"
            ):
                message = json.loads(encoded)
                if (
                    message["kind"] == "message"
                    or message["turn_id"] == self.active["turn_id"]
                ):
                    fragment = (
                        "Mailbox "
                        + identifier
                        + " from "
                        + message["sender"]
                        + ": "
                        + message["text"]
                    )
                    fragment += (
                        "\nCall asm_bridge ack with message_id "
                        + identifier
                        + " before acting."
                    )
                    if len(("\n".join(messages + [fragment])).encode()) > 8192:
                        break
                    messages.append(fragment)
                    self.database.execute(
                        "UPDATE mail SET state='written' WHERE id=?", (identifier,)
                    )
                    if message["kind"] == "task":
                        self.active = message
                        self.task_deadlines[identifier] = time.monotonic() + 300
            self.database.commit()
            if messages:
                self.stop = None
                if name == "Stop":
                    return {"decision": "block", "reason": "\n".join(messages)}
                return {
                    "hookSpecificOutput": {
                        "hookEventName": name,
                        "additionalContext": "\n".join(messages),
                    }
                }
        return {}

    def telemetry(self, payload):
        for resource in payload.get("resourceLogs", []):
            for scope in resource.get("scopeLogs", []):
                for record in scope.get("logRecords", []):
                    attributes = {
                        item["key"]: next(iter(item["value"].values()), None)
                        for item in resource.get("resource", {}).get("attributes", [])
                        + record.get("attributes", [])
                    }
                    if (
                        attributes.get("session.id") != self.session
                        or attributes.get("event.name") != "api_request"
                    ):
                        continue
                    request_id = attributes.get("request_id") or attributes.get(
                        "client_request_id"
                    )
                    if not request_id and attributes.get("event.sequence") is None:
                        self.emit("notice", "Uncorrelated Claude usage ignored")
                        continue
                    if not request_id:
                        request_id = (
                            self.incarnation
                            + ":"
                            + str(attributes.get("event.sequence"))
                        )
                    fresh, cached, written, output = (
                        int(attributes.get(key, 0))
                        for key in (
                            "input_tokens",
                            "cache_read_tokens",
                            "cache_creation_tokens",
                            "output_tokens",
                        )
                    )
                    turn = self.prompt_turns.get(attributes.get("prompt.id"))
                    if not turn:
                        self.emit(
                            "notice",
                            "Claude usage lacks accepted prompt correlation; ignored",
                        )
                        continue
                    self.emit(
                        "usage",
                        {
                            "input_tokens": fresh + cached + written,
                            "cached_input_tokens": cached,
                            "cache_write_input_tokens": written,
                            "output_tokens": output,
                            "reasoning_output_tokens": 0,
                            "total_tokens": fresh + cached + written + output,
                        },
                        turn=turn,
                        event_id="usage:" + request_id,
                    )
