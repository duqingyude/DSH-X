# 去 Electron 改造方案

> 状态：**PR-1 / PR-2 / PR-3 已落地**（改动在工作区，未提交）· 分支 `strip-electron` · 基线 `4079d36`
> 落地情况与还没做的项见 §10 末尾的「落地情况」。
>
> 仓库是 `yyh-001/DSH-X` 的 fork（`origin` = `duqingyude/DSH-X`，`upstream` = `yyh-001/DSH-X`，当前同点）。
> 上游仍在快速迭代，同步策略见 §11——**这一节决定阶段怎么切，不是收尾工作**。
>
> 一句话：外壳换成 **Rust 原生程序**（egui 面板 + tray-icon 托盘 + cargo-packager 打包/更新），
> 核心仍是 Node 服务但**不再自带 node**（用系统的）、**不再嵌任何 WebView**（网页回浏览器），
> 安装包从约 90MB 降到约 10MB。

## 1. 背景与目标

Electron 外壳在本仓库承担了窗口、标签页、托盘、单实例、通知、更新器和安装包，代价是 100MB+ 的 Chromium、
一个必须跟 dsh 的 `node-addon-require-builtin` 指纹对齐的 Electron 版本（当前钉在 44.7.0），以及一大套
只在 Electron 里有意义的代码（webview 租约、分区、权限、下载、preload 桥）。

本次改造要达到的效果：

1. 仓库、安装包、运行时都不再出现 Electron。
2. 分布式不变：Windows NSIS、macOS dmg、Linux AppImage + deb，带桌面快捷方式。
3. 桌面能力保留：托盘、单实例、开机自启、外壳自更新、核心自更新。
4. 安装包尽量小：目标 **≤ 12MB**。
5. 不要求用户机器上有任何额外运行时；缺 Node 时**明确引导**，不静默失败。

## 2. 已定决策

| # | 议题 | 结论 | 对方案的影响 |
|---|---|---|---|
| 1 | 旧 Electron 外壳的自动迁移 | **不做** | 不生成 `latest.yml` 过渡桥，不写迁移器；新安装包装到同一路径并清掉旧 Electron 文件，数据目录 `%APPDATA%\DSH` 不动，用户重新装一次即可 |
| 2 | 管理页放哪 | **留在浏览器**；原生面板只做高频操作 | `public/index.html`（约 7000 行）零重写；不追求内嵌页面 |
| 3 | 面板技术 | **egui**（`eframe`） | 单一 Rust 依赖，编译产物小，无 webview |
| 4 | Node 版本 | **22 与 24 都支持** | 门槛沿用 `package.json` 的 `engines.node >=22.18.0`；CI 两个版本各跑一遍 |
| 5 | pnpm | **随包发** | 体积预算里留 3.5MB；npm 用系统 node 自带的那份 |
| 6 | 看板娘 PNG | **换 WebP** | `public/` 从 12.4MB 降到约 2.5MB，安装包省约 9MB |
| 7 | npm 版 `@yyh001/dsh-x` | **本次一起停掉** | 删 `npm-package/` 与 CI job；`npm deprecate` 提示去下安装包；新核心在 Electron 里启动时直接报错说明 |
| 8 | 侧栏浏览器 | **接受失效** | 不再有 `<webview>`，`dsh-x-sidebar-browser` 只在 Electron 外壳里可用 |
| 9 | 发布身份 | **改用 fork 自己的 `appId`** | 新增根目录 `fork.json` 单点定义 `appId` / 更新源 / 核心包名；安装钩子按上游 `appId` 删旧卸载项并先静默卸载旧版（§11.8） |

## 3. 目标形态

```
┌──────────────── DSH.exe（Rust 原生，约 7MB）─────────────────┐
│ 托盘（tray-icon + muda）        原生面板（eframe/egui）        │
│ 单实例  系统通知  更新器（cargo-packager-updater）             │
│ 本机通道 \\.\pipe\dsh-x-<pid>-<随机> / shell-<pid>.sock   │
└───────────┬───────────────────────────────▲──────────────────┘
            │ spawn（系统 node）             │ host 调用（长度前缀 JSON 帧）
            ▼                               │
┌──── node start.js → server.js（核心，零第三方依赖）───────────┐
│ 管理页/API · 版本 · 插件 · 整合包 · 同步 · host-remote.js      │
└───────────┬──────────────────────────────────────────────────┘
            │ spawn（不变）
            ▼
     dsh 实例（版本 × profile，各自端口）
            └── 用户用默认浏览器打开 http://127.0.0.1:<port>/
```

**界面分工**

- 原生面板（日常入口）：实例列表与状态、启动/停止/重启/打开、版本安装进度、外壳更新、设置
  （语言、主题、关闭到托盘、开机自启、Node 路径）、日志入口、「打开完整管理页」。
- 浏览器（深水区）：整合包、插件市场、MCP、技能、同步、会话等，仍是 `public/index.html`，功能不变。
- `npm start` 保持现状（无外壳、纯浏览器），继续作为开发与排障入口。

## 4. 体积预算

| 组成 | 现在（Electron） | 改造后 | 说明 |
|---|---|---|---|
| Chromium / Electron 运行时 | 安装后约 180MB | 0 | 最大的一块 |
| Rust 外壳二进制 | — | 6–8MB | `lto = true`、`strip = true`、`panic = "abort"`、`opt-level = "z"` |
| 核心 JS（server/registry/sync/packs/compat/perf） | 约 1.2MB | 约 1.2MB | 不变 |
| `public/`（管理页 + 看板娘） | 12.4MB | 约 2.5MB | 看板娘 PNG（约 10MB）转 WebP |
| npm / pnpm JS | 随包 | pnpm 约 3.5MB | npm 用系统 node 自带的 |
| **安装包（NSIS）** | **约 90MB** | **8–12MB** | 目标值 |

## 5. 系统 Node 约定

**查找顺序**：`DSH_RUNTIME_NODE`（环境变量/设置里的 `systemNodePath`）→ PATH 里的 `node`
→ Windows 常见位置（`%ProgramFiles%\nodejs\node.exe`、`%LOCALAPPDATA%\Programs\nodejs\node.exe`）
→ nvm / fnm / volta 的默认版本。

**版本要求**：`node --version` ≥ 22.18.0；22 与 24 均为支持版本。低于门槛按「未安装」处理，不硬跑。

**缺失时的行为**（关键：不静默失败）

- 面板顶部显示一条明确的横幅：「未找到 Node.js（需要 22 或 24）」，附「打开 nodejs.org」「复制 winget 命令」两个按钮；
- 托盘与面板照常存在，实例启动按钮置灰并说明原因；
- 日志里记一条便于排障的记录（找到哪些候选路径、各自版本）。

