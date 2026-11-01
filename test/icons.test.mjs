/**
 * 图标素材：exe / 快捷方式 / 托盘 / 窗口 / 网站 favicon / mac .icns 全部出自同一份 master
 * （assets/icon.png），由 scripts/make-icons.py 生成。历史上 .ico 这一系长期是过期的旧图标
 * （里面还是通用窗格），装出来的程序看着像没换图标，所以用例盯住两件事：尺寸档齐全、
 * 各处副本与 exe 用的那份同源。
 */
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('../', import.meta.url))
const read = (path) => readFileSync(join(ROOT, path))
const sha = (buffer) => createHash('sha256').update(buffer).digest('hex')

/** ICONDIR 每档 16 字节：宽、高（0 表示 256）、调色板、保留、平面、位深、长度、偏移。 */
function icoSizes(buffer) {
  assert.equal(buffer.readUInt16LE(2), 1, 'icon.ico 的类型字段不是 1')
  const count = buffer.readUInt16LE(4)
  const sizes = []
  for (let i = 0; i < count; i += 1) {
    const width = buffer[6 + i * 16]
    sizes.push(width === 0 ? 256 : width)
  }
  return sizes.sort((a, b) => a - b)
}

/** PNG 的宽高在 IHDR 里，紧跟 8 字节签名。 */
function pngSize(buffer) {
  assert.equal(buffer.subarray(1, 4).toString('latin1'), 'PNG', '不是 PNG 文件')
  return { width: buffer.readUInt32BE(16), height: buffer.readUInt32BE(20) }
}

test('assets/icon.ico 覆盖 Windows 各档（16/32/48/64/256）', () => {
  const sizes = icoSizes(read('assets/icon.ico'))
  assert.deepEqual(sizes, [16, 32, 48, 64, 256], 'exe 与快捷方式要的尺寸档缺了会糊')
  assert.deepEqual(icoSizes(read('assets/dsh.ico')), sizes, 'dsh.ico 要和 icon.ico 同一套档位')
})

test('网站 favicon 与 exe 图标同一份素材', () => {
  const icon = sha(read('assets/icon.ico'))
  for (const path of ['docs/favicon.ico', 'public/favicon.ico']) {
    assert.equal(sha(read(path)), icon, path + ' 与 assets/icon.ico 不同：重跑 scripts/make-icons.py')
  }
})

test('托盘 / 窗口图标是 64px 圆角图，mac 有 .icns', () => {
  assert.deepEqual(pngSize(read('assets/tray.png')), { width: 64, height: 64 })
  assert.ok(existsSync(join(ROOT, 'assets/icon.icns')), 'assets/icon.icns 不在：mac job 会整段跳过')
})

test('外壳从包里读的是 tray.png（圆角带透明边），不是方形 master', () => {
  for (const path of ['shell-rs/src/tray.rs', 'shell-rs/src/main.rs']) {
    const src = readFileSync(join(ROOT, path), 'utf8')
    assert.ok(src.includes('../../assets/tray.png'), path + ' 要 include tray.png')
    assert.ok(!src.includes('include_bytes!("../../assets/icon.png")'), path + ' 不该直接 include master')
  }
})

test('Packager.toml 给安装包用的图标都在', () => {
  const toml = readFileSync(join(ROOT, 'shell-rs/Packager.toml'), 'utf8')
  const icons = /^icons = \[(.*)\]$/m.exec(toml)
  assert.ok(icons, 'Packager.toml 要声明 icons')
  const paths = [...icons[1].matchAll(/"([^"]+)"/g)].map((match) => match[1])
  assert.ok(paths.length >= 2, 'exe 图标要给 .ico 和 .png 两份')
  for (const path of paths) {
    assert.ok(existsSync(join(ROOT, 'shell-rs', path)), path + ' 不存在')
  }
})

test('exe 与安装包的图标都接到 assets/icon.ico 上', () => {
  // exe：Windows 只认资源段，cargo-packager 不会往里塞，所以由 build.rs 编 icon.rc。
  const build = readFileSync(join(ROOT, 'shell-rs/build.rs'), 'utf8')
  assert.match(build, /embed_resource::compile\("icon\.rc"/, 'build.rs 要编 icon.rc，否则 exe 是通用空白图标')
  const rc = readFileSync(join(ROOT, 'shell-rs/icon.rc'), 'utf8')
  assert.match(rc, /ICON "\.\.\/assets\/icon\.ico"/, 'icon.rc 要指向 assets/icon.ico')
  // 安装包与卸载程序：NSIS 的 MUI_ICON / MUI_UNICON。
  const toml = readFileSync(join(ROOT, 'shell-rs/Packager.toml'), 'utf8')
  assert.match(toml, /^installer_icon = "\.\.\/assets\/icon\.ico"$/m, '[nsis] installer_icon 没配')
  const nsi = readFileSync(join(ROOT, 'shell-rs/nsis/installer.nsi'), 'utf8')
  assert.ok(nsi.includes('!define MUI_ICON "${INSTALLERICON}"'), '模板要设安装程序图标')
  assert.ok(nsi.includes('!define MUI_UNICON "${INSTALLERICON}"'), '模板要设卸载程序图标')
})