# AGENTS.md

给在这个仓库里干活的 AI 的说明书，人看也欢迎。**动手前先读「硬规矩」与「关键机制」两节。**

## 这是什么

DSH-X：DeepSeek Harness（`dsh`）的第三方启动器。`package.json` 的 `name` 是 `dsh-x`，包名和安装目录名都别改。它启动的是 dsh 官方原版 Web 界面，自己不重做 dsh 的页面。外壳已经**从 Electron 换成 Rust 原生外壳**（`shell-rs/`），发布身份（appId / 更新源 / 核心包名）集中在根目录 `fork.json`。

```
原生外壳  shell-rs/（eframe/egui 面板 + 托盘 + 本机通道 + 更新器）  常驻托盘，启动直接开管理页，托盘菜单起停核心
Node 核心 start.js → server.js        管理页后端 + 版本 / 插件 / 整合包 / 同步的全部业务；由外壳作为子进程拉起
管理界面 public/index.html           单页手写 HTML + 原生 JS，无框架、无构建步骤，用系统浏览器打开
启动进度页 public/booting.html       轮询 /api/state 显示阶段与安装进度，dsh 就绪后自己跳进去
注入钩子 compat/ perf/               被注入 dsh 进程，修补运行时行为（见下）
```

**零第三方运行时依赖**：核心与页面的 import 都是 `node:` 内置或相对路径，没有构建步骤。运行 dsh 的 Node 来自用户机器（22 与 24 都支持，`>=22.18.0`）：外壳按「`DSH_RUNTIME_NODE` → PATH 的 node → ProgramFiles → nvm/fnm」找到后用环境变量交给核心，核心再用它跑 dsh / npm / pnpm（见 `platform.js` 的 `RUNTIME_NODE`）。找不到 Node 时面板显示横幅、启动按钮置灰。
唯一的构建期依赖是 Rust 工具链（编译 `shell-rs/`）与 `cargo-packager`；打包前另需 `npm install`（只装打包脚本用的东西，核心本身不需要）。

## 怎么跑、怎么验

| 命令 | 干什么 |
|---|---|
| `npm start` | 起管理页（开发入口，浏览器模式） |
| `npm run server` | 只起网页服务，不开浏览器 |
| `npm test` | 全部用例（`node --test`，1000+ 用例，约 3 分钟） |
| `node --test test/packs.test.mjs` | 单个用例文件 |
| `npm run shell` | 编译并运行原生外壳（`cargo run --release --manifest-path shell-rs/Cargo.toml`） |
| `cargo test --manifest-path shell-rs/Cargo.toml` | 外壳单测（版本门槛、帧编解码、管道名随机串、核心路径推导） |
| `npm run dist` | 打安装包（`node scripts/pack-rs.mjs`，先 `cargo install cargo-packager --locked`） |
| `node scripts/make-pack.mjs` | 把 `packs/<名字>/` 打成 `.dspack`（产物在 `release/packs/`） |

- **跑测试别把输出接管道**（`| head`、`| tail` 会把输出缓冲成 0 字节，等于白跑）；要留证据就重定向到文件。
- 没有 lint / 格式化配置，**也别加**：风格靠已有代码和这份文档。
- 用例里有三只"假服务器"（`test/fake-s3.mjs`、`fake-webdav.mjs`、`fake-proxy.mjs`），网络相关改动靠它们离线验证。
- 真机冒烟：`$env:DSH_CORE_DIR="<仓库根>"; $env:PORT="3799"; .\shell-rs\target\debug\dsh-x.exe`。`PORT` 会被 `settings.js` 的 `resolvePort()` 优先读取，用来避开本机已占用的 3780。

## 硬规矩

- **改完不要自动 commit、不要自动 `npm run dist`、不要发版。** 改动留在工作区，等用户明确指令；用户习惯自己按功能拆提交。
- commit message 用**中文**，格式 `类型: 说明`。类型只有四个，和发版说明的四块一一对应：`feat`（新增功能）/ `perf`（体验优化）/ `fix`（问题修复）/ `chore`（其他变更）。`node scripts/release-notes.mjs` 按前缀拆块生成草稿。
- README 中英成对改（`README.md` + `README.en.md`）；**改 README 默认是"砍"不是"加"**——用户会周期性说"写得太多"。
- Release notes 按 `docs/releases/TEMPLATE.md` 写进 `docs/releases/v<版本>.md`（**入库**，中英同篇、四块固定、一行一条、挂 `@贡献者`）；不写根因、验证过程、安装说明。预发布（rc / alpha）在 GitHub 上勾 Pre-release。QQ 群公告压到 400 字符内（只写用户看得见的变化，下载只留落地页一条链接），存一份到 `release/群公告-<版本>.txt`。
- 注释写**为什么**（这个仓库的风格：每个不显然的决定都带一段解释，很多是踩坑记录），用中文；不写逐行翻译代码的注释。
- `release/release-key.pem` 与 `release/packager-key.pem` 是发版签名私钥：不进仓库、别提交、别把内容打印到日志；动相关流程时提醒用户备份。