**交给核心**：找到后通过 `DSH_RUNTIME_NODE` 传给核心，`platform.js` 的 `RUNTIME_NODE` 直接用它；
dsh、npm、pnpm 子进程全部走它，不再有 `ELECTRON_RUN_AS_NODE`。

**P0 必须验证**：dsh 在纯 node（非 Electron）下能启动——`node-addon-require-builtin` 是否有对应 ABI 的预编译。
dsh 官方本来就以 node CLI 形式发布，预期没问题，但这是全案前置条件，先验半天。

## 6. 外壳 ↔ 核心契约

原则：**`server.js` 一行不改**。`start.js` 在存在 `DSH_SHELL_IPC` 时装一个 `host-remote.js`，
把原来的 `setHost({...})` 换成一条本机套接字上的 JSON 帧调用（见本节末的「传输」）。

| host 成员 | 现在（Electron） | 改造后 |
|---|---|---|
| `openTab` / `openWindow` | 内部标签 / 独立窗口 | 用**默认浏览器**打开 URL（两者行为合一） |
| `notify` | 系统通知 | 系统通知（Windows Toast / `notify-send` / `osascript`） |
| `desktop.dialog` | 原生对话框 | `rfd` |
| `desktop.clipboard` | Electron clipboard | `arboard` |
| `desktop.open` / `reveal` | `shell.openExternal` / `showItemInFolder` | 系统命令（可先复用核心 `filemanager-*` 的现有实现） |
| `browser.clearCache` / `clearAll` | 清 webview 分区 | 返回 `unsupported`，页面按 `browserHost` 自动置灰 |
| `shellUpdate.*` | electron-updater | cargo-packager-updater |
| `exit` / `relaunch` | `app.quit` / `app.relaunch` | 停核心后退出 / 重拉自身 |

**反向（外壳 → 核心）**：`GET /api/tray`（1.5s 轮询）、`POST /api/tray/action`、`POST /api/wake`；
外壳的更新进度走同一条通道的**事件帧**（`{event:'shellUpdate',data}`，连接建立时先推一次），
核心转 SSE 给管理页——不再有 `POST /api/internal/shell-update` 这条 HTTP 路由。

**托盘菜单**：展示逻辑（文案、可用性、请求体）从 `shell/model.mjs` 搬回核心 `trayStateText()`
（`server.js:1571`），Rust 只按返回的 `label` / `actions` 渲染菜单，点击把 `body` 原样 POST 回
`/api/tray/action`。Rust 侧零业务判断，菜单规则继续由 Node 用例覆盖。

**环境变量**（外壳 → 核心）：`DSH_RUNTIME_NODE`、`DSH_CORE=1`、`DSH_SHELL_PREFS=1`、
`DSH_SHELL_IPC`、`DSH_SHELL_EXE`、`DSH_SHELL_PID`、`DSH_SHELL_VERSION`。
`DSH_APP_WINDOW` / `DSH_INTERNAL_TABS` 都不再设置（原生外壳没有内嵌窗口，页面按 `openModeWindow` 降级）。

**传输（已落地）**：一条本机套接字（Windows 命名管道 `\\.\pipe\dsh-x-<外壳 pid>-<8 位随机>`，
unix `<APP_DIR>/ipc/shell-<pid>.sock`，目录 0700 / 文件 0600），外壳当服务端、核心当客户端；
帧 = 4 字节小端长度（上限 1 MiB）+ UTF-8 JSON：请求 `{id,method,params}`、响应 `{id,ok,result|error}`、
事件 `{event,data}`。鉴权靠对端 pid（`GetNamedPipeClientProcessId` / `SO_PEERCRED`），不再有 token；
通道 EOF 就是外壳死亡信号。九方案对比与取舍见 `docs/shell-ipc-plan.md`，排障用 `node scripts/ipc-call.mjs`。

## 7. 改动清单

### 7.1 核心（Node）

| 文件 | 动作 | 要点 |
|---|---|---|
| `host-remote.js` | 新增 | 实现上表 host；`createIpcTransport` 走本机套接字，`start.js` 按 `DSH_SHELL_IPC` 注入 |
| `start.js` | 改 | `DSH_SHELL_IPC` 时 `setHost(remoteHost())`；通道 EOF 退出 + `DSH_SHELL_PID` 看门狗兜底；`__DSH_SHOW__` 通道退役 |
| `platform.js` | 改 | 删 `IN_ELECTRON`；`PACKAGED = DSH_CORE === '1'`；`RUNTIME_NODE` 用 `DSH_RUNTIME_NODE`/PATH 的 node；`runtimeNodeEnv()` 返回 `{}` |
| `server.js` | 改 | `:1002` `shellPid = DSH_SHELL_PID \|\| process.ppid`；`:1571` 托盘菜单带 `label/actions`；`:3309-3370` 删 GitHub 安装包自更新；启动时若 `process.versions.electron` 直接报错「请安装新版 DSH-X」 |
| `registry.js` | 改 | 删 `DSH_ELECTRON_BIN`（`:289`）；npm 用系统 node 自带的，`ensureNpm`（`:210`）留作兜底 |
| `filemanager-windows.js` / `-mac.js` / `-linux.js` | 改 | `launcherExecutable()` 改读 `DSH_SHELL_EXE`，删 `IN_ELECTRON` 分支；自启逻辑本身不动 |
| `apply-update.mjs` | 改 | 等核心与外壳双双退出；重启拉外壳 exe；`update-ok` 标记由外壳写 |
| `core-npm.js` | 改 | `dshX.electron` 比对换成 `dshX.shell`（外壳最低版本） |
| `public/` | 微调 | 看板娘换 WebP；`versions.electron` 文案；删除 `internal-tabs.js`（标签页 UI 的注入目标）。`idle.js` 保留——它是浏览器里管理页的省电逻辑，跟外壳无关 |

### 7.2 外壳（删除 Electron，新建 Rust）

新增 `shell-rs/`（约 1500 行 Rust）：`Cargo.toml`、`Packager.toml`、
`src/{main,supervisor,panel,tray,bridge,updater,prefs,notify,single_instance}.rs`

> 落地进度（P1/P2/P3 已完成）：目录名用 `shell-rs/` 而不是 `shell/`，因为 `shell/` 装着要删除的 Electron 外壳，
> 两者不能同名（§11.5）。`main` / `supervisor` / `panel` / `tray` / `bridge` / `prefs` / `notify` / `single_instance`
> 已能编译并真机跑通「面板 → 核心 → 桥 → 托盘」；`updater` 目前只会返回 `status: "disabled"`（见 §8：外壳自更新未接）。
>
> PR-3 已把下表的「删除」全部执行完：`shell/`、`npm-package/`、三个 Electron 打包脚本、`public/internal-tabs.js`
> 与它们的用例都已进 `git rm`。`prefs` 的默认值没有另存 JSON，直接以 `prefs.rs` 的 `Default` 为源，
> 由 `test/prefs-parity.test.mjs` 解析 Rust 源码与 `settings.js` 的 `DEFAULTS` 逐项比对。

