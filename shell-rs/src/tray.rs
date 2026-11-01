// 托盘：常驻的操作入口，并且跟着启动项走（面板模型一变就重建菜单）。
// 旧 Electron 外壳里这套逻辑在 shell/main.mjs 的托盘段；菜单项刻意保持一致，
// 免得用户换外壳后找不到「退出」在哪。
//
// 平台注意：tray-icon 要求「同线程有事件循环」——Windows 靠 winit 的消息泵，
// macOS 还要求创建发生在主线程且循环已经在跑，所以只能在 eframe 的 AppCreator 里装，
// 不能在 main() 里提前 new（那时事件循环还没起来，macOS 会出怪状态）。

use std::collections::HashMap;
use std::sync::Mutex;

use eframe::egui;
use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::model::PanelModel;

const ID_SHOW: &str = "dsh-x.tray.show";
const ID_OPEN: &str = "dsh-x.tray.open";
const ID_START: &str = "dsh-x.tray.start";
const ID_STOP: &str = "dsh-x.tray.stop";
const ID_QUIT: &str = "dsh-x.tray.quit";
/// 启动项子菜单项的 id 前缀；真正的 payload 在 ROSTER 里，id 只保证不撞。
const ID_PRESET: &str = "dsh-x.tray.preset.";

/// 托盘菜单最多列这么多启动项，再多就让用户去面板看（原生菜单太长反而难找）。
const MENU_LIMIT: usize = 8;

/// 启动项子菜单里能做的事，和核心 trayAction 的动作名一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetAction {
    Open,
    Start,
    Stop,
    Restart,
}

impl PresetAction {
    /// 报给核心的动作名（server.js 的 trayAction 只认这四个）。
    pub fn verb(self) -> &'static str {
        match self {
            PresetAction::Open => "open",
            PresetAction::Start => "start",
            PresetAction::Stop => "stop",
            PresetAction::Restart => "restart",
        }
    }
}

/// 托盘菜单点了什么。面板每帧取走一批，和桥事件走同一条处理路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    OpenManager,
    /// 起停外壳拉起的核心（不是启动项实例）。
    StartCore,
    StopCore,
    Quit,
    Preset {
        id: String,
        target_id: String,
        instance_id: String,
        action: PresetAction,
    },
}

// 菜单回调由 muda 在自己的线程上调用，事件只能先落进队列，等 egui 的 logic 来取；
// 队列是进程级的（一个外壳只有一份托盘），所以用静态量比传引用简单。
static PENDING: Mutex<Vec<Action>> = Mutex::new(Vec::new());
/// 菜单项 id → 动作。子菜单每次重建都会换一批 id（带自增序号），这里跟着换。
static ROSTER: Mutex<Option<HashMap<String, Action>>> = Mutex::new(None);

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
    icon: TrayIcon,
    /// 上次建菜单时的模型签名：变了才重建，免得每帧都动原生菜单。
    signature: String,
    /// 菜单项要跟着 Tray 活着：muda 只保留原生句柄，rust 侧对象没人拿着会被释放。
    keep: Vec<Box<dyn IsMenuItem>>,
}

/// 装托盘。返回 None 表示这个环境装不了（比如 Linux 没有 AppIndicator），
/// 面板照常可用——托盘是便利入口，不该成为启动的硬依赖。
pub fn install(ctx: egui::Context) -> Option<Tray> {
    // 菜单事件不是 egui 的输入，不会自动触发重绘；这里顺手 request_repaint，
    // 窗口藏着的时候 eframe 会因此跑一次 logic（见 App::logic 的文档）。
    let menu_ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id.0.as_str();
        let action = match id {
            ID_SHOW => Some(Action::Show),
            ID_OPEN => Some(Action::OpenManager),
            ID_START => Some(Action::StartCore),
            ID_STOP => Some(Action::StopCore),
            ID_QUIT => Some(Action::Quit),
            _ => ROSTER
                .lock()
                .ok()
                .and_then(|roster| roster.as_ref().and_then(|roster| roster.get(id).cloned())),
        };
        let Some(action) = action else { return };
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
    let (menu, keep) = build_menu(None, "zh")?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("DSH-X")
        .with_icon(icon)
        .build()
        .ok()?;
    Some(Tray {
        icon: tray,
        signature: signature_of(None, "zh"),
        keep,
    })
}

impl Tray {
    /// 模型变了就重建菜单。每帧调用没关系：签名没变时只做一次字符串比较。
    pub fn sync(&mut self, model: Option<&PanelModel>, lang: &str) {
        let signature = signature_of(model, lang);
        if signature == self.signature {
            return;
        }
        let Some((menu, keep)) = build_menu(model, lang) else {
            return;
        };
        self.icon.set_menu(Some(Box::new(menu)));
        self.keep = keep;
        self.signature = signature;
    }
}

fn tr<'a>(lang: &str, zh: &'a str, en: &'a str) -> &'a str {
    if lang == "en" { en } else { zh }
}

