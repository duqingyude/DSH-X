// 外壳自更新（cargo-packager-updater）。现在这一步只提供状态字段，真正的检查/下载/安装
// 在 P3 接上——但接口先定下来，核心的 /api/shell-update/* 已经按这个形状读。
use serde_json::{json, Value};

pub fn state() -> Value {
    json!({
        "status": "disabled",
        "current": env!("CARGO_PKG_VERSION"),
        "latest": "",
        "error": "",
    })
}

pub fn check() -> Value {
    // 没有接更新源之前不假装检查过：状态里给 disabled，页面就不会显示「检查更新」的转圈。
    state()
}
