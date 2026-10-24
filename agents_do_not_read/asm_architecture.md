## External Harness Runtime

### Proposed design — one dispatcher, two transports

```text
          CLAUDE CODE                          CURSOR CLI
        native MCP tools                     native shell tool
               │                                    │
               │                           Existing ASM binary
               │                           communication helper
               │                           ┌──────────────────┐
               │                           │ list             │
               │                           │ call + JSON stdin│
               │                           └────────┬─────────┘
               │                                    │
          MCP over HTTP                      Plain JSON over HTTP
               │                              NO MCP
               ▼                                    ▼
┌──────────────────── Owning ASM process ──────────────────────┐
│                                                             │
│       /mcp endpoint                  /tools + /call endpoints│
│              └──────────────────┬──────────────────┘         │
│                                 ▼                           │
│                    Shared communication bridge              │
│              • Authenticate child session                   │
│              • Require an active turn                       │
│              • Enforce enabled tools and caller permissions │
│              • Preserve cancellation and execution ordering │
│                                 │                           │
│                                 ▼                           │
│                   Existing canonical tool handlers          │
│                                                             │
│  ┌─────────────────┐ ┌─────────────────┐ ┌────────────────┐ │
│  │ MESSAGE BOARD   │ │ DIRECT MESSAGES │ │ LIFECYCLE      │ │
│  │                 │ │                 │ │                │ │
│  │ Channels/posts  │ │ send_message    │ │ list_agents    │ │
│  │ Read/search     │ │ followup_task   │ │ interrupt_agent│ │
│  │ Subscriptions   │ │                 │ │ wait_agent     │ │
│  └────────┬────────┘ └────────┬────────┘ └───────┬────────┘ │
│           │                  │                  │          │
│     Existing board      Existing agent queues/controller   │
└─────────────────────────────────────────────────────────────┘
```

**Independent toggles**
- Board enabled → expose board tools.
- Direct enabled → expose `send_message` and `followup_task`.
- Lifecycle tools follow their existing gates—not either communication toggle.

**Cursor setup**
```text
Child environment:
  helper executable path ──► existing ASM binary
  session file path ───────► private file: endpoint + credential

Prompt:
  short helper instructions; discover schemas through `list`
```

Results return through the same request path. Incoming messages and board notifications retain ASM’s existing delivery behavior; the CLI adds no background listener or automatic wake mechanism.

**No new daemon, interpreter, board implementation, or MCP dependency for Cursor.**