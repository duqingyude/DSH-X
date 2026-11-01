<p align="center">
  <img src="docs/hero.png" alt="DSH-X" width="880" />
</p>

<p align="center">
  <a href="https://github.com/deepseek-ai/deepseek-harness">DeepSeek Harness</a> 的第三方桌面启动器 ——<br />
  启动项与多实例，版本、插件、整合包、MCP 和技能管理。
</p>

<p align="center">
  <img alt="Windows" src="https://img.shields.io/badge/-Windows-0078D6?style=flat-square" />
  <img alt="macOS" src="https://img.shields.io/badge/-macOS-000000?style=flat-square&logo=apple&logoColor=white" />
  <img alt="Linux" src="https://img.shields.io/badge/-Linux-FCC624?style=flat-square&logo=linux&logoColor=black" />
</p>

<p align="center">
  <a href="https://github.com/duqingyude/DSH-X/releases">
    <img src="https://img.shields.io/github/v/release/duqingyude/DSH-X?label=Release&color=4D6BFE" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/license-MIT-4D6BFE" alt="License" />
  <br>
  <img src="https://img.shields.io/github/downloads/duqingyude/DSH-X/total?label=downloads&color=4D6BFE" alt="Downloads" />
  <img src="https://img.shields.io/github/stars/duqingyude/DSH-X?label=stars&color=4D6BFE" alt="Stars" />
  <img src="https://img.shields.io/github/contributors/duqingyude/DSH-X?label=contributors&color=4D6BFE" alt="Contributors" />
</p>

<p align="center">
  <samp><a href="https://duqingyude.github.io/DSH-X/">主页</a> · <a href="https://github.com/duqingyude/DSH-X/releases/latest">下载</a> · <a href="docs/releases">更新日志</a> · <a href="CONTRIBUTING.md">参与贡献</a> · <a href="README.en.md">English</a> · <strong>中文</strong></samp>
</p>

> [!IMPORTANT]
> **DSH-X 启动的是 DeepSeek Harness 官方原版 Web 页面。**  
> 它只负责版本安装、启动和插件管理，不修改也不重做 DSH 的网页界面。DSH-X 本身是社区开源项目，并非 DeepSeek 官方产品。

## 功能

- 🚀 **启动项与多实例** — 按项选择用户目录、profile、版本和端口；支持并行启动，运行中也可添加启动项。
- 📦 **版本管理** — 独立管理已安装版本，支持检测新版本、手动安装、卸载和为启动项指定版本。
- 🧩 **插件与整合包** — 按环境安装、开关和更新插件，可配置官方自定义插件市场的默认索引源；整合包市场支持瀑布流浏览、搜索和排序，也能导入、导出整合包。
- 🛠️ **MCP 与技能** — 配置 MCP 服务器，导入、管理和开关技能。
- 🪟 **两种打开方式** — 系统浏览器标签页，或没有地址栏的浏览器应用窗口。
- 🎨 **外观** — 浅色 / 深色主题、透明度、开场与启动项动效，以及独立的背景和大肥鱼开关。
- 🔄 **更新与恢复** — 自更新、故障恢复、可设置保留数量的旧版本清理；启动项当前与上一次使用的版本额外保留。
- ⚙️ **桌面系统开关** — 硬件加速、自动下载更新（安装手动确认）、任务通知与提示音、关窗口隐藏到托盘、防休眠。
- 💻 **Windows / macOS / Linux** — 三个平台的安装包，支持托盘 / 菜单栏常驻和开机自启；运行 dsh 用系统 Node，npm / pnpm 由启动器随包提供包装。

## 设计

### 为什么不用 Electron

