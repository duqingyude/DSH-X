<p align="center">
  <img src="docs/hero-en.png" alt="DSH-X" width="880" />
</p>

<p align="center">
  A third-party desktop launcher for <a href="https://github.com/deepseek-ai/deepseek-harness">DeepSeek Harness</a> —<br />
  launch entries and multiple instances, version, plugin, modpack, MCP and skill management.
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
  <samp><a href="https://duqingyude.github.io/DSH-X/">Website</a> · <a href="https://github.com/duqingyude/DSH-X/releases/latest">Download</a> · <a href="docs/releases">Changelog</a> · <a href="CONTRIBUTING.en.md">Contributing</a> · <strong>English</strong> · <a href="README.md">中文</a></samp>
</p>

> [!IMPORTANT]
> **DSH-X starts DeepSeek Harness's own web page.**
> It only handles version installation, launching and plugin management and never modifies or rewrites DSH's web UI. DSH-X is a community project, not an official DeepSeek product.

## Features

- 🚀 **Launch entries and multiple instances** — choose a home directory, profile, version and port per entry; start entries concurrently and add entries while DSH is running.
- 📦 **Version management** — a dedicated manager for detecting new releases, manually installing, removing and assigning versions to launch entries.
- 🧩 **Plugins and packs** — install, toggle and update plugins by environment and configure default indexes for the official custom plugin market; browse, search and sort the masonry-style community market, or import and export packs.
- 🛠️ **MCP and skills** — configure MCP servers and import, manage and toggle skills.
- 🪟 **Two opening modes** — a system browser tab, or a standalone browser window without an address bar.
- 🎨 **Appearance** — light / dark themes, transparency, opening and launch-entry animations, and separate background and Big Fish switches.
- 🔄 **Updates and recovery** — self-update, profile recovery and configurable old-version retention; each launch entry's current and previous versions are kept additionally.
- ⚙️ **Desktop system switches** — hardware acceleration, automatic update download (install manually), task notifications and sound, hide to tray on close, and keep-awake.
- 💻 **Windows / macOS / Linux** — installers for all three platforms, tray / menu bar operation and launch at login, plus bundled npm / pnpm wrappers.

## Getting started

