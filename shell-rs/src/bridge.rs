// 本机 HTTP 桥：核心（另一个进程里的 start.js）通过它回调外壳的 host 成员（§6）。
// 只绑 127.0.0.1，token 每个进程随机生成、只走环境变量；请求不带 token 一律 403。
// 同步能力（剪贴板、对话框、通知、打开链接）都在这里直接做，不需要惊动界面线程；
// 只有「把面板叫出来」「退出」「重启」这类要动窗口的事才发事件给主循环。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tiny_http::{Header, Server};

use crate::prefs::Prefs;
use crate::supervisor::Status;

/// 从桥发回主循环的事件。
pub enum Event {
    Show,
    Exit,
    Relaunch,
}

pub struct BridgeCtx {
    pub token: String,
    pub status: Arc<Mutex<Status>>,
    pub prefs: Arc<Mutex<Prefs>>,
    pub events: Sender<Event>,
    pub shell_version: String,
    pub node_version: String,
}

/// 起桥；返回监听端口（调用方拿它拼 DSH_SHELL_URL）。
pub fn start(ctx: BridgeCtx) -> std::io::Result<u16> {
    let server = Server::http("127.0.0.1:0")
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, error.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .unwrap_or(0);
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            handle(request, &ctx);
        }
    });
    Ok(port)
}

fn json_response(value: &Value) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("Content-Type 是合法头");
    tiny_http::Response::from_string(value.to_string())
        .with_header(header)
        .with_status_code(200)
}

fn error_response(status: u16, message: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("Content-Type 是合法头");
    tiny_http::Response::from_string(json!({ "error": message }).to_string())
        .with_header(header)
        .with_status_code(status)
}

fn handle(mut request: tiny_http::Request, ctx: &BridgeCtx) {
    let route = request.url().trim_start_matches('/').to_string();
    let expected = format!("Bearer {}", ctx.token);
    let authorized = request
        .headers()
        .iter()
        .any(|header| header.field.equiv("Authorization") && header.value.as_str() == expected);
    if !authorized {
        // 不带 token 的请求可能来自本机别的程序：不执行、不给细节。
        let _ = request.respond(error_response(403, "外壳桥缺少 token"));
        return;
    }
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    let input: Value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(Value::Null)
    };
    let Some(route) = route.strip_prefix("host/") else {
        let _ = request.respond(error_response(404, "外壳桥没有这个路由"));
        return;
    };
    let response = match route {
        "open" => {
            let url = input.get("url").and_then(Value::as_str).unwrap_or("").to_string();
            open_url(&url);
            json!({ "ok": true })
        }
        "wake" => {
            // 面板叫到前面；管理页没有内嵌窗口，地址交给系统浏览器。
            let _ = ctx.events.send(Event::Show);
            let url = ctx.status.lock().unwrap().url.clone();
            if !url.is_empty() {
                let _ = open::that(url);
            }
            json!({ "ok": true })
        }
        "notify" => {
            notify(ctx, &input);
            json!({ "ok": true })
        }
        "exit" => {
            let _ = ctx.events.send(Event::Exit);
            json!({ "ok": true })
        }
        "relaunch" => {
            let _ = ctx.events.send(Event::Relaunch);
            json!({ "ok": true })
        }
        "info" => json!({ "info": info(ctx) }),
        "desktop/notify" => {
            notify(ctx, &input);
            json!({ "ok": true })
        }
        "desktop/open" => {
            let url = input.get("url").and_then(Value::as_str).unwrap_or("");
            open_url(url);
            json!({ "ok": true })
        }
        "desktop/reveal" => {
            let path = input.get("path").and_then(Value::as_str).unwrap_or("");
            if !path.is_empty() {
                let _ = open::that_detached(path);
            }
            json!({ "ok": true })
        }
        "desktop/clipboard" => {
            let text = input.get("text").and_then(Value::as_str).unwrap_or("");
            match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.to_string())) {
                Ok(()) => json!({ "ok": true }),
                Err(error) => json!({ "ok": false, "error": error.to_string() }),
            }
        }
        "desktop/dialog" => dialog(&input),
        "browser/clearCache" | "browser/clearAll" => {
            // 原生外壳没有内嵌浏览器，也就没有分区可清；插件看到 false 会自己降级。
            json!({ "ok": false, "error": "原生外壳没有内嵌浏览器分区" })
        }
        "shellUpdate/state" => json!({ "state": crate::updater::state() }),
        "shellUpdate/check" => json!({ "state": crate::updater::check() }),
        "shellUpdate/download" | "shellUpdate/install" => {
            json!({ "state": crate::updater::state() })
        }
        _ => {
            let _ = request.respond(error_response(404, "外壳桥没有这个路由"));
            return;
        }
    };
    let _ = request.respond(json_response(&response));
}

