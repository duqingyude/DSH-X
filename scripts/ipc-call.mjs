#!/usr/bin/env node
/**
 * 手动调一次外壳通道（排障用）。
 *
 * 换成套接字之后不能再拿 curl 试了；这条命令就是用核心同一套帧协议问一句、打印回包：
 *   node scripts/ipc-call.mjs info
 *   node scripts/ipc-call.mjs desktop/dialog '{"kind":"message","options":{"message":"hi"}}'
 * 通道名默认取 DSH_SHELL_IPC（外壳 spawn 核心时才有），也可以用 --path 直接指。
 * 注意：外壳默认只认「自己拉起的核心」的 pid，本工具是独立进程，所以外壳要用
 * DSH_SHELL_IPC_ALLOW_ANY=1 起才连得进去（默认关着，为的是别人摸到管道名也进不来）。
 */
import { createIpcTransport } from '../host-remote.js'

const args = process.argv.slice(2)
const pathIndex = args.indexOf('--path')
const explicit = pathIndex >= 0 ? args.splice(pathIndex, 2)[1] : ''
const [method, raw] = args

if (!method) {
  console.error('用法：node scripts/ipc-call.mjs [--path <通道名>] <动作> [JSON 参数]')
  console.error('动作：open / wake / notify / exit / relaunch / info / desktop/* / browser/* / shellUpdate/*')
  process.exit(1)
}

const pipe = explicit || process.env.DSH_SHELL_IPC || ''
if (!pipe) {
  console.error('没有通道名：设置 DSH_SHELL_IPC，或用 --path 指定（Windows 是 \\.\pipe\...，unix 是 .sock 路径）')
  console.error('外壳需要以 DSH_SHELL_IPC_ALLOW_ANY=1 启动，否则会被对端 pid 校验挡下。')
  process.exit(1)
}

let params = null
if (raw) {
  try {
    params = JSON.parse(raw)
  } catch (error) {
    console.error('参数不是合法 JSON：' + error.message)
    process.exit(1)
  }
}

const transport = createIpcTransport(pipe, { log: (line) => console.error(line) })
try {
  const result = await transport.request(method, params, { timeoutMs: 10_000 })
  console.log(JSON.stringify(result, null, 2))
} catch (error) {
  console.error('调用失败：' + (error?.message || error))
  process.exitCode = 1
} finally {
  transport.close()
}