- **体积与内存**：Electron 版安装包是 90 MB 级，还要常驻一个 Chromium；现在的 Windows 安装包约 15 MB，界面用系统已有的浏览器，外壳与核心各一个进程。
- **启动体验**：现在双击**没有任何窗口**——不弹面板、不弹控制台，核心报出管理页地址后直接用系统浏览器打开；Electron 版要先起窗口，再在窗口里加载页面。
- **安全面**：不再内置 Chromium，也就没有内嵌浏览器分区、证书例外、缓存清理这些要一直维护的东西（这些能力现在直接移除，见下表）。管理页仍然只监听本机并校验 Origin / Host。
- **更新粒度**：业务与页面都在核心（发 npm 包，几 MB），只有外壳二进制变了才需要重新下载安装包。
- **版本约束**：dsh 的 node addon 对 Electron 运行时做指纹校验，外壳只能钉在少数几个 Electron 版本上，等于把外壳的升级节奏和 dsh 上游绑死；换成系统 Node 之后这层约束没有了。

### 现在的架构

```
DSH-X.exe（Rust 外壳：单实例 / 托盘 / 面板 / 本机通道 / 更新器）
  │  spawn（CREATE_NO_WINDOW，不留控制台）
  ▼
node start.js（核心：管理页后端与全部业务：版本 / 插件 / 整合包 / 同步 / MCP / 技能）
  │  管理页 http://127.0.0.1:<端口>  →  系统浏览器

外壳 ⇄ 核心：一条本机套接字（Windows 命名管道 / Unix 域套接字），帧 = 4 字节长度前缀 + JSON
  ↑ 核心调外壳：打开页面、系统通知、文件对话框、剪贴板、退出、重启
  ↓ 外壳调核心：面板模型（panel/state）、起停启动项（panel/action）、改系统开关（panel/settings）
```

- **外壳只做高频操作**：起停重启启动项、看状态与日志、打开管理页、六个系统开关、切换语言。版本管理、插件市场、整合包、MCP / 技能仍然只在管理页里做——外壳是遥控器，不做第二套管理台。
- **设置的唯一写者是核心**：面板点开关也发给核心，核心写 `settings.json` 后再把新模型推回来，避免两边各写一半、互相覆盖。
- **通道不占端口、不用 token**：连上后比对对端 pid（Windows 命名管道取客户端 pid，Linux / mac 用 0600 的域套接字）。外壳退出就是连接 EOF，核心立刻跟着退出，不需要轮询。
- **默认没有任何窗口**：面板藏在托盘后面，只有托盘点「打开面板」或核心反复起不来时才出现；关窗默认收进托盘、核心继续跑。核心的起停与状态在面板 header 和托盘里都能操作。
- **两套版本、两条更新通道**：根 `package.json` 是核心版本（发 npm），`shell-rs/Cargo.toml` 是外壳版本（安装包）。核心更新只换数据目录里的 `core/current`，外壳更新才重装安装包。
- **发布身份集中**：appId、更新源、核心包名、tag 前缀全在根目录 `fork.json`，回贡上游只改这一处。

## 与 Electron 版的变化

| 方面 | 旧版（Electron 外壳） | 现在 |
|---|---|---|
| 运行时 | 自带 Chromium + 自带 Node | 系统 Node（22.18+ 的 22 / 24），不再自带 node.exe |
| 安装包 | 90 MB 级 | 约 15 MB |
| 启动 | 先起窗口，再在窗口里加载页面 | 无窗口，直接开系统浏览器 |
| 界面 | 管理页 + 窗口内标签页 | 管理页 + 原生面板（托盘里的窗口） |
| 系统通知署名 | Windows 上算在 Electron / PowerShell 名下 | 显示 DSH-X（注册自己的 AppUserModelId） |
| 内嵌浏览器 | 右侧栏浏览器、`sidebar_browser` 工具 | 移除，改用系统浏览器 |
| 下载 / 权限 / 证书例外 | 外壳里的一整套 UI | 移除（跟内嵌浏览器一起没了） |
| npm 版 | `npm i -g @yyh001/dsh-x` | 停止发布 |

### 代码层面的改动

