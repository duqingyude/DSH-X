// 打 Rust 外壳的安装包（NSIS / dmg / AppImage / deb），是 npm run dist 的唯一通道。
// 核心层清单只有一份（pack-common.mjs 的 copyAppFiles），核心包与安装包共用。
// node/ 的包装脚本一律转调系统 Node：外壳已经把它找好、通过 DSH_RUNTIME_NODE 传给核心
//（见 shell-rs/src/supervisor.rs）；安装包里不再带 node.exe，也不再转调 Electron 本体。
//
// 用法：node scripts/pack-rs.mjs [--skip-build]。需要先 cargo install cargo-packager --locked。
import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { chmod, cp, mkdir, rm, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import {
  ROOT,
  PKG,
  VENDOR,
  NODE_VERSION,
  copyAppFiles,
  copyNpmModules,
  copyPnpm,
  downloadNodeFile,
  run,
} from './pack-common.mjs'

const IS_WINDOWS = process.platform === 'win32'
const IS_MAC = process.platform === 'darwin'
const ARCH = process.arch === 'arm64' ? 'arm64' : 'x64'
const SHELL_DIR = join(ROOT, 'shell-rs')
const STAGE = join(ROOT, 'release', 'rs-stage')
const CORE_STAGE = join(STAGE, 'core')
const OUT = join(ROOT, 'release', 'packager')
const FORK = JSON.parse(readFileSync(join(ROOT, 'fork.json'), 'utf8'))

// 版本与身份：Packager.toml 只认静态配置，所以它和 Cargo.toml / fork.json 各写了一份，
// 这里把三处对齐——drift 了就直接报错，而不是打出一个版本号对不上的安装包。
function checkPackagerConfig() {
  const toml = readFileSync(join(SHELL_DIR, 'Packager.toml'), 'utf8')
  const field = (key) => (toml.match(new RegExp(`^${key} = "([^"]+)"`, 'm')) || [])[1]
  const cargo = readFileSync(join(SHELL_DIR, 'Cargo.toml'), 'utf8')
  const version = (cargo.match(/^version = "([^"]+)"/m) || [])[1]
  if (field('version') !== version) {
    throw new Error(`Packager.toml 的 version (${field('version')}) 与 Cargo.toml (${version}) 不一致`)
  }
  if (field('identifier') !== FORK.appId) {
    throw new Error(`Packager.toml 的 identifier (${field('identifier')}) 与 fork.json 的 appId (${FORK.appId}) 不一致`)
  }
  return version
}

// 系统 Node 由外壳负责找（§5），所以这里的包装脚本只做转调，不自带 node.exe。
// 以前 Electron 版是转调 Electron 本体（ELECTRON_RUN_AS_NODE=1），那套到了 Rust 外壳就没了。
const NO_NODE = 'DSH-X: 找不到系统 Node，请先安装 Node 22 或 24（外壳设置里可以指定路径）'
async function writeNodeWrappers(nodeDir) {
  await mkdir(nodeDir, { recursive: true })
  const shim = join(nodeDir, 'node')
  await writeFile(shim, [
    '#!/bin/sh',
    `if [ -z "$DSH_RUNTIME_NODE" ]; then echo "${NO_NODE}" >&2; exit 1; fi`,
    'exec "$DSH_RUNTIME_NODE" "$@"',
    '',
  ].join('\n'))
  await chmod(shim, 0o755)
  const shims = { npm: 'npm/bin/npm-cli.js', npx: 'npm/bin/npx-cli.js', pnpm: 'pnpm/bin/pnpm.cjs', pnpx: 'pnpm/bin/pnpx.cjs' }
  for (const [name, cli] of Object.entries(shims)) {
    const file = join(nodeDir, name)
    await writeFile(file, [
      '#!/bin/sh',
      'dir="$(cd "$(dirname "$0")" && pwd)"',
      `exec "$dir/node" "$dir/node_modules/${cli}" "$@"`,
      '',
    ].join('\n'))
    await chmod(file, 0o755)
  }
  await writeFile(join(nodeDir, 'node.cmd'), [
    '@ECHO off',
    'SETLOCAL',
    'IF NOT DEFINED DSH_RUNTIME_NODE (',
    `  ECHO ${NO_NODE} 1>&2`,
    '  EXIT /B 1',
    ')',
    '"%DSH_RUNTIME_NODE%" %*',
    '',
  ].join('\r\n'))
  await writeFile(join(nodeDir, 'pnpm.cmd'), [
    '@ECHO off',
    'SETLOCAL',
    'SET "PNPM_JS=%~dp0node_modules\\pnpm\\bin\\pnpm.cjs"',
    'call "%~dp0node.cmd" "%PNPM_JS%" %*',
    '',
  ].join('\r\n'))
  await writeFile(join(nodeDir, 'pnpx.cmd'), [
    '@ECHO off',
    'SETLOCAL',
    'call "%~dp0node.cmd" "%~dp0node_modules\\pnpm\\bin\\pnpx.cjs" %*',
    '',
  ].join('\r\n'))
}

// npm / pnpm 的 JS 包来自官方 node 发行包（只取 JS，不要 node 本体）——
// 用户机器上装没装 npm 不确定，插件安装又离不开 pnpm。
const DIST = `node-v${NODE_VERSION}-${IS_MAC ? 'darwin' : IS_WINDOWS ? 'win' : 'linux'}-${ARCH}`
const EXTRACTED = join(VENDOR, DIST)
async function ensureNodeDist() {
  await mkdir(VENDOR, { recursive: true })
  if (existsSync(join(EXTRACTED, IS_WINDOWS ? 'node.exe' : 'bin/node'))) return
  const archive = IS_WINDOWS ? `${DIST}.zip` : `${DIST}.tar.gz`
  const file = join(VENDOR, archive)
  await downloadNodeFile(archive, file)
  if (IS_WINDOWS) run('powershell', ['-NoProfile', '-Command', `Expand-Archive -Force '${file}' '${VENDOR}'`])
  else run('tar', ['-xzf', file, '-C', VENDOR])
}

async function buildCoreStage(shellVersion) {
  await rm(CORE_STAGE, { recursive: true, force: true })
  await mkdir(CORE_STAGE, { recursive: true })
  await copyAppFiles(CORE_STAGE)
  const nodeDir = join(CORE_STAGE, 'node')
  const modules = join(EXTRACTED, IS_WINDOWS ? '' : 'lib', 'node_modules')
  await copyNpmModules(modules, nodeDir)
  await copyPnpm(nodeDir, join(modules, 'npm', 'bin', 'npm-cli.js'))
  await writeNodeWrappers(nodeDir)
  await writeFile(join(CORE_STAGE, 'package.json'), `${JSON.stringify({
    // 核心包名跟着 fork.json 走：上游也发 dsh-x-core，同名会互相覆盖（§11.8）。
    name: FORK.corePackage,
    productName: FORK.productName,
    version: PKG.version,
    private: true,
    type: 'module',
    main: 'server.js',
    // 外壳最低版本：核心更新时用它判断要不要先提示更新外壳（旧的 dshX.electron 已废弃）。
    dshX: { shell: shellVersion },
  }, null, 2)}\n`)
  console.log(`核心层：${CORE_STAGE}（${PKG.version}）`)
}

function buildShell() {
  // opt-level=s + lto + strip（见 Cargo.toml），面板程序不需要极限性能，体积优先。
  run('cargo', ['build', '--release', '--manifest-path', join(SHELL_DIR, 'Cargo.toml')])
}

function bundle(formats) {
  const args = ['packager', '--config', 'Packager.toml', '-f', formats]
  // 有私钥就签名：cargo-packager 的 minisign 签名是外壳自更新的前提。
  const key = join(ROOT, 'release', 'packager-key.pem')
  if (existsSync(key)) args.push('-k', key)
  // 在 shell-rs/ 里跑：Packager.toml 里的相对路径都是相对它写的。
  const result = spawnSync('cargo', args, { cwd: SHELL_DIR, stdio: 'inherit', windowsHide: false, shell: IS_WINDOWS })
  if (result.status !== 0) throw new Error('cargo packager failed')
  console.log(`安装包输出：${OUT}`)
}

const shellVersion = checkPackagerConfig()
await ensureNodeDist()
if (!process.argv.includes('--skip-build')) buildShell()
await buildCoreStage(shellVersion)
bundle(IS_MAC ? 'app,dmg' : IS_WINDOWS ? 'nsis' : 'appimage,deb')
