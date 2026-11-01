// 原生面板：只放高频操作（起停重启、看状态与日志、打开管理页）。
// 其余全部留在管理页（浏览器里），所以这里刻意不做标签页、不做设置表单——
// 设置改了要通知核心重读，跨进程那套同步不值得为几个勾选框做。

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::bridge::Event;
use crate::prefs::Prefs;
use crate::supervisor::{Phase, Status, Supervisor};

pub struct Panel {
    pub status: Arc<Mutex<Status>>,
    pub events: Receiver<Event>,
    pub supervisor: Option<Arc<Supervisor>>,
    pub node: Option<PathBuf>,
    pub node_version: String,
    pub core: Option<PathBuf>,
    pub prefs: Arc<Mutex<Prefs>>,
    /// 退出后要不要把自己再拉起来（核心更新 / 外壳更新用得到）。主循环在 run_native 之后读它。
    pub relaunch: Arc<std::sync::atomic::AtomicBool>,
    /// 托盘句柄。装不上就是 None（Linux 缺 AppIndicator 之类），面板照常可用。
    pub tray: Option<crate::tray::Tray>,
    /// 用户真的要走（托盘退出 / 桥喊 exit），此时关窗不该被「关闭到托盘」拦下来。
    pub quitting: bool,
    /// 已经替用户开过的管理页地址。面板默认不弹，核心报了地址就直接开浏览器；
    /// 记地址而不是记一个 bool，是为了核心换端口重启后还能再开一次对的页面。
    pub opened_url: String,
    /// 核心彻底起不来时自动把面板亮出来（这时日志是唯一线索），只亮一次。
    pub failure_shown: bool,
    /// 用户有没有要过面板（托盘「打开面板」/ 核心起不来）。
    ///
    /// 为什么要这个标记：eframe 自己在第一帧画完后无条件 window.set_visible(true)
    /// （eframe-0.37.0/src/native/epi_integration.rs:441-453 的 post_rendering，
    /// 本来是防白闪的），NativeOptions 里传 visible(false) 压不住它。所以每帧
    /// 在 logic 末尾主动发一次 Visible(false)，直到用户真的要窗口——这条命令在
    /// post_rendering 之后处理（wgpu_integration.rs:874 vs :883），能把它盖回去。
    pub wants_window: bool,
    /// Windows：DWM 隐身开关（窗口 + 现在是不是隐身中）。见 cloak() 的注释。
    #[cfg(windows)]
    pub cloak: Option<(std::sync::Arc<winit::window::Window>, bool)>,
}

impl Panel {
    fn apply_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Show => {
                    self.wants_window = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::Exit => {
                    if let Some(supervisor) = &self.supervisor {
                        supervisor.stop();
                    }
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Event::Relaunch => {
                    if let Some(supervisor) = &self.supervisor {
                        supervisor.stop();
                    }
                    self.relaunch
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// 托盘菜单和桥事件最终做的是同一批事，只是入口不同（一个是用户点的，一个是核心喊的）。
    fn apply_tray(&mut self, ctx: &egui::Context) {
        use crate::tray::Action;
        for action in crate::tray::take() {
            match action {
                Action::Show => {
                    self.wants_window = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Action::OpenManager => {
                    let url = self
                        .status
                        .lock()
                        .map(|status| status.url.clone())
                        .unwrap_or_default();
                    if !url.is_empty() {
                        let _ = open::that(url);
                    }
                }
                Action::Start => {
                    let running = self
                        .status
                        .lock()
                        .map(|status| matches!(status.phase, Phase::Running | Phase::Starting))
                        .unwrap_or(false);
                    if !running {
                        if let Some(supervisor) = &self.supervisor {
                            supervisor.restart();
                        }
                    }
                }
                Action::Stop => {
                    if let Some(supervisor) = &self.supervisor {
                        supervisor.stop();
                    }
                }
                Action::Quit => {
                    if let Some(supervisor) = &self.supervisor {
                        supervisor.stop();
                    }
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// 面板默认藏着：用户双击启动器想要的是管理页，不是另一个窗口。
    /// 核心把管理页地址打出来（status.url）就开默认浏览器；核心反复起不来才把面板亮出来。
    fn auto_open(&mut self, ctx: &egui::Context) {
        let status = match self.status.lock() {
            Ok(status) => status.clone(),
            Err(_) => return,
        };
        if status.phase == Phase::Running && !status.url.is_empty() && status.url != self.opened_url {
            self.opened_url = status.url.clone();
            if let Err(error) = open::that(&status.url) {
                // 开浏览器失败不致命：托盘里的「打开管理页」还能再试一次。
                if let Ok(mut status) = self.status.lock() {
                    status.push_line(format!("打不开浏览器：{error}"));
                }
            }
        }
        if !self.failure_shown && status.phase == Phase::Failed {
            self.failure_shown = true;
            self.wants_window = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Windows：让窗口在 DWM 里隐身（cloak），而不是只隐藏窗口。
    ///
    /// 为什么要这么绕：eframe 第一帧画完会无条件 window.set_visible(true)
    /// （eframe-0.37.0/src/native/epi_integration.rs:441-453，本意是防白闪），
    /// 而我们的 Visible(false) 命令在它之后才处理，中间隔着一帧——实测窗口会
    /// 真真切切闪一下（采样到 ~44ms 可见）。cloak 是 DWM 层面的属性，窗口
    /// 「可见」也不参与合成，从创建那一刻起就不会出现在屏幕上。
    #[cfg(windows)]
    fn cloak(&mut self) {
        let Some((window, cloaked)) = &mut self.cloak else {
            return;
        };
        let want = !self.wants_window;
        if *cloaked == want {
            return;
        }
        if set_cloaked(window, want) {
            *cloaked = want;
        }
    }

    fn banner(&self, ui: &mut egui::Ui) {
        if self.node.is_none() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 0),
                "没有找到可用的 Node（需要 22.18 或更高的 22/24 版本）。装一个，或在管理页设置里指路径。",
            );
        } else if !self.node_version.is_empty() && !crate::supervisor::version_ok(&self.node_version)
        {
            ui.colored_label(
                egui::Color32::from_rgb(200, 120, 0),
                format!(
                    "Node {} 太旧，需要 22.18 或更高的 22/24 版本。",
                    self.node_version
                ),
            );
        }
        if self.core.is_none() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                "找不到核心目录（start.js）。安装包损坏时重装一次。",
            );
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui, status: &Status) {
        let ready = self.supervisor.is_some();
        let running = matches!(status.phase, Phase::Running | Phase::Starting);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready && !running, egui::Button::new("启动"))
                .clicked()
            {
                if let Some(supervisor) = &self.supervisor {
                    supervisor.restart();
                }
            }
            if ui
                .add_enabled(ready && running, egui::Button::new("停止"))
                .clicked()
            {
                if let Some(supervisor) = &self.supervisor {
                    supervisor.stop();
                }
            }
            if ui
                .add_enabled(ready && running, egui::Button::new("重启"))
                .clicked()
            {
                if let Some(supervisor) = &self.supervisor {
                    supervisor.restart();
                }
            }
            ui.separator();
            if status.url.is_empty() {
                ui.weak("管理页地址：还没出来");
            } else {
                ui.label("管理页：");
                ui.hyperlink_to(&status.url, &status.url);
                if ui.button("在浏览器里打开").clicked() {
                    let _ = open::that(&status.url);
                }
            }
        });
        if !status.error.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(200, 60, 60), &status.error);
        }
    }

    fn switches(&self, ui: &mut egui::Ui) {
        // 只读展示：真正的开关在管理页，这里写坏了反而会和核心的设置读写打架。
        let prefs = self.prefs.lock().unwrap().clone();
        let pick = |value: bool| if value { "开" } else { "关" };
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("硬件加速 {}", pick(prefs.hardware_acceleration)));
            ui.separator();
            ui.label(format!("自动更新 {}", pick(prefs.auto_update)));
            ui.separator();
            ui.label(format!("任务通知 {}", pick(prefs.task_notifications)));
            ui.separator();
            ui.label(format!("提示音 {}", pick(prefs.notification_sound)));
            ui.separator();
            ui.label(format!("关闭到托盘 {}", pick(prefs.close_to_tray)));
            ui.separator();
            ui.label(format!("保持唤醒 {}", pick(prefs.keep_awake)));
        });
    }
}