- 新增 `shell-rs/`：单实例与启动（`main.rs`）、起停核心（`supervisor.rs`）、面板（`panel.rs`）、托盘（`tray.rs`）、本机通道（`bridge.rs`）、通知（`notify.rs`）、Windows 应用身份（`identity.rs`）、防休眠（`power.rs`）、设置（`prefs.rs`）。
- 新增 `host-remote.js`：核心侧的代理，把 `setHost()` 的每个成员翻译成一帧 JSON，`server.js` 不用为外壳改一行。
- 新增 `fork.json` / `fork.js`（发布身份）、`scripts/pack-rs.mjs` + `shell-rs/Packager.toml` + `shell-rs/nsis/`（cargo-packager 出三端安装包；NSIS 模板中英双语、默认跟随系统语言）。
- 删除 `shell/`（Electron 主进程、preload、窗口内标签页、electron-updater）、`npm-package/`、`electron-builder.config.mjs`、`scripts/pack.mjs` / `pack-mac.mjs` / `pack-npm.mjs` / `electron-prune.mjs` / `mac-signing.mjs`、`public/internal-tabs.js`。
- `platform.js` 收口：删掉 `IN_ELECTRON` / `NPM_MODE` / `RUNTIME_NODE_ENV`，`RUNTIME_NODE` 一律来自 `DSH_RUNTIME_NODE`（没有就用 `process.execPath`）。
- `server.js`：去掉内嵌窗口与标签页分支、node 运行时切换（只剩系统 Node）、GitHub 安装包自更新（交给外壳）；新增 `panelState()` / `panelAction()` / `panelSettings()` 供面板反向调用。
- `apply-update.mjs`：等核心与外壳双双退出再换目录（监督者会在核心退出后自动重启，只等核心会撞上 Windows 的 EBUSY）。
- 用例：`shell-*` 系用例改成核心侧断言，新增 `host-remote` / `panel-model` / `fork-identity` / `prefs-parity` / `pack-nsis`。

## 快速开始