## 目录地图

| 路径 | 是什么 |
|---|---|
| `start.js` | 入口：端口与日志；被外壳拉起时安装 `host-remote.js` 的本机通道、通道 EOF 就退出，另有看门狗盯着 `DSH_SHELL_PID` 兜底 |
| `server.js` | 管理页后端 + 全部业务（最大文件）：HTTP 路由、版本管理、插件安装、整合包、启动失败自愈 |
| `host-remote.js` | 核心侧的外壳代理：把 `setHost()` 的能力转成一条本机套接字上的 JSON 帧（4 字节小端长度 + UTF-8）；`createIpcTransport` 管连接与帧，`createRemoteHost` 管缓存与降级 |
| `fork.json` / `fork.js` | 发布身份（appId、productName、updateOwner/Repo、corePackage、dataDir、tagPrefix）；核心与打包脚本都只从这里读 |
| `settings.js` | 设置读写；`safe*` 一族是校验函数（端口、路径、代理、语言……都从这过） |
| `registry.js` / `version.js` | dsh 版本发现、pnpm 交互（进度解析）、版本号比较；装版本失败时把 npm 的收尾报错换成具体提示（`describeNpmFailure`：ETARGET 发版不齐 / 404 没有这个包） |
| `plugins.js` | 插件开关：直接改 profile 的 `cordis.patch.yml`，不需要 dsh 在跑 |
| `mcp.js` / `skills.js` | MCP server 与技能的读写、探测、开关 |
| `packs.js` | 整合包：`.dspack`（manifest v5 ZIP）的解析 / 检查 / 安装 / 导出 / 回滚 |
| `zip.js` / `zipfile.js` | zip 的写与读（手写，无依赖） |
| `sync.js` | 同步引擎：S3 / WebDAV / 本地目录 / ZIP 四种后端共用一个"远端"抽象 |
| `proxy.js` | `netFetch`：Node 全局 fetch 不看系统代理，所有外网请求走它 |
| `platform.js` | 平台常量、`PACKAGED` 与 `RUNTIME_NODE`（来自 `DSH_RUNTIME_NODE`）；只有常量，没有行为 |
| `filemanager.js` | 按平台挑一套文件管理实现，导出 `fileManager` 与 `APP_DIR`；业务代码只认它 |
| `filemanager-base.js` | 那个接口本身：契约、默认实现、其他 POSIX 平台的 xdg-open 兜底（`PosixFileManager`） |
| `filemanager-windows.js` / `filemanager-mac.js` / `filemanager-linux.js` | 三套实现类，都随跨平台核心包发，运行时按平台动态 import |
| `plugin-tool.js` | 命令行开关插件（dsh 起不来时的救命工具），有可执行位 |
| `stdio-unblock.cjs` | stdout/stderr 阻塞模式修补（`--require` 注入用） |
| `public/` | 管理界面与看板娘素材（`index.html` 是主体；`booting.html` 是启动进度页，由 `/booting` 渲染） |
| `scripts/` | 打包（`pack-rs.mjs` / `pack-common.mjs` / `pack-core.mjs`）、图标（`make-icons.py`）、落地页素材、发版说明草稿（`release-notes.mjs`）与发版清单 / SBOM |
| `plugins/` `packs/` | 插件源码（发 npm）与随包发的整合包配方（见「关键机制」，**两个目录别合并**） |
| `skills/` | 给 AI 用的技能源码（随仓库分发，不进安装包）：`dsh-x-selfcheck` 只读自检环境，`node skills/dsh-x-selfcheck/scripts/selfcheck.mjs`，零依赖 |
| `compat/` `perf/` | 注入 dsh 进程的钩子：兼容修补与加速 |
| `shell-rs/` | Rust 原生外壳：`main.rs` 单实例与启动、`supervisor.rs` 起停核心、`panel.rs` egui 面板、`tray.rs` 托盘、`bridge.rs` 本机通道（命名管道 / UDS）、`notify.rs` 系统通知、`identity.rs` Windows 应用身份（AUMID 与通知署名）、`prefs.rs` 设置、`updater.rs` 更新器（目前只返回 `disabled`）；`Cargo.toml` 是外壳版本与 `env!` 注入的 appId/productName |
| `test/` | 用例与假服务器 |
| `docs/` | GitHub Pages 落地页 + 看板娘设计稿 + `releases/`（入库的发版说明）+ `strip-electron-plan.md`（去 Electron 改造的方案与进度）+ `build-installers-plan.md`（三端出包方案） |
| `assets/` | 图标 master（`icon.png` → `make-icons.py` 出全套） |
| `data/` `release/` `vendor/` | 运行时数据与产物，gitignored，可重建，**不用整理** |

