import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * 本 fork 的发布身份（根目录 fork.json）。
 *
 * 这些值必须和上游隔开：沿用同一套 appId / 更新源 / 核心包名，两个安装包会互相升级、
 * npm 包互相覆盖（详见 docs/strip-electron-plan.md §11.8）。集中放一个文件，是为了回贡上游时
 * 只改这一处，而不是在 server.js / core-npm.js / platform.js / 打包脚本里各改一遍。
 *
 * 打包时 fork.json 会随 APP_FILES 烤进核心目录，所以这里按「与本文件同目录」读，不用往上找仓库根。
 */

const ROOT = dirname(fileURLToPath(import.meta.url))
const RAW = JSON.parse(readFileSync(join(ROOT, 'fork.json'), 'utf8'))

export const FORK = Object.freeze({
  appId: String(RAW.appId || ''),
  productName: String(RAW.productName || 'DSH-X'),
  upstreamAppId: String(RAW.upstreamAppId || ''),
  updateOwner: String(RAW.updateOwner || ''),
  updateRepo: String(RAW.updateRepo || ''),
  corePackage: String(RAW.corePackage || ''),
  dataDir: String(RAW.dataDir || 'DSH'),
  tagPrefix: String(RAW.tagPrefix || 'v'),
})

/** GitHub 上的仓库坐标，更新检查与下载地址都用它（server.js 的 APP_REPO）。 */
export const APP_REPO = `${FORK.updateOwner}/${FORK.updateRepo}`
