// 本机通道：核心（另一个进程里的 start.js）通过它回调外壳的 host 成员（§6）。
//
// 为什么不是 HTTP（原来这里跑的是 tiny_http + Bearer token）：监听要占一个端口、token 只
// 能证明「知道秘密」、外壳崩了还得靠 3 秒轮询才发现。换成本地套接字之后，EOF 就是外壳
// 死亡信号，对端还能按 pid 认亲；协议只有一层帧——4 字节小端长度 + UTF-8 JSON，两端各几十
// 行，不需要 HTTP 的那一大半复杂度。名字、权限、鉴权的取舍见 docs/shell-ipc-plan.md 方案 B。
//
// 同步能力（剪贴板、对话框、通知、打开链接）都在这里直接做，不需要惊动界面线程；
// 只有「把面板叫出来」「退出」「重启」这类要动窗口的事才发事件给主循环。

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use interprocess::local_socket::{prelude::*, ListenerOptions};
// 名字类型按平台各取一个：Windows 用命名空间名（管道），unix 用文件路径。
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use serde_json::{json, Value};

use crate::prefs::Prefs;
use crate::supervisor::Status;

/// 单帧上限，和 host-remote.js 的 MAX_FRAME_BYTES 一致：超了说明流已经错位（或者对面在使坏），
/// 断连比继续读安全——不能让一个恶意长度把内存吃光。
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// 从通道发回主循环的事件。
pub enum Event {
    Show,
    Exit,
    Relaunch,
    /// 核心推来的面板模型（连上时拉一次，之后状态一变就推）。
    Panel(crate::model::PanelModel),
    /// 要显示给用户的一句话（面板拉模型失败之类）。ok 为假时按错误配色显示。
    Notice { ok: bool, message: String },
    /// 核心那条连接断了（被停掉、崩了、或正在重启）。面板据此把「运行中」的旧模型
    /// 标成不可操作，别让用户点下去才看到「核心还没连上外壳通道」。
    CoreDown,
}

/// 外壳反向调核心时的等待上限。面板方法是「查一下状态」，慢过 3 秒就说明对面不是这套协议。
pub const REPLY_TIMEOUT_MS: u64 = 3_000;
/// 启动项动作要真去拉进程、装版本，给足时间；核心自己对 busy 有锁，再久也没意义。
pub const ACTION_TIMEOUT_MS: u64 = 60_000;

/// 反向调用：外壳 → 核心（panel/state、panel/action、panel/settings）。
///
/// 帧形状不变，靠 method 字段分流：带 method 的是请求，带 id 没 method 的是回包。
/// 连接对象放在这里而不是每个调用方各存一份：连接是懒的，谁先说话谁建连。
pub struct Link {
    conn: Mutex<Option<Arc<Conn>>>,
    pending: Mutex<HashMap<u64, Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
}

impl Link {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            conn: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn connected(&self) -> bool {
        self.conn.lock().map(|conn| conn.is_some()).unwrap_or(false)
    }

    fn attach(&self, conn: &Arc<Conn>) {
        if let Ok(mut slot) = self.conn.lock() {
            *slot = Some(Arc::clone(conn));
        }
    }

    /// 只摘自己那条：核心重启时新连接可能已经接上，旧连接的 EOF 不能把新连接清掉。
    fn detach(&self, conn: &Arc<Conn>) {
        if let Ok(mut slot) = self.conn.lock() {
            let ours = slot.as_ref().is_some_and(|current| Arc::ptr_eq(current, conn));
            if ours {
                *slot = None;
            }
        }
    }

