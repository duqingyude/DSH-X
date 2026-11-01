/**
 * 桌面端系统开关的两端约定：settings.js 写 / 校验，原生外壳（shell-rs/src/prefs.rs）读。
 * 两边默认值必须一致，否则「没写过这个键」在页面上和外壳里会是两个意思。
 *
 * 外壳已经是 Rust 了，这里用解析源码的方式比对默认值：只为了一条断言去跑 cargo 不值得，
 * 而 Rust 侧那张表就是 Default::default()，读出来和 settings.js 的 DEFAULTS 逐项对。
 */
import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { isolateUserHome } from './isolated-home.mjs'

const folder = mkdtempSync(join(tmpdir(), 'dsh-prefs-parity-'))
isolateUserHome(folder)
const { saveSettings, loadSettings, DEFAULTS } = await import('../settings.js')
test.after(() => rmSync(folder, { recursive: true, force: true }))

// Windows 上 git 默认把工作区转成 CRLF，这里的正则与逐行匹配都按 \n 写，先统一成 LF 再解析
// （不然本地过、CI 红：Windows runner 上这条曾经报「prefs.rs 里的 Default 实现没找到」）。
const PREFS_RS = readFileSync(fileURLToPath(new URL('../shell-rs/src/prefs.rs', import.meta.url)), 'utf8').replace(/\r\n/g, '\n')

/** 把 Rust 的 `impl Default for Prefs` 拆成 { camelCase 键: 默认值 }。 */
function rustDefaults() {
  const block = PREFS_RS.match(/impl Default for Prefs \{[\s\S]*?\n    \}\n\}/)
  assert.ok(block, 'prefs.rs 里的 Default 实现没找到')
  const out = {}
  for (const [, key, raw] of block[0].matchAll(/^\s{12}(\w+): (.+?),$/gm)) {
    const value = raw.trim()
    const camel = key.replace(/_([a-z])/g, (_, c) => c.toUpperCase())
    if (value === 'true' || value === 'false') out[camel] = value === 'true'
    else if (value === 'String::new()') out[camel] = ''
    else {
      const str = value.match(/^"(.*)"\.to_string\(\)$/)
      assert.ok(str, `prefs.rs 里 ${key} 的默认值读不懂：${value}`)
      out[camel] = str[1]
    }
  }
  return out
}

const PREF_DEFAULTS = rustDefaults()
const KEYS = Object.keys(PREF_DEFAULTS)

test('默认值两端一致（settings.js 的 DEFAULTS 与 prefs.rs 的 Default）', () => {
  assert.ok(KEYS.length >= 9, '解析出来的键太少，prefs.rs 的写法可能变了')
  for (const key of KEYS) {
    assert.equal(DEFAULTS[key], PREF_DEFAULTS[key], `${key} 的默认值在两边不一样`)
  }
})

test('七个开关能存能读，保存后的值原样回来', async () => {
  const patch = {
    hardwareAcceleration: false,
    autoUpdate: true,
    taskNotifications: false,
    notificationSound: false,
    closeToTray: false,
    keepAwake: true,
    browserIgnoreCertificate: true,
  }
  const saved = await saveSettings(patch)
  for (const [key, value] of Object.entries(patch)) assert.equal(saved[key], value)
  const loaded = await loadSettings()
  for (const [key, value] of Object.entries(patch)) assert.equal(loaded[key], value)
})

test('脏值回默认：true 型只认 true，false 型只认 false', async () => {
  const saved = await saveSettings({
    hardwareAcceleration: 'no',
    autoUpdate: 'true',
    taskNotifications: 0,
    notificationSound: 'false',
    closeToTray: null,
    keepAwake: {},
    browserIgnoreCertificate: 'yes',
  })
  assert.equal(saved.hardwareAcceleration, true)
  assert.equal(saved.autoUpdate, false)
  assert.equal(saved.taskNotifications, true)
  assert.equal(saved.notificationSound, true)
  assert.equal(saved.closeToTray, true)
  assert.equal(saved.keepAwake, false)
  // 忽略证书默认关（脏值当关）
  assert.equal(saved.browserIgnoreCertificate, false)
})