**顶层 .js 平铺是有意的，不是没整理**：核心文件按 `scripts/pack-common.mjs` 的 `APP_FILES` 原样拷进安装包的 `core/`（Windows：`<安装目录>/core/`；macOS：`DSH-X.app/Contents/Resources/core/`），外壳按「`DSH_CORE_DIR` → 数据目录的 `core/current` → exe 同级 `core/`」选核心。要把它们收进 `src/` 得同时改 `APP_FILES`、`supervisor.rs` 的 `core_dir()`、`server.js` 的根路径推导和核心更新路径，并重新打包验证，别只挪文件。

## 关键机制

### 外壳与核心的契约

- 外壳 `spawn` 核心：`DSH_RUNTIME_NODE`（系统 Node）、`DSH_SHELL_IPC`（本机通道名）、`DSH_SHELL_PID`、`DSH_SHELL_EXE`、`DSH_SHELL_VERSION`、`DSH_SHELL_PREFS=1`、`DSH_CORE=1`；核心在 `start.js` 里用 `remoteHostFromEnv()` 装上 `host-remote.js`，之后 `setHost(REMOTE_HOST)`——`server.js` 一行都不用为外壳改。
- 通道是**一条本机套接字**（Windows 命名管道 `\\.\pipe\dsh-x-<外壳 pid>-<8 位随机>`，unix `<APP_DIR>/ipc/shell-<pid>.sock`，目录 0700 / 文件 0600），外壳当服务端、核心当客户端。帧 = 4 字节小端长度（上限 1 MiB）+ UTF-8 JSON：请求 `{id,method,params}`，响应 `{id,ok,result|error}`，事件 `{event,data}`（连接建立时外壳先推一次 `shellUpdate`）。
- 核心调外壳的动作名：`open` / `wake` / `notify` / `exit` / `relaunch` / `info` / `desktop/*` / `browser/*` / `shellUpdate/*`。实现见 `shell-rs/src/bridge.rs` 的 `route()`，映射表见 `docs/strip-electron-plan.md` §6。
- 鉴权不靠 token：连上后比对**对端 pid**（`GetNamedPipeClientProcessId` / `SO_PEERCRED`，mac 的 xucred 没有 pid 时退回文件权限）。排障用 `node scripts/ipc-call.mjs info`（读 `DSH_SHELL_IPC`），没有 curl 可用了；它是个独立进程，外壳得用 `DSH_SHELL_IPC_ALLOW_ANY=1` 起才放行（默认严格）。
- 核心**不许** import 外壳、外壳**不许** import 核心；两边只认上面的帧契约。
- 生命周期：通道 EOF 就是外壳死亡信号，`start.js` 立刻关服务退出；`watchShell()` 每 3 秒看一次 `DSH_SHELL_PID` 兜底（通道被安全软件掐断、或外壳卡死不肯关套接字时只有这条还认得出来）。

### 面板、托盘与双向帧（`docs/shell-panel-plan.md`）

- 通道是**双向**的，帧形状不变、靠有没有 `method` 分流：核心 → 外壳的 `{event,data}`（目前 `panel` / `shellUpdate`），外壳 → 核心的 `{id,method,params}` 与回包。反向调用 3 个方法：`panel/state`（整份面板模型）、`panel/action`（起停，参数与托盘一致，60 秒超时）、`panel/settings`（只收白名单键，见 `server.js` 的 `PANEL_SETTINGS_KEYS`）。
- **设置的唯一写者是核心**：面板点开关不写 `settings.json`，发给核心走页面同一个 `saveManagerSettings()`，核心再推新模型回来；外壳只读 `prefs.rs` 里的 `hardwareAcceleration` 与 `closeToTray`。所以别给面板加「自己存一份开关」的缓存路径。
- 模型形状是 `server.js` 的 `panelState()` ⇄ `shell-rs/src/model.rs` 一一对应，**改一边必须改另一边**（`test/panel-model.test.mjs` 盯着核心那侧）。核心允许给 `null` 表示「还不知道」，`PanelModel::parse()` 解析前递归删 `null` 再让 `serde(default)` 兜底——不这么做一个 `null` 就让整份模型作废、面板停在上一帧。
- 窗口藏着时 eframe 只跑 `logic()` 不跑 `ui()`，没人再 `request_repaint_after`，所以往主循环送事件一律走 `bridge.rs` 的 `emit()`（发完 `ui.request_repaint()`）。**别直接 `ctx.events.send`**，否则「托盘点打开面板」要等下一次偶然重绘。
- 托盘菜单跟着模型重建：`tray.rs` 的 `Tray::sync()` 比一份 `signature`，变了才 `set_menu`；菜单项动作只登记进 `ROSTER`，由面板 `apply_tray()` 统一走 `link.action()`。
- `DSH_SHELL_IPC_ALLOW_ANY=1` 放开的连接是 `Trust::Debug`：能下指令，但**不许顶替核心当反向通道**（否则 `scripts/ipc-call.mjs` 一接上，面板点按钮就把请求发给它，面板显示「核心还没连上外壳通道」）。
- **`Supervisor::stop()` 是「暂停」不是「拆线程」**（`shell-rs/src/supervisor.rs`）：它会杀子进程、把 status 落到 `Stopped`（清 pid 与 url），但监督线程留在等待分支里等 `restart()`；退出外壳走 `shutdown()`。以前 `stop()` 顺手 `break` 了循环，害处有两个：status 停在 `Running` 让面板显示「运行中」却连不上通道，而且「启动核心」再也没人执行。面板 header 的「启动核心 / 停止核心」、托盘的同一项、以及托盘那几项预设动作都靠这条约定。

