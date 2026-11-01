// 面板配色：照管理页的调色板取（public/launcher.css:2-10），两处看起来才是同一个产品。
//
// 为什么不用 egui 自带主题：自带那套是给调试工具用的灰蓝，和管理页的浅色卡片差得远；
// 而且深浅两套都要给（用户可能在系统里开深色），所以颜色集中在这里，panel.rs 只认字段名。

use eframe::egui::{self, Color32};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub danger: Color32,
    pub ok: Color32,
    pub warn: Color32,
}

/// 浅色：管理页就是这套（#eef3fd 底、白卡片、#3b6fe0 主色）。
const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xee, 0xf3, 0xfd),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    line: Color32::from_rgb(0xd8, 0xe2, 0xf7),
    text: Color32::from_rgb(0x16, 0x23, 0x3d),
    muted: Color32::from_rgb(0x6b, 0x7d, 0x9c),
    accent: Color32::from_rgb(0x3b, 0x6f, 0xe0),
    danger: Color32::from_rgb(0xe1, 0x1d, 0x48),
    ok: Color32::from_rgb(0x16, 0xa3, 0x4a),
    warn: Color32::from_rgb(0xd9, 0x77, 0x06),
};

/// 深色：同一组色相压暗，不为深色另设计一套。
const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0f, 0x16, 0x22),
    card: Color32::from_rgb(0x18, 0x21, 0x30),
    line: Color32::from_rgb(0x2a, 0x34, 0x46),
    text: Color32::from_rgb(0xe6, 0xed, 0xf7),
    muted: Color32::from_rgb(0x93, 0xa1, 0xb8),
    accent: Color32::from_rgb(0x5b, 0x8d, 0xef),
    danger: Color32::from_rgb(0xf4, 0x6a, 0x8a),
    ok: Color32::from_rgb(0x4a, 0xd6, 0x8a),
    warn: Color32::from_rgb(0xf0, 0xa4, 0x4a),
};

pub fn palette(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}

/// 面板里所有控件（按钮、输入框、滚动条）的底色都从这里来，省得每个控件单独上色。
/// 返回当前实际用的调色板：auto 时以 egui 解析出来的主题为准。
pub fn apply(ctx: &egui::Context, preference: egui::ThemePreference) -> Palette {
    // 先让 egui 定下主题（System 会去问系统），再按它的结果取色。
    ctx.set_theme(preference);
    let dark = ctx.theme() == egui::Theme::Dark;
    let p = palette(dark);
    // 两套风格都改：用户把系统切成深色时，egui 会自己换 theme，不重设就会跳回默认灰蓝。
    ctx.all_styles_mut(|style| {
        let mut visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        visuals.panel_fill = p.bg;
        visuals.window_fill = p.card;
        visuals.extreme_bg_color = p.card;
        visuals.override_text_color = Some(p.text);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.line);
        visuals.widgets.inactive.weak_bg_fill = p.card;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.line);
        visuals.widgets.hovered.weak_bg_fill = p.line;
        visuals.widgets.active.weak_bg_fill = p.line;
        visuals.selection.bg_fill = p.accent.gamma_multiply(0.28);
        visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);
        visuals.hyperlink_color = p.accent;
        style.visuals = visuals;
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.interact_size.y = 26.0;
    });
    p
}

/// 顶部条 / 底部条：没有圆角、只留一条分隔线，贴着窗口边。
pub fn bar(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.card)
        .inner_margin(egui::Margin::symmetric(14, 8))
}

/// 启动项卡片。
pub fn card(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, 10))
}

/// 实例状态 → 颜色。核心给的字符串只有这四种加一个空值，不认识的按「停着」画。
pub fn status_color(p: &Palette, status: &str) -> Color32 {
    match status {
        "running" => p.ok,
        "starting" | "stopping" => p.warn,
        "failed" => p.danger,
        _ => p.muted,
    }
}