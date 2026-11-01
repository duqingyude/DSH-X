# 原生外壳的面板与托盘：实现方案

> 状态：P0–P4 已落地（分支 strip-electron）。用户要求见 m04660。
> 相关文档：docs/strip-electron-plan.md（去 Electron 总方案）、docs/shell-ipc-plan.md（通道选型）。

## 1 目标

Rust 外壳原先只有「核心看门狗 + 五项写死的托盘菜单」，日常高频操作全要去管理页（打开浏览器、等页面加载、找按钮）。
本次把外壳补成**不打开管理页也能做完日常事**：

- 面板列出所有启动项与实例，能起、停、重启、打开页面；状态 1 秒内跟随核心。
- 托盘菜单跟随启动项（不是写死的五项），点哪一项就是对哪一项操作。
- 设置抽屉能改六个系统开关、界面语言、跑 dsh 的 Node——写盘仍由核心做。
- 核心起不来时给出能照着做的提示，而不是一片空白。
- 验收：npm test 与 cargo test 全绿；真机点过启动/停止/开关/换语言。

## 2 管理页有什么，我们挑了什么

管理页（public/index.html）的分区与本次取舍：

| 管理页分区 | 内容 | 外壳里做不做 | 理由 |
|---|---|---|---|
| 启动项 / 实例 | 起停、重启、打开、端口、启动耗时 | **做**（面板主体 + 托盘） | 每天都在点，最值得搬 |
| 常规 → 系统 | 六个开关、界面语言 | **做**（设置抽屉） | 改一次就生效，不需要开大页面 |
| 常规 → Node | 换跑 dsh 的 Node | **做**（选择文件） | 环境出问题时的第一反应 |
| 版本管理 | 装/删 dsh 版本、换通道 | 不做 | 低频、要进度条与列表，页面更合适 |
| 插件 / 整合包 / MCP / 技能 | 安装、开关、市场 | 不做 | 表单多、要看说明，页面更合适 |
| 环境 / 外观 / 日志 / 高级设置 / 关于 | 诊断、主题、日志、端口、代理、更新 | 不做 | 排障场景，本来就该开大页面 |

结论：**只搬「高频 + 一眼就能决定」的操作**。外壳的角色是遥控器，不是第二套管理台。

## 3 三个设计判断

### 3.1 不另写一套业务逻辑

面板的动作与托盘菜单走**同一个核心函数**：server.js 的 trayAction()。面板模型也只是 trayLaunches() 的摊平
（去掉启动项图标的 base64，一帧塞不下）。

为什么：两套实现迟早出现「托盘说在跑、面板说没跑」。现在只有一份状态源，外壳纯粹是投影。

### 3.2 通道复用，不新开连接

外壳 ↔ 核心本来是「核心发请求、外壳应答」的单向用法（host-remote.js → bridge.rs）。这次把同一根套接字改成**双向**，
帧形状不变，靠有没有 method 字段分流：

| 帧 | 方向 | 含义 |
|---|---|---|
| {id, method, params} | 外壳 → 核心 | 反向调用（面板问状态、点按钮、改设置） |
| {id, ok, result / error} | 核心 → 外壳 | 上面那类请求的回包 |
| {event, data} | 核心 → 外壳 | 事件推送（目前是 panel 与 shellUpdate） |
| 同上两种 | 核心 → 外壳 / 外壳 → 核心 | 原有的正向调用（通知、开窗口……） |

为什么不另开一条：多一条连接就多一套生命周期（谁先连、断了怎么重连、鉴权做几遍）。

### 3.3 设置的唯一写者是核心

面板**不写 settings.json**。点一下开关就把新值发给核心，核心走页面同一个 saveManagerSettings()
（校验、落盘、emitState()），然后推一份新模型回来。

外壳只读 prefs.rs 做两件事：hardwareAcceleration（要赶在 app ready 之前）与 closeToTray（关窗时用）。
其余开关一律以核心推来的模型为准，面板自己不留副本——也就不存在「面板显示开、文件里是关」。
## 4 协议契约

### 4.1 panel/state（外壳 → 核心）

参数 {}，返回 panelState() 的形状（server.js:1666，shell-rs/src/model.rs 一一对应）：

| 字段 | 含义 |
|---|---|
| core.{version,profile} | 核心版本与当前 profile |
| shell.version | 外壳版本（核心据此判断要不要提示更新） |
| node.{path,version} | 跑 dsh 的 Node |
| prefs | 六个开关 + nodeRuntime / systemNodePath / openMode / theme / lang |
| presets[] | 启动项：id / instanceId / name / version / profile / port / status / url / busy / installed / targetId |
| instances[] | 实例：instanceId / name / version / profile / home / status / url / startedAt / startupMs / startupPhase |
| progress | 安装或插件进度，形状跟着核心走（外壳只当字符串与数字用） |
| failure | 最近一次启动失败，外壳只显示 message |
| updatedAt | 核心侧生成时刻（毫秒） |