| 模块 | 职责 |
|---|---|
| `main.rs` | 单实例 → 读 prefs → 找 node → 起桥 → spawn 核心 → 建托盘与面板 |
| `supervisor.rs` | 起/停/重启核心，解析管理页端口，崩溃重启，日志转存 |
| `panel.rs` | egui 面板：实例列表与操作、版本与安装进度、更新、设置、Node 缺失引导 |
| `tray.rs` | 托盘图标与动态菜单；托盘不可用时降级为「关面板即退出」 |
| `bridge.rs` | 本机通道（命名管道 / UDS + 对端 pid 校验），路由见 §6 |
| `updater.rs` | cargo-packager-updater：检查/下载/安装，进度推给核心（当前是返回 `disabled` 的占位） |
| `prefs.rs` | 读 `settings.json` 的六个系统开关 + `browserControl`；默认值以本文件的 `Default` 为源，`test/prefs-parity.test.mjs` 与 `settings.js` 逐项比对 |
| `notify.rs` | 系统通知（三平台各一套后端，统一接口） |

**删除**：`shell/*.mjs`（main、tabs、model、pages、desktop、browser-guests、updater、prefs、env）、
`preload.cjs`、`preload-browser.cjs`、`shell/package.json`、`electron-builder.config.mjs`、
`scripts/pack-mac.mjs`（早已无人调用）、`scripts/electron-prune.mjs`、`scripts/mac-signing.mjs`、
`npm-package/`、`public/internal-tabs.js`。

### 7.3 打包 / CI

- `scripts/pack-rs.mjs`（已落地，**现在是 `npm run dist` 的唯一通道**）：`copyAppFiles` → `cargo build --release` → `cargo packager --config Packager.toml`。
  核心层先落到 `release/rs-stage/core`（`package.json` 的 `name` 用 `fork.json` 的 `corePackage`，
  `dshX.shell` 记外壳版本），`node/` 里的 `node` / `npm` / `pnpm` 包装改成转调 `DSH_RUNTIME_NODE`
  （没有它时报错提示先装 Node 22/24，不再像 Electron 版转调 Electron 本体）。
  脚本同时校验 `Packager.toml` 的 `version` == `shell-rs/Cargo.toml`、`identifier` == `fork.json.appId`，不一致直接失败。
  Electron 版的 `pack.mjs` 与 `pack-mac.mjs` 已在 PR-3 删掉，不再有两条通道。
- 产物名是 cargo-packager 的 `{name}_{version}_{arch}-setup.exe`（实测 `dsh-x_0.1.24_x64-setup.exe`），
  不是 `DSH-Setup-<版本>.exe`；不想让用户看到这个名字就得在 `bundle()` 之后重命名，CI 的 gh 上传也要跟着改。
- `.github/workflows/build.yml`（已落地，**旧的 Electron 版工作流已删、`build-rs` 的内容并进 `build.yml`**）：三平台各出一个包，
  `Swatinem/rust-cache` 缓存 `shell-rs`，macOS 矩阵 arm64 + x64；mac 的 dmg 要 `assets/icon.icns`，
  仓库里还没有（`make-icons.py` 没出 icns），所以 mac job 先探文件、没有就整段跳过，别红着骗人。
  Linux job 要装 `libxkbcommon-dev` / `libwayland-dev` / `libdbus-1-dev`（eframe + rfd），并在 tag 上
  跑 `node scripts/pack-core.mjs --publish` 发跨平台核心包（原来是 mac arm64 job 发，但 mac job 会因缺 icns 跳过）。
  这三平台还没在 CI 真跑过，只有 Windows 的 NSIS 全链路在本机验证过。
- 根 `package.json`：删四个 Electron devDependencies；`scripts` 里 `electron` → `shell`（`cargo run`），
  `dist` 指向新打包脚本；`keywords` 去掉 `electron`。
- 版本：根 `package.json` = 核心版本；`shell/Cargo.toml` = 外壳版本；`Packager.toml` 引用它，脚本校验一致。
- 工作流：Electron 步骤与 `npm-package` job 已随旧 `build.yml` 一起删；产物名用 cargo-packager 的
  `dsh-x_<外壳版本>_<arch>-setup.exe` / `.dmg` / `.AppImage` / `.deb`（更新 feed 等外壳更新器接上再加）。
  发核心包的 job 在 Linux 上跑 `pack-core.mjs --publish`，包名读 `fork.json` 的 `corePackage`
  （`@duqingyude/dsh-x-core`）——**不能继续发上游的 `dsh-x-core`**，两个同名包会互相覆盖（§11.8）。
- 发布身份：`appId`、更新源仓库、核心包名、tag 前缀全部从根目录 `fork.json` 读，打包脚本、外壳、核心、CI
  都不写死（字段表与消费点见 §11.8；不隔开的话两个安装包会互相升级）。
- 签名私钥：cargo-packager 的 minisign 私钥（`release/packager-key.pem`）按 `release/release-key.pem`
  的规矩——不进库、不打印、提醒备份。

### 7.4 测试与文档

- 新增：`host-remote.test.mjs`（假外壳验桥）、`fork-identity.test.mjs`（`fork.json` / `Packager.toml` / 源码里不许写死上游身份）、
  `prefs-parity.test.mjs`（`prefs.rs` 的 `Default` vs `settings.js` 的 `DEFAULTS`）。核心侧托盘菜单字段的断言并进 `server.test.mjs`/`open-window.test.mjs`，没有单独建 `tray-menu.test.mjs`。
- 改写：`browser-control`（只留核心侧）、`desktop-api`（`versions.electron` → `versions.node`）、
  `update-prompt`（删 electron-updater 用例）、`settings-page`、`core-npm`（`dshX.shell`）。
- 删除：`shell-split`、`shell-desktop`、`shell-prefs`、`shell-model`、`quit-confirm`、`electron-prune`、
  `npm-package`、`mac-signing` 这些只测 Electron 外壳的用例。
- 文档：`README.md` / `README.en.md`（架构段改「Rust 原生外壳 + 浏览器界面」）、`AGENTS.md`
  （目录地图、打包、关键机制整段重写）、`docs/desktop-api.md`（`versions.electron` → `versions.node` + `versions.shell`，
  补一节桥）、发版说明与 QQ 群公告草稿（写清功能变化与「需先装 Node 22/24」）。

## 8. 打包、快捷方式、自启、托盘、更新