### 版本、实例与数据

- 每个 dsh 版本装在数据目录（默认 `%APPDATA%\DSH\data`，设置里可改）的 `versions/<版本>/`。
- **一个「版本 × profile」一个实例**，可以同时跑，各占各的端口：默认给会起 web 的 profile 传 `--port 0`，由系统现挑，启动器再从 dsh 打印的地址里读回真实端口。
- 端口也能手工钉死（`settings.json` 的 `instancePorts`，键 `版本@profile`）：钉了就传 `--port <它>`，链接每次启动都一样。钉住的组合起不来时（`EADDRINUSE`）报错要点名端口和占用者。**这是 dsh 实例的端口，跟管理页端口（`port`）是两回事，两者不能撞。**
- dsh 本体数据在 `DSH_HOME`（默认 `~/.dsh`，可配），跨版本共享，所以换版本不用重装插件。
- 启动器自己的设置与日志在 `%APPDATA%\DSH`（macOS 为 `~/Library/Application Support/DSH`）。

### profile

dsh 的环境隔离单位。模板名（web / headless / acp / sdk / …）会自动初始化，自定义名必须先有 `package.json`，否则 dsh 拒绝启动。插件按 profile 分，会话与记忆共享。启动器提供切换与开关；插件开关就是往 `cordis.patch.yml` 写 `- id: <行> / disabled: true`，删掉该块即恢复。

恢复档（`recovery.js`，官方 desktop `sanitizeProfile` 的同款语义）：`POST /api/recover` 把补丁层**改名**成 `cordis.patch.yml.bak-<时间戳>`（备份即改名、不解析补丁内容），再按侧车记录摘掉清单里非 `@deepseek-ai/*` 的 bundle；`POST /api/recover/restore` 把备份改回来、现役补丁先挪到新备份、并按侧车放回被摘的 bundle。改补丁层/清单前都过 `withProfileLock`（profile 目录里一个存 PID 的 `lock` 文件，`wx` 独占创建、属主僵死才清理）——与官方锁同名同语义。

### 插件与整合包（两个目录，别合并）

- `plugins/<包名>/` 是**插件实现的源码与发布源**（npm 包：`dsh-x-memory`、`dsh-x-sync`、`dsh-x-aquarium`），**不随安装包发**：插件都发到 npm，用户从插件页 / 整合包装，装完能看远程版本、能更新。改了这里的代码要 `npm publish`（版本号记得加）。
- `packs/dsh-x-recommended/` 是**整合包配方**（`manifest.json` + `dspack.json` + README），**不含代码**，随安装包发；依赖一律写 npm 版本号。
- 没有「内置插件」这条路：预置到当前 profile 的开关、随包发插件的复制逻辑、整合包里的 `"…": "bundled"` 依赖解析都已撤（`planInstall` 见到 `bundled` 会直接报错）。

### 文件管理：一个接口，三套实现

每用户目录与安装布局、路径大小写、目录链接类型、系统文件对话框、打开或定位、交给系统默认程序打开地址、登录自启——**全部**走 `filemanager.js` 导出的 `fileManager`，业务代码里不再写 `process.platform === 'win32' ? … : …`。

- 契约在 `filemanager-base.js` 的 `FileManager` 上，每个方法的注释就是规范。
- `filemanager.js` 用**动态 import** 只加载当前平台那一套（三套都随跨平台核心包发）。它有顶层 await，所以实现类静态 import 的是 `filemanager-base.js` 而不是它——反过来 import 会死锁。
- 自启项要执行的外壳：Windows 读 `DSH_SHELL_EXE`，没设就找 exe 同级的 `dsh-x.exe`；Linux 依次看 `APPIMAGE`、`DSH_SHELL_EXE`；mac 用 `.app` 布局走 `open -a`。源码运行不写自启。
- 还留在 `server.js` 里的平台判断是**另一摊事**，故意没收进来：pnpm 子进程怎么起、`killTree`、GB18030 解码、自更新包的名字。