    /// 发一个请求等回包。超时或没有连接都返回 Err——调用方负责把它变成界面上的提示。
    pub fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let conn = match self.conn.lock() {
            Ok(slot) => slot.clone(),
            Err(_) => None,
        };
        let Some(conn) = conn else {
            // 这句话会原样显示在面板的提示条里，所以要能照着做：不是「稍后再试」，
            // 而是「核心没在跑」——去托盘或面板 header 点启动。
            return Err("核心没有在运行（外壳通道已断开），先启动核心再试".to_string());
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, tx);
        }
        conn.send(&serde_json::json!({ "id": id, "method": method, "params": params }));
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.forget(id);
                Err(format!("核心 {timeout:?} 内没有回应 {method}，可能需要更新核心"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.forget(id);
                Err("核心的回应丢了".to_string())
            }
        }
    }

    /// 启动项动作：参数与托盘菜单完全一致，核心那边走同一个 trayAction。
    pub fn action(
        &self,
        id: &str,
        action: &str,
        target_id: &str,
        instance_id: &str,
    ) -> Result<Value, String> {
        self.call(
            "panel/action",
            serde_json::json!({
                "id": id,
                "action": action,
                "targetId": target_id,
                "instanceId": instance_id,
            }),
            Duration::from_millis(ACTION_TIMEOUT_MS),
        )
    }

    /// 改系统开关：白名单在核心（server.js 的 PANEL_SETTINGS_KEYS），外壳只管转发。
    pub fn settings(&self, patch: Value) -> Result<Value, String> {
        self.call(
            "panel/settings",
            serde_json::json!({ "patch": patch }),
            Duration::from_millis(REPLY_TIMEOUT_MS),
        )
    }

    /// 连接断了：把所有等待方叫醒，别让它们干等到超时。
    fn fail_pending(&self) {
        let waiters = self
            .pending
            .lock()
            .map(|mut pending| pending.drain().map(|(_, waiter)| waiter).collect::<Vec<_>>())
            .unwrap_or_default();
        for waiter in waiters {
            let _ = waiter.send(Err("外壳通道断了".to_string()));
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    /// 收到回包：交给对应请求的等待方。没登记的 id 直接丢（可能是超时之后才到的迟到回包）。
    fn fulfill(&self, frame: Value) {
        let Some(id) = frame.get("id").and_then(Value::as_u64) else {
            return;
        };
        let waiter = self.pending.lock().ok().and_then(|mut pending| pending.remove(&id));
        let Some(waiter) = waiter else {
            return;
        };
        let result = if frame.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            Ok(frame.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(frame
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("核心返回了失败，但没说原因")
                .to_string())
        };
        let _ = waiter.send(result);
    }
}

pub struct BridgeCtx {
    pub status: Arc<Mutex<Status>>,
    pub prefs: Arc<Mutex<Prefs>>,
    pub events: Sender<Event>,
    pub shell_version: String,
    pub node_version: String,
    /// 当前核心进程的 pid：由 supervisor 每次 spawn 后写进来，用来认连接的对端。
    pub core_pid: Arc<AtomicU32>,
    /// 反向调用的通道句柄：面板拿它问状态、点按钮。
    pub link: Arc<Link>,
    /// 已经拿到过一份面板模型：之后的连接拉不到模型时不再弹「核心过旧」，
    /// 免得第二个连接（排障工具、核心重连）把好模型盖成一条吓人的提示。
    pub has_model: AtomicBool,
    /// egui 上下文：窗口藏着时主循环没有新的重绘请求（隐藏时只跑 logic、不跑 ui，
    /// 也就没人再 request_repaint_after），核心这一侧推进来的事件就得自己敲一下，
    /// 否则「打开面板」要等到下一次偶然的重绘才生效。由 main 在 creator 里填。
    pub repaint: Arc<Mutex<Option<eframe::egui::Context>>>,
}

/// 发事件 + 叫醒界面。所有往主循环送事件的地方都走这里，别直接 events.send。
fn emit(ctx: &BridgeCtx, event: Event) {
    let _ = ctx.events.send(event);
    if let Ok(guard) = ctx.repaint.lock() {
        if let Some(ui) = guard.as_ref() {
            ui.request_repaint();
        }
    }
}

/// 起通道；返回交给核心的名字（写进 DSH_SHELL_IPC）。
/// Windows 是 `\\.\pipe\dsh-x-<pid>-<8 位随机>`，unix 是 `<APP_DIR>/ipc/shell-<pid>.sock`。
pub fn start(ctx: BridgeCtx) -> io::Result<String> {
    let ctx = Arc::new(ctx);
    let (name, listener) = listen()?;
    if std::env::var("DSH_SHELL_IPC_ALLOW_ANY").is_ok_and(|value| value == "1") {
        eprintln!("外壳通道放开了对端校验（DSH_SHELL_IPC_ALLOW_ANY=1）：任何本机进程都能连，只用于排障");
    }
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => {
                    let ctx = Arc::clone(&ctx);
                    std::thread::spawn(move || serve(stream, ctx));
                }
                Err(error) => eprintln!("外壳通道接受了坏连接：{error}"),
            }
        }
    });
    Ok(name)
}

/// Windows：命名管道。名字里带 pid 与随机串——pid 让核心认出「这是这次运行的那条」，
/// 随机串只防误连与抢先建同名管道，不当安全边界（安全边界是对端 pid 校验）。
#[cfg(windows)]
fn listen() -> io::Result<(String, LocalSocketListener)> {
    let name = format!("dsh-x-{}-{}", std::process::id(), random_suffix());
    let listener = ListenerOptions::new()
        .name(name.as_str().to_ns_name::<GenericNamespaced>()?)
        .create_sync()?;
    // Node 的 net.connect({ path }) 直接认管道全名。
    Ok((format!(r"\\.\pipe\{name}"), listener))
}

