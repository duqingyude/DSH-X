/**
 * 发布身份（§11.8）：fork 的 appId / 核心包名 / 更新源只能有一处来源（根目录 fork.json）。
 * 这里盯住最容易漂的几处：Packager.toml 与 Cargo.toml 的版本、Packager.toml 的 identifier、
 * 以及 shell-rs 里不许再出现上游身份的字面量——一旦出现，两个安装包就会互相升级、互相卸载。
 */
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('../', import.meta.url))
const read = (path) => readFileSync(join(ROOT, path), 'utf8')
const FORK = JSON.parse(read('fork.json'))

test('fork.json 字段齐全，且与上游身份不重叠', () => {
  for (const key of ['appId', 'productName', 'upstreamAppId', 'updateOwner', 'updateRepo', 'corePackage', 'dataDir', 'tagPrefix']) {
    assert.equal(typeof FORK[key], 'string', `fork.json 缺 ${key}`)
    assert.ok(FORK[key].length > 0, `fork.json 的 ${key} 是空的`)
  }
  assert.notEqual(FORK.appId, FORK.upstreamAppId, '换过身份才谈得上隔离：appId 不能等于上游的')
  assert.match(FORK.appId, /^[a-z0-9]+(\.[a-z0-9-]+)+$/, 'appId 要是反向域名')
})

test('Packager.toml 的 identifier 与版本跟 fork.json / Cargo.toml 对齐', () => {
  const toml = read('shell-rs/Packager.toml')
  const field = (key) => (toml.match(new RegExp(`^${key} = "([^"]+)"`, 'm')) || [])[1]
  assert.equal(field('identifier'), FORK.appId)
  const cargoVersion = (read('shell-rs/Cargo.toml').match(/^version = "([^"]+)"/m) || [])[1]
  assert.equal(field('version'), cargoVersion, 'Packager.toml 与 Cargo.toml 的版本必须一致')
})

test('Rust 外壳里没有写死的上游身份（appId / 仓库 / 核心包名）', () => {
  const files = readdirSync(join(ROOT, 'shell-rs', 'src')).filter((name) => name.endsWith('.rs'))
  assert.ok(files.length > 0, 'shell-rs/src 里应该有源码')
  for (const file of files) {
    const text = readFileSync(join(ROOT, 'shell-rs', 'src', file), 'utf8')
    // build.rs 通过 env! 注入 fork.json 的值，源码里不该出现任何一段上游标识。
    for (const literal of [FORK.upstreamAppId, 'yyh-001', 'dsh-x-core']) {
      assert.ok(!text.includes(literal), `shell-rs/src/${file} 里写死了 ${literal}，应该从 fork.json 读`)
    }
  }
})

test('打包脚本从 fork.json 取身份，不自己拼', () => {
  const script = read('scripts/pack-rs.mjs')
  assert.match(script, /FORK\.corePackage/, '核心包名要读 fork.json')
  assert.match(script, /FORK\.appId/, 'appId 校验要读 fork.json')
  assert.ok(!script.includes('yyh-001'), '打包脚本里不该出现上游身份')
})