### 注入进 dsh 的钩子（compat / perf）

- `compat/register.mjs`、`perf/register.mjs` 由启动器注入；worker 线程注入 `compat/worker-events.cjs`（worker 的 execArgv 是空的，只能靠 NODE_OPTIONS）。
- NODE_OPTIONS 里的路径走 `nodeOptionsPath()`：反斜杠换正斜杠、含空白才包引号——**引号会吃反斜杠**，所以不能只加引号。**别退回「NODE_PATH 传目录 + 裸文件名」**：原生外壳不消费 NODE_PATH。
- 改会话事件词汇表（`SESSION_TYPES`）要**两处同步**：`compat/session-events.mjs` 与 `compat/worker-events.cjs`。
- `perf/plugin-profile.mjs` 是**插件体检**：正常启动时用 V8 采样按包统计启动期开销，稳定后写到 `<APP_DIR>/plugin-profile/<版本>@<profile>.json`；管理页实例行的「明细」读它。

### 桌面端系统开关（设置页「常规 → 系统」）

- 六个键（`hardwareAcceleration` / `autoUpdate` / `taskNotifications` / `notificationSound` / `closeToTray` / `keepAwake`）存在 `settings.json`，**外壳直接读文件**（`shell-rs/src/prefs.rs`），不走管理页接口：外壳和核心分开更新，读同一份文件避免协议版本问题。默认值在 `settings.js` 的 `DEFAULTS` 与 `prefs.rs` 的 `Default` 两处，**必须一致**（`test/prefs-parity.test.mjs` 逐项比对）。
- 托盘每 1.5 秒按 mtime 重读并实时应用；`closeToTray` 关窗时藏到托盘而不是退出。
- `DSH_SHELL_PREFS=1` 由外壳写入，`/api/settings` 的 `shellPrefs` 据此告诉页面外壳认不认这组键。
- 任务通知：`server.js` 在实例启动失败 / 意外退出时调 `host.notify`；文案、系统通知都在外壳里；**正常启动不发通知**。这个开关只管外壳自己的提醒，插件通知走 `/api/desktop/notify`，由插件自己的开关决定。
- 通知署名来自 `notify.rs` 的 `app_id(DSH_APP_ID)`：不传它 notify-rust 会退回 PowerShell 的 AUMID，Windows 就把通知算成「Windows PowerShell」（`identity.rs` 开机时注册同名 AUMID 的 `DisplayName` / `IconUri`，否则通知中心里是一串裸 id）。`notificationSound` 决定要不要 `.sound_name("Default")`——不设它 notify-rust 生成的 toast 带 `<audio silent="true"/>`，开着提示音也听不见。

### 启动行为：默认没有任何窗口

- 用户双击后**不弹面板、不弹黑框**：装出来的 exe 是 GUI 程序（`main.rs:3` 的 `cfg_attr(not(debug_assertions), windows_subsystem = "windows")`，调试构建保留控制台），核心子进程带 `CREATE_NO_WINDOW`（`supervisor.rs` 的 `creation_flags(0x0800_0000)`）。
- 核心把管理页地址打出来后，`panel.rs` 的 `auto_open` 用系统默认浏览器打开；面板只在这两种情况出现：托盘点「打开面板」（`tray.rs` 的 `Action::Show`），或核心反复起不来（`Phase::Failed`，只亮一次，日志是唯一线索）。
- **别把面板改成启动即可见**：这条是用户明确要过的（`m02400`）。改动集中在 `panel.rs` 的 `wants_window` / `cloak`，`main.rs` 的 creator 里建窗后立刻隐身。
- 坑（踩过，别再踩）：eframe 第一帧画完会**无条件** `window.set_visible(true)`（`eframe-0.37.0/src/native/epi_integration.rs:441-453`，本意是防白闪），`NativeOptions` 里传 `.with_visible(false)` 压不住它；只发 `ViewportCommand::Visible(false)` 也压不住中间那一帧（实测可见约 44ms）。所以 Windows 上改用 DWM 的 `DWMWA_CLOAK` 属性（`panel.rs` 的 `set_cloaked` / `cloak`，依赖 `windows-sys` 的 `Win32_Graphics_Dwm`），`Visible(false)` 留作兜底。
- 单实例：第二次双击不再弹面板，`supervisor.rs` 的 `remember_url` / `open_running_manager` 读 `<APP_DIR>/shell-url.txt` 里上次的管理页地址，探活后直接开浏览器。
- **外壳起的任何子进程都要过 `supervisor::no_window`**（`CREATE_NO_WINDOW`）。GUI 父进程 spawn 控制台程序时 Windows 会给子进程新开一个控制台窗口，屏幕上就是一闪而过的 CMD / 终端窗口：踩过两次——核心 `node.exe` 少了它会留一个黑框常驻，`node --version` 探测少了它会启动时闪两下（找 node 探一次、面板要版本再探一次，`node_version` 现在还有按路径缓存）。
- 验证：起 exe 后用 `EnumWindows` 看 `标题=DSH-X` 的窗口应是 `wsVisible=False`、`DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED=14)` 为 `1`；`/api/state` 应为 200；核心故意弄坏（给个只有 `start.js` 且 `process.exit(1)` 的目录）时面板应自己亮出来（`cloaked=0`）。
### 内置浏览器控制（设置页「浏览器控制」）

