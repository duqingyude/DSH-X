# 编译三端安装包方案设计

> 状态：方案已定；产物名的配套修正已落地（README.md / README.en.md / docs/index.html / AGENTS.md），三端实跑与 CI 空跑待验收
> 适用版本：DSH-X 0.1.24（Rust 原生外壳 + Node 核心）
> 相关文档：[`strip-electron-plan.md`](strip-electron-plan.md)（外壳与核心的契约）、[`shell-ipc-plan.md`](shell-ipc-plan.md)（本机通道）

## 0 一句话结论

**三端安装包不在本机一条命令里出，走「GitHub Actions 三平台并行」为主通道，本机只作单平台验证**：cargo-packager 的 `--target` 只用来找二进制、不做交叉编译，dmg 只能由 macOS 的 `hdiutil` 生成，所以 Windows / Linux / macOS 必须在各自的操作系统上跑 `node scripts/pack-rs.mjs`。发布时推 `v*` tag，由 `.github/workflows/build.yml` 的 windows / linux / macos 三个 job 并行出包并上传。

## 1 现状

### 1.1 唯一的打包入口

`node scripts/pack-rs.mjs`（`npm run dist`）按顺序做四件事：

1. `checkPackagerConfig()`（`scripts/pack-rs.mjs:34`）校验三处身份一致：`Packager.toml` 的 `version` == `shell-rs/Cargo.toml` 的 `version`；`Packager.toml` 的 `identifier` == `fork.json` 的 `appId`；`[nsis] template` 指向的模板必须存在（丢了会悄悄装回 `%LOCALAPPDATA%\DSH-X`）。
2. `ensureNodeDist()` 从官方 node 发行包只取 npm / pnpm 的 JS（`vendor/`），不带 node 本体。
3. `buildCoreStage()` 组装 `release/rs-stage/core`：`copyAppFiles` + npm/pnpm + `node/` 包装脚本（一律转调 `DSH_RUNTIME_NODE`）。
4. `cargo build --release` 后 `cargo packager --config Packager.toml -f <formats>`（在 `shell-rs/` 里跑，配置里的相对路径都相对它）。

支持的开关只有一个：`--skip-build`（跳过 `cargo build`，复用已有的 `target/release/dsh-x`）。`release/packager-key.pem` 存在时会追加 `-k` 做 minisign 签名（外壳自更新的前提，目前更新器还没接）。

格式按平台分支（`scripts/pack-rs.mjs:160`）：

| 平台 | formats | 产物 |
| --- | --- | --- |
| Windows | `nsis` | `dsh-x_<版本>_x64-setup.exe` |
| macOS | `app,dmg` | `DSH-X_<版本>_aarch64.dmg` / `DSH-X_<版本>_x64.dmg` |
| Linux | `appimage,deb` | `dsh-x_<版本>_x86_64.AppImage` + `dsh-x_<版本>_amd64.deb` |

### 1.2 产物命名规则（别按直觉改名）

cargo-packager 0.11.8 的产物名是它自己拼的，`Packager.toml` 里没有配置项：

- NSIS 用 `name`（`dsh-x`）+ 架构：`dsh-x_0.1.24_x64-setup.exe`（实测，14.8 MB）。
- deb 用 `name` + `amd64`（`x86_64` 被改写成 `amd64`，`deb/mod.rs:450-462`）。
- AppImage 用 `name` + `x86_64`，**不改写**（`appimage/mod.rs:75-122`）：`dsh-x_0.1.24_x86_64.AppImage`。
- dmg 用 **`product_name`**（`DSH-X`）+ 架构（`dmg/mod.rs:32-42`）：`DSH-X_0.1.24_aarch64.dmg`、`DSH-X_0.1.24_x64.dmg`。

`name`（包名）与 `product_name`（显示名）在 `shell-rs/Packager.toml` 里是两个字段，所以 exe / deb / AppImage 小写、dmg 大写 **不是笔误**。README 与落地页必须用这组真实名字，改名只能改 `Packager.toml`。

### 1.3 CI 现状

`.github/workflows/build.yml`（190 行）：