- **安装包**：cargo-packager —— Windows NSIS（每用户）、macOS dmg + `.app`、Linux AppImage、deb。
- **桌面快捷方式**：NSIS 模板建桌面与开始菜单快捷方式；deb 装 `.desktop`；mac 拖入 Applications。
  P0 验证模板是否自带该选项，没有就加一段 NSIS include。
- **开机自启**：核心 `filemanager-*` 继续负责（注册表 Run / LaunchAgent / XDG），只把 exe 来源换成
  `DSH_SHELL_EXE`；旧版自启项指向同一路径，重装后自动指向新 exe。
- **托盘**：Windows 通知区、macOS 菜单栏、Linux 需 `libayatana-appindicator3`（AppImage 打包进去，
  deb 写依赖）。没有托盘环境的机器降级为「无托盘，关面板即退出」。
- **启动体验：默认没有任何窗口**（已实现）：装出来的 exe 是 GUI 程序（`windows_subsystem = "windows"`），
  核心子进程带 `CREATE_NO_WINDOW`，所以双击既没有黑框也没有面板；核心把管理页地址打出来后由
  `panel.rs` 的 `auto_open` 交给系统默认浏览器。面板只在两处出现：托盘点「打开面板」，或核心反复起不来
  （`Phase::Failed`，只亮一次）。第二次双击走单实例，读 `<APP_DIR>/shell-url.txt` 探活后直接开浏览器。
  **Windows 的隐身不能只靠 `ViewportCommand::Visible(false)`**：eframe 画完第一帧会无条件
  `window.set_visible(true)`（`epi_integration.rs:441-453`），中间会闪约 44ms；实际做法是用 DWM 的
  `DWMWA_CLOAK` 在窗口级别隐身（`panel.rs` 的 `set_cloaked` / `cloak`，`winit` + `windows-sys` 的 Dwm 特性），
  `Visible(false)` 只作兜底。
  黑框同理：GUI 外壳 spawn 任何控制台程序都会新开一个窗口，所有子进程（核心、`node --version` 探测、pnpm）
  都必须走 `supervisor.rs` 的 `no_window()`（`CREATE_NO_WINDOW`）；`node_version` 还按路径缓存，否则一次启动会探两遍、闪两下。
- **外壳更新**：cargo-packager-updater 读 GitHub releases 的 JSON（`version`/`url`/`signature`/`platforms`）；
  Windows 装 NSIS、Linux 替换 AppImage；macOS 暂时不支持（ad-hoc 签名，与现状一致）。
- **核心更新**：完全不动（npm 通道 `/api/self/*` → `apply-update.mjs` → 失败回滚），只换 node 与重启通道。
- **安装目录**：默认 `%LOCALAPPDATA%\Programs\DSH-X`。cargo-packager 把 currentUser 的默认目录写死成
  `%LOCALAPPDATA%\<产物名>`、也没有配置项，所以 `shell-rs/nsis/installer.nsi` 是从 cargo-packager 0.11.8 抄下来的
  改版模板（`[nsis] template` 指向它），相对上游只改三处：默认目录加 `Programs`、沿用「上次安装目录」前先确认
  `uninstall.exe` 还在（否则装回一个已被删掉的旧路径）、卸载时递归删目录。升级 cargo-packager 要重新比对上游模板，
  `test/pack-nsis.test.mjs` 盯着这三处。
- **旧版替换（自动钩子未做）**：原计划让安装钩子先认出上游的
  Electron 安装——**不能按卸载项键名认**：实测上游装的是 `%LOCALAPPDATA%\Programs\DSH`（不是 DSH-X），
  卸载项键名是 electron-builder 生成的随机 GUID（本机是 `6fddfcf6-d8ca-51f1-9134-650d98fb23d6`），
  只有 `DisplayName = DSH-X` 和 `InstallLocation` 前缀能拿来认。**但 cargo-packager 的 NSIS 没有安装钩子**，
  所以现在只在 README / 发版说明里写「先卸载旧 Electron 版」，不做自动卸载；将来要自动化，得在 `pack-rs.mjs`
  里另加一步（先跑 `uninstall.exe /S` 再装）或在 NSIS 模板里插 include，别在文档里当成已有。
  数据目录 `%APPDATA%\DSH` 与 `DSH_HOME` 不动，profile、会话、插件全部保留。
- **卸载残留（已修）**：核心是外壳的子进程，强杀外壳会让核心成孤儿并继续占着文件与管理页端口。两处收口：
  核心自己看门狗（`start.js` 每 3 秒查 `DSH_SHELL_PID`，见 §13）、模板的卸载段改成 `RMDir /REBOOTOK /r`
  （上游按字母序父目录在前、又是非递归，删不掉 `core`）。实测 `/S` 卸载后安装目录、快捷方式、卸载项全部清空。
- **不提供并存**：身份隔离后两个产品技术上能同时启动，但共用 `%APPDATA%\DSH` 的 `settings.json`、日志与
  `versions/`，会互相覆盖配置、抢端口。安装器与文档一律写「先卸载上游」，详见 §11.8。
- **npm 版退场**：删除 `npm-package/`；对已发布版本执行 `npm deprecate @yyh001/dsh-x`，提示去下载安装包；
  新核心在检测到 `process.versions.electron` 时直接报错退出并给出同一句提示（防止老 CLI 拉到新核心后一脸懵）。

## 9. 功能对照

| 功能 | 结论 | 说明 |
|---|---|---|
| 安装包 / 桌面快捷方式 / 开机自启 / 托盘 / 自更新 | **保留** | 载体换了，能力不变 |
| 管理页全部业务（版本/插件/整合包/MCP/技能/同步） | **保留** | 浏览器打开，代码零改动 |
| dsh 实例页面 | **保留** | 浏览器打开 |
| 插件桌面 API（`/api/desktop/*`） | **保留** | 桥 + 系统对话框/剪贴板/通知 |
| 系统通知、提示音、免打扰、退出确认、单实例 | **保留** | 原生实现 |
| 启动进度页（`public/booting.html`） | **保留** | 浏览器标签 |
| `window.ipc`（自绘标题栏、拖窗、内嵌控件） | **删除** | 页面在浏览器里，不再需要 |
| 窗口内标签页 | **删除** | 一开始就依赖 WebView |
| 侧栏浏览器原生 `<webview>` / `sidebar_browser` 工具 | **失效** | 浏览器里官方右栏退化为 iframe 或不可用；插件需给出明确提示 |
| 权限（麦克风）、下载、证书忽略、webview 分区清缓存 | **删除** | 交给浏览器自己处理 |

## 10. 阶段与验收