- 两个键（`browserControl` / `browserIgnoreCertificate`）同样是「核心写 `settings.json`、外壳直接读」。
- `browserControl` 管 agent 那一半：开着时 `ensureDesktopPlugins` 补装 `dsh-x-sidebar-browser`，关掉时不再补装并给已装插件补 `disabled: true`（`sidebarBrowserPatchArgs`）。侧栏 Browser 本身不受影响；改完要重启 dsh 实例。
- **原生外壳没有内嵌浏览器**：`browserIgnoreCertificate` 与清理按钮（`/api/browser/clear-cache`、`/api/browser/clear-data`）没有落点，桥对 `browser/*` 返回 `ok: false`，页面据此禁用按钮。

### 插件桌面 API（`/api/desktop`）

- 插件跑在 dsh 子进程里，拿不到外壳。能力走本机 HTTP：插件 → `server.js` 的 `/api/desktop/*` → `host.desktop` → `host-remote.js` → `shell-rs/src/bridge.rs`。协议见 `docs/desktop-api.md`。
- 环境变量由 `dshEnv()` 注入：`DSH_X_MANAGER_URL` 总会给；`DSH_X_DESKTOP=1` 只有 `host.desktop` 存在时才给，插件按它静默降级。
- 请求校验在 `desktop.js`（核心纯函数，可单测）；外壳侧门控在 `bridge.rs` / `notify.rs`（通知开关 / 声音 / 对话框选项）。
- `reveal` 只收绝对且真实存在的路径；对话框父窗口只在面板可见时挂。

### 桌面增强（设置里一键安装）

- 三个首方插件 `dsh-x-desktop-notify` / `dsh-x-context-menu` / `dsh-x-sidebar-browser` **不在安装包里，也不随启动自动装**：设置 → 常规 → 桌面增强 右侧「一键安装」才装。清单和版本门槛在 `desktop-plugins.js`（dsh ≥ 0.2.0，侧栏浏览器 ≥ 0.2.1-rc.2，同版本预发布算满足）；安装走 `POST /api/desktop-enhance/install`，复用插件页的 `addPlugin`。
- 只装当前 profile 里缺的；内核版本低于插件下限时那一项跳过并在响应里给原因。
- 发布时三个包要发到 npm，`desktop-plugins.js` 的 `spec` 钉精确版本（首发先出 `0.0.0-stage` 占位会导致装到空壳）；确认三个包都有 `meta.title` + `locale/{zh,en}.json`。

### 核心更新（core-npm.js / apply-update.mjs）

- 核心作为跨平台 npm 包发版：`fork.json` 的 `corePackage`（当前 `@duqingyude/dsh-x-core`）。`scripts/pack-core.mjs` 把 `release/rs-stage/core`（`pack-rs.mjs` 组装的那份）打成 tarball，包里的 `package.json` 的 `main` 是 `server.js`，并带 `dshX.shell`（构建所用的外壳版本）。
- **版本分两套**：根 `package.json` 是核心版本，`shell-rs/Cargo.toml` / `Packager.toml` 是外壳版本。只改核心就只 bump 根版本；改外壳（Rust / 面板 / 托盘 / 桥）才 bump 外壳版本。
- 客户端：`/api/self` 的 `app` 来自 npm registry 的 dist-tags，`/api/self/app-download` 下载 tarball 并校验 `dist.integrity`，`/api/self/app-apply` 切换。registry 跟着设置里的下载源走，`DSH_CORE_REGISTRY` 可覆盖。
- 安装布局：安装目录里的基线核心在 `core/`；更新后的核心在数据目录 `core/current`（Windows `%APPDATA%\DSH\core\current`）。外壳启动时按 `DSH_CORE_DIR` → `core/current` → 同级 `core/` 选核心；核心声明 `dshX.shell` 与当前外壳不一致时 `checkAppUpdate` 返回 `reason: 'shell'`，页面提示先更新外壳。
- 核心切换：`apply-update.mjs` 等核心与外壳**双双退出**（`--shell-pid`）后再替换目录并重启，45 秒内等不到启动标记 `<APP_DIR>/update-ok` 就回滚。Rust supervisor 会在核心退出后 800ms 自动重启，只等核心会撞 Windows 的 EBUSY。

