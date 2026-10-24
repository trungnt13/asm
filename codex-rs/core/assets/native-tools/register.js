export function register(on) {
  let executable;
  const names = new Set();
  on("session.start", async ($, e, next) => {
    executable = await $.env.get("ASM_NATIVE_EXE");
    const result = await invoke($, ["list"]);
    for (const tool of result.tools) {
      await $.tool.register(tool);
      names.add("mcp__asm__" + tool.name);
    }
    return next(e);
  });
  on("tool.call", async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    const {tool, tool_use_id, agentId, consent, ...arguments_} = e;
    const result = await invoke($, ["call", tool.slice("mcp__asm__".length)], JSON.stringify(arguments_));
    return {result: result.content ?? result.error, ...(result.isError ? {isError: true} : {})};
  }).catch(async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    return {isError: true, result: "ASM call failed; outcome may be unknown. Do not retry automatically."};
  });

  async function invoke($, args, input) {
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
}
