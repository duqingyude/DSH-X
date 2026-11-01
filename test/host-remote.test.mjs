/**
 * 远程 host（host-remote.js）：核心与原生外壳分居两个进程之后，host 成员走一条本机套接字，
 * 协议是 4 字节小端长度 + JSON 帧（docs/shell-ipc-plan.md 方案 B）。
 * 这里用一只假外壳看住协议形状、吞错行为和两个同步成员的缓存语义——真通道由 bridge.rs 实现。
 */
import assert from 'node:assert/strict'
import { createServer } from 'node:net'
import { readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { createRemoteHost, remoteHostFromEnv } from '../host-remote.js'

const read = (path) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8')

function writeFrame(socket, value) {
  const body = Buffer.from(JSON.stringify(value), 'utf8')
  const header = Buffer.alloc(4)
  header.writeUInt32LE(body.length, 0)
  socket.write(Buffer.concat([header, body]))
}

/** 把一帧拆成 3 字节 + 剩下的两段写：用来证明实现不是「一次 read 就当一帧」。 */
function writeSplitFrame(socket, value) {
  const body = Buffer.from(JSON.stringify(value), 'utf8')
  const header = Buffer.alloc(4)
  header.writeUInt32LE(body.length, 0)
  const whole = Buffer.concat([header, body])
  socket.write(whole.subarray(0, 3))
  setTimeout(() => socket.write(whole.subarray(3)), 20)
}

/**
 * 假外壳：一条本机套接字，按 method 返回预置结果；没登记的一律回 {ok:false}。
 * push() 用来模拟外壳主动推的事件帧（真实实现里连接建立时先推一次外壳状态）。
 */
async function fakeShell(routes = {}, { onConnect, chunked = [], silent = [] } = {}) {
  const seen = []
  const sockets = new Set()
  const server = createServer((socket) => {
    sockets.add(socket)
    socket.on('close', () => sockets.delete(socket))
    let rest = Buffer.alloc(0)
    socket.on('data', (chunk) => {
      rest = rest.length ? Buffer.concat([rest, chunk]) : chunk
      while (rest.length >= 4) {
        const length = rest.readUInt32LE(0)
        if (rest.length < 4 + length) return
        const text = rest.subarray(4, 4 + length).toString('utf8')
        rest = rest.subarray(4 + length)
        const frame = JSON.parse(text)
        seen.push(frame)
        // silent：故意不回（超时用例）；chunked：分两段回（半包用例）
        if (silent.includes(frame.method)) continue
        const handler = routes[frame.method]
        if (!handler) {
          writeFrame(socket, { id: frame.id, ok: false, error: '没有这个方法' })
          continue
        }
        const reply = { id: frame.id, ok: true, result: handler(frame.params) }
        if (chunked.includes(frame.method)) writeSplitFrame(socket, reply)
        else writeFrame(socket, reply)
      }
    })
    onConnect?.(socket)
  })
  // Windows 命名管道与 unix 域套接字在 Node 里都是 net.connect({ path })，测试统一走文件路径。
  const path = process.platform === 'win32'
    ? `\\\\.\\pipe\\dsh-x-test-${process.pid}-${Math.random().toString(16).slice(2, 10)}`
    : join(tmpdir(), `dsh-x-test-${process.pid}-${Math.random().toString(16).slice(2, 10)}.sock`)
  await new Promise((resolve) => server.listen(path, resolve))
  return {
    seen,
    path,
    push: (frame) => { for (const socket of sockets) writeFrame(socket, frame) },
    close: () => new Promise((resolve) => {
      for (const socket of sockets) socket.destroy()
      server.close(resolve)
    }),
  }
}

test('打开页面：标签页与独立窗口都折成同一个 open 方法（新外壳交给系统浏览器）', async () => {
  const shell = await fakeShell({ open: () => ({ ok: true }) })
  try {
    const host = createRemoteHost({ path: shell.path })
    host.openTab({ id: 'x', url: 'http://127.0.0.1:4100/', title: '实例' })
    host.openWindow('http://127.0.0.1:4200/')
    await waitFor(() => shell.seen.length >= 2)
    assert.deepEqual(shell.seen.map((row) => row.method), ['open', 'open'])
    assert.equal(shell.seen[0].params.url, 'http://127.0.0.1:4100/')
    assert.equal(shell.seen[0].params.title, '实例')
    assert.equal(shell.seen[1].params.url, 'http://127.0.0.1:4200/')
    // 每条请求都带 id，响应才能对回调用方
    assert.equal(shell.seen[0].id, 1)
    assert.equal(shell.seen[1].id, 2)
  } finally {
    await shell.close()
  }
})

test('桌面能力：参数原样转给外壳，结果原样带回', async () => {
  const shell = await fakeShell({ 'desktop/dialog': () => ({ canceled: false, response: 0 }) })
  try {
    const host = createRemoteHost({ path: shell.path })
    const result = await host.desktop.dialog({ kind: 'confirm', title: '删掉这个版本？' })
    assert.deepEqual(result, { canceled: false, response: 0 })
    assert.deepEqual(shell.seen[0].params, { kind: 'confirm', title: '删掉这个版本？' })
  } finally {
    await shell.close()
  }
})

test('同步成员读缓存：shellUpdate.state() 不阻塞，check() 的结果会更新它', async () => {
  let calls = 0
  const shell = await fakeShell({
    'shellUpdate/check': () => { calls += 1; return { status: 'available', current: '1.0.0', latest: '1.1.0', error: '' } },
    'shellUpdate/state': () => ({ status: 'idle', current: '1.0.0', latest: '', error: '' }),
    info: () => ({ theme: 'dark', versions: { shell: '1.0.0' } }),
  })
  try {
    const host = createRemoteHost({ path: shell.path })
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
    await shell.close()
  }
})

test('desktop.info() 同步返回缓存，后台自己刷新', async () => {
  const shell = await fakeShell({
    info: () => ({ theme: 'dark', versions: { shell: '9.9.9' } }),
    'shellUpdate/state': () => ({ status: 'idle', current: '9.9.9', latest: '', error: '' }),
  })
  try {
    const host = createRemoteHost({ path: shell.path })
    assert.deepEqual(host.desktop.info(), {}, '第一次调用还没数据，返回空对象而不是 undefined')
    await waitFor(() => host.desktop.info().versions?.shell === '9.9.9')
  } finally {
    await shell.close()
  }
})

test('外壳没了：不抛、不卡，降级成空结果与 error 状态', async () => {
  const shell = await fakeShell({})
  const path = shell.path
  await shell.close()
  const logs = []
  const host = createRemoteHost({ path, log: (line) => logs.push(line), timeout: 2_000 })
  host.openTab({ url: 'http://127.0.0.1:1/' }), host.notify({ kind: 'failed' })
  assert.deepEqual(await host.browser.clearCache(), { ok: false, error: '外壳没有回应' })
  const state = await host.shellUpdate.check()
  assert.equal(state.status, 'error')
  assert.ok(logs.length >= 2, '失败要看得到日志，不能静默')
})

test('exit / relaunch 发完请求后自己也要退：通道没回应也不留孤儿进程', async () => {
  const shell = await fakeShell({ exit: () => ({ ok: true }), relaunch: () => ({ ok: true }) })
  const codes = []
  try {
    const host = createRemoteHost({ path: shell.path, exit: (code) => codes.push(code) })
    host.exit(0)
    await waitFor(() => codes.length === 1)
    host.relaunch()
    await waitFor(() => codes.length === 2)
    assert.deepEqual(codes, [0, 0])
    assert.deepEqual(shell.seen.map((row) => row.method), ['exit', 'relaunch'])
  } finally {
    await shell.close()
  }
})

test('环境变量装配：没有通道名返回 null，否则照名字建 host', () => {
  assert.equal(remoteHostFromEnv({}), null)
  assert.equal(remoteHostFromEnv({ DSH_SHELL_IPC: '  ' }), null)
  const host = remoteHostFromEnv({ DSH_SHELL_IPC: join(tmpdir(), 'dsh-x-nope.sock') })
  assert.equal(typeof host.openTab, 'function')
})

test('外壳版本先由环境变量兜底：通道的第一轮刷新回来前，/api/self 也能读到 shell.current', () => {
  const host = remoteHostFromEnv({
    DSH_SHELL_IPC: join(tmpdir(), 'dsh-x-nope.sock'),
    DSH_SHELL_VERSION: '0.1.24',
  })
  // 同步读缓存：这时还没发过请求，只能是兜底值；留空会让页面「关于」里的外壳版本闪一下空。
  assert.equal(host.shellUpdate.state().current, '0.1.24')
  assert.equal(host.shellUpdate.state().status, 'disabled')
  assert.equal(remoteHostFromEnv({ DSH_SHELL_IPC: join(tmpdir(), 'dsh-x-nope.sock') }).shellUpdate.state().current, '')
})

test('外壳推来的事件帧直接进缓存（连接建立时它就推一次外壳状态）', async () => {
  const shell = await fakeShell({}, {
    onConnect: (socket) => writeFrame(socket, { event: 'shellUpdate', data: { status: 'disabled', current: '0.1.30' } }),
  })
  try {
    const host = createRemoteHost({ path: shell.path })
    const seen = []
    host.onShellUpdateState((state) => seen.push(state))
    // 触发一次连接：事件帧是连接建立后立刻推的，不需要等任何请求的响应
    host.onWake()
    await waitFor(() => host.shellUpdate.state().current === '0.1.30')
    assert.equal(seen.at(-1).current, '0.1.30')
  } finally {
    await shell.close()
  }
})

test('帧被拆成两段到达也能拼回来：长度前缀不是消息边界', async () => {
  // 先给半截，剩下的稍后：按「一次 read 就是一帧」写的实现会在这里失败。
  const shell = await fakeShell({ 'browser/clearCache': () => ({ ok: true }) }, { chunked: ['browser/clearCache'] })
  try {
    const host = createRemoteHost({ path: shell.path })
    assert.deepEqual(await host.browser.clearCache(), { ok: true })
  } finally {
    await shell.close()
  }
})

test('超时：外壳不回应时按超时返回 fallback，并记日志', async () => {
  const shell = await fakeShell({}, { silent: ['shellUpdate/check'] })
  const logs = []
  try {
    const host = createRemoteHost({ path: shell.path, timeout: 120, log: (line) => logs.push(line) })
    const started = Date.now()
    const result = await host.shellUpdate.check()
    assert.equal(result.status, 'error')
    assert.ok(Date.now() - started < 3_000, '不能卡住调用方')
    assert.match(logs.join('\n'), /外壳没有在 120 毫秒内回应 shellUpdate\/check/)
  } finally {
    await shell.close()
  }
})

test('通道 EOF 就是外壳死亡信号：onShellGone 只在该连上过之后才被叫', async () => {
  const shell = await fakeShell({ info: () => ({ theme: 'dark' }) })
  let gone = 0
  const host = createRemoteHost({ path: shell.path, onShellGone: () => { gone += 1 } })
  host.onWake()
  await waitFor(() => shell.seen.length >= 1)
  assert.equal(gone, 0, '还连着的时候不能误报')
  await shell.close()
  await waitFor(() => gone === 1)
  assert.equal(gone, 1)
})

test('内核连接失败不算外壳死了：没连上过就不叫 onShellGone', async () => {
  let gone = 0
  const host = createRemoteHost({
    path: join(tmpdir(), `dsh-x-missing-${process.pid}.sock`),
    timeout: 200,
    onShellGone: () => { gone += 1 },
  })
  await host.browser.clearCache()
  assert.equal(gone, 0)
})

test('start.js 在 startServer 之前装远程 host，有外壳时不再开浏览器页', () => {
  const start = read('start.js')
  assert.match(start, /import \{ remoteHostFromEnv \} from '\.\/host-remote\.js'/)
  assert.match(start, /const REMOTE_HOST = remoteHostFromEnv\(process\.env, \{ log, onShellGone: shellGone \}\)/)
  // 顺序没法用一条正则兜住（中间隔了几百行），直接按位置比：main() 里 setHost 必须在 startServer 之前，
  // 否则另一个实例双击时打进来的 /api/wake 会落到默认 host 上（那条路在原生外壳里没有接收方）。
  const body = start.slice(start.indexOf('async function main()'))
  const hostAt = body.indexOf('setHost(REMOTE_HOST)')
  const serverAt = body.indexOf('await startServer()')
  assert.ok(hostAt > 0, 'main() 里要装远程 host')
  assert.ok(hostAt < serverAt, 'host 必须在 startServer 之前装好，否则 /api/wake 会丢')
  assert.match(start, /if \(!APP_WINDOW && !REMOTE_HOST\) openPage\(MANAGER_URL\)/)
  assert.match(start, /if \(!REMOTE_HOST\) setHost\(\{ onWake: \(\) => showManager\(\) \}\)/)
  // 通道断了要能收摊：只靠 3 秒看门狗的话，卸载程序会先撞上还占着目录的核心。
  assert.match(start, /function shellGone\(\) \{[\s\S]{0,200}shutdown\(\)\.finally\(\(\) => process\.exit\(0\)\)/)
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
