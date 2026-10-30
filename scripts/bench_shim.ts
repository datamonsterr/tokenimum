// Exercise the shipped shim's tool callback, including a real tkm-hook process.
import { $ } from "bun"
import { Tokenimum } from "../shims/opencode.ts"
import piExtension from "../shims/pi.ts"

let calls = 0
let completed = false
const cwd = process.cwd()
const host = process.argv[2]
if (host === "opencode") {
  // Wrap Bun's shell only to verify that the shim actually waited for a successful child.
  const shell = (strings: TemplateStringsArray, ...values: unknown[]) => {
    calls++
    const command = $(strings, ...values)
    const wrapper = {
      cwd(path: string) { command.cwd(path); return wrapper },
      quiet() { command.quiet(); return wrapper },
      nothrow() { command.nothrow(); return wrapper },
      async text() {
        const result = await command
        completed = result.exitCode === 0
        return result.text()
      },
    }
    return wrapper
  }
  const plugin = await Tokenimum({ client: { tui: { showToast: async () => {} } }, $: shell, directory: cwd } as any)
  await plugin["tool.execute.after"]!({ sessionID: "bench-opencode", tool: "bash", args: { command: "true" } } as any, { output: "ok", metadata: { exit: 0 } } as any)
} else if (host === "pi") {
  const hooks = new Map<string, Function>()
  piExtension({
    on(name: string, fn: Function) { hooks.set(name, fn) },
    async exec(command: string, args: string[], options: { cwd: string, timeout: number }) {
      calls++
      const child = Bun.spawn([command, ...args], { cwd: options.cwd, timeout: options.timeout, stdout: "pipe", stderr: "ignore" })
      const stdout = await new Response(child.stdout).text()
      const code = await child.exited
      completed = code === 0
      return { code, killed: false, stdout }
    },
  } as any)
  const hook = hooks.get("tool_result")
  if (!hook) throw new Error("Pi tool_result callback missing")
  await hook({ toolName: "bash", input: { command: "true" }, content: "ok", isError: false }, { cwd, sessionManager: { getSessionId: () => "bench-pi" }, ui: { notify() {} } })
} else {
  throw new Error(`unknown shim: ${host}`)
}
if (calls !== 1 || !completed) throw new Error(`shim did not complete one successful hook process: ${host}`)