### 外壳更新与打包

- 安装包由 `cargo-packager` 生成（`shell-rs/Packager.toml`，`npm run dist` 调 `scripts/pack-rs.mjs`）：Windows NSIS（每用户）、macOS dmg（`signing_identity = "-"`，ad-hoc 签名）、Linux AppImage + deb。产物在 `release/packager/`。
- `pack-rs.mjs` 先组装 `release/rs-stage/core`（`copyAppFiles` + npm/pnpm JS + 写 `node/` 包装：一律转调 `DSH_RUNTIME_NODE`），再 `cargo build --release`，最后 `cargo packager`。它还会校验 `Packager.toml` 的 version 等于 `Cargo.toml`、identifier 等于 `fork.json.appId`。
- 默认装到 `%LOCALAPPDATA%\Programs\DSH-X`。cargo-packager 的 NSIS 把 currentUser 目录写死成 `%LOCALAPPDATA%\<产物名>` 且没有配置项，所以 `shell-rs/nsis/installer.nsi` 是抄自 cargo-packager 0.11.8 的改版模板（`[nsis] template` 指向它），相对上游只改三处：默认目录加 `Programs`、沿用「上次安装目录」前先确认 `uninstall.exe` 还在（否则装回一个已删掉的旧路径）、卸载递归删目录（上游非递归 + 按字母序，删不掉 `core`）。升级 cargo-packager 要重新比对上游模板，`test/pack-nsis.test.mjs` 盯着这三处。
- 发布身份全从 `fork.json` 读：appId、productName、updateOwner/Repo、corePackage。**别在别处硬编码**（`test/fork-identity.test.mjs` 会查）。Windows 卸载项键名是 `DSH-X`（DisplayName），不是 appId；旧 Electron 版的卸载项是 GUID 且装在 `%LOCALAPPDATA%\Programs\DSH`。cargo-packager 没有安装钩子，所以**目前只能让用户先手动卸载旧 Electron 版**（README 里写了）；要自动处理得在 `pack-rs.mjs` 里另加一步，别在文档里当成已有。
- 外壳自更新：`updater.rs` 目前只返回 `status: "disabled"`（未接 cargo-packager-updater）；mac / Linux 也不提供。核心更新照旧走 npm，不受影响。
- `.github/workflows/build.yml` 三平台并行出包（Windows / Ubuntu / macOS arm64 + x64），Linux job 顺带发跨平台核心 npm 包（`pack-core.mjs --publish`）。mac job 还留着 `if [ -f assets/icon.icns ]` 守卫，但 `assets/icon.icns` 已入库，守卫现在总是通过（可删）。
- **手动触发（`gh workflow run build --ref <分支>`）用不了**：GitHub 只认默认分支上的工作流，而默认分支 `master` 是上游镜像、根本没有 `.github/workflows/`，所以 dispatch 会立刻 `startup_failure`（1 秒失败、无 job）。要验证 CI 就推一个临时 tag（`v0.0.0-ciN`）——tag 上没有对应 Release 时各 job 只上传 artifacts、不会动 Release——跑完把 tag 删掉。
- Linux job 的系统库要装全：`libgtk-3-dev` / `libglib2.0-dev` / `libayatana-appindicator3-dev`（rfd 在 Linux 上走 GTK3，tray-icon 走 ayatana；缺了 `glib-sys` 的 build script 直接失败）。macOS 的用例只在 arm64 job 上跑（失败后原样重试一次：runner 偶发实例启动 120 秒超时，本机复现不出来，真回归两次都会红）；`macos-15-intel` 那台只出 x64 的 dmg（`matrix.run_tests`），因为同一份源码在 arm64 与 Windows / Linux 上都已经跑过，而 intel runner 上实例起停超时成片出现（连续两次各挂 4 条）。
- 本机为什么不能一条命令出三端、Docker 里怎么出 Linux 包、三端产物名与失败模式，见 `docs/build-installers-plan.md`。产物名是 cargo-packager 拼的、改不了：exe/deb/AppImage 用包名 `dsh-x_`，dmg 用 `product_name` 的 `DSH-X_`。
- 安装界面中英双语、**默认跟系统语言走**：`shell-rs/Packager.toml` 的 `[nsis] languages = ["SimpChinese", "English"]` 插两份 MUI 语言包，模板里的 `DSHX_PICK_LANGUAGE` 宏（`.onInit` 与 `un.onInit` 各调一次）把中文系统（2052/1028/3076/4100）归到简体中文、其余归到英文。列出两个语言后 cargo-packager 会传 `display_language_selector=true`，模板**故意不插** `MUI_LANGDLL_DISPLAY`，所以不弹选择框；`test/pack-nsis.test.mjs` 盯着「不弹框 + 两个语言 + 宏调用两次」。
- 图标只有一份 master（`assets/icon.png`）：`python scripts/make-icons.py` 出 `assets/{icon,dsh,tray}.ico`、`tray.png`、`icon.icns` 与 `public/`、`docs/` 两份 favicon。
  - exe / 快捷方式：「Windows 只认资源段」，所以 `shell-rs/build.rs` 用 `embed-resource` 编 `icon.rc` 把 `assets/icon.ico` 嵌进二进制（cargo-packager 不会做这件事；编不进去就 panic，别让它静默退回通用空白图标）。
  - 安装包 / 卸载程序：`[nsis] installer_icon` + 模板里的 `MUI_ICON` / `MUI_UNICON`。
  - 托盘与窗口图标：`shell-rs` 里从 `tray.png` 解码缩小（`image` crate 三个平台都要，别塞进 `cfg(windows)` 依赖），窗口用 `ViewportBuilder` 的 `with_icon`。

