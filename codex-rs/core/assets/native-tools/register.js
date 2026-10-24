export function register(on) {
  let descriptor;
  const names = new Set();
  on("session.start", async ($, e, next) => {
    descriptor = JSON.parse(await $.fs.read(await $.env.get("ASM_NATIVE_DESCRIPTOR")));
    const response = await $.http.fetch(descriptor.url + "/tools", {
      method: "POST", headers: {Authorization: "Bearer " + descriptor.token, "Content-Type": "application/json"}, body: "{}"
    });
    if (!response.ok) throw new Error("ASM tool discovery failed: " + response.status);
    for (const tool of JSON.parse(response.text).tools) {
      await $.tool.register({...tool, isDeferred: false});
      names.add("mcp__asm__" + tool.name);
    }
    return next(e);
  });
  on("tool.call", async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    const {tool, tool_use_id, agentId, consent, ...arguments_} = e;
    const response = await $.http.fetch(descriptor.url + "/call", {
      method: "POST", headers: {Authorization: "Bearer " + descriptor.token, "Content-Type": "application/json"},
      body: JSON.stringify({name: tool.slice("mcp__asm__".length), arguments: arguments_})
    });
    if (!response.ok) return {isError: true, result: "ASM call failed: " + response.status};
    const result = JSON.parse(response.text);
    return {result: result.content, ...(result.isError ? {isError: true} : {})};
  }).catch(async ($, e, next) => {
    if (!names.has(e.tool)) return next(e);
    return {isError: true, result: "ASM call failed; outcome may be unknown. Do not retry automatically."};
  });
}
