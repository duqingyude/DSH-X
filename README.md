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
- 💻 **Windows / macOS / Linux** — 三个平台的安装包，支持托盘 / 菜单栏常驻和开机自启，自带 npm / pnpm 包装。

## 快速开始

从 [Releases](https://github.com/duqingyude/DSH-X/releases/latest) 下载对应平台的安装包：

| 平台 | 系统要求 | 安装包 |
| --- | --- | --- |
| Windows | Windows 10 及以上，x64 | `dsh-x_<版本>_x64-setup.exe`（NSIS，每用户安装） |
| macOS | macOS 13 及以上 | `dsh-x_<版本>_aarch64.dmg`（Apple Silicon）/ `dsh-x_<版本>_x64.dmg`（Intel） |
| Linux | x64；AppImage 依赖 FUSE | `dsh-x_<版本>_amd64.AppImage` / `dsh-x_<版本>_amd64.deb` |

### Windows

下载 `dsh-x_<版本>_x64-setup.exe` 安装，从桌面打开 **DSH-X**。安装包未做 Authenticode 签名，SmartScreen 可能提示「未知发布者」，见[签名与杀软误报](#签名与杀软误报)。

### macOS

打开 dmg，把 **DSH-X** 拖进「应用程序」。目前使用 ad-hoc 签名、未做公证，首次打开可能需要在系统安全提示中手动允许；如果提示「已损坏」或「无法验证开发者」，右键打开，或执行：

```sh
xattr -dr com.apple.quarantine /Applications/DSH-X.app
```

mac 的外壳自更新暂不提供，核心仍走 npm 更新。安装前请先卸载旧的 Electron 版 DSH-X（两者不能共存）。

### Linux

下载 `dsh-x_<版本>_amd64.AppImage`，`chmod +x` 后直接运行；没有 FUSE 时用 `./dsh-x_<版本>_amd64.AppImage --appimage-extract-and-run`。

Debian / Ubuntu 也可以装 deb（装到 `/opt/DSH-X`、带菜单项、`apt remove dsh-x` 卸干净），但 **deb 版没有外壳自更新**，换版本要去发布页重新下载：

```sh
sudo apt install ./dsh-x_<版本>_amd64.deb
```

配置在 `~/.config/DSH`；两种包的核心更新都走 npm。

### 校验下载

Release 页面每个文件旁边有 sha256，本地对一下即可：

```sh
certutil -hashfile dsh-x_<版本>_x64-setup.exe SHA256   # Windows
shasum -a 256 dsh-x_<版本>_aarch64.dmg                # macOS
sha256sum dsh-x_<版本>_amd64.AppImage                 # Linux
```

## 使用

- 在主页添加或编辑启动项，选择环境与版本后启动；编辑启动项时可打开 **版本管理**，手动安装或卸载版本。
- 设置 → 常规 → **打开 dsh 的方式**，可选系统浏览器或浏览器应用窗口（没有地址栏）。旧设置里的「桌面窗口」已经没有对应实现，会改用系统浏览器。
- 管理页默认端口为 `3780`，被占用时会顺延。
- 手机或其他电脑访问 dsh：设置 → 高级设置 → **Web 绑定**选「局域网」，下次启动生效。

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

产物在 `release/packager/`：Windows 出 NSIS 安装包，macOS 出 dmg，Linux 出 AppImage 与 deb。安装包由 cargo-packager 生成，核心目录随包装进 `core/`；发布身份（appId、更新源、核心包名）统一读根目录的 `fork.json`。

发版前检查版本与产物（发布清单与 SBOM 只在本机自查，不上传 Release）：

```sh
node scripts/release-manifest.mjs check-tag v0.1.22
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
