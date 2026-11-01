/**
 * 远程 host：核心跑在子进程、外壳是独立的原生程序（Rust）时，setHost 换成一层本机 IPC。
 *
 * 为什么要有这一层：§6 的约定是 server.js 一行不改，host 成员照旧由 setHost 注入；
 * 换了外壳之后这些成员要跨进程，于是 start.js 看到 DSH_SHELL_IPC 就把它们换成对外壳通道
 * 的调用。通道是一对本地套接字（Windows 命名管道 / unix 域套接字），协议是 4 字节小端
 * 长度 + UTF-8 JSON 帧（见 docs/shell-ipc-plan.md 方案 B）。
 *
 * 为什么不再用 HTTP + token：端口要占一个、token 只能证明「知道秘密」，而且外壳崩了只能
 * 靠 3 秒轮询才发现。套接字上 EOF 就是外壳死亡信号，对端还能按 pid 认亲。
 *
 * 三条约束（都是踩过才知道的）：
 * - 同步成员不能让调用方等 IO：shellUpdate.state() / desktop.info() 读缓存，缓存由外壳推来的
 *   事件帧或后台刷新填，否则 /api/self 这种同步拼装会卡住。
 * - 外壳崩了不能把核心带崩：这些调用都发生在 HTTP 请求里，一律记日志后返回空，不往上抛。
 * - 超时要短：管理页的请求会等这些调用，卡住就是整页转圈。
 */
import { connect } from 'node:net'

/** 普通调用超时：够本机 IPC 走完一轮，又不至于把管理页吊住。 */
export const DEFAULT_TIMEOUT_MS = 10_000
/** 退出/重启这类「发完就走」的调用不等太久：外壳没回应，自己也该退。 */
const EXIT_TIMEOUT_MS = 2_000
/** 同步缓存多久算过期（state() / info() 被调用时顺手在后台刷一次）。 */
const CACHE_TTL_MS = 5_000
/** 单帧上限，和 Rust 侧 bridge.rs 的 MAX_FRAME 一致：超了说明流已经错位，只能断连。 */
export const MAX_FRAME_BYTES = 1024 * 1024

/**
 * 兜底状态。current 优先用外壳通过 DSH_SHELL_VERSION 报的版本：通道的第一轮后台刷新
 * 还没回来时，/api/self 会先读到这个兜底值——留空的话页面「关于」里的外壳版本会闪成空。
 */
const DISABLED_STATE = (current = '') => ({ status: 'disabled', current: String(current || ''), latest: '', error: '' })

function frameTo(header, text) {
  const body = Buffer.from(text, 'utf8')
  header.writeUInt32LE(body.length, 0)
  return Buffer.concat([header, body])
}

/**
 * 一条到外壳通道的连接。懒连接：第一次调用时才连，连不上就让调用方拿到错误（call() 会吞）。
 * 断了下次调用自己重连——外壳重启核心时核心是全新进程，重连只在「外壳自己重开通道」时才用到。
 *
 * @param {string} path 通道名（Windows 是 \\.\pipe\...，unix 是文件路径）
 * @param {object} options
 * @param {Function} [options.log] 排障日志
 * @param {Function} [options.onEvent] 外壳推来的事件帧（目前只有 shellUpdate）
 * @param {Function} [options.onGone] 连接建立过又断掉：外壳没了
 */
