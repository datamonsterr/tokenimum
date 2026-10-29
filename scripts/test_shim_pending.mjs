import assert from 'node:assert/strict'
import { Tokenimum } from '../shims/opencode.ts'
import piExtension from '../shims/pi.ts'

const delay = () => new Promise(resolve => setTimeout(resolve, 10))
for (const host of ['opencode', 'pi']) {
  let pending = false
  let release
  const deferred = () => new Promise(resolve => { release = () => resolve({ code: 0, killed: false, stdout: JSON.stringify({ pending }) }) })
  let tool, start
  if (host === 'pi') {
    const hooks = new Map()
    const ctx = { cwd: '/tmp', sessionManager: { getSessionId: () => 's' }, ui: { notify() {} } }
    piExtension({ on(name, handler) { hooks.set(name, handler) }, exec: deferred, sendMessage() {} })
    start = () => hooks.get('session_start')({}, ctx)
    tool = () => hooks.get('tool_result')({}, ctx)
  } else {
    const shell = () => ({ cwd() { return this }, quiet() { return this }, nothrow() { return this }, text() { return deferred().then(value => value.stdout) } })
    const plugin = await Tokenimum({ client: { tui: { showToast: async () => {} } }, $: shell, directory: '/tmp' })
    start = () => plugin.event({ event: { type: 'session.created', properties: { info: { id: 's' } } } })
    tool = () => plugin['tool.execute.after']({ sessionID: 's' }, {})
  }
  let finished = false
  const unknown = tool().then(() => { finished = true })
  await delay(); assert.equal(finished, false, `${host} must await unknown`); release(); await unknown
  const beginning = start(); release(); await beginning
  finished = false
  const detached = tool().then(() => { finished = true })
  await delay(); assert.equal(finished, true, `${host} must detach when clear`); release(); await detached
  await delay()
  pending = true
  const marked = start(); release(); await marked
  finished = false
  const waiting = tool().then(() => { finished = true })
  await delay(); assert.equal(finished, false, `${host} must await pending`); release(); await waiting
}
