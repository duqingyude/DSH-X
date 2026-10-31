/**
 * 远程 host（host-remote.js）：核心与原生外壳分居两个进程之后，host 成员改走本机 HTTP 桥。
 * 这里用一只假桥看住协议形状、吞错行为和两个同步成员的缓存语义——真桥由 Rust 侧实现。
 */
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { createServer as createNetServer } from 'node:net'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { createRemoteHost, remoteHostFromEnv } from '../host-remote.js'
import { isolateUserHome } from './isolated-home.mjs'

const read = (path) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8')

/** 假桥：记录每个请求，按 path 返回预置结果；没登记的 path 回 404。 */
async function fakeBridge(routes = {}) {
  const seen = []
  const server = createServer((req, res) => {
    let raw = ''
    req.on('data', (chunk) => { raw += chunk })
    req.on('end', () => {
      const body = raw ? JSON.parse(raw) : {}
      seen.push({ path: req.url, auth: req.headers.authorization, body })
      const route = routes[req.url]
      res.writeHead(route ? 200 : 404, { 'content-type': 'application/json' })
      res.end(JSON.stringify(route ? route(body) : { error: '没有这条路' }))
    })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  return {
    seen,
    url: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

test('打开页面：标签页与独立窗口都折成同一个 /host/open（新外壳交给系统浏览器）', async () => {
  const bridge = await fakeBridge({ '/host/open': () => ({ ok: true }) })
  try {
    const host = createRemoteHost({ url: bridge.url, token: 'tok-1' })
    host.openTab({ id: 'x', url: 'http://127.0.0.1:4100/', title: '实例' })
    host.openWindow('http://127.0.0.1:4200/')
    await waitFor(() => bridge.seen.length >= 2)
    assert.deepEqual(bridge.seen.map((row) => row.path), ['/host/open', '/host/open'])
    assert.equal(bridge.seen[0].body.url, 'http://127.0.0.1:4100/')
    assert.equal(bridge.seen[0].body.title, '实例')
    assert.equal(bridge.seen[1].body.url, 'http://127.0.0.1:4200/')
    // token 只走请求头，不进 body / 不进 URL
    assert.equal(bridge.seen[0].auth, 'Bearer tok-1')
  } finally {
    await bridge.close()
  }
})

test('桌面能力：请求原样转给外壳，结果原样带回', async () => {
  const bridge = await fakeBridge({ '/host/desktop/dialog': () => ({ result: { confirmed: true, canceled: false } }) })
  try {
    const host = createRemoteHost({ url: bridge.url, token: 'tok-2' })
    const result = await host.desktop.dialog({ kind: 'confirm', title: '删掉这个版本？' })
    assert.deepEqual(result, { result: { confirmed: true, canceled: false } })
    assert.deepEqual(bridge.seen[0].body, { kind: 'confirm', title: '删掉这个版本？' })
  } finally {
    await bridge.close()
  }
})

test('同步成员读缓存：shellUpdate.state() 不阻塞，check() 的结果会更新它', async () => {
  let calls = 0
  const bridge = await fakeBridge({
    '/host/shellUpdate/check': () => { calls += 1; return { state: { status: 'available', current: '1.0.0', latest: '1.1.0', error: '' } } },
    '/host/shellUpdate/state': () => ({ state: { status: 'idle', current: '1.0.0', latest: '', error: '' } }),
    '/host/info': () => ({ info: { theme: 'dark', versions: { shell: '1.0.0' } } }),
  })
  try {
    const host = createRemoteHost({ url: bridge.url, token: 'tok-3' })
    // 第一次调用发生在任何响应回来之前：必须是同步返回的默认值，不能 await
    assert.deepEqual(host.shellUpdate.state(), { status: 'disabled', current: '', latest: '', error: '' })
    const checked = await host.shellUpdate.check()
    assert.equal(calls, 1)
    assert.equal(checked.status, 'available')
    // 外壳推来的状态：只合并已知字段，其余保留
    const listener = []
    host.onShellUpdateState((state) => listener.push(state))
    host.shellUpdate.ingest({ status: 'ready', latest: '1.1.0' })
    assert.equal(host.shellUpdate.state().status, 'ready')
    assert.equal(host.shellUpdate.state().current, '1.0.0', '推来的部分字段不能把已有的 current 冲掉')
    assert.equal(listener.length, 1)
  } finally {
    await bridge.close()
  }
})

test('desktop.info() 同步返回缓存，后台自己刷新', async () => {
  const bridge = await fakeBridge({
    '/host/info': () => ({ info: { theme: 'dark', versions: { shell: '9.9.9' } } }),
    '/host/shellUpdate/state': () => ({ state: { status: 'idle', current: '9.9.9', latest: '', error: '' } }),
  })
  try {
    const host = createRemoteHost({ url: bridge.url, token: 'tok-4' })
    assert.deepEqual(host.desktop.info(), {}, '第一次调用还没数据，返回空对象而不是 undefined')
    await waitFor(() => host.desktop.info().versions?.shell === '9.9.9')
  } finally {
    await bridge.close()
  }
})

test('外壳没了：不抛、不卡，降级成空结果与 error 状态', async () => {
  const bridge = await fakeBridge({})
  const url = bridge.url
  await bridge.close()
  const logs = []
  const host = createRemoteHost({ url, token: 'tok-5', log: (line) => logs.push(line), timeout: 2_000 })
  host.openTab({ url: 'http://127.0.0.1:1/' }), host.notify({ kind: 'failed' })
  assert.deepEqual(await host.browser.clearCache(), { ok: false, error: '外壳没有回应' })
  const state = await host.shellUpdate.check()
  assert.equal(state.status, 'error')
  assert.ok(logs.length >= 2, '失败要看得到日志，不能静默')
})

test('exit / relaunch 发完请求后自己也要退：桥没回应也不留孤儿进程', async () => {
  const bridge = await fakeBridge({ '/host/exit': () => ({ ok: true }), '/host/relaunch': () => ({ ok: true }) })
  const codes = []
  try {
    const host = createRemoteHost({ url: bridge.url, token: 'tok-6', exit: (code) => codes.push(code) })
    host.exit(0)
    await waitFor(() => codes.length === 1)
    host.relaunch()
    await waitFor(() => codes.length === 2)
    assert.deepEqual(codes, [0, 0])
    assert.deepEqual(bridge.seen.map((row) => row.path), ['/host/exit', '/host/relaunch'])
  } finally {
    await bridge.close()
  }
})

test('环境变量装配：没有桥地址返回 null，有地址没 token 直接抛', () => {
  assert.equal(remoteHostFromEnv({}), null)
  assert.equal(remoteHostFromEnv({ DSH_SHELL_URL: '' }), null)
  assert.throws(() => remoteHostFromEnv({ DSH_SHELL_URL: 'http://127.0.0.1:1' }), /token/)
  const host = remoteHostFromEnv({ DSH_SHELL_URL: 'http://127.0.0.1:1', DSH_SHELL_TOKEN: 't' })
  assert.equal(typeof host.openTab, 'function')
})

test('外壳版本先由环境变量兜底：桥的第一轮刷新回来前，/api/self 也能读到 shell.current', () => {
  const host = remoteHostFromEnv({
    DSH_SHELL_URL: 'http://127.0.0.1:1',
    DSH_SHELL_TOKEN: 't',
    DSH_SHELL_VERSION: '0.1.24',
  })
  // 同步读缓存：这时还没发过请求，只能是兜底值；留空会让页面「关于」里的外壳版本闪一下空。
  assert.equal(host.shellUpdate.state().current, '0.1.24')
  assert.equal(host.shellUpdate.state().status, 'disabled')
  assert.equal(remoteHostFromEnv({ DSH_SHELL_URL: 'http://127.0.0.1:1', DSH_SHELL_TOKEN: 't' }).shellUpdate.state().current, '')
})

test('start.js 在 startServer 之前装远程 host，有外壳时不再开浏览器页', () => {
  const start = read('start.js')
  assert.match(start, /import \{ remoteHostFromEnv \} from '\.\/host-remote\.js'/)
  assert.match(start, /const REMOTE_HOST = remoteHostFromEnv\(process\.env, \{ log \}\)/)
  // 顺序没法用一条正则兜住（中间隔了几百行），直接按位置比：main() 里 setHost 必须在 startServer 之前，
  // 否则另一个实例双击时打进来的 /api/wake 会落到默认 host 上（那条路在原生外壳里没有接收方）。
  const body = start.slice(start.indexOf('async function main()'))
  const hostAt = body.indexOf('setHost(REMOTE_HOST)')
  const serverAt = body.indexOf('await startServer()')
  assert.ok(hostAt > 0, 'main() 里要装远程 host')
  assert.ok(hostAt < serverAt, 'host 必须在 startServer 之前装好，否则 /api/wake 会丢')
  assert.match(start, /if \(!APP_WINDOW && !REMOTE_HOST\) openPage\(MANAGER_URL\)/)
  assert.match(start, /if \(!REMOTE_HOST\) setHost\(\{ onWake: \(\) => showManager\(\) \}\)/)
})

test('外壳推送更新进度：POST /api/internal/shell-update 要 token，状态进 host 缓存', async (t) => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-host-push-'))
  const app = isolateUserHome(root)
  // 先探一个空闲端口再让服务去听：PORT 是模块级常量，导入之后没法改
  const probe = createNetServer()
  await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve))
  const port = probe.address().port
  await new Promise((resolve) => probe.close(resolve))
  mkdirSync(app, { recursive: true })
  writeFileSync(join(app, 'settings.json'), JSON.stringify({ dataDir: join(root, 'data'), dshHome: join(root, 'home'), seedMarket: false }))
  process.env.PORT = String(port)
  process.env.DSH_AGENTS_HOME = join(root, 'agents')
  process.env.DSH_SHELL_TOKEN = 'push-token'

  const pushed = { status: 'ready', current: '1.0.0', latest: '1.2.0', error: '' }
  const bridge = await fakeBridge({
    '/host/info': () => ({ info: { theme: 'dark', versions: { shell: '1.0.0' } } }),
    '/host/shellUpdate/state': () => ({ state: pushed }),
  })
  const server = await import('../server.js')
  const base = await server.startServer()
  t.after(async () => {
    await server.stopAll()
    await bridge.close()
    try { rmSync(root, { recursive: true, force: true }) } catch { /* 临时目录，留给系统清 */ }
  })
  server.setHost(createRemoteHost({ url: bridge.url, token: 'push-token', log: () => {} }))

  const post = (body, token) => fetch(base + '/api/internal/shell-update', {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...(token ? { authorization: 'Bearer ' + token } : {}) },
    body: JSON.stringify(body),
  })
  assert.equal((await post(pushed, '')).status, 403, '没 token 的推送不能改更新状态')
  assert.equal((await post(pushed, 'wrong')).status, 403)
  assert.equal((await post(pushed, 'push-token')).status, 200)
  const state = await (await fetch(base + '/api/shell-update')).json()
  assert.equal(state.status, 'ready')
  assert.equal(state.latest, '1.2.0')
})

/** 小轮询：等一个异步副作用出现，比固定 sleep 稳。 */
async function waitFor(predicate, timeout = 3_000) {
  const until = Date.now() + timeout
  while (Date.now() < until) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error('等不到预期结果')
}