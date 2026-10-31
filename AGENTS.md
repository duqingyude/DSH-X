# AGENTS.md

给在这个仓库里干活的 AI 的说明书，人看也欢迎。**动手前先读「硬规矩」与「关键机制」两节。**

## 这是什么

DSH-X：DeepSeek Harness（`dsh`）的第三方启动器。`package.json` 的 `name` 是 `dsh-x`，包名和安装目录名都别改。它启动的是 dsh 官方原版 Web 界面，自己不重做 dsh 的页面。外壳已经**从 Electron 换成 Rust 原生外壳**（`shell-rs/`），发布身份（appId / 更新源 / 核心包名）集中在根目录 `fork.json`。

```
原生外壳  shell-rs/（eframe/egui 面板 + 托盘 + 本机 HTTP 桥 + 更新器）  常驻托盘，托盘菜单起停核心
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
| `cargo test --manifest-path shell-rs/Cargo.toml` | 外壳单测（版本门槛、token、核心路径推导） |
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
| `start.js` | 入口：端口与日志；被外壳拉起时安装 `host-remote.js` 的本机桥、并看门狗式盯着 `DSH_SHELL_PID` |
| `server.js` | 管理页后端 + 全部业务（最大文件）：HTTP 路由、版本管理、插件安装、整合包、启动失败自愈 |
| `host-remote.js` | 核心侧的外壳代理：把 `setHost()` 的能力转成 `POST http://127.0.0.1:<桥端口>/host/*` |
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
| `shell-rs/` | Rust 原生外壳：`main.rs` 单实例与启动、`supervisor.rs` 起停核心、`panel.rs` egui 面板、`tray.rs` 托盘、`bridge.rs` 本机 HTTP 桥、`prefs.rs` 设置、`updater.rs` 更新器（目前只返回 `disabled`）；`Cargo.toml` 是外壳版本与 `env!` 注入的 appId/productName |
| `test/` | 用例与假服务器 |
| `docs/` | GitHub Pages 落地页 + 看板娘设计稿 + `releases/`（入库的发版说明）+ `strip-electron-plan.md`（本次改造的方案与进度） |
| `assets/` | 图标 master（`icon.png` → `make-icons.py` 出全套） |
| `data/` `release/` `vendor/` | 运行时数据与产物，gitignored，可重建，**不用整理** |

**顶层 .js 平铺是有意的，不是没整理**：核心文件按 `scripts/pack-common.mjs` 的 `APP_FILES` 原样拷进安装包的 `core/`（Windows：`<安装目录>/core/`；macOS：`DSH-X.app/Contents/Resources/core/`），外壳按「`DSH_CORE_DIR` → 数据目录的 `core/current` → exe 同级 `core/`」选核心。要把它们收进 `src/` 得同时改 `APP_FILES`、`supervisor.rs` 的 `core_dir()`、`server.js` 的根路径推导和核心更新路径，并重新打包验证，别只挪文件。

## 关键机制

### 外壳与核心的契约

- 外壳 `spawn` 核心：`DSH_RUNTIME_NODE`（系统 Node）、`DSH_SHELL_URL`（桥地址）、`DSH_SHELL_TOKEN`、`DSH_SHELL_PID`、`DSH_SHELL_VERSION`、`DSH_CORE=1`；核心在 `start.js` 里用 `remoteHostFromEnv()` 装上 `host-remote.js`，之后 `setHost(REMOTE_HOST)`——`server.js` 一行都不用为外壳改。
- 核心调外壳一律 `POST <DSH_SHELL_URL>/host/<动作>` 带 `Authorization: Bearer <token>`：`open` / `wake` / `notify` / `exit` / `relaunch` / `info` / `desktop/*` / `browser/*` / `shellUpdate/*`。实现见 `shell-rs/src/bridge.rs`，映射表见 `docs/strip-electron-plan.md` §6。
- 外壳把更新进度推回核心：`POST /api/internal/shell-update`（校验同一个 token），核心转 SSE 给管理页。
- 核心**不许** import 外壳、外壳**不许** import 核心；两边只认上面的 HTTP 契约。
- 核心看门狗：`start.js` 每 3 秒检查 `DSH_SHELL_PID`，外壳没了就关服务退出（否则外壳被强杀后核心会成孤儿，占着管理页端口和文件）。

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
- 发布身份全从 `fork.json` 读：appId、productName、updateOwner/Repo、corePackage。**别在别处硬编码**（`test/fork-identity.test.mjs` 会查）。Windows 卸载项键名是 `DSH-X`（DisplayName），不是 appId；旧 Electron 版的卸载项是 GUID 且装在 `%LOCALAPPDATA%\Programs\DSH`。cargo-packager 没有安装钩子，所以**目前只能让用户先手动卸载旧 Electron 版**（README 里写了）；要自动处理得在 `pack-rs.mjs` 里另加一步，别在文档里当成已有。
- 外壳自更新：`updater.rs` 目前只返回 `status: "disabled"`（未接 cargo-packager-updater）；mac / Linux 也不提供。核心更新照旧走 npm，不受影响。
- `.github/workflows/build.yml` 三平台并行出包（Windows / Ubuntu / macOS arm64 + x64），Linux job 顺带发跨平台核心 npm 包（`pack-core.mjs --publish`），mac 缺 `assets/icon.icns` 时整段跳过。

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
- 测试输出别接管道（会说第二遍，因为真有人踩）。另外在 DSH 启动器里跑测试时，宿主会注入自己的 `NODE_OPTIONS`，涉及 `--require` 的断言要先排除继承项（`test/bundled-runtime.test.mjs` 有现成写法）。
- 改 `shell-rs/` 后先 `cargo build --manifest-path shell-rs/Cargo.toml`，再真机起一次面板（面板能起来、管理页能开、托盘能唤回）才算过。PowerShell 的 `Set-Content` 默认写 UTF-16，会把 `.rs` 改成乱码；改文件用 `tools.edit` / `tools.write`。
- `data/`、`release/`、`vendor/` 里的东西可以随便清（都是产物），但 `release/release-key.pem`、`release/packager-key.pem` 除外。