- 触发：`push` tag `v*`、`workflow_dispatch`；`permissions: contents: write`。
- 三个 job：`windows-latest`、`ubuntu-24.04`、`macos`（矩阵 `macos-14` arm64 + `macos-15-intel` x64）。
- 每个 job 都跑 `npm test` → `dtolnay/rust-toolchain@stable` → `Swatinem/rust-cache@v2`（workspaces: `shell-rs`）→ `cargo install cargo-packager --locked` → `node scripts/pack-rs.mjs` → 上传 artifact（`DSH-X-windows` / `DSH-X-linux` / `DSH-X-mac-<arch>`）。
- tag 上额外 `gh release upload --clobber`（**工作流不创建 Release**，Release 不存在时这一步会跳过）。
- Linux job 顺带发跨平台核心 npm 包：`node scripts/pack-core.mjs --publish`，需要 `NPM_TOKEN`，没有就跳过。
- Linux 只装了四个系统库：`libxkbcommon-dev libwayland-dev libdbus-1-dev pkg-config`——这是 `eframe`/`winit` 与 `rfd`（默认 features = `xdg-portal` + `wayland`，无 gtk3）的最小集。
- macOS job 有一段 `if [ -f assets/icon.icns ]` 守卫（历史遗留：`assets/icon.icns` 早已入库，守卫现在总是通过）。

### 1.4 本机能力（截至 2026-11-01）

| 能力 | 状态 |
| --- | --- |
| Windows NSIS | 全链路实测通过（打包 → 静默安装 → 冒烟 → 卸载） |
| Docker Desktop | 已装，但 daemon 默认不随开机启动，需手动拉起；镜像拉取慢 |
| WSL Ubuntu 24.04 | 存在，但 `sudo` 需要密码，装不了依赖 |
| macOS | 没有机器；dmg 无法本机产出 |
| `gh` CLI | 已登录 `duqingyude`，可手工补传资产 |

### 1.5 版本与身份（两套版本，别混）

- 核心版本：根 `package.json` 的 `version`（当前 `0.1.24`），决定核心 npm 包 `@duqingyude/dsh-x-core`。
- 外壳版本：`shell-rs/Cargo.toml` 的 `version`，必须同时写进 `shell-rs/Packager.toml` 的 `version`（`pack-rs.mjs` 会校验）。只改核心不 bump 外壳版本，安装包文件名就与上次相同。
- 发布身份：`fork.json` 的 `appId`（`io.github.duqingyude.dsh-x`）、`productName`（`DSH-X`）、`updateOwner`/`updateRepo`、`corePackage`。`Packager.toml` 的 `identifier` 必须等于 `appId`。

## 2 为什么不能一条命令出三端

三个硬约束，缺一不可：

1. **cargo-packager 不交叉编译**。`cargo packager --help` 对 `--target` 的说明是「Target triple to use for detecting your app binaries」——它只去 `target/<triple>/release/` 找现成的二进制。想交叉编译得自己先 `cargo build --target …`（还要目标平台的链接器与系统库），那不是 cargo-packager 的职责。
2. **dmg 只能 macOS 出**。cargo-packager 的 dmg 后端调 `hdiutil`，Windows / Linux 上没有。
3. **NSIS 与 AppImage 各自依赖宿主工具链**。NSIS 要 `makensis`（cargo-packager 会自动下载到 `%LOCALAPPDATA%\.cargo-packager\NSIS`），AppImage 要在 Linux 上拼 `AppDir`。Linux 上出现 Windows 路径或反过来都不成立。

所以「一条命令」只可能是「一条命令触发三个平台的机器」——那就是 CI，而不是本机脚本。

## 3 候选方案

### A. 全 CI（推荐，主通道）

推 tag / 手动 `workflow_dispatch` → 三平台 job 并行 → artifact + Release 资产。

- 优点：唯一的「三端一次出齐」路径；身份与版本由仓库里的文件决定，可复现；不依赖开发者装三套工具链；Windows 产物还能顺带在 GitHub 的干净环境里验证。
- 缺点：单次约 10–20 分钟；首次要真跑一遍才知道 mac/Linux 的依赖是否齐；调试靠日志，看不到本地环境。

### B. 本机分平台（推荐，验证通道）

Windows 直接跑；Linux 用 Docker（`ubuntu:24.04` + rustup + node 22）；macOS 找真机或借一台。

- 优点：改一行 Rust 就能马上出包验证，排错快。
- 缺点：每个平台都要自己搭环境；Docker 里跑 `cargo install cargo-packager` 要几分钟，镜像拉取也慢；macOS 没机器就是没有。

