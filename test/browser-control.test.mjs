/**
 * 内置浏览器（设置 → 常规 → 浏览器）的核心侧约定：核心负责放开官方侧栏浏览器那一行，
 * 证书策略与真正的数据清理在原生外壳里（测试没法拉起 GUI，所以只查核心这一半）。
 *
 * 证书开关本身的读写语义在 test/prefs-parity.test.mjs 里（settings.js 与 prefs.rs 两端一致）。
 */
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const read = (path) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8')

const server = read('server.js')

test('启动补丁只放开侧栏 Browser：不碰插件行，desktop profile 不写补丁', () => {
  assert.match(server, /- id: ui-sidebar-browser\\n  disabled: false\\n/, 'ui-sidebar-browser 始终放开')
  assert.doesNotMatch(server, /AGENT_BROWSER_PLUGIN/, 'agent 插件不再由补丁停用')
  assert.match(server, /if \(String\(profile \?\? ''\)\.toLowerCase\(\) === 'desktop'\) return \[\]/, 'desktop profile 不用补丁')
  assert.doesNotMatch(server, /BROWSER_CONTROL/, '浏览器总开关已撤')
})

test('设置接口回显证书开关和外壳能力字段', () => {
  assert.match(server, /browserIgnoreCertificate: stored\.browserIgnoreCertificate === true/)
  assert.match(server, /browserHost: Boolean\(host\.browser\)/, '页面据它决定清理按钮能不能点')
  assert.match(server, /\('browserIgnoreCertificate' in body \? \{ browserIgnoreCertificate: body\.browserIgnoreCertificate === true \}/)
})

test('清理路由只转发给外壳：没有 host.browser 时 503，不让浏览器模式假装成功', () => {
  assert.match(
    server,
    /url\.pathname === '\/api\/browser\/clear-cache' \|\| url\.pathname === '\/api\/browser\/clear-data'/,
    '两条 POST 路由',
  )
  assert.match(server, /if \(!host\.browser\) \{[\s\S]{0,220}?send\(res, 503/, '没有外壳能力返回 503')
  assert.match(server, /const action = url\.pathname\.endsWith\('\/clear-cache'\) \? 'clearCache' : 'clearAll'/, '按路径分派')
})
