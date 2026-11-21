export function register(on) {
  let executable;
  const names = new Set();
  on("session.start", async ($, e, next) => {
    executable = await $.env.get("ASM_NATIVE_EXE");
    const result = await invoke($, executable, ["list"]);
    for (const tool of result.tools) {
      await $.tool.register(tool);
      names.add("mcp__asm__" + tool.name);
    }
    await invoke($, executable, ["attest"], JSON.stringify({tools: await $.tool.list(), commands: await $.command.list()}));
    return next(e);
  });
  on("tool.call", async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    const {tool, tool_use_id, agentId, consent, ...arguments_} = e;
    const id = typeof tool_use_id === "string" ? tool_use_id : "";
    const result = await invoke($, executable, ["call", tool.slice("mcp__asm__".length), id], JSON.stringify(arguments_));
    if (!Array.isArray(result.content)) result.content = [{type: "text", text: result.error || "ASM helper returned no tool content."}];
    if (result.isError && !result.notDispatched) result.content.unshift({type: "text", text: "ASM tool failed. This is a tool result, not permission to retry an uncertain operation."});
    return {result: result.content.map(item => item.type === "image"
      ? {type: "image", source: {type: "base64", media_type: item.mimeType, data: item.data}}
      : item)};
  }).catch(async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    return {deny: "ASM call failed; outcome may be unknown. Do not retry automatically."};
  });
}

async function invoke($, executable, args, input) {
  // Native HTTP and process.run have short deadlines; spawn lasts until exit or cancellation.
  const stream = $.process.spawn({argv: [executable, "--asm-native-tools", ...args], input});
  let stdout = "";
  try {
    while (true) {
      const item = await stream.next();
      if (item.done) {
        const result = JSON.parse(stdout);
        if (item.value.code !== 0 && !result.isError) throw new Error("ASM helper failed");
        return result;
      }
      if (item.value.stream === "stdout") stdout += item.value.text;
    }
  } finally {
    await stream.return();
  }
}