### C. 交叉编译（否决）

在 Windows 上 `cargo build --target x86_64-unknown-linux-gnu`，再喂给 cargo-packager。

- 否决理由：`eframe`/`winit`/`rfd` 都链系统库（X11/Wayland/dbus），交叉编译要先准备 sysroot 与交叉链接器；dmg 仍然出不来（`hdiutil` 是 macOS 私有）。投入产出比最差。

### D. 自建 runner（暂不做）

在自有机器上跑 self-hosted runner，省掉 GitHub 的排队。

- 暂不做：当前发布频率低，排队成本小于维护成本；等 CI 真的成为瓶颈再上。

### E. 只发源码，让用户自己编译（否决）

- 否决理由：安装包是产品形态的一部分（快捷方式、卸载项、图标、托盘、双语安装界面），发源码等于把这一层丢掉。

### 对比

| 维度 | A 全 CI | B 本机分平台 | C 交叉编译 | D 自建 runner | E 只发源码 |
| --- | --- | --- | --- | --- | --- |
| 三端一次出齐 | 是 | 否（要三台机器） | 部分（无 dmg） | 是 | — |
| 可复现 | 高 | 中（看本机环境） | 低 | 高 | 低 |
| 排错速度 | 慢（看日志） | 快 | 中 | 快 | — |
| 额外成本 | 无 | 装三套工具链 | 高（sysroot） | 维护机器 | 用户流失 |
| 结论 | **主通道** | **验证通道** | 否决 | 暂不做 | 否决 |

## 4 推荐方案

**A 为主、B 为验证，两者共用同一份入口 `scripts/pack-rs.mjs`**，不新增第二套打包脚本（否则两个入口迟早漂移）。

分工：

- 日常开发：只跑 `npm test` 与 `cargo test`，不出安装包。
- 改外壳（Rust / 面板 / 托盘 / 桥 / 图标 / 安装模板）：本机 Windows 跑 `npm run dist`，装一次、冒烟一次。
- 发版：先本地把 Windows 包验过，再打 tag 推上去，由 CI 出三端；mac/Linux 的包从 artifact 或 Release 取。
- 没有 mac 机器时的兜底：CI 的 mac job 是唯一出口；出了问题时只能靠日志 + 在 CI 里临时加 `-x` 调试。

边界（明确不做的事）：

- 不追求「本机一条命令出三端」。
- 不在 CI 里替用户签名（Windows 无 Authenticode、mac 是 ad-hoc）。
- 不让打包脚本改 `fork.json` / 版本号（版本由人显式 bump）。

## 5 三端命令手册

### 5.1 Windows（本机，已验证）

前置：Node 22 或 24、Rust 工具链、`cargo install cargo-packager --locked`。

```powershell
npm run dist                 # = node scripts/pack-rs.mjs
npm run dist -- --skip-build # 只重打（复用 target/release/dsh-x）
```

产物：`release/packager/dsh-x_<版本>_x64-setup.exe`。验证：

```powershell
Start-Process release/packager/dsh-x_<版本>_x64-setup.exe -ArgumentList '/S','/D=C:\Temp\dshx-check' -Wait
# 装完的布局：dsh-x.exe + core/ + uninstall.exe；HKCU 卸载项 DisplayName=DSH-X
```

注意：`/D=` 后面**不要加引号**（NSIS 的 `/D` 只吃裸路径）；重打前先退出正在运行的外壳，否则 `cargo build` 会报 `failed to remove file …dsh-x.exe`（os error 5）。

### 5.2 Linux（Docker，推荐姿势）

宿主是 Windows 时，用本机已有的 `ubuntu:24.04` 镜像，把仓库挂进去；**一定要用容器里的 `/bin/bash -lc`**，否则 `bash` 会解析到宿主的包装器（实测会报 `bash: C:/…/@dsh-ops/bash-win32-x64/build.sh: No such file or directory`，exit 127）。

```bash
docker run --rm -v "$PWD":/work -w /work ubuntu:24.04 /bin/bash -lc '
  set -e
  apt-get update
  apt-get install -y --no-install-recommends \
    build-essential curl ca-certificates file git pkg-config \
    libxkbcommon-dev libwayland-dev libdbus-1-dev libssl-dev
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
  NODE=22.19.0
  curl -fsSLo /tmp/node.tar.xz https://nodejs.org/dist/v$NODE/node-v$NODE-linux-x64.tar.xz
  tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1
  . "$HOME/.cargo/env"
  cargo install cargo-packager --locked
  node scripts/pack-rs.mjs
'
```