/// 只放 http(s)：核心已经过滤过一遍，这里再挡一次——桥是本机可访问的，不能当任意命令入口。
fn open_url(url: &str) {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let _ = open::that_detached(url);
    }
}

fn info(ctx: &BridgeCtx) -> Value {
    json!({
        "versions": {
            "shell": ctx.shell_version,
            "node": ctx.node_version,
        },
        // 主题交给系统：面板是 egui 画的，管理页在浏览器里，两边都跟系统走。
        "theme": "system",
    })
}

fn notify(ctx: &BridgeCtx, input: &Value) {
    // 插件只能把通知关小声，不能打开用户关掉的提示音：真正开关在这里。
    let prefs = ctx.prefs.lock().unwrap().clone();
    if !prefs.task_notifications {
        return;
    }
    let title = input
        .get("title")
        .or_else(|| input.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or("DSH-X");
    let body = input
        .get("body")
        .or_else(|| input.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");
    crate::notify::send(title, body);
}

fn dialog(input: &Value) -> Value {
    let kind = input.get("kind").and_then(Value::as_str).unwrap_or("open");
    let options = input.get("options").cloned().unwrap_or(Value::Null);
    let title = options.get("title").and_then(Value::as_str).unwrap_or("DSH-X");
    let default_path = options.get("defaultPath").and_then(Value::as_str).unwrap_or("");
    match kind {
        "save" => {
            let mut dialog = rfd::FileDialog::new().set_title(title);
            if !default_path.is_empty() {
                dialog = dialog.set_file_name(default_path);
            }
            match dialog.save_file() {
                Some(path) => json!({ "canceled": false, "path": path.to_string_lossy() }),
                None => json!({ "canceled": true, "path": "" }),
            }
        }
        "message" => {
            let message = options.get("message").and_then(Value::as_str).unwrap_or("");
            let detail = options.get("detail").and_then(Value::as_str).unwrap_or("");
            let buttons: Vec<String> = options
                .get("buttons")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let mut dialog = rfd::MessageDialog::new().set_title(title).set_description(message);
            if !detail.is_empty() {
                dialog = dialog.set_description(format!("{message}\n\n{detail}"));
            }
            // Electron 的 buttons 数组在 rfd 里只有这三档；超过三个时留前三个已经够用
            // （桌面 API 文档里约定的用法最多就是「是/否/取消」）。
            let kind = match buttons.len() {
                0 | 1 => rfd::MessageButtons::Ok,
                2 => rfd::MessageButtons::OkCancel,
                _ => rfd::MessageButtons::YesNoCancel,
            };
            let response = match dialog.set_buttons(kind).show() {
                rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => 0,
                rfd::MessageDialogResult::No | rfd::MessageDialogResult::Cancel => 1,
                rfd::MessageDialogResult::Custom(_) => 2,
            };
            json!({ "canceled": false, "response": response })
        }
        _ => {
            let directory = options
                .get("properties")
                .and_then(Value::as_array)
                .map(|items| items.iter().any(|item| item.as_str() == Some("openDirectory")))
                .unwrap_or(false);
            let multiple = options
                .get("properties")
                .and_then(Value::as_array)
                .map(|items| items.iter().any(|item| item.as_str() == Some("multiSelections")))
                .unwrap_or(false);
            let mut dialog = rfd::FileDialog::new().set_title(title);
            if !default_path.is_empty() {
                dialog = dialog.set_directory(default_path);
            }
            if directory {
                return match dialog.pick_folder() {
                    Some(path) => json!({ "canceled": false, "paths": [path.to_string_lossy()] }),
                    None => json!({ "canceled": true, "paths": [] }),
                };
            }
            let picked = if multiple { dialog.pick_files() } else { dialog.pick_file().map(|path| vec![path]) };
            match picked {
                Some(paths) => json!({
                    "canceled": false,
                    "paths": paths.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>(),
                }),
                None => json!({ "canceled": true, "paths": [] }),
            }
        }
    }
}