### 网络与子进程环境

- 外网请求一律走 `netFetch`（`proxy.js`）：先代理后直连、回环地址不代理。Node 22.19 的全局 fetch 不认代理。
- 给 dsh 子进程的 PATH 末尾追加运行时的 `.bin` 与写死 node 的 shim，但**用户自己的 pnpm 优先**（`orderRuntimePaths`）。store 主版本跟着 pnpm 主版本走（8→v3 / 10→v10 / 11、12→v11），拿错版本去动 profile 会 `ERR_PNPM_UNEXPECTED_STORE`，所以**插件命令**会先按 profile 的 `node_modules/.modules.yaml` 记的 store 挑一次 pnpm。便携兜底那份是 pnpm 11.27.1，版本钉在 `pack-common.mjs` 的 `PNPM_VERSION`。
- dsh CLI 在参数解析阶段就拒掉保留 profile 名（`desktop`，官方留给自家桌面端），两个入口各有一套绕法：启动换 `reserved-profile-boot.mjs`；插件命令给原件建目录链接别名（`.dsh-alias-desktop` → `desktop`，Windows 用 junction）。

## 改代码时容易踩的

- **server.js 有 7000+ 行，别用 `tools.read` 的默认 2000 行读出来再整体写回**（本仓库被这样截断过一次）。整文件改动用 `edit` 的 `replace_all`；真要读全文先确认 `totalLines`。
- **Node 22.19：注册过 ESM 钩子后 `error.stack` 变成只读**。`compat/session-events.mjs` 已把它包进 try/catch，**别去掉**。
- **插件 import 失败会让 dsh 秒退**（插件树的单点故障）。涉及 profile 依赖、补丁层的改动要格外保守，改完用 `plugin-tool.js list` 或真启动一次验证。
- **Windows 的 junction**：pnpm 用 junction 指向 `.pnpm`，目标被删就悬空，读目录会报 `UNKNOWN ... -4094`；清理时要扫进 `@scope` 一层。
- 管理页端口每次启动可能变（被占用会顺延）：旧链接 401、旧标签页里"插件包加载失败"都是正常现象。
- cookie 堆到 16 KB 会让全站 `HTTP 431`：给 cookie 加内容前先算总量。
- 测试输出别接管道（会说第二遍，因为真有人踩）。另外在 DSH 启动器里跑测试时，宿主会注入自己的 `NODE_OPTIONS`，涉及 `--require` 的断言要先排除继承项（`test/bundled-runtime.test.mjs` 有现成写法），也会注入 `DSH_PROFILE`（宿主自己的 profile，比如 desktop），它会盖掉用例默认的 web——`isolateUserHome()` 统一清掉了，自己拼 `env` 的用例也要显式 `DSH_PROFILE: ''`。
- **`public/*.js` 是页面另外请求的脚本**：浏览器扩展 / 安全软件会拦掉（控制台 `net::ERR_BLOCKED_BY_CLIENT`），主脚本随后就抛错、整页初始化停住——按钮全失灵，看着像「安装坏了」（用户真机上报过）。所以引用它们的全局变量必须带兜底（`public/index.html` 的 `const createLogView = window.createLogView || …`），新增外部脚本同理；`test/log-view.test.mjs` 有一条用例盯着这行。
- 改 `shell-rs/` 后先 `cargo build --manifest-path shell-rs/Cargo.toml`，再真机起一次面板（面板能起来、管理页能开、托盘能唤回）才算过。PowerShell 的 `Set-Content` 默认写 UTF-16，会把 `.rs` 改成乱码；改文件用 `tools.edit` / `tools.write`。
- `data/`、`release/`、`vendor/` 里的东西可以随便清（都是产物），但 `release/release-key.pem`、`release/packager-key.pem` 除外。