产物在宿主的 `release/packager/`：`dsh-x_<版本>_x86_64.AppImage`、`dsh-x_<版本>_amd64.deb`。

加速技巧：

- 先 `docker pull ubuntu:24.04` 预拉镜像（第一次拉 `rust:1-bookworm` 曾卡在 `Pulling fs layer` 数分钟）。
- 把 `~/.cargo` 与 `target/` 挂成命名卷，避免每次重装 cargo-packager 与重编依赖。
- 只重打不重编：容器里已经有 `target/release/dsh-x` 时加 `--skip-build`。

Debian/Ubuntu 真机上也一样，把 Docker 那一层去掉即可（需要 root 装上面那四个 `-dev` 包）。

### 5.3 macOS（只能在 mac 上）

前置：Xcode Command Line Tools（`xcode-select --install`）、Rust 工具链、Node 22。

```bash
npm run dist        # cargo build --release + cargo packager -f app,dmg
```

产物：`release/packager/DSH-X_<版本>_aarch64.dmg`（Apple Silicon）、`DSH-X_<版本>_x64.dmg`（Intel）。

- 用 `signing_identity = "-"` 做 ad-hoc 签名：不签名时 Gatekeeper 在 arm64 上会判「已损坏」，用户打不开。
- 未公证（notarize），首次打开要右键 → 打开，或 `xattr -dr com.apple.quarantine /Applications/DSH-X.app`。
- `assets/icon.icns` 已入库（`scripts/make-icons.py` 用 Pillow 直接写 ICNS，不依赖 `iconutil`），dmg 里的 .app 直接用这张图标。

### 5.4 产物校验清单（每个平台都跑一遍）

1. 文件名与本文 §1.2 一致（大小写、`x86_64` / `amd64` 的不同）。
2. 体积无异常（Windows 当前约 14.8 MB；Linux AppImage 量级相近；dmg 更大属正常）。
3. 安装后布局：可执行文件 + `core/`（含 `server.js`、`host-remote.js`、`fork.json`、`node/`）。
4. 启动后 `curl http://127.0.0.1:<端口>/api/state` 返回 200；`/api/self` 的 `shell.current` 等于外壳版本。
5. exe / .app / AppImage 的图标是看板娘（不是通用空白窗格）；托盘图标存在。
6. 卸干净：Windows `uninstall.exe /S` 后目录与 HKCU 卸载项都没了；deb 用 `apt remove dsh-x`；dmg 直接删 .app。
7. `sha256sum` 记进发版说明/Release（发布页每个资产旁要能看到）。

## 6 失败模式与排错

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `error: failed to remove file …\shell-rs\target\release\dsh-x.exe` / `os error 5` | 外壳正在运行，占用 exe | 先托盘退出或 `taskkill /F /IM dsh-x.exe`，再重打 |
| `failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine` | Docker Desktop 没启动 | `Start-Process 'C:\Program Files\Docker\Docker\Docker Desktop.exe'`，等约 15 秒 |
| 容器里 `error: linker cc not found` / `libxkbcommon` 缺失 | 漏装 `build-essential` / 四个 `-dev` 包 | 按 §5.2 的 apt 行补齐 |
| 容器里 `bash: C:/…/@dsh-ops/bash-win32-x64/build.sh: No such file`（exit 127） | 宿主的 `bash` 包装器截走了命令 | 改成 `docker run … /bin/bash -lc '…'` |
| `Packager.toml 的 version 与 Cargo.toml 不一致` | 只 bump 了一处 | 两处一起改；`identifier` 也要跟 `fork.json.appId` 对齐 |
| `Packager.toml 的 nsis template … 不存在` | 模板被改名/删了 | 安装包会退回 `%LOCALAPPDATA%\DSH-X`，先把模板放回去 |
| mac 上 `hdiutil: create failed` | 磁盘空间不足，或同名卷已挂载 | 换 `out_dir` / `hdiutil detach` 已挂载的卷 |
| tag 上 `gh release upload` 报 release 不存在 | 工作流不建 Release | 先在网页上建好 Release，再重跑 job |
| 安装界面是英文（中文系统上） | `languages` 少了 SimpChinese，或 `DSHX_PICK_LANGUAGE` 宏丢了 | 见 AGENTS.md「外壳更新与打包」最后一条，`test/pack-nsis.test.mjs` 盯着 |
| 装完启动没反应、没有窗口 | 面板默认隐藏，核心起来后自动开浏览器；也可能核心没找到系统 Node | 看 `%APPDATA%\DSH\manager.log`；面板从托盘图标打开 |

