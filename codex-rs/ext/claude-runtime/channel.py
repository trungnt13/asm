"""Small MCP channel server. Claude owns its stdio transport and model execution."""

import json
import queue
import sys
import threading
from pathlib import Path

from bridge import LIMIT, exchange


def main():
    config = json.loads(Path(sys.argv[1]).read_text())
    incoming = queue.Queue(maxsize=64)
    connected = False
    page_count = config["instruction_pages"]
    instruction_guide = (
        f"ASM instructions revision {config['instruction_revision']} has {page_count} pages. "
        "Call read_instructions once per page, with index 0 through page count minus one; "
        "fetch every page separately before acting on the first task or calling ack. "
        "Follow all instruction pages. Each channel message has message_id; call ack with that exact ID "
        "before acting. Ack records receipt, not understanding, permission approval, or task success."
    )

    def read_requests():
        while True:
            line = sys.stdin.readline(LIMIT + 1)
            if not line:
                break
            if len(line) > LIMIT:
                incoming.put(None)
                return
            try:
                incoming.put(json.loads(line))
            except ValueError:
                incoming.put(None)
                return
        incoming.put(None)

    threading.Thread(target=read_requests, daemon=True).start()
    while True:
        try:
            request = incoming.get(timeout=0.3)
        except queue.Empty:
            request = {}
        if request is None:
            return
        method = request.get("method")
        identifier = request.get("id")
        result = None
        error = None
        if method == "initialize":
            result = {
                "protocolVersion": "2025-06-18",
                "capabilities": {"experimental": {"claude/channel": {}}, "tools": {}},
                "serverInfo": {"name": "asm_bridge", "version": "1"},
                "instructions": instruction_guide,
            }
        elif method == "notifications/initialized":
            exchange(config, "/connect", {})
            connected = True
        elif method == "ping":
            result = {}
        elif method == "tools/list":
            result = {
                "tools": [
                    {
                        "name": "read_instructions",
                        "description": f"Read one immutable ASM instruction page. There are {page_count} pages; fetch every index before initial task acknowledgement.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "page": {
                                    "type": "integer",
                                    "minimum": 0,
                                    "maximum": max(0, page_count - 1),
                                }
                            },
                            "required": ["page"],
                            "additionalProperties": False,
                        },
                    },
                    {
                        "name": "ack",
                        "description": "Acknowledge receipt of the active ASM task; does not approve tool execution.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"message_id": {"type": "string"}},
                            "required": ["message_id"],
                            "additionalProperties": False,
                        },
                    },
                ]
            }
        elif method == "tools/call":
            params = request.get("params", {})
            tool_name = params.get("name")
            if tool_name not in ("ack", "read_instructions"):
                error = {"code": -32602, "message": "Unknown tool"}
            else:
                endpoint = "/ack" if tool_name == "ack" else "/instructions"
                observation = exchange(config, endpoint, params.get("arguments", {}))
                result = {
                    "content": [
                        {
                            "type": "text",
                            "text": observation.get("error")
                            or observation.get(
                                "text",
                                "Receipt acknowledged; understanding and task success are not verified",
                            ),
                        }
                    ],
                    "isError": "error" in observation,
                }
        elif identifier is not None:
            error = {"code": -32601, "message": "Method not found"}
        if identifier is not None:
            reply = {"jsonrpc": "2.0", "id": identifier}
            reply["error" if error else "result"] = error or result
            print(json.dumps(reply), flush=True)
        if connected:
            message = exchange(config, "/channel", {})
            if message:
                print(
                    json.dumps(
                        {
                            "jsonrpc": "2.0",
                            "method": "notifications/claude/channel",
                            "params": {
                                "content": message["text"],
                                "meta": {
                                    "message_id": message["id"],
                                    "sender": message["sender"],
                                },
                            },
                        }
                    ),
                    flush=True,
                )


if __name__ == "__main__":
    main()