/// unix：用户目录下的域套接字。目录 0700、套接字 0600——通道只给同一个用户，
/// 这比 token 强一档（不靠秘密，靠文件系统权限 + 对端 pid）。
#[cfg(unix)]
fn listen() -> io::Result<(String, LocalSocketListener)> {
    use std::os::unix::fs::PermissionsExt;

    let dir = crate::prefs::app_dir().join("ipc");
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    let path = dir.join(format!("shell-{}.sock", std::process::id()));
    // 上一轮异常退出可能留下同名文件：先删，否则 bind 报 EADDRINUSE。
    let _ = std::fs::remove_file(&path);
    let name = path.to_string_lossy().to_string();
    let listener = ListenerOptions::new()
        .name(name.as_str().to_fs_name::<GenericFilePath>()?)
        .create_sync()?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok((name, listener))
}

/// 一条连接。写要串行：帧头与帧体必须连着写进去，两个线程交叉写就会把流切碎。
struct Conn {
    stream: LocalSocketStream,
    write: Mutex<()>,
}

impl Conn {
    fn send(&self, value: &Value) {
        let body = value.to_string();
        let bytes = body.as_bytes();
        if bytes.len() > MAX_FRAME_BYTES {
            eprintln!("外壳通道要发的帧太大（{} 字节），已丢弃", bytes.len());
            return;
        }
        let _guard = self.write.lock().unwrap();
        let mut writer = &self.stream;
        if writer.write_all(&(bytes.len() as u32).to_le_bytes()).is_err() {
            return;
        }
        let _ = writer.write_all(bytes);
    }
}

fn serve(stream: LocalSocketStream, ctx: Arc<BridgeCtx>) {
    let Some(trust) = peer_trust(&stream, &ctx) else {
        // 不是我们拉起来的那个进程：一个字节都不回，别让外面探出「这里有个 DSH-X」。
        eprintln!("外壳通道拒绝了来源不明的连接");
        return;
    };
    let conn = Arc::new(Conn { stream, write: Mutex::new(()) });
    // 只有真核心配当反向调用通道。排障工具（DSH_SHELL_IPC_ALLOW_ANY=1）也能连、也能下指令，
    // 但它一接上就被当成核心，面板点按钮会发到它那儿（它没有 handler，只能回错误），
    // 面板于是显示「核心还没连上外壳通道」——踩过，所以这里分开。
    if trust == Trust::Core {
        ctx.link.attach(&conn);
    }
    // 连上先把外壳状态推一次：核心不用等下一次后台刷新才知道外壳版本。
    conn.send(&json!({ "event": "shellUpdate", "data": crate::updater::state() }));
    // 再问一次面板模型。核心连上时还没推过任何模型（它不知道面板要用），
    // 这一步让面板在核心刚起来的半秒内就有内容，而不用等下一次 emitState。
    if trust == Trust::Core {
        pull_panel(&ctx);
    }
    let mut reader = BufReader::new(&conn.stream);
    loop {
        match read_frame(&mut reader) {
            Ok(Some(frame)) => {
                // 三类帧：带 method 的是核心发来的请求；带 event 的是核心推的事件；
                // 剩下的（有 id 没 method）是我们自己请求的回包。
                if frame.get("method").and_then(Value::as_str).is_some() {
                    let conn = Arc::clone(&conn);
                    let ctx = Arc::clone(&ctx);
                    // 每帧一个线程：对话框、打开浏览器都会阻塞，不能让它们卡住后面的请求。
                    std::thread::spawn(move || handle_frame(&conn, &ctx, frame));
                } else if let Some(event) = frame.get("event").and_then(Value::as_str) {
                    handle_event(&ctx, event, frame.get("data"));
                } else {
                    ctx.link.fulfill(frame);
                }
            }
            // EOF：核心退出（或外壳自己关核心）了，这条连接没有继续的必要。
            Ok(None) => break,
            Err(error) => {
                eprintln!("外壳通道读帧失败：{error}");
                break;
            }
        }
    }
    if trust == Trust::Core {
        ctx.link.detach(&conn);
        // 还在等回包的调用立刻失败：核心已经走了，等到超时只会让界面多卡几秒。
        ctx.link.fail_pending();
        // 让面板知道核心没了：模型是上一份的，按钮得灰掉，状态不能停在「运行中」。
        emit(&ctx, Event::CoreDown);
    }
}

