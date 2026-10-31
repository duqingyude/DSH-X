/**
 * 远程 host：核心跑在子进程、外壳是独立的原生程序（Rust）时，setHost 换成一层本机 HTTP。
 *
 * 为什么要有这一层：§6 的约定是 server.js 一行不改，host 成员照旧由 setHost 注入；
 * 换了外壳之后这些成员要跨进程，于是 start.js 看到 DSH_SHELL_URL 就把它们换成对外壳桥
 * （127.0.0.1:<随机端口>）的调用。桥用 Bearer token 认亲，token 只走环境变量，不落盘。
 *
 * 三条约束（都是踩过才知道的）：
 * - 同步成员不能让调用方等网络：shellUpdate.state() / desktop.info() 读缓存，缓存由外壳推送
 *   （POST /api/internal/shell-update）或按需在后台刷新，否则 /api/self 这种同步拼装会卡住。
 * - 外壳崩了不能把核心带崩：这些调用都发生在 HTTP 请求里，一律记日志后返回空，不往上抛。
 * - 超时要短：管理页的请求会等这些调用，卡住就是整页转圈。
 */

/** 默认超时：够本机 HTTP 走完一轮，又不至于把管理页吊住。 */
export const DEFAULT_TIMEOUT_MS = 15_000
/** 退出/重启这类「发完就走」的调用不等太久：外壳没回应，自己也该退。 */
const EXIT_TIMEOUT_MS = 2_000
/** 同步缓存多久算过期（state() / info() 被调用时顺手在后台刷一次）。 */
const CACHE_TTL_MS = 5_000

/**
 * 兜底状态。current 优先用外壳通过 DSH_SHELL_VERSION 报的版本：桥的第一轮后台刷新
 * 还没回来时，/api/self 会先读到这个兜底值——留空的话页面「关于」里的外壳版本会闪成空。
 */
const DISABLED_STATE = (current = '') => ({ status: 'disabled', current: String(current || ''), latest: '', error: '' })

/**
 * @param {object} options
 * @param {string} options.url        外壳桥地址（DSH_SHELL_URL），例如 http://127.0.0.1:53421
 * @param {string} options.token      Bearer token（DSH_SHELL_TOKEN）
 * @param {Function} [options.fetchImpl] 测试注入用
 * @param {Function} [options.log]    排障日志（默认丢弃，免得测试刷屏）
 * @param {Function} [options.exit]   测试注入用：真退进程由调用方决定
 * @param {number} [options.timeout]  普通调用超时
 */
export function createRemoteHost({
  url,
  token,
  fetchImpl = (...args) => globalThis.fetch(...args),
  log = () => {},
  exit = (code) => process.exit(code),
  timeout = DEFAULT_TIMEOUT_MS,
  shellVersion = process.env.DSH_SHELL_VERSION || '',
} = {}) {
  if (!url) throw new Error('外壳桥地址为空（DSH_SHELL_URL）')
  if (!token) throw new Error('外壳桥缺 token（DSH_SHELL_TOKEN）')
  const base = String(url).replace(/\/+$/, '')
  /** 同步成员的缓存：见文件头第一条约束。 */
  let info = {}
  let shellState = DISABLED_STATE(shellVersion)
  let infoAt = 0
  let stateAt = 0
  let refreshing = false
  const listeners = new Set()

  function signalFor(timeoutMs) {
    // 0 / 负数表示不设超时（下载这类长任务走这里）
    return timeoutMs > 0 ? AbortSignal.timeout(timeoutMs) : undefined
  }

  /** 一次桥调用。失败抛错，由 call() 决定要不要吞。 */
  async function request(path, body, { timeoutMs = timeout } = {}) {
    const res = await fetchImpl(base + '/host/' + path, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: 'Bearer ' + token },
      body: JSON.stringify(body ?? {}),
      signal: signalFor(timeoutMs),
    })
    const data = await res.json().catch(() => ({}))
    if (!res.ok) throw new Error(data?.error || '外壳桥 HTTP ' + res.status)
    return data
  }

  /** 吞错版：只记日志，返回 fallback。 */
  async function call(path, body, { timeoutMs = timeout, fallback = null } = {}) {
    try {
      return await request(path, body, { timeoutMs })
    } catch (error) {
      log('外壳调用失败：' + path + '（' + (error?.message || error) + '）')
      return fallback
    }
  }

  /**
   * 同步成员的后台刷新：state() / info() 每次被调用都看一眼缓存是否过期，过期就发一轮。
   * 用 refreshing 去重，避免 1.5s 一次的托盘轮询把桥打满。
   */
  function refreshSoon() {
    const now = Date.now()
    const stale = now - infoAt > CACHE_TTL_MS || now - stateAt > CACHE_TTL_MS
    if (!stale || refreshing) return
    refreshing = true
    void Promise.all([
      request('info', {}).then((data) => { info = data?.info || data || {}; infoAt = Date.now() }, () => {}),
      request('shellUpdate/state', {}).then((data) => { shellState = data?.state || data || DISABLED_STATE(); stateAt = Date.now() }, () => {}),
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

  /** 外壳推来一份新状态（POST /api/internal/shell-update 转进来）。 */
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
    // 外壳负责「先停核心再退」：这里发完请求后自己也要退，避免桥没响应时留一个孤儿进程。
    exit: (code = 0) => { void call('exit', { code }, { timeoutMs: EXIT_TIMEOUT_MS }).finally(() => exit(code)) },
    relaunch: () => { void call('relaunch', {}, { timeoutMs: EXIT_TIMEOUT_MS }).finally(() => exit(0)) },
    desktop,
    browser: {
      clearCache: () => call('browser/clearCache', {}, { fallback: { ok: false, error: '外壳没有回应' } }),
      clearAll: () => call('browser/clearAll', {}, { fallback: { ok: false, error: '外壳没有回应' } }),
    },
    shellUpdate,
    onShellUpdateState: (listener) => { if (typeof listener === 'function') listeners.add(listener) },
  }
}

/**
 * 从环境变量装一个远程 host；没有 DSH_SHELL_URL 就返回 null（浏览器模式 / 旧路）。
 * token 缺失直接抛：宁可起不来，也不要起一个「以为有外壳」却没权限的空壳。
 */
export function remoteHostFromEnv(env = process.env, options = {}) {
  const url = String(env.DSH_SHELL_URL || '').trim()
  if (!url) return null
  return createRemoteHost({
    url,
    token: String(env.DSH_SHELL_TOKEN || ''),
    shellVersion: String(env.DSH_SHELL_VERSION || ''),
    ...options,
  })
}