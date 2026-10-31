// 原生外壳：单实例 → 读设置 → 找 Node 与核心目录 → 起本机桥 → 起核心 → 开面板。
// 这里刻意不做任何业务：起停、端口、更新、插件全在核心（start.js + server.js）里，
// 外壳只提供窗口、托盘、系统能力和一条让核心回调自己的 HTTP 桥（§6 的契约）。

mod bridge;
mod notify;
mod panel;
mod prefs;
mod supervisor;
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
        // 已经有一个在跑：把它叫到前面去。桥地址没落盘，所以只能重新拉起一次面板——
        // 新进程会因为锁直接退出，用户看到的是原来那个窗口（Windows 上会闪一下）。
        eprintln!("DSH-X 已经在运行");
        return;
    }

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

    // 先起桥：核心一启动就会回调（desktop.info / shellUpdate.state），桥没起来它会当成外壳不在。
    let token = supervisor::make_token();
    let shell_exe = std::env::current_exe()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let bridge_port = match bridge::start(BridgeCtx {
        token: token.clone(),
        status: status.clone(),
        prefs: prefs_shared.clone(),
        events,
        shell_version: env!("CARGO_PKG_VERSION").to_string(),
        node_version: node_version.clone(),
    }) {
        Ok(port) => port,
        Err(error) => {
            // 桥起不来核心也能跑，只是插件通知、托盘联动这些会降级：不因此拦住启动。
            eprintln!("外壳桥起不来：{error}");
            0
        }
    };
    let shell_url = if bridge_port > 0 {
        format!("http://127.0.0.1:{bridge_port}")
    } else {
        String::new()
    };

    let supervisor = match (&node, &core) {
        (Some(node), Some(core)) if bridge_port > 0 => {
            let supervisor = Supervisor::new(
                node.clone(),
                core.clone(),
                ShellEnv {
                    url: shell_url,
                    token,
                    exe: shell_exe,
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
        supervisor: supervisor.clone(),
        node: node.clone(),
        node_version: node_version.clone(),
        core: core.clone(),
        prefs: prefs_shared.clone(),
        relaunch: relaunch_flag.clone(),
        tray: None,
        quitting: false,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(env!("DSH_PRODUCT_NAME"))
            .with_inner_size([640.0, 520.0])
            .with_min_inner_size([480.0, 360.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        env!("DSH_PRODUCT_NAME"),
        options,
        Box::new(move |cc| {
            // 托盘只能在事件循环跑起来之后装（macOS 硬性要求，Windows 上也要同线程的消息泵）。
            let mut panel = panel;
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

/// 这些常量只被面板/桥用到，抽出来是为了让 clippy 不报 dead_code（构建期由 build.rs 写入）。
#[allow(dead_code)]
fn shell_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