presets 与 instances 可能指向同一件事（有启动项的实例、临时起的实例），由核心负责摊平。

**一条实用约束**：核心允许给 null 表示「还不知道」（实例刚起没有 url、失败记录没有 profile）。
PanelModel::parse() 解析前递归删掉对象里的 null，让 serde 的 default 兜住。不这么做的话一个 null 就让整份模型解析失败、
面板停在上一帧（踩过：点启动后提示「核心推来的面板模型读不懂：invalid type: null, expected a string」）。

### 4.2 panel/action（外壳 → 核心）

参数与托盘菜单完全一致：{ id, action, targetId, instanceId }，action 是 open / start / stop / restart。
核心把它原样转给 trayAction()：过期校验（targetId 与 instanceId 对不上就拒）、busy 拒绝、装没装，全只有一份实现。
超时 60 秒（装版本、起进程都可能慢）。

### 4.3 panel/settings（外壳 → 核心）

参数 { patch: { … } }，白名单在核心（server.js 的 PANEL_SETTINGS_KEYS）：
taskNotifications / notificationSound / closeToTray / keepAwake / autoUpdate / lang / theme / systemNodePath。
白名单之外一律丢掉——这条通道不能让外壳改数据目录、端口、代理这类东西。超时 3 秒。

### 4.4 事件帧与叫醒

核心每次 emitState()（200ms 节流）推一帧 {event: "panel", data: 模型}；外壳解析后更新面板与托盘。

**隐藏窗口必须自己叫醒**：窗口藏着时 eframe 只跑 logic() 不跑 ui()，没人再 request_repaint_after，
所以 bridge.rs 里所有送事件的地方都走 emit()——发完顺手 ui.request_repaint()。
不这么做的话「托盘点打开面板」要等下一次偶然重绘才生效。

### 4.5 排障连接不算核心

DSH_SHELL_IPC_ALLOW_ANY=1 放开的连接被标成 Trust::Debug：能下指令、能收自己那份回包，但**不许**顶替核心当反向调用通道。
原因很实在：scripts/ipc-call.mjs 一接上，面板点按钮就把请求发给它了，它没有 handler，面板于是显示「核心还没连上外壳通道」。

## 5 界面

```
┌──────────────────────────────────────────────────────────────┐
│ (D) DSH-X v0.1.24 │ ● 运行中 · pid 9580 │ [打开管理页] [⋯] │  header
├──────────────────────────────────────────────────────────────┤
│ 启动项  运行中 1                                             │
│ ┌──────────────────────────────────────────────────────────┐ │
│ │ ● DSH                              [⟳] [■] [↗]           │ │  body
│ │   web:3080 · 0.2.1-rc.4                                  │ │
│ └──────────────────────────────────────────────────────────┘ │
│ ▸ 核心输出                                                   │
├──────────────────────────────────────────────────────────────┤
│ Node v24.19.0 │ 核心目录                      │ 外壳 v0.1.24  │  footer
└──────────────────────────────────────────────────────────────┘
```

- **header**：圆形徽标、名字与核心版本、状态点（灰/黄/绿/红）、「打开管理页」（核心报出地址后才可点）、「⋯」抽屉开关。
- **body**：一行一个启动项；左侧状态点，标题下是 profile:端口 · 版本（没跑就只显示版本）；右侧三个按钮按状态启用。
  卡片下方还有一行提示：没装的版本说「到管理页里装一次」、正在操作说「正在操作…」。
- **核心输出**：折叠区，显示 supervisor 收到的最后几十行日志——起不来时唯一的线索。
- **footer**：跑 dsh 的 Node 版本、核心目录、外壳版本。
- **i18n**：tr(lang, 中文, English)，语言来自模型（核心的 settings.lang）；切换后整屏立刻跟随。
- **主题**：theme 取 auto 时跟系统深浅色（theme.rs 的 apply() 把两套 egui 风格一起改）。

## 6 托盘

```
打开面板 / 打开管理页
──────────
● DSH · web:3080 · 0.2.1-rc.4      ▸  打开页面 / 启动 / 停止 / 重启
○ 另一个启动项                      ▸  …
（超过 8 条：还有更多启动项，请在面板里操作）
──────────
启动核心 / 停止核心 / 退出 DSH-X
```

