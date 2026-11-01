// 系统通知：三平台各一套后端由 notify-rust 挑；这里只管「要不要发」以外的格式与失败处理。
// 发不出去不该影响任何业务流程（插件调用、实例崩溃提醒都只是锦上添花）。

/// sound 来自设置里的「提示音」；不设 sound_name 时 notify-rust 生成的 toast 带
/// `<audio silent="true"/>`，用户开着提示音也听不到一声。
pub fn send(title: &str, body: &str, sound: bool) {
    let mut notification = notify_rust::Notification::new();
    // appname 给 Linux 的分组用；Windows 只认 app_id——不传它就退回 notify-rust 的兜底
    // （PowerShell 的 AUMID），通知会署名「Windows PowerShell」。见 identity.rs。
    notification
        .summary(title)
        .appname(env!("DSH_PRODUCT_NAME"))
        .app_id(env!("DSH_APP_ID"));
    if sound {
        // "Default" 是 Windows 自带的通知音（winrt 的 Sound::from_str 认这几个名字）。
        notification.sound_name("Default");
    }
    if !body.is_empty() {
        notification.body(body);
    }
    let _ = notification.show();
}