/// 模型里会影响菜单内容的字段拼成一个签名：只比这个，不深比较整个模型。
fn signature_of(model: Option<&PanelModel>, lang: &str) -> String {
    let mut out = String::from(lang);
    if let Some(model) = model {
        for preset in model.presets.iter().take(MENU_LIMIT) {
            out.push_str(&format!(
                "|{}:{}:{}:{}:{}:{}",
                preset.id, preset.name, preset.status, preset.busy, preset.installed, preset.url
            ));
        }
        out.push_str(&format!("#{}:{}", model.presets.len(), model.running()));
    }
    out
}

/// 每个启动项一个子菜单：打开 / 启动 / 停止 / 重启，按当前状态禁用不可能的操作。
fn build_menu(model: Option<&PanelModel>, lang: &str) -> Option<(Menu, Vec<Box<dyn IsMenuItem>>)> {
    let mut keep: Vec<Box<dyn IsMenuItem>> = Vec::new();
    let mut top: Vec<usize> = Vec::new();
    let mut roster: HashMap<String, Action> = HashMap::new();

    fn add(keep: &mut Vec<Box<dyn IsMenuItem>>, top: &mut Vec<usize>, item: Box<dyn IsMenuItem>) {
        top.push(keep.len());
        keep.push(item);
    }

    add(&mut keep, &mut top, Box::new(MenuItem::with_id(
        ID_SHOW,
        tr(lang, "打开面板", "Show panel"),
        true,
        None,
    )));
    add(&mut keep, &mut top, Box::new(MenuItem::with_id(
        ID_OPEN,
        tr(lang, "打开管理页", "Open console"),
        true,
        None,
    )));

    if let Some(model) = model {
        if !model.presets.is_empty() {
            add(&mut keep, &mut top, Box::new(PredefinedMenuItem::separator()));
        }
        for (index, preset) in model.presets.iter().take(MENU_LIMIT).enumerate() {
            let running = preset.is_running();
            // ● 运行中 / ○ 没在跑 / … 正在操作：一眼看出哪些在跑，不必展开。
            let marker = if preset.busy {
                "…"
            } else if running {
                "●"
            } else {
                "○"
            };
            let title = format!("{marker} {} · {}", preset.name, preset.caption());
            let first_leaf = keep.len();
            let leaves: [(PresetAction, &str, bool); 4] = [
                (
                    PresetAction::Open,
                    tr(lang, "打开页面", "Open page"),
                    running && !preset.url.is_empty(),
                ),
                (
                    PresetAction::Start,
                    tr(lang, "启动", "Start"),
                    !running && !preset.busy && preset.installed,
                ),
                (
                    PresetAction::Stop,
                    tr(lang, "停止", "Stop"),
                    running && !preset.busy,
                ),
                (
                    PresetAction::Restart,
                    tr(lang, "重启", "Restart"),
                    running && !preset.busy,
                ),
            ];
            for (offset, (action, text, enabled)) in leaves.iter().enumerate() {
                let id = format!("{ID_PRESET}{index}.{offset}");
                roster.insert(
                    id.clone(),
                    Action::Preset {
                        id: preset.id.clone(),
                        target_id: preset.target_id.clone(),
                        instance_id: preset.instance_id.clone(),
                        action: *action,
                    },
                );
                keep.push(Box::new(MenuItem::with_id(id, *text, *enabled, None)));
            }
            let refs: Vec<&dyn IsMenuItem> = keep[first_leaf..].iter().map(|item| item.as_ref()).collect();
            let submenu = Submenu::with_id_and_items(
                format!("{ID_PRESET}menu.{index}"),
                title,
                true,
                &refs,
            )
            .ok()?;
            add(&mut keep, &mut top, Box::new(submenu));
        }
        if model.presets.len() > MENU_LIMIT {
            add(&mut keep, &mut top, Box::new(MenuItem::with_id(
                format!("{ID_PRESET}more"),
                tr(lang, "还有更多启动项，请在面板里操作", "More presets in the panel"),
                false,
                None,
            )));
        }
    }

    add(&mut keep, &mut top, Box::new(PredefinedMenuItem::separator()));
    // 模型还没到（核心刚起、连接没建好）时不知道实例状态，两个核心项都先给出来。
    let preset_running = model.map(|model| model.running() > 0).unwrap_or(true);
    add(&mut keep, &mut top, Box::new(MenuItem::with_id(
        ID_START,
        tr(lang, "启动核心", "Start core"),
        !preset_running,
        None,
    )));
    add(&mut keep, &mut top, Box::new(MenuItem::with_id(
        ID_STOP,
        tr(lang, "停止核心", "Stop core"),
        true,
        None,
    )));
    add(&mut keep, &mut top, Box::new(PredefinedMenuItem::separator()));
    add(&mut keep, &mut top, Box::new(MenuItem::with_id(
        ID_QUIT,
        tr(lang, "退出 DSH-X", "Quit DSH-X"),
        true,
        None,
    )));

    let refs: Vec<&dyn IsMenuItem> = top.iter().map(|&index| keep[index].as_ref()).collect();
    let menu = Menu::with_items(&refs).ok()?;
    if let Ok(mut slot) = ROSTER.lock() {
        *slot = Some(roster);
    }
    Some((menu, keep))
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