- 每个启动项一个子菜单，标题带状态圆点与 caption()；子项按状态置灰（没跑就没有「停止」）。
- 菜单**跟着模型重建**：Tray::sync() 比一份 signature（语言 + 每个启动项的 id/名字/状态/busy/装没装/url + 条数 + 在跑条数），
  变了才 set_menu——每帧都重建会让菜单闪，也会在用户正点的时候把菜单抽走。
- 点菜单项不直接操作：塞进 ROSTER（菜单 id → 动作），由面板的 apply_tray() 统一走 link.action()，
  这样「哪个按钮点了什么」只有一条路径。
## 7 阶段与验收

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | 通道双向（createIpcTransport 的 onRequest / sendEvent、Link）、panelState / panelAction / panelSettings、test/panel-model.test.mjs | 完成 |
| P1 | panel.rs 重写（header / 卡片 / 抽屉 / 页脚）、theme.rs、model.rs | 完成 |
| P2 | 托盘动态菜单（tray.rs 重写 + signature + ROSTER） | 完成 |
| P3 | prefs 热重读（800ms 节流）、keep_awake（power.rs）、选择 Node、i18n 全量 | 完成 |
| P4 | 本文件、AGENTS.md 契约补充、全量测试、真机冒烟 | 完成 |

验收实测（2026-11-01，PORT=3796、DSH_CORE_DIR=仓库根、debug 构建）：

| 动作 | 结果 |
|---|---|
| 点 ▶ | 实例约 4.7 秒起来，卡片变「● DSH / web:3080 · 0.2.1-rc.4」，按钮变 ⟳ ■ ↗，header 出现「运行中 1」 |
| 点「⋯」 | 抽屉打开，五个开关 + 语言 + Node 路径 + 「选择 Node…」 |
| 点「保持唤醒」 | settings.json 的 keepAwake 由 false 变 true |
| 语言切 English | 面板与抽屉整屏变英文（含每行说明），settings.json 的 lang 变 en |
| 重启核心 | 面板跟着显示新 pid；托盘菜单按新模型重建 |

## 8 风险与降级

| 风险 | 处理 |
|---|---|
| 旧核心没有 panel/* 方法 | pull_panel 失败时提示「核心没有回应面板查询，可能是核心版本过旧」；拿到过一次模型后不再重复弹（has_model） |
| 核心崩了 / 还没连上 | 面板仍有 header 与页脚（数据来自外壳自己），启动项区显示「正在等核心连上外壳通道…」 |
| 通道断了 | start.js 的 shellGone() 让核心跟着退出，外壳的 supervisor 重启核心并重连 |
| 模型里有 null | PanelModel::parse() 先删 null（见 4.1） |
| 托盘装不上（Linux 缺 appindicator） | 降级：没有托盘就不藏窗口，关面板即退出 |
| 菜单项太多 | 超过 8 条折叠成一句提示，其余去面板操作 |

## 9 非目标

- 不在外壳里做版本管理、插件市场、MCP/技能管理（表单与说明太多，页面更合适）。
- 不做第二套设置存储：settings.json 的唯一写者仍是核心。
- 不做内嵌浏览器 / 内嵌标签页（那条路已经随 Electron 一起删掉了）。
- 不做外壳自更新界面（updater.rs 目前只返回 disabled，见 docs/build-installers-plan.md）。

## 10 改动清单

| 文件 | 改动 |
|---|---|
| server.js | panelState() / panelAction() / panelSettings() / schedulePanelPush() / PANEL_SETTINGS_KEYS（含 systemNodePath） |
| host-remote.js | 双向帧：onRequest、sendEvent、panel.push()；remoteHostFromEnv 支持 handlers |
| start.js | PANEL_HANDLERS 三个方法；启动后敲一次 shellUpdate.state() 让通道立刻连上 |
| shell-rs/src/model.rs | PanelModel 及子结构、parse()（null 容错）、running() / failure_text() / caption() |
| shell-rs/src/bridge.rs | Link（反向调用 + 超时）、Event::Panel / Notice、emit() + repaint、Trust::{Core,Debug} |
| shell-rs/src/panel.rs | 整个界面：header / 卡片 / 抽屉 / 页脚 / 托盘事件 / prefs 热重读 |
| shell-rs/src/tray.rs | 动态菜单 + signature + ROSTER |
| shell-rs/src/theme.rs | 浅色 / 深色调色板与卡片样式 |
| shell-rs/src/power.rs | keep_awake（Windows SetThreadExecutionState，其他平台空实现） |
| test/panel-model.test.mjs | 模型形状、动作转发、设置白名单 |
| test/host-remote.test.mjs | 双向帧、事件帧、半包、超时、EOF、外壳版本兜底 |