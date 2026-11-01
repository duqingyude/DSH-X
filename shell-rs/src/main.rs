// 打包后是 GUI 程序：没有这行，双击安装出来的 exe 会先弹一个控制台黑框。
// 调试构建（cargo run / cargo build）保留控制台，方便直接看日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 原生外壳：单实例 → 读设置 → 找 Node 与核心目录 → 起本机通道 → 起核心 → 开浏览器。
// 这里刻意不做任何业务：起停、端口、更新、插件全在核心（start.js + server.js）里，
// 外壳只提供窗口、托盘、系统能力和一条让核心回调自己的本机通道（§6 的契约）。

mod bridge;
mod identity;
mod model;
mod notify;
mod panel;
mod power;
mod prefs;
mod supervisor;
mod theme;
mod tray;
mod updater;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui;

use bridge::BridgeCtx;
use panel::Panel;
use prefs::Prefs;
use supervisor::{ShellEnv, Status, Supervisor};

fn main() {
    // 单实例：同一个用户下只留一个外壳，否则两个桥、两份托盘、两份核心进程会互相抢端口。
    let instance = match single_instance::SingleInstance::new(env!("DSH_APP_ID")) {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("单实例锁创建失败：{error}");
            return;
        }
    };
    if !instance.is_single() {
        // 已经有了一个在跑：面板默认是藏着的，这时候要是什么都不做，用户再点一次图标会
        // 以为「双击没反应」。所以把上次记下的管理页地址拿到浏览器里打开。
        supervisor::open_running_manager();
        return;
    }

    let shell_exe = std::env::current_exe()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    // 先把应用身份定下来：通知署名与图标都看它（见 identity.rs），而且必须在任何通知之前。
    #[cfg(windows)]
    identity::claim(env!("DSH_APP_ID"), env!("DSH_PRODUCT_NAME"), &shell_exe);

    let prefs = Prefs::load();
    let node = supervisor::find_node(&prefs);
    let node_version = node
        .as_deref()
        .and_then(supervisor::node_version)
        .unwrap_or_default();
    let core = supervisor::core_dir();
    if core.is_none() {
        eprintln!("找不到核心目录（start.js）");
    }

    let status = Arc::new(Mutex::new(Status::default()));
    let prefs_shared = Arc::new(Mutex::new(prefs));
    let (events, event_rx) = mpsc::channel();
    // 面板自己起的线程（动作结果、设置回执）也往这条通道塞事件，所以要留一份发送端。
    let events_tx = events.clone();

    // 先起通道：核心一启动就会回调（desktop.info / shellUpdate.state），通道没起来它会当成外壳不在。
    let core_pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
    // 反向调用句柄：桥与面板共用一条连接，谁先说话谁建连（见 bridge.rs 的 Link）。
    let link = bridge::Link::new();
    // 界面上下文要等窗口建出来才有，先留个槽位给桥（见 BridgeCtx::repaint）。
    let repaint = Arc::new(std::sync::Mutex::new(None));
    let shell_ipc = match bridge::start(BridgeCtx {
        status: status.clone(),
        prefs: prefs_shared.clone(),
        events,
        shell_version: env!("CARGO_PKG_VERSION").to_string(),
        node_version: node_version.clone(),
        core_pid: core_pid.clone(),
        link: link.clone(),
        has_model: std::sync::atomic::AtomicBool::new(false),
        repaint: repaint.clone(),
    }) {
        Ok(name) => name,
        Err(error) => {
            // 通道起不来核心也能跑，只是插件通知、托盘联动这些会降级：不因此拦住启动。
            eprintln!("外壳通道起不来：{error}");
            String::new()
        }
    };

    let supervisor = match (&node, &core) {
        (Some(node), Some(core)) if !shell_ipc.is_empty() => {
            let supervisor = Supervisor::new(
                node.clone(),
                core.clone(),
                ShellEnv {
                    ipc: shell_ipc,
                    exe: shell_exe,
                    core_pid: core_pid.clone(),
                },
                status.clone(),
            );
            supervisor.start();
            Some(supervisor)
        }
        _ => None,
    };

    let relaunch_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let panel = Panel {
        status: status.clone(),
        events: event_rx,
        events_tx,
        supervisor: supervisor.clone(),
        node: node.clone(),
        node_version: node_version.clone(),
        core: core.clone(),
        prefs: prefs_shared.clone(),
        relaunch: relaunch_flag.clone(),
        tray: None,
        quitting: false,
        opened_url: String::new(),
        failure_shown: false,
        wants_window: false,
        link,
        model: None,
        notice: None,
        drawer: false,
        prefs_stamp: None,
        prefs_checked: None,
        keep_awake_applied: false,
        #[cfg(windows)]
        cloak: None,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(env!("DSH_PRODUCT_NAME"))
            // 760 宽是「标题 + 三四个按钮 + 状态」一行放得下的尺寸；设置抽屉再占 300。
            .with_inner_size([760.0, 600.0])
            .with_min_inner_size([560.0, 420.0])
            // 默认不弹面板：启动器对用户来说就是「开管理页」，窗口藏在托盘后面。
            // 托盘「打开面板」或核心起不来时才显示（见 panel.rs 的 auto_open）。
            // 隐藏状态的窗口不进任务栏，用户看不到任何多余的东西。
            .with_visible(false)
            // 面板 / 任务栏图标：exe 里的图标只在资源管理器与快捷方式上看得到，
            // 窗口自己的图标要从这里给，否则 Windows 会画一枚通用的空白窗格。
            .with_icon(window_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        env!("DSH_PRODUCT_NAME"),
        options,
        Box::new(move |cc| {
            // 托盘只能在事件循环跑起来之后装（macOS 硬性要求，Windows 上也要同线程的消息泵）。
            let mut panel = panel;
            // 桥在窗口建起来之前就起好了，这里把界面上下文补进去（隐藏时靠它叫醒重绘）。
            if let Ok(mut slot) = repaint.lock() {
                *slot = Some(cc.egui_ctx.clone());
            }
            // 窗口一建好就先隐身：eframe 第一帧画完会强行 set_visible(true)，只靠
            // Visible(false) 会闪一下（见 panel.rs 的 wants_window 注释）。
            // 高 DPI 的坑：窗口建出来的那一刻系统报的缩放常常还是 1.0，之后才切成 1.5。
            // 结果窗口停在 760x600 物理像素，而 egui 按 1.5 倍画 1140x900，右边和下边
            // 就被裁掉了（面板上的按钮、页脚都看不见）。这里按真实缩放再要一次尺寸。
            if let Some(window) = cc.winit_window() {
                let scale = window.scale_factor();
                if scale > 1.0 {
                    let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
                        (760.0 * scale) as u32,
                        (600.0 * scale) as u32,
                    ));
                }
                #[cfg(windows)]
                {
                    let cloaked = panel::set_cloaked(window, true);
                    panel.cloak = Some((window.clone(), cloaked));
                }
            }
            panel.tray = tray::install(cc.egui_ctx.clone());
            if panel.tray.is_none() {
                eprintln!("托盘装不上：这台机器可能没有托盘支持，面板照常可用");
            }
            Ok(Box::new(panel))
        }),
    );

    if let Some(supervisor) = &supervisor {
        supervisor.stop();
    }
    if let Err(error) = result {
        eprintln!("面板起不来：{error}");
    }
    if relaunch_flag.load(std::sync::atomic::Ordering::SeqCst) {
        // 和 app.relaunch() 的语义一致：本进程已经退出，新进程接单实例锁。
        let exe = std::env::current_exe();
        if let Ok(exe) = exe {
            let _ = std::process::Command::new(exe).spawn();
        }
    }
}

/// 面板窗口 / 任务栏图标：和托盘同一份 64px 圆角图（assets/tray.png，由 scripts/make-icons.py
/// 从 master 切出来），不额外带一份素材。窗口图标不比托盘图标大，64 已够高 DPI 用。
fn window_icon() -> Arc<egui::IconData> {
    const BYTES: &[u8] = include_bytes!("../../assets/tray.png");
    let image = image::load_from_memory(BYTES)
        .expect("assets/tray.png 读不出来（打包时会带上，源码运行从仓库根取）")
        .resize(64, 64, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let (width, height) = image.dimensions();
    Arc::new(egui::IconData { rgba: image.into_raw(), width, height })
}

/// 这些常量只被面板/桥用到，抽出来是为了让 clippy 不报 dead_code（构建期由 build.rs 写入）。
#[allow(dead_code)]
fn shell_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