/// 连上之后主动拉一次模型。拉不到多半是核心比外壳旧（没有 panel 方法），
/// 这时候给一句能照着做的提示，而不是让面板空着。
fn pull_panel(ctx: &Arc<BridgeCtx>) {
    let ctx = Arc::clone(ctx);
    std::thread::spawn(move || {
        let result = ctx
            .link
            .call("panel/state", json!({}), Duration::from_millis(REPLY_TIMEOUT_MS));
        match result {
            Ok(value) => match crate::model::PanelModel::parse(value) {
                Ok(model) => {
                    // 拿到模型就记一笔：后到的连接拉不到模型时不再弹「核心过旧」，
                    // 免得第二个连接（排障工具、核心重连）把好模型盖成一条吓人的提示。
                    ctx.has_model.store(true, Ordering::Relaxed);

                    emit(&ctx, Event::Panel(model));
                }
                Err(error) => eprintln!("核心回的面板模型读不懂：{error}"),
            },
            Err(error) => {
                eprintln!("拉面板模型失败：{error}");
                if ctx.has_model.load(Ordering::Relaxed) {
                    return;
                }
                emit(&ctx, Event::Notice {
                    ok: false,
                    message: "核心没有回应面板查询，可能是核心版本过旧；请先更新核心。".to_string(),
                });
            }
        }
    });
}

/// 核心推来的事件。目前只有面板模型；不认识的协议不吭声（旧核心/新核心都别互相当故障）。
fn handle_event(ctx: &BridgeCtx, event: &str, data: Option<&Value>) {
    match event {
        "panel" => {
            let Some(data) = data else { return };
            match crate::model::PanelModel::parse(data.clone()) {
                Ok(model) => {
                    emit(&ctx, Event::Panel(model));
                }
                Err(error) => eprintln!("核心推来的面板模型读不懂：{error}"),
            }
        }
        _ => {}
    }
}

/// 一条连接的可信级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Trust {
    /// 就是自己拉起的那个核心：既能下指令，也能当反向调用通道。
    Core,
    /// 排障工具（`scripts/ipc-call.mjs`，靠 DSH_SHELL_IPC_ALLOW_ANY=1 连进来）：
    /// 只让它下指令，不许它顶替核心收事件、收回包。
    Debug,
}

/// 认出对端是谁。core_pid 还是 0 时放行：那是「外壳刚起来、核心还没 spawn」，
/// 这时候能连进来的只有本机（管道名随机、套接字 0600），不值得为几毫秒的窗口误杀。
/// macOS 的 xucred 拿不到 pid，只能退回名字里的 pid 与目录权限。
///
/// `DSH_SHELL_IPC_ALLOW_ANY=1` 是排障开关：`scripts/ipc-call.mjs` 是个独立进程，pid 永远对不上，
/// 想用它就得让外壳放开校验（默认关着，启动时会在 stderr 提醒）。
fn peer_trust(stream: &LocalSocketStream, ctx: &BridgeCtx) -> Option<Trust> {
    let expected = ctx.core_pid.load(Ordering::Relaxed);
    let matched = if expected == 0 {
        true
    } else {
        match stream.peer_creds().ok().and_then(|creds| creds.pid()) {
            Some(pid) => pid as u32 == expected,
            // 拿不到 pid 的平台（macOS 的 xucred）只能信名字里的 pid 与目录权限。
            None => true,
        }
    };
    if matched {
        return Some(Trust::Core);
    }
    if std::env::var("DSH_SHELL_IPC_ALLOW_ANY").is_ok_and(|value| value == "1") {
        return Some(Trust::Debug);
    }
    None
}

/// 读一帧。返回 Ok(None) 表示对端正常关闭（EOF 正好落在帧边界上）。
fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("帧长度 {length} 不合法"),
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn handle_frame(conn: &Conn, ctx: &BridgeCtx, frame: Value) {
    let id = frame.get("id").cloned().unwrap_or(Value::Null);
    let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
    let params = frame.get("params").cloned().unwrap_or(Value::Null);
    match route(ctx, method, &params) {
        Ok(result) => conn.send(&json!({ "id": id, "ok": true, "result": result })),
        Err(error) => conn.send(&json!({ "id": id, "ok": false, "error": error })),
    }
}

