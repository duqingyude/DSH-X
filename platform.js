import { FORK } from './fork.js'

/**
 * 平台常量，外加「核心用哪个 node 拉子进程」的那点推导。
 *
 * 文件管理那一摊的行为（每用户目录在哪、外壳在哪、怎么弹对话框、怎么打开目录、
 * 开机自启）不在这里，在 filemanager.js 选出来的实现类里——这边只留常量，
 * 是为了让打包脚本能在任何一台机器上 import 它，不被平台判断牵连。
 */

export const IS_WINDOWS = process.platform === 'win32'
export const IS_MAC = process.platform === 'darwin'
export const IS_LINUX = process.platform === 'linux'

/**
 * 打包后的核心：跑在安装包里（resources/core）或用户目录的 core/current。
 * 原生外壳 supervisor 拉起核心时设 DSH_CORE=1——核心不在安装目录里，
 * 但路径和更新行为都要按安装包处理。
 */
export const PACKAGED = process.env.DSH_CORE === '1'

/**
 * 拉子进程（dsh、npm、pnpm）用的 node。
 *
 * 原生外壳用系统 node 跑核心，并通过 DSH_RUNTIME_NODE 把它报上来（supervisor 已经
 * 校验过版本）；源码运行时就是正在跑的这个 node。不再有「Electron 本体当 node」的
 * 分支——0.1.24 之后再装 Electron 外壳，核心会直接拒绝启动（见 start.js）。
 */
export function runtimeNode(env = process.env) {
  const explicit = String(env.DSH_RUNTIME_NODE || '').trim()
  return explicit || process.execPath
}

/** 进程启动时定一次；server.js / registry.js / settings.js 都从这里取，别再各自读 process.execPath。 */
export const RUNTIME_NODE = runtimeNode()

/** 每用户目录的名字（settings.json、manager.log 都在它下面）。 */
export const APP_DIR_NAME = 'DSH'

/** macOS 包的名字和 bundle id：Info.plist、LaunchAgent、自更新都认它们。 */
export const MAC_APP_NAME = 'DSH-X.app'
/* bundle id 来自 fork.json：LaunchAgent 文件名、自更新签名校验都用它（方案 §11.8）。 */
export const MAC_BUNDLE_ID = FORK.appId
/** macOS .app 里的主程序名（cargo-packager 按 productName 命名）。 */
export const LAUNCHER_NAME = 'DSH-X'