export function createIpcTransport(path, { log = () => {}, onEvent = () => {}, onGone = () => {} } = {}) {
  let socket = null
  let rest = Buffer.alloc(0)
  let everConnected = false
  let nextId = 1
  const pending = new Map()

  function settleAll(error) {
    for (const [id, entry] of pending) {
      pending.delete(id)
      if (entry.timer) clearTimeout(entry.timer)
      entry.reject(error)
    }
  }

  /** 丢掉当前连接：'close' 事件会跟着来，那里负责拒绝未完成的调用。 */
  function drop() {
    if (!socket) return
    const dead = socket
    socket = null
    everConnected = false
    rest = Buffer.alloc(0)
    dead.removeAllListeners()
    dead.destroy()
  }

  function dispatch(frame) {
    if (frame && typeof frame.event === 'string') {
      onEvent(frame)
      return
    }
    const entry = pending.get(frame?.id)
    if (!entry) return
    pending.delete(frame.id)
    if (entry.timer) clearTimeout(entry.timer)
    if (frame.ok) entry.resolve(frame.result ?? null)
    else entry.reject(new Error(frame.error || '外壳调用失败'))
  }

  function onData(chunk) {
    rest = rest.length ? Buffer.concat([rest, chunk]) : chunk
    while (rest.length >= 4) {
      const length = rest.readUInt32LE(0)
      if (length === 0 || length > MAX_FRAME_BYTES) {
        // 帧一旦错位就没法再对齐，继续读只会把后面的字节当长度；断连比死循环安全。
        log(`外壳通道收到不合法的帧长度 ${length}，断开重连`)
        settleAll(new Error('外壳通道的帧长度不合法'))
        const gone = everConnected
        drop()
        if (gone) onGone()
        return
      }
      if (rest.length < 4 + length) return
      const text = rest.subarray(4, 4 + length).toString('utf8')
      rest = rest.subarray(4 + length)
      let frame
      try {
        frame = JSON.parse(text)
      } catch (error) {
        log('外壳通道收到坏 JSON，跳过一帧（' + (error?.message || error) + '）')
        continue
      }
      dispatch(frame)
    }
  }

  function ensure() {
    if (socket) return socket
    const fresh = connect({ path })
    socket = fresh
    fresh.setNoDelay(true)
    fresh.on('connect', () => { everConnected = true })
    fresh.on('data', onData)
    // 'error' 之后一定跟 'close'：只在 close 里收尾，省得两遍。
    fresh.on('error', (error) => log('外壳通道出错：' + (error?.message || error)))
    fresh.on('close', () => {
      const gone = everConnected
      drop()
      settleAll(new Error('外壳通道已断开'))
      if (gone) onGone()
    })
    return fresh
  }

  /** 发一次请求并等回帧。失败抛错，由调用方决定要不要吞。 */
  function request(method, params, { timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
    return new Promise((resolve, reject) => {
      const id = nextId++
      const entry = { resolve, reject, timer: null }
      if (timeoutMs > 0) {
        entry.timer = setTimeout(() => {
          pending.delete(id)
          reject(new Error(`外壳没有在 ${timeoutMs} 毫秒内回应 ${method}`))
        }, timeoutMs)
        entry.timer.unref?.()
      }
      pending.set(id, entry)
      const target = ensure()
      const header = Buffer.alloc(4)
      target.write(frameTo(header, JSON.stringify({ id, method, params: params ?? null })), (error) => {
        if (!error) return
        pending.delete(id)
        if (entry.timer) clearTimeout(entry.timer)
        reject(error)
      })
    })
  }

  function close() {
    settleAll(new Error('外壳通道已关闭'))
    const wasConnected = everConnected
    drop()
    return wasConnected
  }

  return { request, close, get connected() { return everConnected } }
}

/**
 * @param {object} options
 * @param {string} options.path        通道名（DSH_SHELL_IPC），例如 \\.\pipe\dsh-x-1234-ab12cd34
 * @param {Function} [options.log]     排障日志（默认丢弃，免得测试刷屏）
 * @param {Function} [options.exit]    测试注入用：真退进程由调用方决定
 * @param {Function} [options.onShellGone] 外壳通道断了：外壳已经没了，核心该收摊
 * @param {number} [options.timeout]   普通调用超时
 */
export function createRemoteHost({
  path,
  log = () => {},
  exit = (code) => process.exit(code),
  timeout = DEFAULT_TIMEOUT_MS,
  shellVersion = process.env.DSH_SHELL_VERSION || '',
  onShellGone = () => {},
} = {}) {
  if (!path) throw new Error('外壳通道名为空（DSH_SHELL_IPC）')
  /** 同步成员的缓存：见文件头第一条约束。 */
  let info = {}
  let shellState = DISABLED_STATE(shellVersion)
  let infoAt = 0
  let stateAt = 0
  let refreshing = false
  const listeners = new Set()

  /** 吞错版：只记日志，返回 fallback。 */
  async function call(method, params, { timeoutMs = timeout, fallback = null } = {}) {
    try {
      return await transport.request(method, params, { timeoutMs })
    } catch (error) {
      log('外壳调用失败：' + method + '（' + (error?.message || error) + '）')
      return fallback
    }
  }

  /**
   * 同步成员的后台刷新：state() / info() 每次被调用都看一眼缓存是否过期，过期就发一轮。
   * 用 refreshing 去重，避免 1.5s 一次的托盘轮询把通道打满。
   */
  function refreshSoon() {
    const now = Date.now()
    const stale = now - infoAt > CACHE_TTL_MS || now - stateAt > CACHE_TTL_MS
    if (!stale || refreshing) return
    refreshing = true
    void Promise.all([
      // 没回应就保留缓存：外壳可能刚推来一份事件帧，拿兜底值冲掉就等于白推。
      call('info', {}).then((data) => { if (!data) return; info = data.info || data; infoAt = Date.now() }),
      call('shellUpdate/state', {}).then((data) => { if (!data) return; shellState = data.state || data; stateAt = Date.now() }),
    ]).finally(() => { refreshing = false })
  }

  function emit(state) {
    for (const listener of listeners) {
      try {
        listener(state)
      } catch (error) {
        log('外壳更新状态订阅者出错：' + (error?.message || error))
      }
    }
  }

  /** 外壳推来一份新状态（连接建立时先推一次，将来外壳更新器也会推）。 */
  function ingest(state) {
    if (!state || typeof state !== 'object') return shellState
    shellState = { ...shellState, ...state }
    stateAt = Date.now()
    emit(shellState)
    return shellState
  }

  /**
   * 桌面能力要同步回 info（/api/desktop 的 versions 与 theme 直接读它）。
   * 缓存还没填时返回空对象——插件看到的是「暂时没有版本信息」，不是能力缺失。
   */
  function desktopInfo() {
    refreshSoon()
    return info
  }

  const transport = createIpcTransport(path, {
    log,
    onEvent: (frame) => { if (frame.event === 'shellUpdate') ingest(frame.data) },
    // EOF 就是外壳死亡信号：比看门狗轮询快，也让卸载程序立刻能删目录。
    onGone: () => onShellGone(),
  })

  const desktop = {
    info: desktopInfo,
    notify: (payload) => call('desktop/notify', payload, { fallback: {} }),
    open: (payload) => call('desktop/open', payload, { fallback: {} }),
    reveal: (payload) => call('desktop/reveal', payload, { fallback: {} }),
    clipboard: (payload) => call('desktop/clipboard', payload, { fallback: {} }),
    dialog: (payload) => call('desktop/dialog', payload, { fallback: { canceled: true } }),
  }

  const shellUpdate = {
    state: () => { refreshSoon(); return shellState },
    check: async () => {
      const data = await call('shellUpdate/check', {}, { fallback: null })
      return data?.state || data || { ...shellState, status: 'error', error: '外壳没有回应更新检查' }
    },
    download: async () => {
      const data = await call('shellUpdate/download', {}, { timeoutMs: 0, fallback: null })
      return data?.state || data || { ...shellState, status: 'error', error: '外壳没有回应下载请求' }
    },
    install: async () => call('shellUpdate/install', {}, { timeoutMs: 0, fallback: {} }),
    ingest,
  }

  return {
    onWake: () => void call('wake', {}),
    // 新外壳没有内部标签页：两种打开方式都交给系统默认浏览器（§6）。
    openTab: (tab) => void call('open', { url: tab?.url || '', title: tab?.title || '' }),
    openWindow: (target) => void call('open', { url: String(target || ''), title: '' }),
    notify: (event) => void call('notify', event, { timeoutMs: EXIT_TIMEOUT_MS }),
    // 外壳负责「先停核心再退」：这里发完请求后自己也要退，避免通道没响应时留一个孤儿进程。
    exit: (code = 0) => { void call('exit', { code }, { timeoutMs: EXIT_TIMEOUT_MS }).finally(() => exit(code)) },
    relaunch: () => { void call('relaunch', {}, { timeoutMs: EXIT_TIMEOUT_MS }).finally(() => exit(0)) },
    desktop,
    browser: {
      clearCache: () => call('browser/clearCache', {}, { fallback: { ok: false, error: '外壳没有回应' } }),
      clearAll: () => call('browser/clearAll', {}, { fallback: { ok: false, error: '外壳没有回应' } }),
    },
    shellUpdate,
    onShellUpdateState: (listener) => { if (typeof listener === 'function') listeners.add(listener) },
    /** 测试与收尾用：断开通道（真实运行时由外壳那边的 EOF 触发）。 */
    close: () => transport.close(),
  }
}

/**
 * 从环境变量装一个远程 host；没有 DSH_SHELL_IPC 就返回 null（浏览器模式）。
 * 名字缺失时 start.js 按「没有外壳」处理：宁可少一层能力，也不要起一个空壳。
 */
export function remoteHostFromEnv(env = process.env, options = {}) {
  const path = String(env.DSH_SHELL_IPC || '').trim()
  if (!path) return null
  return createRemoteHost({
    path,
    shellVersion: String(env.DSH_SHELL_VERSION || ''),
    ...options,
  })
}
