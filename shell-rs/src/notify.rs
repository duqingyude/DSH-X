// 系统通知：三平台各一套后端由 notify-rust 挑；这里只管「要不要发」以外的格式与失败处理。
// 发不出去不该影响任何业务流程（插件调用、实例崩溃提醒都只是锦上添花）。

pub fn send(title: &str, body: &str) {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).appname("DSH-X");
    if !body.is_empty() {
        notification.body(body);
    }
    let _ = notification.show();
}