/// 动作表。方法名与 host-remote.js 里的调用一一对应（原来是 `POST /host/<方法>`）。
fn route(ctx: &BridgeCtx, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "open" => {
            let url = params.get("url").and_then(Value::as_str).unwrap_or("");
            open_url(url);
            Ok(json!({ "ok": true }))
        }
        "wake" => {
            // 面板叫到前面；管理页没有内嵌窗口，地址交给系统浏览器。
            emit(&ctx, Event::Show);
            let url = ctx.status.lock().unwrap().url.clone();
            if !url.is_empty() {
                let _ = open::that(url);
            }
            Ok(json!({ "ok": true }))
        }
        "notify" => {
            notify(ctx, params);
            Ok(json!({ "ok": true }))
        }
        "exit" => {
            emit(&ctx, Event::Exit);
            Ok(json!({ "ok": true }))
        }
        "relaunch" => {
            emit(&ctx, Event::Relaunch);
            Ok(json!({ "ok": true }))
        }
        "info" => Ok(json!({ "info": info(ctx) })),
        "desktop/notify" => {
            notify(ctx, params);
            Ok(json!({ "ok": true }))
        }
        "desktop/open" => {
            let url = params.get("url").and_then(Value::as_str).unwrap_or("");
            open_url(url);
            Ok(json!({ "ok": true }))
        }
        "desktop/reveal" => {
            let path = params.get("path").and_then(Value::as_str).unwrap_or("");
            if !path.is_empty() {
                let _ = open::that_detached(path);
            }
            Ok(json!({ "ok": true }))
        }
        "desktop/clipboard" => {
            let text = params.get("text").and_then(Value::as_str).unwrap_or("");
            match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.to_string())) {
                Ok(()) => Ok(json!({ "ok": true })),
                Err(error) => Ok(json!({ "ok": false, "error": error.to_string() })),
            }
        }
        "desktop/dialog" => Ok(dialog(params)),
        "browser/clearCache" | "browser/clearAll" => {
            // 原生外壳没有内嵌浏览器，也就没有分区可清；插件看到 false 会自己降级。
            Ok(json!({ "ok": false, "error": "原生外壳没有内嵌浏览器分区" }))
        }
        "shellUpdate/state" => Ok(json!({ "state": crate::updater::state() })),
        "shellUpdate/check" => Ok(json!({ "state": crate::updater::check() })),
        "shellUpdate/download" | "shellUpdate/install" => {
            Ok(json!({ "state": crate::updater::state() }))
        }
        _ => Err(format!("外壳通道没有这个动作：{method}")),
    }
}

/// 只放 http(s)：核心已经过滤过一遍，这里再挡一次——通道是本机可访问的，不能当任意命令入口。
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
    crate::notify::send(title, body, prefs.notification_sound);
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
            // rfd 只有这三档按钮组合；桌面 API 文档里约定的用法最多就是「是/否/取消」。
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

/// 管道名里的随机串：不是密码学强度，只求别让两个进程撞名、别一眼猜到。
fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut seed = nanos as u64 ^ ((std::process::id() as u64) << 32);
    let mut suffix = String::new();
    for _ in 0..8 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let digit = (seed % 16) as u8;
        suffix.push(char::from(if digit < 10 { b'0' + digit } else { b'a' + digit - 10 }));
    }
    suffix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json_text: &str) -> Vec<u8> {
        let body = json_text.as_bytes();
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reads_frames_in_order() {
        let mut bytes = frame(r#"{"id":1,"method":"info"}"#);
        bytes.extend(frame(r#"{"id":2,"method":"wake"}"#));
        let mut reader = std::io::Cursor::new(bytes);
        let first = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(first["method"], "info");
        let second = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(second["method"], "wake");
        // 帧边界上的 EOF 是正常结束，不是错误。
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_oversized_and_empty_frames() {
        let mut too_big = ((MAX_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        too_big.extend_from_slice(b"{}");
        assert!(read_frame(&mut std::io::Cursor::new(too_big)).is_err());
        let zero = 0u32.to_le_bytes().to_vec();
        assert!(read_frame(&mut std::io::Cursor::new(zero)).is_err());
    }

    #[test]
    fn rejects_broken_json() {
        let mut bytes = frame("not json");
        bytes.push(b' ');
        assert!(read_frame(&mut std::io::Cursor::new(bytes)).is_err());
    }

    #[test]
    fn suffix_is_8_hex_chars() {
        let suffix = random_suffix();
        assert_eq!(suffix.len(), 8);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
        // 同一进程里两次也不能一样：管道名撞了，第二个外壳就抢不到名字。
        assert_ne!(suffix, random_suffix());
    }
}
