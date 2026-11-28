import { readFile, mkdir } from "node:fs/promises";
import { Agent as HttpAgent, request as httpRequest } from "node:http";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";

const sdkVersion = "1.5.2";
const directAgent = new HttpAgent({ keepAlive: true });
const write = process.stdout.write.bind(process.stdout);
const emit = (frame) => write(JSON.stringify(frame) + "\n");
// SDK logs are not protocol frames and can contain provider or workspace details.
process.stdout.write = () => true;
process.stderr.write = () => true;
const apiKey = process.env.CURSOR_API_KEY;
delete process.env.CURSOR_API_KEY;
const sdkRoot = process.env.ASM_CURSOR_SDK_ROOT ?? join(homedir(), ".local/share/asm/cursor-sdk");
const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
let agent, active, bridge, baseInstructions;
let selection, effortId, effortChoices, baselineEffort, effectiveEffort;
let firstPrompt = true;

class Rejection extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

function errorCode(error) {
  if (error instanceof Rejection) return error.code;
  if (error?.name === "AuthenticationError" || error?.code === "unauthenticated") return "authentication";
  if (error?.name === "RateLimitError" || error?.code === "resource_exhausted") return "usage_limit";
  return "sdk_error";
}

process.on("uncaughtException", () => { emit({ type: "error", code: "sdk_error" }); process.exitCode = 1; input.close(); });
process.on("unhandledRejection", () => { emit({ type: "error", code: "sdk_error" }); process.exitCode = 1; input.close(); });

function ready() {
  emit({ type: "ready", id: agent.agentId, model: selection.id, effort: effectiveEffort,
    baselineEffort, effortChoices, version: sdkVersion });
}

function setEffort(value) {
  if (value !== null && !effortId) {
    if (value === "none") return;
    throw new Rejection("invalid_effort");
  }
  const effort = value ?? baselineEffort;
  if (effort !== null && !effortChoices.includes(effort)) throw new Rejection("invalid_effort");
  const params = selection.params.filter(({ id }) => id !== effortId);
  if (effortId && effort !== null) params.push({ id: effortId, value: effort });
  selection = { id: selection.id, params };
  effectiveEffort = effort;
}

async function request(path, body, signal) {
  const payload = JSON.stringify(body);
  // Keep the bearer and tool payload on loopback. Only ASM bounds tool execution.
  const response = await new Promise((resolve, reject) => {
    const request = httpRequest(bridge.url + path, {
      method: "POST", agent: directAgent, signal,
      headers: {
        "Content-Type": "application/json",
        "Content-Length": Buffer.byteLength(payload),
        Authorization: "Bearer " + bridge.token,
      },
    }, resolve);
    request.on("error", reject);
    request.end(payload);
  });
  try {
    if (response.statusCode !== 200) throw new Rejection("tool_boundary");
    const chunks = [];
    let bytes = 0;
    for await (const chunk of response) {
      bytes += chunk.length;
      if (bytes > 32 * 1024 * 1024) throw new Rejection("tool_boundary");
      chunks.push(chunk);
    }
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } finally {
    response.destroy();
  }
}

async function toolDefinitions(activeTurn) {
  const { tools } = await request("/tools", {}, AbortSignal.timeout(10000));
  if (!Array.isArray(tools)) throw new Rejection("tool_boundary");
  const entries = tools.map(({ name, description, inputSchema }) => {
    if (!/^[a-zA-Z0-9_-]{1,64}$/.test(name)) throw new Rejection("tool_boundary");
    return [name, {
      description,
      inputSchema,
      async execute(args, { toolCallId } = {}) {
        if (active !== activeTurn || activeTurn.cancelled) {
          throw new Error("No active ASM tool turn.");
        }
        try {
          const result = await request("/call", { name, arguments: args, nativeToolUseId: toolCallId },
            activeTurn.stop.signal);
          if (!Array.isArray(result.content)) throw new Rejection("tool_boundary");
          const isError = result.isError === true;
          if (isError && !result.notDispatched) {
            result.content.unshift({ type: "text", text: "ASM tool failed. Do not retry an uncertain operation." });
          }
          if (result.mailBatch === true) activeTurn.receivedMailBatches++;
          return { content: result.content, isError };
        } catch {
          throw new Error("ASM tool result unavailable; its outcome may be unknown. Do not retry automatically.");
        }
      },
    }];
  });
  const customTools = Object.fromEntries(entries);
  if (entries.length !== Object.keys(customTools).length) throw new Rejection("tool_boundary");
  return customTools;
}