| 阶段 | 内容 | 验收 | 工期 |
|---|---|---|---|
| **P0 可行性** | ① dsh 在系统 node 22 / 24 下启动（三平台）② Rust 骨架：托盘 + egui 面板 + 单实例 ③ `cargo packager` 出四个产物，验证快捷方式与 `/S` 静默安装 | 三平台各出一个能装的包，面板能开 | 3–4 天 |
| **P1 进程拆分** | 找 node + 缺失引导、`supervisor`、`host-remote.js`、桥、端口解析、崩溃重启；核心侧平台收口 | `npm start` 与「有外壳」两条路都通；托盘点启动/停止有效 | 3–4 天 |
| **P2 面板与托盘** | 实例列表与操作、版本与安装进度、设置页、日志、更新提示、通知、退出确认、自启 | 面板可完成日常全部操作；托盘与面板状态一致 | 4–5 天 |
| **P3 打包与更新** | 四产物、快捷方式、更新 feed + 签名、核心更新改重启通道、旧版替换与身份核对（§11.8） | 干净机器：装包 → 快捷方式 → 自启 → 外壳更新 → 核心更新 → 回滚；另一台装过上游的机器：装包时自动卸载旧版 | 4–5 天 |
| **P4 清理瘦身** | 删 Electron 依赖/脚本/用例/npm 包；看板娘 WebP；文档与发版说明；安装包体积复核 | `npm test` 全绿；`git grep -i electron` 只剩历史说明；安装包 ≤12MB | 3–4 天 |

合计 **17–22 人日**。每阶段结束都可以只发核心（npm 通道）而不出安装包，风险可控。

### 落地情况（2026-10-31，分支 `strip-electron` 工作区）

| 项 | 状态 |
|---|---|
| PR-1 核心 host 抽象 | 完成：`host-remote.js` + `start.js` 接线 + `platform.js` 收口（后续已删 `IN_ELECTRON`/`NPM_MODE`/`RUNTIME_NODE_ENV`；当时那条 `POST /api/internal/shell-update` 已被外壳通道的事件帧取代） |
| PR-2 Rust 外壳 | 完成：`shell-rs/`（面板、托盘、桥、supervisor、prefs、单实例、notify）+ `scripts/pack-rs.mjs` + `Packager.toml`，Windows NSIS 静默装、起核心、桌面/开始菜单快捷方式、卸载项都真机验证过 |
| PR-3 切默认入口 + 清理 | 完成：Electron 外壳、npm 版、旧打包脚本与对应用例全部删除；`package.json`/`platform.js`/README/AGENTS/CONTRIBUTING/SECURITY/desktop-api 已改写；`npm test` 1003 用例全绿（996 pass / 7 skip） |
| 打开方式 | 「独立桌面窗口」只剩遗留设置值：原生外壳没有内嵌窗口，页面按 `openModeWindow:false` 不再列这一项，老设置会提示并落到系统浏览器（README 已改成两种方式） |
| 安装包更新入口 | 不再拼 `releases/latest/download/DSH-Setup.exe`（cargo-packager 的包名带版本与架构，没有稳定直链），改为指向 releases 页；自动更新归外壳更新器（未接） |
| 设置页系统开关 | `desktopShell` 改成「有内嵌窗口 **或** 桥在（`host.desktop`）」，原生外壳下六个开关可用；`/api/self` 的 `shell.current` 先由 `DSH_SHELL_VERSION` 兜底，桥刷新回来后覆盖 |
| 外壳通道 | 完成：环回 HTTP + Bearer 换成一条本机套接字（命名管道 / UDS）+ 长度前缀 JSON 帧；鉴权靠对端 pid，`tiny_http` / `DSH_SHELL_URL` / `DSH_SHELL_TOKEN` / `/api/internal/shell-update` 全删；通道 EOF 即时退出，看门狗降级为兜底；排障加 `scripts/ipc-call.mjs`（细节见 `docs/shell-ipc-plan.md` §11） |
| 页面清理 | Electron 时代的用户可见文案去掉「Electron」；窗口按钮/拖动、内部标签页、`?window=1` 排版与回主页图标整块删除（`launcher.css`、`theme.css`、`launcher-boot.css`、`index.html` 共约 180 行） |
| 启动体验 | 双击后无控制台、无面板，直接系统默认浏览器打开管理页；第二次双击读 `shell-url.txt` 探活后开浏览器；面板只在托盘点开或核心起不来时亮（Windows 用 DWM `DWMWA_CLOAK` 隐身，单发 `Visible(false)` 会闪 44ms，见 §8） |
| 发布身份 | 页面里的 GitHub 链接、`scripts/release-notes.mjs` 的 compare 链接、两个整合包 manifest 的 homepage 都改成 `duqingyude/DSH-X`；`scripts/release-sbom.mjs` 的 `REPO` 改读 `fork.js` |
| SBOM | 去掉 Electron 条目（`node_modules/electron` 已不存在，跑 `npm run release:sbom` 不再报错）；npm / pnpm / 旧包的 node 与 corepack 照旧按整包声明 |
| 安装目录 | 默认 `%LOCALAPPDATA%\Programs\DSH-X`：cargo-packager 把 currentUser 目录写死且无配置项，所以换了自建模板 `shell-rs/nsis/installer.nsi`（只改默认目录、沿用旧目录前先验证 `uninstall.exe`、卸载递归删）；实测 `/S` 装到新路径，旧路径被删时不会再装回去 |
| 卸载残留 | 已修：模板卸载段 `RMDir /REBOOTOK /r`，配合核心看门狗；实测卸载后安装目录（含 `core/`）、桌面/开始菜单快捷方式、卸载项全部清空 |
| 旧版自动卸载钩子 | **未做**：cargo-packager 的 NSIS 没有安装钩子，现在只在文档里要求先手动卸载旧 Electron 版（§8） |
| 外壳自更新 | **未做**：`updater.rs` 只返回 `status: "disabled"`，核心更新走 npm 不受影响（§8） |
| mac dmg | `assets/icon.icns` 已由 `make-icons.py` 产出（Pillow 直接写 icns，不依赖 iconutil）；CI 里 mac job 仍保留「没有 icns 就跳过」的保险 |
| 图标统一 | 一处 master（`assets/icon.png`）出全套：exe / 卸载程序 / 安装包 / 快捷方式 / 托盘 / 面板窗口 / 网页 favicon 都换成同一张 mascot。**Windows 的 exe 图标必须自己编 `.rc`**——cargo-packager 不写资源段，之前装出来的 `dsh-x.exe` 与安装包一直是通用空白窗格；现在 `build.rs` 用 `embed-resource` 嵌 `assets/icon.ico`，编不进去直接 panic |
| 安装包汉化 | `[nsis] languages = ["SimpChinese"]`（只有一个语言，不弹选择框）；渲染出的脚本里有 `MUI_LANGUAGE "SimpChinese"`、`SimpChinese.nsh` 与 `MUI_LANGDLL_DISPLAY`，安装器窗口标题已是「DSH-X 安装」；卸载程序图标由模板里的 `MUI_UNICON` 一起换掉（`NsisConfig` 没有 `uninstaller_icon` 字段） |
| 看板娘 WebP | 未做（属 P4 的独立小项，与外壳无关） |
| 三平台 CI | 未真跑过：只有 Windows 的 NSIS 全链路在本机验证 |

