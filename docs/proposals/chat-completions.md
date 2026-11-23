# Chat Completions compatibility proposal

Status: investigation only. No bridge or transport has been validated.

ASM speaks Responses. A Chat Completions-only endpoint cannot be used as a Responses
base URL, and `wire_api = "chat"` remains unsupported. Prefer a provider-operated
Responses endpoint. If only Chat Completions is available, evaluate a trusted
Responses-to-Chat bridge before adding another wire protocol to ASM.

- Start with a private, explicit provider profile over Responses HTTP. Keep the
  upstream URL, exact model ID, catalog metadata, and credentials in local
  configuration. Give a local bridge its own client key; do not send personal
  OpenAI credentials to it. Do not add a bundled route until live validation.
- Start with text and direct function tools. Turn off Code Mode, hosted search,
  native tool search, WebSockets, and remote compaction for the trial. Inspect
  the actual request: direct mode alone does not guarantee a function-only
  tool list. Do not silently drop tools or their results during translation.
- Require an authenticated stream, then a read-only function call and its
  result on the next request. Check call IDs, complete arguments, result order,
  provider-specific reasoning replay, and completion after the terminal event.
  An interrupted stream must not finalize or dispatch an incomplete call.
- Before claiming normal session support, verify a second user turn, cold
  resume, and local compaction. Keep replay data scoped to its provider; do not
  change global reasoning serialization or send reasoning to a different
  provider. Keep rollout data and captures private.
- If no approved bridge meets those checks, evaluate an opt-in Chat Completions
  adapter at the existing model-client dispatch boundary. Reuse provider
  authentication, transport policy, and the harness response event stream.
  Do not restore the retired upstream implementation wholesale.