impl eframe::App for Panel {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 事件必须在画之前处理：核心喊「打开管理页」时窗口可能正藏着。
        self.apply_events(ctx);
        self.apply_tray(ctx);
        self.auto_open(ctx);
        // 「关闭到托盘」是旧外壳就有的开关：点 × 只藏窗口，核心继续跑。
        // 真正要走的情况（托盘退出 / 核心请求重启）会先把 quitting 立起来，不被这里拦。
        if ctx.input(|input| input.viewport().close_requested()) && !self.quitting {
            let close_to_tray = self
                .prefs
                .lock()
                .map(|prefs| prefs.close_to_tray)
                .unwrap_or(false);
            if close_to_tray {
                self.wants_window = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        // 见 wants_window 的注释：eframe 会在第一帧后强行显示窗口，这里每帧盖回去。
        #[cfg(windows)]
        self.cloak();
        if !self.wants_window {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let status = self.status.lock().unwrap().clone();

        egui::Panel::top("status").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("DSH-X");
                ui.separator();
                ui.label(format!("核心：{}", status.phase.label()));
                if status.pid > 0 {
                    ui.weak(format!("pid {}", status.pid));
                }
            });
            ui.add_space(2.0);
            self.banner(ui);
            self.controls(ui, &status);
            ui.add_space(2.0);
            self.switches(ui);
        });

        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.horizontal(|ui| {
                match (&self.node, &self.core) {
                    (Some(node), Some(core)) => {
                        ui.weak(format!("Node {}", self.node_version));
                        ui.separator();
                        ui.weak(core.display().to_string());
                        ui.separator();
                        ui.weak(node.display().to_string());
                    }
                    _ => {
                        ui.weak("核心没有起来：先修上面的问题。");
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("核心输出");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for line in &status.tail {
                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                    }
                });
        });

        // 日志线程不会主动叫醒界面：半秒刷一次就够，别让面板空转。
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }

    fn on_exit(&mut self) {
        if let Some(supervisor) = &self.supervisor {
            supervisor.stop();
        }
    }
}

/// 给窗口设置 DWM 的 cloak 属性（隐身但不改变「窗口可见」状态）。
/// 拿不到句柄或系统不认（老系统）就返回 false，调用方当没成功、每帧再试。
#[cfg(windows)]
pub(crate) fn set_cloaked(window: &winit::window::Window, cloaked: bool) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;
    let value: u32 = if cloaked { 1 } else { 0 };
    // 返回 HRESULT：0 是 S_OK，负数才是失败。
    let result = unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_CLOAK as u32, &value as *const u32 as *const _, 4)
    };
    result >= 0
}