验收口径提醒：P3 那行「另一台装过上游的机器：装包时自动卸载旧版」目前**不成立**——先手动卸旧版再装。

三个配套约定：

- **每个阶段结束、开始下一阶段之前先同步上游**（`upstream → master → strip-electron`，见 §11.3），再谈验收——
  否则「绿」的基准一直在动。
- 交付按三条可独立检视的批次走，全部合进 `strip-electron`（`master` 是上游镜像，不接受提交）：
  **PR-1** = 核心 host 抽象（`host-remote.js` + `platform.js` 收口 + 外壳用例条件跳过，旧外壳照常能跑）；
  **PR-2** = Rust 外壳长出来（`shell-rs/` + `scripts/pack-rs.mjs` + CI 并行出包，不替换默认入口）；
  **PR-3** = 默认入口切换 + P4 清理（唯一一次大删除）。
  PR-1/PR-2 可以随时合——它们只增不改；PR-3 才是需要一口气合完的那条（见 §11.5）。
- 若最终要回贡上游，按 §11.9 把 `strip-electron` 压成 3–5 个语义提交再提 PR。

## 11. 与上游同步（fork 模型）

本仓库是 `yyh-001/DSH-X` 的 fork：`origin` = `duqingyude/DSH-X`，`upstream` = `yyh-001/DSH-X`，
当前三条线都在同一个提交 `4079d36` 上（fork 是纯镜像，还没有自己的提交）。

上游还在快速迭代，所以「去 Electron」的长期成本**不在写 Rust，而在每次同步上游都要把它的改动分流到新外壳里**。
本节定三件事：分支模型、冲突的机械规则、自动化。

### 11.1 单向下行：upstream → master → strip-electron

```
  yyh-001/DSH-X (upstream)
        │  git fetch upstream && git merge --ff-only     ← 永不冲突
        ▼
  master（上游镜像，禁止提交）
        │  git merge master                              ← 冲突只出现在这一层，一次解完
        ▼
  strip-electron（本改造线）── feat/* 短分支在此合并
```

三条铁律：

1. **`master` 是上游镜像，永远不提交**，只用 `--ff-only` 前进。现在它与上游同点，保持这个状态是零成本的；
   一旦允许在 master 上提交，「同步上游」就从 `ff-only` 退化成一次充满冲突的合并。
2. **`strip-electron` 只合 `master`，永不直接 `git merge upstream/master`。** 同一改动从两条路进入会让
   `rerere` 的判定失效（同一语义的补丁变成两套），冲突也会成倍。
3. **禁止向上游 push**：`git remote set-url --push upstream no_push`。fork 的提交一旦误推到上游，
   收拾起来比重做整个分支还贵。

好处：上游一天几十个提交也无所谓——master 永远是干净的镜像，所有冲突都集中在「master → strip-electron」
这一次合并里，而且每次只看一次。

### 11.2 分支模型

| 分支 | 角色 | 允许的操作 |
|---|---|---|
| `master` | 上游镜像 | 只 `merge --ff-only upstream/master`；不提交、不 rebase |
| `strip-electron` | 改造线（相当于 fork 的集成分支） | 合 `master`、合 `feat/*`；发版从这里出 |
| `feat/<topic>` | 单个改动（如 `feat/host-remote`、`feat/rust-shell`） | 从 `master` 切，合回 `strip-electron` |

暂时不引入 `develop`：现在只有一个改造方向，多一层分支只是多一次合并。等「上游不想收、我们要长期维护 fork」
再加 `develop`（见 11.9）。

### 11.3 每天开工前的同步（约 30 秒）

```bash
git fetch upstream
git switch master
git merge --ff-only upstream/master        # 不会冲突，除非有人往 master 提交过
git switch strip-electron
git merge master                           # 冲突按 11.4 处理
npm test
```

- 开 `git config rerere.enabled true` 与 `git config rerere.autoupdate true`：同一处冲突第二次自动套用
  上次的解法。这是长分支最省事的一个开关，而且必须写在 `CONTRIBUTING.md` 里，别只记在本地。
- 频率：**开工前、每次动 Rust 之前、每次发包之前**。上游活跃期建议 2–3 天一次；别攒两周。
- 同步后跑 `git log --oneline master@{1}..master` 扫一眼提交标题，就够判断这次有没有要移植的外壳改动。

### 11.4 上游改动的四类分流（省事的关键在这一张表）

| 类别 | 典型路径 | 处理 |
|---|---|---|
| **① 业务与页面** | `server.js` 的业务段、`public/`、`*.js`、`packs/`、`plugins/`、业务用例 | 自动合入，**零处理**。这是 fork 继续吃到上游新功能的主渠道 |
| **② 外壳实现** | `shell/*.mjs`、`preload*.cjs`、`shell/package.json`、`electron-builder.config.mjs`、`npm-package/`、`scripts/pack*.mjs` | 合并通常干净（11.5：删除留到最后），但**行为要移植进 `shell-rs/`**，见 11.6 |
| **③ 共享元数据** | 根 `package.json` 的 deps/scripts、`AGENTS.md`、`README*`、`.github/workflows/*` | 冲突取 ours，**再手工补上游新增项**（新规矩、新 CI 步骤、新依赖）；提交信息写明取了上游什么 |
| **④ 二进制资源** | 看板娘 PNG、`assets/icon.png` | 不手工合并，重跑 `scripts/mascot-webp.mjs` / `scripts/make-icons.py` |

一条判断标准：**第 ① 类占上游提交的绝大多数，它们必须零成本通过。** 所以「尽量不改业务文件」不是洁癖，
而是保住这条免费通道；任何为了让移植更顺手而复制/挪动业务文件的做法，都是在拆自己的路。

### 11.5 「删除」是终态的一次提交，不是过程

P0–P3 期间**一个上游文件都不删**：`shell/*.mjs`、`preload*.cjs`、`electron-builder.config.mjs` 照旧存在，
只是不再是默认入口。这样上游改动这些文件时，合并依然是干净的。

- 好处还有一个：过渡期 `npm run electron` 仍能起旧外壳做对照（前提是核心同时支持两种 host，即 PR-1），
  「新外壳某处行为不对」时能立刻拿旧外壳比。
- 删除只发生在 P4 / PR-3 的那一次提交里。此后若上游又改这些路径，用机械规则处理，**不手工解冲突**：