Download the installer for your platform from [Releases](https://github.com/duqingyude/DSH-X/releases/latest):

| Platform | Requirements | Installer |
| --- | --- | --- |
| Windows | Windows 10 or later, x64 | `dsh-x_<version>_x64-setup.exe` (NSIS, per-user) |
| macOS | macOS 13 or later | `dsh-x_<version>_aarch64.dmg` (Apple Silicon) / `dsh-x_<version>_x64.dmg` (Intel) |
| Linux | x64; AppImage needs FUSE | `dsh-x_<version>_amd64.AppImage` / `dsh-x_<version>_amd64.deb` |

### Windows

Download `dsh-x_<version>_x64-setup.exe`, install it and open **DSH-X** from the desktop. The installer has no Authenticode signature, so SmartScreen may show an "unknown publisher" prompt — see [Signing and antivirus false positives](#signing-and-antivirus-false-positives).

### macOS

Open the dmg and drag **DSH-X** into Applications. The package is ad-hoc signed and not notarized, so the first launch may require manual approval through macOS security prompts; if macOS says the app is damaged or cannot verify the developer, right-click Open, or run:

```sh
xattr -dr com.apple.quarantine /Applications/DSH-X.app
```

Shell auto-update is currently unavailable, while core updates still come from npm. Uninstall the old Electron build first; the two cannot coexist.

### Linux

Download `dsh-x_<version>_amd64.AppImage`, `chmod +x` it and run it. If FUSE is unavailable, use `./dsh-x_<version>_amd64.AppImage --appimage-extract-and-run`.

On Debian / Ubuntu you can also install the deb: it lands in `/opt/DSH-X`, adds a menu entry and uninstalls with `apt remove dsh-x`, but **the deb build has no shell auto-update** — grab the new installer from the releases page:

```sh
sudo apt install ./dsh-x_<version>_amd64.deb
```

Settings live in `~/.config/DSH`; core updates use npm for both packages.

### Verifying a download

The releases page lists a sha256 next to every file — compare it locally:

```sh
certutil -hashfile dsh-x_<version>_x64-setup.exe SHA256 # Windows
shasum -a 256 dsh-x_<version>_aarch64.dmg              # macOS
sha256sum dsh-x_<version>_amd64.AppImage               # Linux
```

## Usage

- Add or edit a launch entry on the home page, select its environment and version, then start it. Open **Version management** from the launch-entry editor to manually install or remove versions.
- Settings → General → **How dsh opens** offers the system browser or a browser app window (no address bar). The old "desktop window" mode has no implementation any more and falls back to the system browser.
- The manager defaults to port `3780` and tries the next port if occupied.
- To reach dsh from another device: Settings → Advanced → **Web binding** → LAN, applied on the next start.

## Screenshots

**Multiple launch entries · Dark theme**

<p align="center">
  <img src="docs/screenshot-launches.png" alt="DSH-X v0.1.16 with multiple launch entries" width="820" />
</p>

<details>
<summary>Management pages · Dark theme (expand to view)</summary>

**Environments**

<p align="center">
  <img src="docs/screenshot-profiles-dark-en.jpg" alt="DSH-X v0.1.16 dark environments" width="820" />
</p>

**Plugin management**

<p align="center">
  <img src="docs/screenshot-plugins-dark-en.jpg" alt="DSH-X v0.1.16 dark plugin management" width="820" />
</p>

**Modpack market**

<p align="center">
  <img src="docs/screenshot-market-dark-en.jpg" alt="DSH-X v0.1.16 dark modpack market" width="820" />
</p>

**General settings**

<p align="center">
  <img src="docs/screenshot-settings-dark-en.jpg" alt="DSH-X v0.1.16 dark general settings" width="820" />
</p>

</details>

<details>
<summary>Management pages · Light theme (expand to view)</summary>

**Launcher home**

<p align="center">
  <img src="docs/screenshot-home-en.jpg" alt="DSH-X launch entries" width="820" />
</p>

**Environments**

<p align="center">
  <img src="docs/screenshot-profiles-en.jpg" alt="DSH-X environments" width="820" />
</p>

**Plugin management**

<p align="center">
  <img src="docs/screenshot-plugins-en.jpg" alt="DSH-X plugins by environment" width="820" />
</p>

**Modpack market**

<p align="center">
  <img src="docs/screenshot-market-en.jpg" alt="DSH-X modpack market" width="820" />
</p>

**General settings**

<p align="center">
  <img src="docs/screenshot-settings-en.jpg" alt="DSH-X settings page" width="820" />
</p>

</details>

## Modpacks

Packs install a group of plugins and configuration into a selected environment (profile). The Plugins page lists environments, identifies installed packs and lets you toggle individual plugins. Packs use the [DSH-PackForge](https://github.com/DSH-PackForge/DSH-PackForge) `.dspack` format (manifest v5, older versions accepted).

- **Import** — the community market, a local file, a direct link or GitHub `owner/repo`. The selected environment is the default target; change it before confirming if needed.
- **Install** — review plugins and file changes before confirming; overwritten files are backed up and failed installs roll back.
- **Export** — package the current environment's dependencies, patch layer and configuration as a `.dspack`, without `node_modules` or credentials.

There are two pack recipes:

- [Recommended pack](packs/dsh-x-recommended/manifest.json) — configuration, memory, sync and Big Fish decoration plugins, all installed from npm (sync via the optional `dsh-x-sync` plugin).
- [Desktop workbench](packs/dsh-x-workbench/README.md) — brings dsh-tauri desktop's sidebar workbench, worktrees, scheduler, archive and notifications to plain dsh, plus ZCode's `agent-browser` and `electron` skills.

Run `node scripts/make-pack.mjs` to build a `.dspack`, then import it as a local file.

## Wallpaper

A companion Wallpaper Engine wallpaper — [DSH-X · 大肥鱼桌面终端](https://steamcommunity.com/sharedfiles/filedetails/?id=3814706575): a DSH-X brand intro, the mascot, local time and an audio spectrum, switching between light and dark palettes by time of day. Sources live in [`wallpaper/`](wallpaper/); run `node scripts/export-wallpaper.mjs` to export the Wallpaper Engine project.

<p align="center">
  <img src="wallpaper/preview.jpg" alt="DSH-X wallpaper" width="820" />
</p>

## Signing and antivirus false positives

The release-manifest signature verifies file integrity and is separate from Windows publisher signing. The current Windows installer has no Authenticode signature, so SmartScreen may show an "unknown publisher" prompt. Report antivirus false positives by submitting the installer to [Microsoft](https://www.microsoft.com/en-us/wdsi/filesubmission) or the relevant antivirus vendor.

## Notes

> [!WARNING]
> **Upstream is moving fast** — the dsh core and its plugin ecosystem change frequently and may include breaking changes; confirm plugin compatibility before upgrading.

> [!NOTE]
> **Security** — dsh can execute local code, and DSH-X only installs and launches it. Use it in trusted environments only.

## Development

Needs Node.js 22.18+ locally (22 and 24 both work) and the Rust toolchain when building the shell. The launcher has no third-party runtime dependencies: `npm start` runs the manager, `npm run server` the server only, `npm test` the tests and `npm run shell` compiles and runs the native shell.

Development, commit and release conventions: [CONTRIBUTING.en.md](CONTRIBUTING.en.md).

### Packaging

```sh
cargo install cargo-packager --locked
npm run dist
```

Artifacts land in `release/packager/`: an NSIS installer on Windows, a dmg on macOS, AppImage and deb on Linux. cargo-packager builds them and ships the core under `core/`; publishing identity (appId, update source, core package) comes from `fork.json`.

Builds also write an SBOM and release manifest for local checking (not uploaded to the release). Check the version and artifacts before publishing:

```sh
node scripts/release-manifest.mjs check-tag v0.1.22
node scripts/release-manifest.mjs verify
```

The GitHub Actions `build` workflow builds Windows / macOS / Linux installers and publishes the npm core package on tags. macOS currently ships dmg only (ad-hoc signing; shell auto-update is disabled).

The release-manifest private key lives in `release/release-key.pem` (git-ignored, **back it up**).

## Contributing

- Security issues: [SECURITY.md](SECURITY.md) — not a public issue.
- What changed in each version: [docs/releases](docs/releases).
- Feedback: **QQ group [993579665](https://qm.qq.com/q/7AD2g70HqS)**
- Plugins group 2 (DeepSeek Harness): [join here](https://qm.qq.com/q/hCjdOJOjSw)

## Related projects

- [deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) — the upstream dsh agent platform
- [DSH-PackForge](https://github.com/DSH-PackForge/DSH-PackForge) — `.dspack` format and pack tooling
- [deepseek-harness-desktop](https://github.com/dsh-tauri/deepseek-harness-desktop) — a Tauri desktop build in the same ecosystem; this repository's pack recipes reference its plugin set

## License

[MIT](LICENSE) © 2026 yyh