从 [Releases](https://github.com/duqingyude/DSH-X/releases/latest) 下载对应平台的安装包：

| 平台 | 系统要求 | 安装包 |
| --- | --- | --- |
| Windows | Windows 10 及以上，x64 | `dsh-x_<版本>_x64-setup.exe`（NSIS，每用户安装） |
| macOS | macOS 13 及以上 | `DSH-X_<版本>_aarch64.dmg`（Apple Silicon）/ `DSH-X_<版本>_x64.dmg`（Intel） |
| Linux | x64；AppImage 依赖 FUSE | `dsh-x_<版本>_x86_64.AppImage` / `dsh-x_<版本>_amd64.deb` |

安装包**不再自带 Node**：三个平台都要机器上有 Node.js 22.18+（22 或 24 都行）。没有的话面板会显示提示，也可以在面板设置里指定 Node 路径。

### Windows

下载 `dsh-x_<版本>_x64-setup.exe` 安装，从桌面打开 **DSH-X**。安装包未做 Authenticode 签名，SmartScreen 可能提示「未知发布者」，见[签名与杀软误报](#签名与杀软误报)。

### macOS

打开 dmg，把 **DSH-X** 拖进「应用程序」。目前使用 ad-hoc 签名、未做公证，首次打开可能需要在系统安全提示中手动允许；如果提示「已损坏」或「无法验证开发者」，右键打开，或执行：

```sh
xattr -dr com.apple.quarantine /Applications/DSH-X.app
```

mac 的外壳自更新暂不提供，核心仍走 npm 更新。安装前请先卸载旧的 Electron 版 DSH-X（两者不能共存）。

### Linux

下载 `dsh-x_<版本>_x86_64.AppImage`，`chmod +x` 后直接运行；没有 FUSE 时用 `./dsh-x_<版本>_x86_64.AppImage --appimage-extract-and-run`。

Debian / Ubuntu 也可以装 deb（装到 `/opt/DSH-X`、带菜单项、`apt remove dsh-x` 卸干净），但 **deb 版没有外壳自更新**，换版本要去发布页重新下载：

```sh
sudo apt install ./dsh-x_<版本>_amd64.deb
```

配置在 `~/.config/DSH`；两种包的核心更新都走 npm。

### 校验下载

Release 页面每个文件旁边有 sha256，本地对一下即可：

```sh
certutil -hashfile dsh-x_<版本>_x64-setup.exe SHA256   # Windows
shasum -a 256 DSH-X_<版本>_aarch64.dmg                # macOS
sha256sum dsh-x_<版本>_x86_64.AppImage                # Linux
```

## 使用

- 在主页添加或编辑启动项，选择环境与版本后启动；编辑启动项时可打开 **版本管理**，手动安装或卸载版本。
- 设置 → 常规 → **打开 dsh 的方式**，可选系统浏览器或浏览器应用窗口（没有地址栏）。旧设置里的「桌面窗口」已经没有对应实现，会改用系统浏览器。
- 管理页默认端口为 `3780`，被占用时会顺延。
- 手机或其他电脑访问 dsh：设置 → 高级设置 → **Web 绑定**选「局域网」，下次启动生效。
- 托盘图标常驻：菜单按启动项分组给出「打开页面 / 启动 / 停止 / 重启」。托盘「打开面板」打开原生面板——起停核心、看核心输出、改系统开关与语言都在这里；关闭面板默认收进托盘，核心继续跑。

## 界面预览

**多启动项 · 深色主题**

<p align="center">
  <img src="docs/screenshot-launches.png" alt="DSH-X v0.1.16 多启动项主页" width="820" />
</p>

<details>
<summary>管理页面 · 深色主题（展开查看）</summary>

**环境列表**

<p align="center">
  <img src="docs/screenshot-profiles-dark.jpg" alt="DSH-X v0.1.16 深色环境列表" width="820" />
</p>

**插件管理**

<p align="center">
  <img src="docs/screenshot-plugins-dark.jpg" alt="DSH-X v0.1.16 深色插件管理" width="820" />
</p>

**整合包市场**

<p align="center">
  <img src="docs/screenshot-market-dark.jpg" alt="DSH-X v0.1.16 深色整合包市场" width="820" />
</p>

**常规设置**

<p align="center">
  <img src="docs/screenshot-settings-dark.jpg" alt="DSH-X v0.1.16 深色常规设置" width="820" />
</p>

</details>

<details>
<summary>管理页面 · 浅色主题（展开查看）</summary>

**启动器主页**

<p align="center">
  <img src="docs/screenshot-home.jpg" alt="DSH-X 启动项主页" width="820" />
</p>

**环境列表**

<p align="center">
  <img src="docs/screenshot-profiles.jpg" alt="DSH-X 环境列表" width="820" />
</p>

**插件管理**

<p align="center">
  <img src="docs/screenshot-plugins.jpg" alt="DSH-X 按环境管理插件" width="820" />
</p>

**整合包市场**

<p align="center">
  <img src="docs/screenshot-market.jpg" alt="DSH-X 整合包市场" width="820" />
</p>

**常规设置**

<p align="center">
  <img src="docs/screenshot-settings.jpg" alt="DSH-X 设置页" width="820" />
</p>

</details>

## 整合包

整合包把一组插件和配置装进选定的环境（profile）。插件页按环境展示，装过包的环境会标明来源，插件仍可单独开关。使用 [DSH-PackForge](https://github.com/DSH-PackForge/DSH-PackForge) `.dspack` 格式（manifest v5，兼容旧版本）。

- **导入** — 社区市场、本地文件、直链或 GitHub `owner/repo`。默认安装到当前选中的环境，确认前可切换目标。
- **安装** — 先查看插件和文件变更，再确认安装；覆盖前备份，失败自动回滚。
- **导出** — 把当前环境的依赖、补丁层与配置打成 `.dspack`，不含 `node_modules` 与凭据。

仓库中有两套配方：

- [推荐整合包](packs/dsh-x-recommended/manifest.json) — 配置管理、记忆、同步和大肥鱼装饰插件，均从 npm 安装（同步由可选插件 `dsh-x-sync` 提供）。
- [桌面工作台](packs/dsh-x-workbench/README.md) — 把 dsh-tauri 桌面版的侧栏工作台、工作树、定时任务、归档和通知在纯 dsh 生态里凑齐，并带上 ZCode 的 `agent-browser` 与 `electron` 技能。

运行 `node scripts/make-pack.mjs` 可生成 `.dspack`，再作为本地文件导入。

## 动态壁纸

配套的 Wallpaper Engine 壁纸[《DSH-X · 大肥鱼桌面终端》](https://steamcommunity.com/sharedfiles/filedetails/?id=3814706575)：DSH-X 品牌开场、大肥鱼、本机时间与音乐频谱，按时间自动切换深浅配色。源码在 [`wallpaper/`](wallpaper/)，`node scripts/export-wallpaper.mjs` 导出 Wallpaper Engine 工程。

<p align="center">
  <img src="wallpaper/preview.jpg" alt="DSH-X 动态壁纸" width="820" />
</p>

## 签名与杀软误报

发布清单的签名用于校验文件完整性，与 Windows 发布者签名不同。目前 Windows 安装包尚未做 Authenticode 签名，可能出现 SmartScreen「未知发布者」提示。杀软误报可向[微软](https://www.microsoft.com/en-us/wdsi/filesubmission)或对应杀软厂商提交安装包申诉。

## 说明

> [!WARNING]
> **上游仍在快速迭代** — dsh 内核与插件生态更新频繁，可能存在破坏性变更；升级前请确认插件兼容性。

> [!NOTE]
> **安全提示** — dsh 具备本地代码执行能力，DSH-X 只负责安装与启动。请仅在可信环境中使用。

## 开发

本机需要 Node.js 22.18+（22 与 24 都支持）与 Rust 工具链（只编译外壳时需要）。启动器没有第三方运行时依赖：`npm start` 启动管理页，只起服务用 `npm run server`，测试用 `npm test`，`npm run shell` 编译并运行原生外壳。

开发、提交与发版约定见 [CONTRIBUTING.md](CONTRIBUTING.md)。

### 打包

```sh
cargo install cargo-packager --locked
npm run dist
```

产物在 `release/packager/`：Windows 出 NSIS 安装包，macOS 出 dmg，Linux 出 AppImage 与 deb。安装包由 cargo-packager 生成，核心目录随包装进 `core/`；发布身份（appId、更新源、核心包名）统一读根目录的 `fork.json`。cargo-packager 不交叉编译、dmg 只能 macOS 出，所以三个平台要在各自的机器上打（或者走 CI），细节见 [docs/build-installers-plan.md](docs/build-installers-plan.md)。

发版前检查版本与产物（发布清单与 SBOM 只在本机自查，不上传 Release）：

```sh
node scripts/release-manifest.mjs check-tag v0.1.24
node scripts/release-manifest.mjs verify
```

GitHub Actions 的 `build` 工作流构建 Windows / macOS / Linux 三个平台的安装包，并在 tag 上发布 npm 核心包。macOS 目前只出 dmg（临时签名，外壳自更新暂不提供）。

发布清单的私钥在 `release/release-key.pem`（已被忽略、不进仓库，**务必备份**）。

## 参与

- 安全问题见 [SECURITY.md](SECURITY.md)，别开公开 issue。
- 每个版本改了什么，见 [docs/releases](docs/releases)。
- 交流 / 反馈：**QQ 群 [993579665](https://qm.qq.com/q/7AD2g70HqS)**（[点击加入](https://qm.qq.com/q/7AD2g70HqS)）
- 插件 2 群（DeepSeek Harness）：[点击链接加入群聊](https://qm.qq.com/q/hCjdOJOjSw)

## 相关项目

- [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) — 上游 dsh agent 平台
- [DSH-PackForge](https://github.com/DSH-PackForge/DSH-PackForge) — 整合包 `.dspack` 格式与配方工具
- [deepseek-harness-desktop](https://github.com/dsh-tauri/deepseek-harness-desktop) — 同生态的 Tauri 桌面版，本仓库的整合包配方参考了它的插件集合

## 许可

[MIT](LICENSE) © 2026 yyh