```bash
git merge master          # 出现修改/删除冲突
node scripts/after-merge.mjs   # 读 scripts/electron-files.json 的删除清单 → git rm → npm test
git commit
```

`after-merge.mjs` 就是个 `git rm -r` 加一条测试，但把它写成脚本的意义是：**冲突处置有唯一正确答案，不靠人记**。
再叠加 `rerere`，同类冲突基本只会真正解一次。

### 11.6 上游外壳改动的移植清单（唯一会静默丢功能的通道）

上游每次新增托盘项、快捷键、通知行为，新外壳里都要重做一遍。没人提醒就会丢，而且丢得无声无息。

- 清单文件 `docs/upstream-shell-changes.md`：一行一条，记「上游提交 / 行为 / 已移植 / 放弃理由」。
- 每次同步后看这段是否为空，非空就必须记一行：

```bash
git log --oneline master@{1}..master -- shell/ npm-package/ electron-builder.config.mjs scripts/pack.mjs
```

- 判据按**行为**记，不按文件记：`shell/` 里大部分是 Electron 特有的窗口 / webview / 分区代码，那些不用移植；
  真正要跟的是托盘菜单、快捷键、通知文案、更新流程、退出确认这类用户可见行为。
- 这一节同时是「是否该回贡上游」的判据：如果清单连续两个月都在涨，说明两条线在分叉，该认真谈一次上游合并（11.9）。

### 11.7 自动化：`.github/workflows/sync-upstream.yml`（新文件）

新增一个 workflow 文件，**不改 `build.yml`**（改它等于自己制造冲突）。

```yaml
name: sync upstream
on:
  workflow_dispatch:
  schedule:
    - cron: '0 22 * * *'   # 每天 06:00 Asia/Shanghai
jobs:
  sync:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      issues: write
    steps:
      - uses: actions/checkout@v4
        with:
          ref: master
          fetch-depth: 0
      - run: git remote add upstream https://github.com/yyh-001/DSH-X.git || true
      - run: git fetch upstream master
      - name: fast-forward master
        run: git merge --ff-only upstream/master && git push origin master
      - uses: actions/setup-node@v4
        with:
          node-version: 24
      - name: merge master into strip-electron
        id: merge
        continue-on-error: true
        run: |
          git switch strip-electron
          git merge origin/master
      - run: npm test
        if: steps.merge.outcome == 'success'
      - run: git push origin strip-electron
        if: steps.merge.outcome == 'success'
      - name: open issue on conflict
        if: steps.merge.outcome == 'failure'
        uses: actions/github-script@v7
        with:
          script: |
            const files = require('child_process').execSync('git diff --name-only --diff-filter=U').toString()
            await github.rest.issues.create({ ...context.repo, title: '同步上游时出现冲突',
              body: '冲突文件：\n' + files + '\n按文档 §11.4/§11.5 处理。' })
```

要点：`schedule` **只在默认分支的 workflow 文件上生效**——而 `master` 就是默认分支，正好合适。
冲突时只开 issue、**绝不自动解冲突**（自动取 ours 会把上游的新功能悄悄丢掉，且没有告警）。
上游是公开仓库，fork 的 Actions 不需要额外 token 就能 fetch。

### 11.8 发布身份：改用 fork 自己的 `appId`（已定）

上游也在发 `DSH-Setup-*.exe`、npm 上的 `dsh-x-core`、`yyh-001` 名下的 releases。**如果沿用同一套身份，
两个安装包会互相升级、npm 包互相覆盖**——用户的更新器会把人带到上游，反过来也一样。所以这一版就改，不留到下一次。

| 项 | 上游 | 本 fork | 落在哪 |
|---|---|---|---|
| 应用 id | `io.github.yyh-001.dsh-x` | `io.github.duqingyude.dsh-x` | Windows 卸载项与 AppUserModelID、mac `CFBundleIdentifier`、自启项文件名、单实例名 |
| 更新源 | `yyh-001/DSH-X` 的 releases | `duqingyude/DSH-X` 的 releases | 外壳更新器读的 `latest.json` 地址 |
| 核心 npm 包 | `dsh-x-core` | `@duqingyude/dsh-x-core` | `/api/self` 的版本检查与 `apply-update.mjs` |
| release tag | `v0.1.24` | `v0.2.0` 起，与上游版本号错开 | 更新器的版本比较（「0.1.25 是哪边发的」会很乱） |
| 数据目录 | `%APPDATA%\DSH` | **不变** | 外壳设置/日志/`versions/`；profile 与会话在 `DSH_HOME`，本来就不受影响 |

- **核心包名这一项比 `appId` 更致命**：新外壳拉到上游的 `dsh-x-core` 里没有 `host-remote.js`，一启动就崩，
  而且崩在用户机器上、看不到原因。所以 `corePackage` 必须跟着改，或者用 `DSH_CORE_REGISTRY` 指向自建源。
- 这些值集中放在根目录 `fork.json`，打包脚本、外壳、核心、CI 都从它读；回贡上游时只改这一个文件（§11.9）。

```json
{
  "appId": "io.github.duqingyude.dsh-x",
  "productName": "DSH-X",
  "upstreamAppId": "io.github.yyh-001.dsh-x",
  "updateOwner": "duqingyude",
  "updateRepo": "DSH-X",
  "corePackage": "@duqingyude/dsh-x-core",
  "dataDir": "DSH",
  "tagPrefix": "v"
}
```

| 读取方 | 消费的字段 | 备注 |
|---|---|---|
| `scripts/pack-rs.mjs` | `appId` / `productName` / 更新源 | 写进 cargo-packager 配置 |
| `shell-rs` 的单实例模块 | `appId` | Windows `CreateMutexW("Local\" + appId)`；Linux 抽象 socket；mac 用 `CFBundleIdentifier` 认领 |
| 安装钩子 / 卸载脚本 | `upstreamAppId` + `productName` | Windows 上游键是 GUID，只能按 `DisplayName = productName` 且 `InstallLocation` 在 `Programs\DSH` 下认，找到后再按它的键删 |
| `filemanager-windows.js` / `-mac.js` / `-linux.js` | `appId` / `productName` | 自启项名字（Run 值名、LaunchAgent 文件名、`dsh-x.desktop`） |
| `server.js` 的 `/api/self`、`core-npm.js` | `updateOwner` / `updateRepo` / `corePackage` | 更新检查与核心包名 |
| `.github/workflows/build.yml` | 全部 | 用 `node -p "require('./fork.json').updateOwner"` 取，不在 YAML 里写死 |

三个平台的真实落点（P3 要在打出来的包上逐个核对，别只看配置文件）：

