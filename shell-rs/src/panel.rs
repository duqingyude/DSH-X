// 原生面板：只放高频操作（起停重启、看状态与日志、开管理页、改系统开关）。
//
// 数据全部来自核心推来的 PanelModel（model.rs）：外壳自己不算状态、不算端口、不读
// settings.json 里的开关值——设置只有核心一个写者，否则两边会各写一半、互相覆盖。
// 插件 / 整合包 / MCP / 技能 / 同步这些低频管理仍在管理页，面板不做第二份。

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::bridge::{Event, Link};
use crate::model::{PanelModel, Preset};
use crate::prefs::Prefs;
use crate::supervisor::{Phase, Status, Supervisor};
use crate::theme;

pub struct Panel {
    pub status: Arc<Mutex<Status>>,
    pub events: Receiver<Event>,
    /// 事件通道的发送端：面板自己起的线程（动作结果、设置回执）也要能往主循环塞事件。
    pub events_tx: Sender<Event>,
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
    /// 反向调用句柄：启动项动作与系统开关都靠它转给核心。
    pub link: Arc<Link>,
    /// 核心推来的面板模型（连上拉一次，之后状态一变就更新）。
    pub model: Option<PanelModel>,
    /// 要显示给用户的一句话（比如核心太旧、拉不到模型）。
    pub notice: Option<(bool, String)>,
    /// 右侧设置抽屉开着没有。
    pub drawer: bool,
    /// settings.json 的修改时间：管理页也能改这些开关，面板要跟着变（设置只有核心一个写者）。
    pub prefs_stamp: Option<std::time::SystemTime>,
    /// 上次看 mtime 的时间：每帧都 stat 一下没必要。
    pub prefs_checked: Option<Instant>,
    /// 已经设过的「保持唤醒」状态（见 power.rs 的注释：重复设没意义）。
    pub keep_awake_applied: bool,
}

/// 选 Node 可执行文件：Windows 上滤出 node.exe，POSIX 不加滤镜（可执行文件常常没后缀）。
fn pick_node() -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new().set_title("选择 node");
    #[cfg(windows)]
    let dialog = dialog.add_filter("node", &["node.exe"]);
    dialog.pick_file()
}

/// 一句中文一句英文：面板只有这两种语言，做成表反而多一层跳转。
fn tr<'a>(lang: impl AsRef<str>, zh: &'a str, en: &'a str) -> &'a str {
    if lang.as_ref() == "en" { en } else { zh }
}

