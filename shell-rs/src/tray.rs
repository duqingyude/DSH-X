// 托盘：常驻的操作入口（打开面板 / 管理页、起停核心、退出）。
// 旧 Electron 外壳里这套逻辑在 shell/main.mjs 的托盘段；菜单项刻意保持一致，
// 免得用户换外壳后找不到「退出」在哪。
//
// 平台注意：tray-icon 要求「同线程有事件循环」——Windows 靠 winit 的消息泵，
// macOS 还要求创建发生在主线程且循环已经在跑，所以只能在 eframe 的 AppCreator 里装，
// 不能在 main() 里提前 new（那时事件循环还没起来，macOS 会出怪状态）。

use std::sync::Mutex;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const ID_SHOW: &str = "dsh-x.tray.show";
const ID_OPEN: &str = "dsh-x.tray.open";
const ID_START: &str = "dsh-x.tray.start";
const ID_STOP: &str = "dsh-x.tray.stop";
const ID_QUIT: &str = "dsh-x.tray.quit";

/// 托盘菜单点了什么。面板每帧取走一批，和桥事件走同一条处理路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    OpenManager,
    Start,
    Stop,
    Quit,
}

// 菜单回调由 muda 在自己的线程上调用，事件只能先落进队列，等 egui 的 logic 来取；
// 队列是进程级的（一个外壳只有一份托盘），所以用静态量比传引用简单。
static PENDING: Mutex<Vec<Action>> = Mutex::new(Vec::new());

fn push(action: Action) {
    if let Ok(mut queue) = PENDING.lock() {
        queue.push(action);
    }
}

/// 取走这一轮攒下的菜单动作。调用方负责真正执行。
pub fn take() -> Vec<Action> {
    match PENDING.lock() {
        Ok(mut queue) => std::mem::take(&mut *queue),
        Err(_) => Vec::new(),
    }
}

/// 托盘图标活着的时候句柄必须一直握着：TrayIcon 一旦被 drop，图标就从托盘消失。
pub struct Tray {
    _icon: TrayIcon,
}

/// 装托盘。返回 None 表示这个环境装不了（比如 Linux 没有 AppIndicator），
/// 面板照常可用——托盘是便利入口，不该成为启动的硬依赖。
pub fn install(ctx: egui::Context) -> Option<Tray> {
    let menu = Menu::new();
    let show = MenuItem::with_id(ID_SHOW, "打开面板", true, None);
    let open = MenuItem::with_id(ID_OPEN, "打开管理页", true, None);
    let start = MenuItem::with_id(ID_START, "启动核心", true, None);
    let stop = MenuItem::with_id(ID_STOP, "停止核心", true, None);
    let quit = MenuItem::with_id(ID_QUIT, "退出 DSH-X", true, None);
    let sep_top = PredefinedMenuItem::separator();
    let sep_bottom = PredefinedMenuItem::separator();
    menu.append_items(&[
        &show,
        &open,
        &sep_top,
        &start,
        &stop,
        &sep_bottom,
        &quit,
    ])
    .ok()?;

    // 托盘事件不是 egui 的输入，不会自动触发重绘；这里顺手 request_repaint，
    // 窗口藏着的时候 eframe 会因此跑一次 logic（见 App::logic 的文档）。
    let menu_ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let action = match event.id.0.as_str() {
            ID_SHOW => Action::Show,
            ID_OPEN => Action::OpenManager,
            ID_START => Action::Start,
            ID_STOP => Action::Stop,
            ID_QUIT => Action::Quit,
            _ => return,
        };
        push(action);
        menu_ctx.request_repaint();
    }));

    // 左键点图标直接叫出面板（Windows 上的肌肉记忆），右键才出菜单。
    let click_ctx = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            push(Action::Show);
            click_ctx.request_repaint();
        }
    }));

    let icon = load_icon()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("DSH-X")
        .with_icon(icon)
        .build()
        .ok()?;
    Some(Tray { _icon: tray })
}

/// 托盘图标用安装包里那份 64px 的圆角图（assets/tray.png）缩出来。
/// 换图标只改 scripts/make-icons.py 的 master（assets/icon.png）再跑一遍脚本。
fn load_icon() -> Option<Icon> {
    // 用 tray.png 而不是 icon.png：后者是 512 的方形 master（不透明四角），
    // 托盘 / 窗口图标要的是切好圆角、带透明边的那一版（scripts/make-icons.py 生成）。
    let bytes = include_bytes!("../../assets/tray.png");
    let image = image::load_from_memory(bytes).ok()?;
    // 32 是 Windows 托盘在 100% 缩放下要的尺寸；系统会自己再缩放到高 DPI 档。
    let resized = image
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let (width, height) = resized.dimensions();
    Icon::from_rgba(resized.into_raw(), width, height).ok()
}
