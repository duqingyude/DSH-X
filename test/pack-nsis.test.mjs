/**
 * 安装路径与卸载残留（Windows NSIS）：
 * cargo-packager 把 currentUser 的默认目录写死成 %LOCALAPPDATA%\<产物名>，也没有配置项能改，
 * 只能换模板（shell-rs/nsis/installer.nsi）。这个用例盯住三件事：默认目录在 Programs 下、
 * 记住的旧目录要验证仍然存在、卸载要递归删目录（否则会留下空的 core/）。
 */
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('../', import.meta.url))
const read = (path) => readFileSync(join(ROOT, path), 'utf8')
const TEMPLATE = 'shell-rs/nsis/installer.nsi'

test('Packager.toml 指向自己的 NSIS 模板，文件也在', () => {
  const toml = read('shell-rs/Packager.toml')
  assert.match(toml, /^template = "nsis\/installer\.nsi"$/m, 'Packager.toml 要声明 [nsis] template')
  assert.ok(existsSync(join(ROOT, TEMPLATE)), TEMPLATE + ' 不存在：安装会退回默认目录')
})

test('每用户安装的默认目录是 %LOCALAPPDATA%\\Programs\\<产物名>', () => {
  const nsi = read(TEMPLATE)
  const start = nsi.indexOf('!else if "${INSTALLMODE}" == "currentUser"')
  const end = nsi.indexOf('Call RestorePreviousInstallLocation')
  const block = nsi.slice(start, end)
  assert.ok(start > 0 && end > start, '模板里找不到 currentUser 的默认目录块')
  assert.ok(block.includes('StrCpy $INSTDIR "$LOCALAPPDATA\\Programs\\${PRODUCTNAME}"'), '默认目录要在 Programs 下')
  assert.ok(!block.includes('StrCpy $INSTDIR "$LOCALAPPDATA\\${PRODUCTNAME}"'), '不能再装到 %LOCALAPPDATA% 根上')
})

test('记住的上次安装目录只有在仍然是个安装时才沿用', () => {
  const nsi = read(TEMPLATE)
  const fn = nsi.slice(nsi.indexOf('Function RestorePreviousInstallLocation'), nsi.indexOf('Function SkipIfPassive'))
  assert.ok(fn.includes('IfFileExists "$4\\uninstall.exe"'), '要先确认旧目录里还有 uninstall.exe')
})

test('卸载时递归删目录，不留空的 core/', () => {
  const nsi = read(TEMPLATE)
  assert.ok(nsi.includes('RMDir /REBOOTOK /r "$INSTDIR\\\\{{this}}"'), 'resources_dirs 要递归删')
})

test('安装界面是中文：Packager.toml 给 NSIS 插 SimpChinese 语言包', () => {
  const toml = read('shell-rs/Packager.toml')
  assert.match(toml, /^languages = \["SimpChinese"\]$/m, 'cargo-packager 默认只插 English，界面会是英文')
  const nsi = read(TEMPLATE)
  assert.ok(nsi.includes('{{#each languages}}'), '模板要把 languages 列表展开')
  assert.ok(nsi.includes('!insertmacro MUI_LANGUAGE'), '模板要真的插 MUI 语言包')
})

test('打包脚本会在模板缺失时报错，而不是默默装到默认目录', () => {
  const script = read('scripts/pack-rs.mjs')
  assert.ok(script.includes('nsis template'), 'pack-rs.mjs 要校验模板')
})