## 7 验收标准

「三端能出包」必须同时满足：

1. Windows 本机 `npm run dist` 出包、静默安装、`/api/state` 200、卸载干净（已满足）。
2. Linux：Docker 里出 AppImage + deb，deb `dpkg -i` 能装、菜单项存在、`apt remove` 干净。
3. macOS：arm64 与 x64 各出 dmg，双击能过 Gatekeeper（右键打开），托盘可用。
4. CI：`workflow_dispatch` 空跑一次三 job 全绿，artifact 里能下到对应产物。
5. 三端产物名与 README / 落地页 `docs/index.html` 的正则一致（`dsh-x_` 与 `DSH-X_` 两类前缀都认）。
6. 全量 `npm test` 与 `cargo test` 仍绿。

## 8 风险与非目标

风险：

- **CI 从未真跑过三端**：`build.yml` 是本机验证 Windows 之后写的，Linux/mac 的依赖是否完整只能靠实跑确认。首次发版前应先 `workflow_dispatch` 空跑。
- **mac 产物名靠源码推导**（`product_name` 前缀），没有实机确认过；第一次出包时如果名字不同，改 `docs/index.html` 的正则与 README 即可（`test/` 没有断言文件名，改起来无副作用）。
- **mac 未公证**：即便 ad-hoc 签名，用户仍可能被 Gatekeeper 拦，需要文档引导。
- **Windows 无签名**：SmartScreen 提示「未知发布者」，只能靠文档解释。
- **Docker 镜像拉取慢**：本机网络下第一次拉 `rust:1-bookworm` 卡过数分钟，务必先拉 `ubuntu:24.04` 或配镜像加速。

非目标：

- 不做交叉编译工具链。
- 不自建 runner。
- 不在 CI 里做代码签名 / 公证（等拿到证书再说）。
- 不把 mac 外壳自更新接进来（`updater.rs` 仍只返回 `disabled`）。

## 9 待办与已知缺口

1. 三个 CI job 从未在 GitHub 上真跑：发版前先 `workflow_dispatch`。
2. `build.yml` 的 mac job 还留着 `if [ -f assets/icon.icns ]` 守卫（`assets/icon.icns` 已入库），可以删掉，让缺失时直接报错而不是整段跳过。
3. 旧 Electron 版没有自动卸载钩子（cargo-packager 的 NSIS 无安装钩子），README 里要求用户先手动卸载。
4. 外壳自更新未接（`updater.rs` 只返回 `status: "disabled"`）；签名私钥 `release/packager-key.pem` 的流程也还没定。
5. Windows 安装包无 Authenticode 签名。
6. 本机 Docker 构建建议固化成一个脚本（`scripts/pack-linux-docker.sh`），目前还是手写长命令——本次方案刻意不新增脚本，等 CI 稳定后再补。

## 附 本机实测记录（2026-11-01）

- Windows：`node scripts/pack-rs.mjs` 成功，`release/packager/dsh-x_0.1.24_x64-setup.exe` 14,843,122 字节；静默装到 `%TEMP%\dshx-nsis-verify`，`/api/state` 200、`/api/self` 的 `shell.current=0.1.24`、`POST /api/desktop/notify` 返回 `{"ok":true}`；`uninstall.exe /S` 后目录与 HKCU 卸载项都清掉。
- Windows 安装界面：中文系统默认简体中文（欢迎页「欢迎使用 DSH-X 安装程序」），英文系统（探针强制 `$LANGUAGE 1033`）为「Welcome to DSH-X Setup」。
- Linux 尝试一：`docker run rust:1-bookworm` 卡在镜像层拉取，放弃。
- Linux 尝试二：`ubuntu:24.04` + rustup + node 22 tarball 的脚本形式，第一次因为 `bash` 解析到宿主包装器 exit 127；改用 `/bin/bash -lc` 后按用户要求先写方案文档，未跑完（后续按 §5.2 执行）。
- macOS：无机器，未尝试。