| 平台 | `appId` 决定的 | 隔离后的行为 |
|---|---|---|
| Windows | `AppUserModelID` 与通知分组；卸载项键名由 cargo-packager 决定（实测用 `DisplayName`，即 `DSH-X`），**不等于 `appId`** | 两个产品的卸载项不打架；装本 fork 时按 `DisplayName + InstallLocation` 认上游那台（它是 GUID 键），别只按 `upstreamAppId` 找 |
| macOS | `Info.plist` 的 `CFBundleIdentifier`、`~/Library/LaunchAgents/<appId>.plist` | LaunchServices 认为是两个应用，`open`、通知权限不会串 |
| Linux | `.desktop` 的 `StartupWMClass` 与自启文件名 | deb 包名仍叫 `dsh-x`：apt 层面就是「替换上游」，这是有意的（同名才能原地升级） |
- 这两个 Windows/mac 的「身份」以前是 Electron 替你设的：`AppUserModelID`、`CFBundleIdentifier`、通知分组都在
  Electron 的启动流程里。Rust 外壳没有这一层——Windows 要显式调 `SetCurrentProcessExplicitAppUserModelID(<appId>)`，
  否则托盘/气泡通知会挂到无名分组上、在通知中心里和别的程序混在一起；Linux 的 `.desktop` 要显式写 `StartupWMClass`，
  否则任务栏认不出「同一个应用」。这是换外壳时最容易漏、又最像 bug 的一处。

- **「能同时启动」不等于「支持共存」**：单实例名带上 `appId` 之后，两个产品确实能同时开着，但它们共用
  `%APPDATA%\DSH` 的 `settings.json`、日志与 `versions/`，会互相覆盖配置、抢端口。所以安装器与文档一律写
  「装本 fork 前先卸载上游」，不给并存留口子；真要并存，`dataDir` 与 `DSH_HOME` 得一起分层，工作量不小，
  且容易把 profile 搅坏。
- 迁移窗口里旧 Electron 版（上游 `appId`）的卸载项会变成孤儿：安装钩子顺手删掉（§8），不需要写迁移器。
### 11.9 如果最终要回贡上游

- 分支模型不变，变的是落点：把 `strip-electron` 整理成 3–5 个语义提交（`git merge --squash` 到一个
  `feat/remove-electron` 分支），向上游提 PR，而不是把整条历史推过去。
- 回贡前改 `fork.json` 回上游身份，并在上游的 Node 矩阵（22 / 24）上跑一遍 `npm test`。
- 上游不收，就长期维护 fork：那时引入 `develop`（= master + 全部改造），`master` 继续当镜像，
  `strip-electron` 退休；`docs/upstream-shell-changes.md` 从「移植清单」变成「差异清单」。

### 11.10 分支寿命与纪律

- 目标：`strip-electron` ≤ 4 周；上游活跃期每 2–3 天同步一次，单次成本约 2 分钟（11.3）。
- **闸门**：任一阶段拖过一周，就把已完成部分先合进 `strip-electron` 并保证 `npm test` 绿——半成品留在
  分支上不丢人，`shell-rs/` 里的新增文件也不产生同步成本。
- **反向纪律**：不要为了「少冲突」把上游文件复制一份再改（例如把 `shell/main.mjs` 拷进 `shell-rs/` 慢慢改），
  那会让 11.4 第 ① 类的免费通道失效，也让上游的修复永远进不来。移植就是重写，重写完把旧文件留给删除那一步。

一句话：**上游的新功能靠 `ff-only` 自动进来，我们的改造靠一个新目录长出来，冲突只发生在两者之间的那条缝——
而那条缝被 11.4 的四类分流和 11.5 的机械删除压到了最小。**

## 12. 测试

- 既有 300+ 用例里，业务部分（`server.js` / `settings.js` / `packs.js` / `sync.js` 等）不受影响。
- 与外壳强相关的用例按 §7.4 重写或删除；新增桥、托盘菜单、prefs 一致性、端到端冒烟四类。
- Rust 侧新增 `cargo fmt --check` 与 `cargo clippy -D warnings`（只在新 CI 里，Node 侧仍不引入 lint 配置）。
- 新增 `test/fork-identity.test.mjs`：`fork.json` 可解析且字段齐全；`shell-rs/`、`scripts/`、`server.js`、
  `core-npm.js`、`filemanager-*.js` 里不再出现 `io.github.yyh-001` 与上游核心包名（`docs/releases/` 的历史
  与 `fork.json` 的 `upstreamAppId` 除外），防止哪天改配置时漏掉一处、又串回上游。

## 13. 风险

| 风险 | 影响 | 对策 |
|---|---|---|
| 用户没装 Node 或版本太低 | 装了跑不起来 | 面板明确引导 + 版本门槛 + 文档写清；启动按钮置灰并说明 |
| `node-addon-require-builtin` 在纯 node 下缺预编译 | 全案不成立 | P0 第一天验证；失败则退回「随包带 node」单一变量方案 |
| Linux 缺 `libayatana-appindicator3` | 无托盘 | AppImage 打包进去；deb 声明依赖；无托盘降级 |
| egui 中文输入法体验 | 面板可用性 | `egui-winit` 已支持 IME，P2 专项验证 |
| 功能面缩小（侧栏浏览器、标签页、内嵌窗口） | 用户预期 | §9 表 + 发版说明写明；侧栏浏览器插件补一条明确的「当前外壳不支持」提示 |
| 旧 Electron 版残留文件 | 磁盘占位、卸载项重复 | 现在只能文档要求手动卸载（cargo-packager 无安装钩子）；自动清理见 §8 的「自动钩子未做」 |
| 上游持续改 Electron 外壳 | 移植工作长期存在、功能悄悄落后 | §11.6 的移植清单 + 每次同步查 `git log -- shell/`；连续两月只涨不减就考虑回贡上游（§11.9） |
| 外壳被强杀/崩溃后核心成孤儿 | 管理页端口被占、文件删不掉、下个实例起不来 | 已修：`start.js` 的看门狗每 3 秒检查 `DSH_SHELL_PID`，父进程消失就关服务退出；外壳正常退出仍走 `supervisor.stop()` |
| fork 与上游发版身份撞车 | 两个安装包互相升级、更新器把人带去上游、核心包互覆盖 | §11.8：`fork.json` 单点定义；P3 在真实安装包上逐个核对卸载项 / `Info.plist` / 自启项 / 单实例名 |

## 14. 非目标

- 不重写 `public/index.html`（约 7000 行）到原生界面。
- 不迁 dsh 本体、不改 `.dspack` / profile / `DSH_HOME` / 数据目录布局。
- 不做旧 Electron 外壳的自动升级与迁移器。
- 不做 macOS Developer ID 签名与公证（保持 ad-hoc，mac 外壳仍不提供自更新）。
- 不引入前端构建步骤；核心仍是零第三方运行时依赖。