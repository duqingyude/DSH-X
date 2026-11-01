/**
 * 面板模型（server.js 的 panelState / panelAction / panelSettings）：原生外壳唯一的数据源。
 *
 * 为什么不测 UI：egui 的面板要靠真起窗口才看得见；这里盯的是契约——字段形状、
 * 动作转发、设置白名单、以及「状态一变就把模型推给外壳」。外壳侧的解析由
 * shell-rs/src/model.rs 的单测兜住，两边字段名必须一致。
 *
 * 全部塞进一个 test 里：server.js 是模块级单例（APP_DIR / SETTINGS_FILE 在 import 时就定了），
 * 一个文件里换第二套隔离目录是换不掉的。
 */
import assert from 'node:assert/strict'
import { isolateUserHome } from './isolated-home.mjs'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'

const PRESET_ID = '0123456789abcdef'

test('面板模型：形状、动作转发、设置白名单与推送', async (t) => {
  const root = mkdtempSync(join(tmpdir(), 'dsh-panel-'))
  const app = isolateUserHome(root)
  const home = join(root, 'home')
  const data = join(root, 'data')
  mkdirSync(app, { recursive: true })
  const profile = join(home, 'profiles', 'web')
  mkdirSync(profile, { recursive: true })
  writeFileSync(join(profile, 'package.json'), JSON.stringify({ private: true, dsh: { profile: { bundles: [] } } }))
  const settingsFile = join(app, 'settings.json')
  writeFileSync(settingsFile, JSON.stringify({
    dataDir: data,
    dshHome: home,
    profile: 'web',
    lang: 'zh',
    theme: 'light',
    // 一个只有名字、没装版本的启动项：模型里要能看到它，且 installed 为假。
    launchPresets: [{ id: PRESET_ID, name: 'DSH', version: 'auto', profile: 'web', port: 0 }],
  }))
  process.env.APPDATA = root
  process.env.DSH_VERSIONS_DATA = data
  process.env.DSH_AGENTS_HOME = join(root, 'agents')
  const server = await import('../server.js')
  t.after(() => { try { rmSync(root, { recursive: true, force: true }) } catch { /* 留给系统清 */ } })
  const settings = () => JSON.parse(readFileSync(settingsFile, 'utf8'))

  await t.test('模型形状：字段齐全、启动项裁掉图标、开关都是布尔', () => {
    const model = server.panelState()
    assert.ok(model.core.version, 'core.version 要在')
    assert.equal(model.core.profile, 'web')
    assert.ok(model.node.version, 'node.version 要在')
    assert.equal(model.prefs.lang, 'zh')
    assert.equal(model.prefs.theme, 'light')
    assert.equal(model.prefs.nodeRuntime, 'system')
    for (const key of ['hardwareAcceleration', 'autoUpdate', 'taskNotifications', 'notificationSound', 'closeToTray', 'keepAwake']) {
      assert.equal(typeof model.prefs[key], 'boolean', 'prefs.' + key + ' 必须是布尔')
    }
    // 内置启动项（全零 id）总会补一条，所以按 id 找我们写进去的那条
    const preset = model.presets.find((row) => row.id === PRESET_ID)
    assert.ok(preset, '模型里要有设置文件里那条启动项')
    assert.equal(preset.name, 'DSH')
    assert.equal(preset.profile, 'web')
    assert.equal(preset.status, 'stopped')
    assert.equal(preset.busy, false)
    assert.equal(preset.installed, false)
    // 模型不带图标：一帧塞不下 base64，面板用首字母徽标
    assert.equal('icon' in preset, false)
    assert.deepEqual(model.instances, [])
    assert.equal(model.progress, null)
    assert.equal(model.failure, null)
    assert.ok(model.updatedAt > 0)
  })

  await t.test('动作：参数原样转给 trayAction，提示文案与托盘同一句', async () => {
    await assert.rejects(
      () => server.panelAction({ id: 'nope', action: 'start', targetId: '', instanceId: '' }),
      /启动项已不存在/,
    )
    // 菜单里那份状态过期时必须拒绝，不能悄悄操作另一个版本
    await assert.rejects(
      () => server.panelAction({ id: PRESET_ID, action: 'start', targetId: 'stale', instanceId: '' }),
      /状态已变化/,
    )
  })

  await t.test('设置：白名单外的键丢掉，允许的键落盘并推一次模型', async () => {
    const pushed = []
    server.setHost({ panel: { push: (model) => pushed.push(model) } })
    await assert.rejects(
      () => server.panelSettings({ patch: { dataDir: join(root, 'elsewhere') } }),
      /没有可改的设置/,
    )
    const result = await server.panelSettings({ patch: { taskNotifications: false, dataDir: join(root, 'elsewhere') } })
    assert.deepEqual(result, { ok: true })
    assert.equal(settings().taskNotifications, false)
    assert.notEqual(settings().dataDir, join(root, 'elsewhere'), '白名单外的键不能写进设置')
    // Node 路径也走这条通道：核心先验一遍能跑，外壳不再直接改 settings.json。
    await server.panelSettings({ patch: { systemNodePath: process.execPath } })
    assert.equal(settings().systemNodePath, process.execPath)
    // 保存会触发 emitState，节流 200ms 后把新模型推给外壳
    await waitFor(() => pushed.length >= 1)
    assert.equal(pushed.at(-1).prefs.taskNotifications, false)
  })

  await t.test('没有外壳时推送是空操作，设置照样落盘', async () => {
    server.setHost({ panel: undefined })
    await server.panelSettings({ patch: { closeToTray: true } })
    assert.equal(settings().closeToTray, true)
  })
})

/** 小轮询：等节流之后的推送出现，比固定 sleep 稳。 */
async function waitFor(predicate, timeout = 3_000) {
  const until = Date.now() + timeout
  while (Date.now() < until) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error('等不到预期结果')
}