impl Panel {
    /// 面板语言：只有 zh / en 两种，返回静态串省得每个调用点克隆一次。
    fn lang(&self) -> &'static str {
        match self.model.as_ref().map(|model| model.prefs.lang.as_str()) {
            Some("en") => "en",
            _ => "zh",
        }
    }

    /// 深浅色偏好：设置里选 auto 就跟系统走（egui 的 System 就是问系统）。
    fn theme_mode(&self) -> egui::ThemePreference {
        let theme = self
            .model
            .as_ref()
            .map(|model| model.prefs.theme.as_str())
            .unwrap_or("auto");
        match theme {
            "dark" => egui::ThemePreference::Dark,
            "light" => egui::ThemePreference::Light,
            _ => egui::ThemePreference::System,
        }
    }

    /// 按 mtime 重读 settings.json。
    ///
    /// 设置只有核心一个写者（面板改开关也是转给核心写），但用户也可能在管理页里改，
    /// 所以外壳每 800ms 看一眼文件有没有变——比让两边各写一半、互相覆盖省事。
    fn refresh_prefs(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.prefs_checked {
            if now.duration_since(last) < Duration::from_millis(800) {
                return;
            }
        }
        self.prefs_checked = Some(now);
        let stamp = std::fs::metadata(crate::prefs::settings_path())
            .and_then(|meta| meta.modified())
            .ok();
        if stamp.is_some() && stamp != self.prefs_stamp {
            if let Ok(mut slot) = self.prefs.lock() {
                *slot = Prefs::load();
            }
        }
        self.prefs_stamp = stamp;
        // 不管文件变没变都对一次：启动时也要把「保持唤醒」设上。
        let keep_awake = self
            .prefs
            .lock()
            .map(|prefs| prefs.keep_awake)
            .unwrap_or(false);
        if keep_awake != self.keep_awake_applied {
            crate::power::apply(keep_awake);
            self.keep_awake_applied = keep_awake;
        }
    }

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
                Event::Panel(model) => {
                    // 推送是节流的，可能迟到：按 updatedAt 只认新的，状态才不会回跳。
                    let newer = self
                        .model
                        .as_ref()
                        .map(|current| model.updated_at >= current.updated_at)
                        .unwrap_or(true);
                    if newer {
                        // 拉到了模型就说明核心支持 panel 方法，先前那句「核心过旧」可以撤了。
                        if self.notice.as_ref().is_some_and(|(ok, _)| !*ok) {
                            self.notice = None;
                        }
                        self.model = Some(model);
                    }
                }
                Event::Notice { ok, message } => self.notice = Some((ok, message)),
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
                Action::StartCore => {
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
                Action::StopCore => {
                    if let Some(supervisor) = &self.supervisor {
                        supervisor.stop();
                    }
                }
                Action::Preset {
                    id,
                    target_id,
                    instance_id,
                    action,
                } => {
                    // 托盘点启动项和面板点按钮走同一条路：都是往核心发 panel/action。
                    self.dispatch_ids(&id, action.verb(), &target_id, &instance_id);
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
                // 开浏览器失败不致命：托盘与 header 里的「打开管理页」还能再试一次。
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

    /// 启动项动作：转给核心（panel/action），核心那边和托盘菜单走同一个 trayAction。
    /// 自己起线程等回包：拉起一个实例要几秒，不能卡住界面线程。
    fn dispatch(&self, row: &Preset, action: &str) {
        self.dispatch_ids(&row.id, action, &row.target_id, &row.instance_id);
    }

    /// 按 id 转发一次动作（托盘那边只有这几个字段，没有整行 Preset）。
    fn dispatch_ids(&self, id: &str, action: &str, target_id: &str, instance_id: &str) {
        let link = Arc::clone(&self.link);
        let tx = self.events_tx.clone();
        let id = id.to_string();
        let target = target_id.to_string();
        let instance = instance_id.to_string();
        let verb = action.to_string();
        std::thread::spawn(move || {
            if let Err(error) = link.action(&id, &verb, &target, &instance) {
                // 成功不用提示：核心会把新状态推回来，界面自己就变了。
                let _ = tx.send(Event::Notice { ok: false, message: error });
            }
        });
    }

    fn push_settings(&self, patch: serde_json::Value) {
        let link = Arc::clone(&self.link);
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            if let Err(error) = link.settings(patch) {
                let _ = tx.send(Event::Notice { ok: false, message: error });
            }
        });
    }

    fn core_state(&self, p: &theme::Palette) -> (egui::Color32, String) {
        let (phase, pid) = self
            .status
            .lock()
            .map(|status| (status.phase, status.pid))
            .unwrap_or((Phase::Idle, 0));
        match phase {
            Phase::Running => (p.ok, format!("运行中 · pid {pid}")),
            Phase::Starting => (p.warn, "启动中…".to_string()),
            Phase::Stopped => (p.muted, "已停止".to_string()),
            Phase::Failed => (p.danger, "启动失败".to_string()),
            Phase::Idle => (p.muted, "未启动".to_string()),
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let lang = self.lang();
        let (dot, state) = self.core_state(p);
        let version = self
            .model
            .as_ref()
            .map(|model| model.core.version.clone())
            .unwrap_or_default();
        let url = self.status.lock().map(|status| status.url.clone()).unwrap_or_default();
        let mut settings_clicked = false;
        let mut open_clicked = false;
        egui::Panel::top("header").frame(theme::bar(p)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 12.0, p.accent);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "D",
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                );
                ui.label(egui::RichText::new("DSH-X").size(16.0).strong().color(p.text));
                if !version.is_empty() {
                    ui.label(egui::RichText::new(format!("v{version}")).size(11.0).color(p.muted));
                }
                ui.separator();
                ui.label(egui::RichText::new("●").color(dot));
                ui.label(egui::RichText::new(state).size(12.0).color(p.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⋯")
                        .on_hover_text(tr(lang, "设置", "Settings"))
                        .clicked()
                    {
                        settings_clicked = true;
                    }
                    if ui
                        .add_enabled(
                            !url.is_empty(),
                            egui::Button::new(tr(lang, "打开管理页", "Open console")),
                        )
                        .clicked()
                    {
                        open_clicked = true;
                    }
                });
            });
        });
        if settings_clicked {
            self.drawer = !self.drawer;
        }
        if open_clicked && !url.is_empty() {
            let _ = open::that(url);
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let lang = self.lang();
        let core_path = self
            .core
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| tr(lang, "找不到核心", "core missing").to_string());
        let node_text = match &self.node {
            Some(_) if !self.node_version.is_empty() => format!("Node {}", self.node_version),
            Some(_) => "Node".to_string(),
            None => tr(lang, "没有 Node", "no Node").to_string(),
        };
        egui::Panel::bottom("footer")
            .frame(theme::bar(p))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(node_text).size(11.0).color(p.muted));
                    ui.separator();
                    ui.label(
                        egui::RichText::new(core_path)
                            .size(11.0)
                            .color(p.muted)
                            .monospace(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} v{}",
                                tr(lang, "外壳", "Shell"),
                                env!("CARGO_PKG_VERSION")
                            ))
                                .size(11.0)
                                .color(p.muted),
                        );
                    });
                });
            });
    }

    /// 顶部的一串提示：Node 缺失 / 版本太旧 / 核心目录没了 / 拉不到模型 / 上次启动失败。
    fn notices(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let lang = self.lang();
        let mut lines: Vec<(egui::Color32, String)> = Vec::new();
        if let Some((ok, message)) = &self.notice {
            lines.push((if *ok { p.ok } else { p.danger }, message.clone()));
        }
        if self.node.is_none() {
            lines.push((
                p.warn,
                tr(
                    lang,
                    "没有找到可用的 Node（要 22.18 以上的 22/24）。装一个，或在管理页设置里指路径。",
                    "No usable Node found (need 22.18+ in the 22/24 line). Install one, or set the path in the console settings.",
                )
                .to_string(),
            ));
        } else if !self.node_version.is_empty() && !crate::supervisor::version_ok(&self.node_version) {
            lines.push((
                p.warn,
                format!(
                    "{}",
                    tr(lang, "Node 版本太旧，需要 22.18 以上的 22/24 版本。", "Node is too old: 22.18+ in the 22/24 line is required.")
                ),
            ));
        }
        if self.core.is_none() {
            lines.push((
                p.danger,
                tr(
                    lang,
                    "找不到核心目录（start.js）。安装包损坏时重装一次。",
                    "Core directory (start.js) is missing. Reinstall if the package is damaged.",
                )
                .to_string(),
            ));
        }
        if let Some(failure) = self.model.as_ref().and_then(|model| model.failure_text()) {
            lines.push((p.danger, format!("{} {failure}", tr(lang, "上次启动失败：", "Last start failed: "))));
        }
        for (color, text) in lines {
            egui::Frame::new()
                .fill(color.gamma_multiply(0.12))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).size(12.0).color(color));
                });
            ui.add_space(4.0);
        }
    }

    fn preset_card(
        &mut self,
        ui: &mut egui::Ui,
        p: &theme::Palette,
        row: &Preset,
        lang: &str,
    ) {
        let mut clicked: Option<&'static str> = None;
        let running = row.is_running();
        let starting = row.status == "starting";
        let stopping = row.status == "stopping";
        theme::card(p).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 4.0, theme::status_color(p, &row.status));
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&row.name).size(13.5).strong().color(p.text));
                    ui.label(egui::RichText::new(row.caption()).size(11.0).color(p.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_open = running && !row.url.is_empty();
                    if ui
                        .add_enabled(can_open, egui::Button::new("↗"))
                        .on_hover_text(tr(lang, "在浏览器里打开", "Open in browser"))
                        .clicked()
                    {
                        clicked = Some("open");
                    }
                    if running || starting {
                        if ui
                            .add_enabled(!stopping, egui::Button::new("■"))
                            .on_hover_text(tr(lang, "停止", "Stop"))
                            .clicked()
                        {
                            clicked = Some("stop");
                        }
                    } else if ui
                        .add_enabled(
                            row.installed,
                            egui::Button::new("▶"),
                        )
                        .on_hover_text(tr(lang, "启动", "Start"))
                        .clicked()
                    {
                        clicked = Some("start");
                    }
                    let restartable = running && !row.busy && row.installed;
                    if ui
                        .add_enabled(restartable, egui::Button::new("⟳"))
                        .on_hover_text(tr(lang, "重启", "Restart"))
                        .clicked()
                    {
                        clicked = Some("restart");
                    }
                });
            });
            if !row.installed {
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "这个版本还没装：到管理页里装一次。",
                        "This version is not installed yet: install it from the console.",
                    ))
                    .size(10.5)
                    .color(p.muted),
                );
            } else if row.busy {
                ui.label(
                    egui::RichText::new(tr(lang, "正在操作…", "Working…"))
                        .size(10.5)
                        .color(p.warn),
                );
            }
        });
        if let Some(action) = clicked {
            self.dispatch(row, action);
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let lang = self.lang();
        let tail = self
            .status
            .lock()
            .map(|status| status.tail.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                self.notices(ui, p);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tr(&lang, "启动项", "Launchers"))
                            .size(13.0)
                            .strong()
                            .color(p.text),
                    );
                    if let Some(model) = &self.model {
                        let running = model.running();
                        if running > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {running}",
                                    tr(&lang, "运行中", "running")
                                ))
                                .size(11.0)
                                .color(p.ok),
                            );
                        }
                    }
                });
                ui.add_space(4.0);
                let presets = self
                    .model
                    .as_ref()
                    .map(|model| model.presets.clone())
                    .unwrap_or_default();
                if presets.is_empty() {
                    // 核心还在启动时模型是空的，这时候说「还没有启动项」会让人以为设置丢了。
                    let text = if self.link.connected() {
                        tr(
                            &lang,
                            "还没有启动项。到管理页里加一个（版本 + profile），之后就能在这里起停。",
                            "No launchers yet. Add one (version + profile) in the console, then start it from here.",
                        )
                    } else {
                        tr(&lang, "正在等核心连上外壳通道…", "Waiting for the core to connect…")
                    };
                    ui.label(egui::RichText::new(text).size(12.0).color(p.muted));
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height((ui.available_height() - 90.0).max(120.0))
                    .show(ui, |ui| {
                        for row in &presets {
                            self.preset_card(ui, p, row, &lang);
                            ui.add_space(6.0);
                        }
                    });
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(tr(&lang, "核心输出", "Core output"))
                        .size(11.5)
                        .color(p.muted),
                )
                .default_open(false)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height(120.0)
                        .show(ui, |ui| {
                            if tail.is_empty() {
                                ui.label(
                                    egui::RichText::new(tr(&lang, "还没有输出", "No output yet"))
                                        .size(11.0)
                                        .color(p.muted),
                                );
                            }
                            for line in &tail {
                                ui.label(
                                    egui::RichText::new(line)
                                        .monospace()
                                        .size(11.0)
                                        .color(p.muted),
                                );
                            }
                        });
                });
            });
    }

    /// 右侧设置抽屉：六个系统开关 + 界面语言。
    ///
    /// 开关的当前值来自核心推来的模型（核心是 settings.json 的唯一写者），点一下就把
    /// 新值发回核心，核心存盘后推一份新模型回来——面板自己不留副本，也就不会两边不一致。
    fn settings_drawer(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let lang = self.lang();
        let prefs = self.model.as_ref().map(|model| model.prefs.clone());
        let mut changes: Vec<(String, serde_json::Value)> = Vec::new();
        let mut close = false;
        let mut picked: Option<PathBuf> = None;
        let core_path = self
            .node
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        egui::Panel::right("settings")
            .resizable(false)
            .exact_size(300.0)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tr(&lang, "设置", "Settings"))
                            .size(14.0)
                            .strong()
                            .color(p.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            close = true;
                        }
                    });
                });
                ui.separator();
                let Some(prefs) = prefs else {
                    ui.label(
                        egui::RichText::new(tr(
                            &lang,
                            "等核心连上之后才能改设置。",
                            "Waiting for the core before settings can change.",
                        ))
                        .size(11.5)
                        .color(p.muted),
                    );
                    return;
                };
                let rows: [(&str, &str, &str, &str, &str, bool); 5] = [
                    (
                        "taskNotifications",
                        "任务通知",
                        "Task notifications",
                        "实例启动失败、意外退出时提醒。",
                        "Warn when an instance fails or quits.",
                        prefs.task_notifications,
                    ),
                    (
                        "notificationSound",
                        "提示音",
                        "Sound",
                        "通知带一声提示音。",
                        "Play a sound with notifications.",
                        prefs.notification_sound,
                    ),
                    (
                        "closeToTray",
                        "关闭到托盘",
                        "Close to tray",
                        "点 × 只藏窗口，核心继续跑。",
                        "Closing the window keeps the core running.",
                        prefs.close_to_tray,
                    ),
                    (
                        "autoUpdate",
                        "自动更新",
                        "Auto update",
                        "只下载，安装仍由你点。",
                        "Downloads only; you still install it.",
                        prefs.auto_update,
                    ),
                    (
                        "keepAwake",
                        "保持唤醒",
                        "Keep awake",
                        "实例运行时不让系统睡眠。",
                        "Stop the system from sleeping while running.",
                        prefs.keep_awake,
                    ),
                ];
                for (key, zh, en, hint_zh, hint_en, value) in rows {
                    let mut current = value;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(tr(&lang, zh, en))
                                    .size(12.5)
                                    .color(p.text),
                            );
                            ui.label(
                                egui::RichText::new(tr(&lang, hint_zh, hint_en))
                                    .size(10.5)
                                    .color(p.muted),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Checkbox::new(&mut current, "")).changed() {
                                changes.push((key.to_string(), serde_json::Value::Bool(current)));
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tr(&lang, "界面语言", "Language"))
                            .size(12.5)
                            .color(p.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let current = if prefs.lang == "en" { "English" } else { "中文" };
                        egui::ComboBox::from_id_salt("panel-lang")
                            .selected_text(current)
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(prefs.lang != "en", "中文")
                                    .clicked()
                                {
                                    changes.push(("lang".to_string(), serde_json::json!("zh")));
                                }
                                if ui
                                    .selectable_label(prefs.lang == "en", "English")
                                    .clicked()
                                {
                                    changes.push(("lang".to_string(), serde_json::json!("en")));
                                }
                            });
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(tr(&lang, "跑 dsh 的 Node", "Node for dsh"))
                        .size(11.5)
                        .color(p.muted),
                );
                ui.label(
                    egui::RichText::new(core_path)
                        .size(10.5)
                        .color(p.muted)
                        .monospace(),
                );
                ui.add_space(4.0);
                if ui
                    .button(tr(&lang, "选择 Node…", "Choose Node…"))
                    .clicked()
                {
                    picked = pick_node();
                }
                ui.label(
                    egui::RichText::new(tr(
                        &lang,
                        "换过之后重启核心才生效。",
                        "Restart the core to use the new Node.",
                    ))
                    .size(10.5)
                    .color(p.muted),
                );
            });
        if close {
            self.drawer = false;
        }
        if let Some(path) = picked {
            // 路径照样交给核心写（设置只有核心一个写者），它那边会先用 node --version
            // 验一遍，选错文件会直接报错；外壳这边先说一句，重启核心才生效。
            self.notice = Some((
                true,
                format!("跑 dsh 的 Node 改成 {}；重启核心后生效。", path.display()),
            ));
            changes.push(("systemNodePath".to_string(), serde_json::json!(path.display().to_string())));
        }
        if !changes.is_empty() {
            let patch = changes
                .into_iter()
                .fold(serde_json::Map::new(), |mut map, (key, value)| {
                    map.insert(key, value);
                    map
                });
            self.push_settings(serde_json::Value::Object(patch));
        }
    }
}

impl eframe::App for Panel {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 事件必须在画之前处理：核心喊「打开管理页」时窗口可能正藏着。
        self.refresh_prefs();
        self.apply_events(ctx);
        self.apply_tray(ctx);
        // 托盘菜单跟着启动项走：模型一变（核心推来的状态、语言切换）就重建。
        let lang = self.lang();
        if let Some(tray) = self.tray.as_mut() {
            tray.sync(self.model.as_ref(), lang);
        }
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
        let palette = theme::apply(ui.ctx(), self.theme_mode());
        self.header(ui, &palette);
        if self.drawer {
            self.settings_drawer(ui, &palette);
        }
        self.footer(ui, &palette);
        self.body(ui, &palette);
        // 日志线程与核心推送都不会主动叫醒界面：半秒刷一次就够，别让面板空转。
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