async function init(frame) {
  if (!apiKey) throw new Rejection("authentication");
  if (!isAbsolute(sdkRoot)) throw new Rejection("sdk_missing");
  const packageDir = join(sdkRoot, "node_modules/@cursor/sdk");
  let sdk;
  try {
    const metadata = JSON.parse(await readFile(join(packageDir, "package.json"), "utf8"));
    if (metadata.name !== "@cursor/sdk" || metadata.version !== sdkVersion) throw new Rejection("sdk_version");
    sdk = await import(pathToFileURL(join(packageDir, "dist/esm/index.js")).href);
  } catch (error) {
    throw error instanceof Rejection ? error : new Rejection("sdk_missing");
  }

  bridge = JSON.parse(await readFile(join(frame.bridgeDirectory, "descriptor.json"), "utf8"));
  if (!/^http:\/\/127\.0\.0\.1:[0-9]+$/.test(bridge.url) ||
      typeof bridge.token !== "string") throw new Rejection("tool_boundary");
  const parsed = /^([^\[\]]+)(?:\[([^\[\]]+)\])?$/.exec(frame.model);
  if (!parsed) throw new Rejection("invalid_model");
  const models = await sdk.Cursor.models.list({ apiKey });
  const id = parsed[1];
  const catalog = models.find((model) => model.id === id) ??
    (id === "auto" ? models.find((model) => model.id === "default") : undefined);
  if (!catalog) throw new Rejection("invalid_model");
  const params = (parsed[2]?.split(",") ?? []).map((param) => {
    const [id, value, extra] = param.split("=");
    if (extra !== undefined || !catalog.parameters?.some((option) => option.id === id &&
        option.values.some((entry) => entry.value === value))) throw new Rejection("invalid_model");
    return { id, value };
  });
  if (new Set(params.map(({ id }) => id)).size !== params.length) throw new Rejection("invalid_model");
  selection = { id: catalog.id, params };
  const effortOption = ["thought_level", "reasoning_effort", "reasoning", "effort"]
    .map((id) => catalog.parameters?.find((option) => option.id === id))
    .find(Boolean);
  effortId = effortOption?.id;
  effortChoices = [...new Set(effortOption?.values.map(({ value }) => value) ?? [])];
  const defaultParams = catalog.variants?.find((variant) => variant.isDefault)?.params;
  baselineEffort = (params.find(({ id }) => id === effortId) ??
    defaultParams?.find(({ id }) => id === effortId))?.value ?? null;
  effectiveEffort = baselineEffort;
  setEffort(frame.effort ?? null);

  baseInstructions = frame.systemInstructions;
  const privateWorkspace = join(frame.bridgeDirectory, "sdk-workspace");
  await mkdir(privateWorkspace, { mode: 0o700 });
  agent = await sdk.Agent.create({
    apiKey, model: selection, tools: ["mcp"], mcpServers: {},
    local: {
      cwd: privateWorkspace, fs: sdk.emptyFs(), settingSources: [], enableAgentRetries: false,
      store: new sdk.JsonlLocalAgentStore(join(frame.bridgeDirectory, "sdk-store")),
    },
  });
  if (agent.model?.id !== selection.id) throw new Rejection("invalid_model");
  ready();
}

async function prompt(frame, turn) {
  try {
    const customTools = await toolDefinitions(turn);
    if (turn.cancelled) {
      emit({ type: "result", status: "cancelled" });
      return;
    }
    const text = [];
    const images = [];
    if (firstPrompt) {
      text.push("Answer conversational requests without calling tools. For workspace tasks, use the ASM exec tool " +
        "with the nested tools object (for example, tools.exec_command); do not look for Cursor workspace tools. " +
        "Use direct_tool only when the task requires a named direct-only tool that exec does not expose.\n" +
        "ASM supplied base instructions (not the user's message); JSON data follows:\n" +
        JSON.stringify({ role: "developer", text: baseInstructions }) + "\nEnd of ASM supplied base instructions.");
      firstPrompt = false;
      baseInstructions = undefined;
    }
    for (const block of frame.blocks) {
      if (block.type === "text") text.push(block.text);
      else {
        text.push("[Inline image " + (images.length + 1) + "]");
        images.push({ data: block.data, mimeType: block.mimeType });
      }
    }

    let displayedText = false;
    const run = await agent.send({ text: text.join("\n\n"), images }, {
      model: selection,
      local: { customTools },
      onDelta({ update }) {
        if (update.type === "text-delta" && update.text) {
          emit({ type: "assistant_delta", text: update.text });
          displayedText = true;
        } else if (update.type === "thinking-delta" && update.text) {
          emit({ type: "thinking_delta", text: update.text });
        } else if (update.type === "thinking-completed") {
          emit({ type: "thinking_end" });
        } else if (update.type === "tool-call-started") {
          emit({ type: "assistant_end", cursorToolCallId: update.callId });
        }
      },
    });
    turn.run = run;
    if (turn.cancelled) await run.cancel();
    const result = await run.wait();
    if (!displayedText && result.result) emit({ type: "assistant_delta", text: result.result });
    emit({ type: "assistant_end" });
    emit({ type: "result", status: result.status, uuid: result.id, usage: result.usage,
      receivedMailBatches: turn.receivedMailBatches, code: result.error && errorCode(result.error) });
  } catch (error) {
    emit({ type: "result", status: "error", code: errorCode(error) });
  } finally {
    active = undefined;
  }
}

try {
  for await (const line of input) {
    const frame = JSON.parse(line);
    if (frame.type === "init" && !agent) await init(frame);
    else if (frame.type === "effort" && agent && !active) {
      try {
        setEffort(frame.effort ?? null);
        ready();
      } catch (error) {
        emit({ type: "error", code: errorCode(error) });
      }
    } else if (frame.type === "prompt" && agent && !active) {
      active = { stop: new AbortController(), receivedMailBatches: 0 };
      void prompt(frame, active);
    } else if (frame.type === "cancel" && active) {
      active.cancelled = true;
      active.stop.abort();
      if (active.run) await active.run.cancel();
    } else {
      throw new Rejection("sdk_error");
    }
  }
} catch (error) {
  emit({ type: "error", code: errorCode(error) });
  process.exitCode = 1;
} finally {
  directAgent.destroy();
  if (active) {
    active.cancelled = true;
    active.stop.abort();
    await active.run?.cancel();
  }
  if (agent) await agent[Symbol.asyncDispose]();
}
