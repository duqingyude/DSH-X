// 起/停/重启核心（就是仓库根上的 start.js）：外壳自己只管窗口、托盘、通道和更新，
// 业务全在核心的 server.js 里。跨进程之后「实例的端口」只有核心自己知道，所以这里还要
// 从它的 stdout 里读回管理页地址（server.js 启动时打的 dsh-versions: http://... 那行）。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::prefs;

// 只有 Windows 要这个扩展 trait：CREATE_NO_WINDOW 从它来。
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 起子进程时把控制台藏掉。外壳是 GUI 进程，它 spawn 一个控制台程序（node / cmd）时
/// Windows 会**给子进程新开一个控制台窗口**——用户看到的就是一闪而过的 CMD 黑框。
/// 所有从外壳起的子进程都要过这里：核心进程、探测版本的 `node --version` 都算。
fn no_window(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
}

/// dsh 与主进程要的 Node 下限：低于它的 Node 连 ESM 补丁钩子都装不上（§5）。
pub const MIN_NODE: (u32, u32, u32) = (22, 18, 0);
/// 日志尾巴留多少行：够面板显示、不够撑爆内存。
const TAIL_LINES: usize = 200;
/// 一分钟内连续失败这么多次就不再自动重启，免得把机器拖死。
const MAX_RESTARTS: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Starting,
    Running,
    Stopped,
    Failed,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Idle => "未启动",
            Phase::Starting => "启动中",
            Phase::Running => "运行中",
            Phase::Stopped => "已停止",
            Phase::Failed => "启动失败",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    pub phase: Phase,
    pub url: String,
    pub pid: u32,
    pub error: String,
    pub tail: VecDeque<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            url: String::new(),
            pid: 0,
            error: String::new(),
            tail: VecDeque::new(),
        }
    }
}

impl Status {
    pub fn push_line(&mut self, line: String) {
        if self.tail.len() >= TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

/// 外壳报给核心的几样东西：本机通道名、自己的可执行文件路径，以及核心自己的 pid
/// （通道那边按它认对端，见 bridge.rs 的 peer_is_core）。环境变量表见 §6。
pub struct ShellEnv {
    pub ipc: String,
    pub exe: String,
    pub core_pid: Arc<AtomicU32>,
}

pub struct Supervisor {
    pub status: Arc<Mutex<Status>>,
    node: PathBuf,
    core_dir: PathBuf,
    shell: ShellEnv,
    stop: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Supervisor {
    /// status 由外面传进来：桥和面板看的是同一份状态，不能让核心线程自己攒一份。
    pub fn new(
        node: PathBuf,
        core_dir: PathBuf,
        shell: ShellEnv,
        status: Arc<Mutex<Status>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            status,
            node,
            core_dir,
            shell,
            stop: Arc::new(AtomicBool::new(false)),
            restart: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
        })
    }

    /// 起监督线程。线程自己负责「崩了就重启」，调用方只看 status。
    pub fn start(self: &Arc<Self>) {
        let this = Arc::clone(self);
        thread::spawn(move || this.run_loop());
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.kill_child();
    }

    pub fn restart(&self) {
        self.restart.store(true, Ordering::SeqCst);
        self.kill_child();
    }

    /// 面板与托盘将来要「直接打开管理页」时用；现在由桥读 status.url，先留接口。
    #[allow(dead_code)]
    pub fn address(&self) -> String {
        self.status.lock().unwrap().url.clone()
    }

    fn kill_child(&self) {
        let mut slot = self.child.lock().unwrap();
        if let Some(child) = slot.as_mut() {
            // 子进程自己会收 SIGTERM / 被 TerminateProcess，server.js 的收尾逻辑在这里跑
            let _ = child.kill();
        }
        if let Some(mut child) = slot.take() {
            let _ = child.wait();
        }
    }

    fn run_loop(self: Arc<Self>) {
        let mut failures = 0u32;
        let mut last_failure = Instant::now();
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            if self.restart.swap(false, Ordering::SeqCst) {
                failures = 0;
            }
            let reason = self.spawn_once();
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            {
                let mut status = self.status.lock().unwrap();
                status.phase = Phase::Stopped;
                status.pid = 0;
                if let Some(reason) = &reason {
                    status.error = reason.clone();
                    status.push_line(format!("核心退出：{reason}"));
                }
            }
            if Instant::now().duration_since(last_failure) > Duration::from_secs(60) {
                failures = 0;
            }
            failures += 1;
            last_failure = Instant::now();
            if failures > MAX_RESTARTS {
                let mut status = self.status.lock().unwrap();
                status.phase = Phase::Failed;
                if status.error.is_empty() {
                    status.error = "核心反复退出，已停止自动重启".to_string();
                }
                break;
            }
            thread::sleep(Duration::from_millis(800));
        }
    }

    /// 起一次核心，阻塞到它退出；返回退出原因（正常退出返回 None）。
    fn spawn_once(&self) -> Option<String> {
        {
            let mut status = self.status.lock().unwrap();
            status.phase = Phase::Starting;
            status.error.clear();
            status.url.clear();
        }
        let mut command = Command::new(&self.node);
        command
            .arg("start.js")
            .current_dir(&self.core_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 本机通道名：Windows 是命名管道全名，unix 是域套接字路径，核心直接拿去 net.connect。
            .env("DSH_SHELL_IPC", &self.shell.ipc)
            .env("DSH_SHELL_PID", std::process::id().to_string())
            .env("DSH_SHELL_EXE", &self.shell.exe)
            // 外壳版本与「我懂这组设置」的标记：核心据此回显外壳版本、放开设置页的六个系统开关。
            .env("DSH_SHELL_VERSION", env!("CARGO_PKG_VERSION"))
            .env("DSH_SHELL_PREFS", "1")
            .env("DSH_RUNTIME_NODE", &self.node)
            // 核心按打包态运行：核心更新走 npm、安装包自更新交给外壳（见 AGENTS.md）。
            .env("DSH_CORE", "1");
        // 外壳是 GUI 程序，直接 spawn 控制台程序（node.exe）会另开一个黑框窗口。
        // CREATE_NO_WINDOW 让核心在后台跑；stdout/stderr 反正走管道，不靠这个控制台。
        no_window(&mut command);
        // 核心目录里的 node/ 有 npm / pnpm 包装，插件安装要用；用户自己的排在后面（核心侧还会再排）。
        let bundled = self.core_dir.join("node");
        if bundled.exists() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut dirs = vec![bundled];
            dirs.extend(std::env::split_paths(&path));
            if let Ok(joined) = std::env::join_paths(dirs) {
                command.env("PATH", joined);
            }
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Some(format!("起不来：{}（{}）", self.node.display(), error));
            }
        };
        let pid = child.id();
        // 通道按这个 pid 认对端：别的进程就算摸到管道名，也进不来。
        self.shell.core_pid.store(pid, Ordering::Relaxed);
        {
            let mut status = self.status.lock().unwrap();
            status.pid = pid;
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(stdout) = stdout {
            let this = self.status.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    this.lock().unwrap().push_line(line.clone());
                    if let Some(url) = parse_manager_url(&line) {
                        {
                            let mut status = this.lock().unwrap();
                            status.url = url.clone();
                            status.phase = Phase::Running;
                        }
                        remember_url(&url);
                    }
                }
            });
        }
        if let Some(stderr) = stderr {
            let this = self.status.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    this.lock().unwrap().push_line(line);
                }
            });
        }
        *self.child.lock().unwrap() = Some(child);

        let mut exited: Option<std::process::ExitStatus> = None;
        let mut poll_error: Option<String> = None;
        loop {
            if self.stop.load(Ordering::SeqCst) {
                self.kill_child();
                return None;
            }
            let mut slot = self.child.lock().unwrap();
            let polled = match slot.as_mut() {
                Some(child) => child.try_wait(),
                None => Ok(None),
            };
            match polled {
                Ok(Some(status)) => {
                    slot.take();
                    exited = Some(status);
                }
                Ok(None) => {}
                Err(error) => {
                    slot.take();
                    poll_error = Some(error.to_string());
                }
            }
            drop(slot);
            if exited.is_some() || poll_error.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if let Some(error) = poll_error {
            return Some(format!("等核心退出时出错：{error}"));
        }
        match exited {
            Some(status) if status.success() => None,
            Some(status) => Some(format!("退出码 {}", status.code().map(|code| code.to_string()).unwrap_or_default())),
            None => None,
        }
    }
}

/// 记下管理页地址：用户再点一次图标（单实例锁把新进程挡下）时要靠它把浏览器叫到已经在跑的那一页。
fn remember_url(url: &str) {
    let path = prefs::app_dir().join("shell-url.txt");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, url);
}

/// 第二个实例被单实例锁挡下时走这里：地址还活着才开浏览器，免得开出一个死页面。
pub fn open_running_manager() {
    let path = prefs::app_dir().join("shell-url.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let url = text.trim();
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return;
    };
    let port = match rest.split('/').next().unwrap_or_default().parse::<u16>() {
        Ok(port) => port,
        Err(_) => return,
    };
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    // 上一轮的外壳可能已经退出了：连不上就当没有，别让用户看到一个空页。
    if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(400)).is_ok() {
        let _ = open::that(url);
    }
}

/// 从核心的启动行里读管理页地址。只认 dsh-versions 那一行：别的日志里也会打印 127.0.0.1。
pub fn parse_manager_url(line: &str) -> Option<String> {
    let index = line.find("dsh-versions: ")?;
    let rest = line[index + "dsh-versions: ".len()..].trim();
    if rest.starts_with("http://127.0.0.1:") || rest.starts_with("http://localhost:") {
        return Some(rest.to_string());
    }
    None
}

/// 找 node：显式环境变量 → 设置里指的系统 node → PATH → 各平台常见安装位置。
pub fn find_node(prefs: &prefs::Prefs) -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("DSH_RUNTIME_NODE") {
        let explicit = explicit.trim().trim_matches('"');
        if !explicit.is_empty() {
            let path = PathBuf::from(explicit);
            if usable(&path) {
                return Some(path);
            }
        }
    }
    if prefs.node_runtime == "system" {
        if let Some(found) = locate(&prefs.system_node_path) {
            return Some(found);
        }
    }
    if let Some(found) = which_node() {
        return Some(found);
    }
    for dir in common_dirs() {
        let candidate = dir.join(node_name());
        if usable(&candidate) {
            return Some(candidate);
        }
    }
    if let Some(found) = scan_version_managers() {
        return Some(found);
    }
    None
}

fn node_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

fn usable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match node_version(path) {
        Some(version) => version_ok(&version),
        None => false,
    }
}

/// 用户显式填的路径：允许带引号、只写命令名（这时按 PATH 找）。
fn locate(given: &str) -> Option<PathBuf> {
    let given = given.trim().trim_matches('"').trim();
    if given.is_empty() {
        return None;
    }
    let path = PathBuf::from(given);
    if path.is_absolute() || given.contains('/') || given.contains('\\') {
        return if usable(&path) { Some(path) } else { None };
    }
    which_node_named(given)
}

fn which_node() -> Option<PathBuf> {
    which_node_named(node_name())
}

fn which_node_named(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if usable(&candidate) {
            return Some(candidate);
        }
        if cfg!(windows) && !name.ends_with(".exe") {
            let exe = dir.join(format!("{name}.exe"));
            if usable(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

fn common_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(program_files).join("nodejs"));
        }
        if let Ok(program_files) = std::env::var("ProgramFiles(x86)") {
            dirs.push(PathBuf::from(program_files).join("nodejs"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs/nodejs"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
    } else {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/usr/local/node/bin"));
    }
    dirs
}

/// nvm / fnm / volta 这些版本管理器：装的位置不固定，扫一遍各自目录下最新的一版。
fn scan_version_managers() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(&appdata).join("nvm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(&local).join("fnm/node-versions"));
            roots.push(PathBuf::from(&local).join("Volta/bin"));
        }
    } else {
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".nvm/versions/node"));
            roots.push(PathBuf::from(&home).join(".local/share/fnm/node-versions"));
            roots.push(PathBuf::from(&home).join(".volta/bin"));
        }
    }
    let mut found: Option<(u64, PathBuf)> = None;
    for root in roots {
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let base = entry.path();
            let candidate = if base.is_dir() { base.join(node_name()).exists().then(|| base.join(node_name())).or_else(|| base.join("installation/bin").join(node_name()).exists().then(|| base.join("installation/bin").join(node_name()))) } else { Some(base) };
            if let Some(candidate) = candidate {
                if let Some(version) = node_version(&candidate) {
                    if version_ok(&version) {
                        let score = version_score(&version);
                        if found.as_ref().map(|(best, _)| score > *best).unwrap_or(true) {
                            found = Some((score, candidate));
                        }
                    }
                }
            }
        }
    }
    found.map(|(_, path)| path)
}

/// 一次启动里 `node --version` 会被问好几遍（找 node 时探测每个候选、面板要显示版本）。
/// 每次都起一个进程既慢又会闪黑框，所以按路径记住结果——同一个进程里 node 的版本不会变。
fn version_cache() -> &'static Mutex<std::collections::HashMap<PathBuf, Option<String>>> {
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, Option<String>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn node_version(node: &Path) -> Option<String> {
    // 缓存坏了（别的线程 panic 过）不该让启动失败：直接当空表用。
    let mut cache = version_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(cached) = cache.get(node) {
        return cached.clone();
    }
    let version = probe_version(node);
    cache.insert(node.to_path_buf(), version.clone());
    version
}

fn probe_version(node: &Path) -> Option<String> {
    let mut command = Command::new(node);
    command.arg("--version");
    // 不藏控制台的话，每次探测都会在屏幕上闪一个 CMD 窗口（GUI 父进程 spawn 控制台程序）。
    no_window(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if parse_version(&text).is_some() {
        Some(text)
    } else {
        None
    }
}

/// "v22.18.0" / "22.18.0" 都认；预发布后缀（-rc.1）直接忽略，比的是主版本号。
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let text = text.trim().trim_start_matches('v');
    let mut parts = text.split('.').take(3);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn version_score(version: &str) -> u64 {
    let (major, minor, patch) = parse_version(version).unwrap_or((0, 0, 0));
    (major as u64) << 32 | (minor as u64) << 16 | patch as u64
}

/// 和支持的版本比：低于 22.18 一律不认（22.18 之前没有 --import 的 ESM 钩子修补能力）。
pub fn version_ok(version: &str) -> bool {
    match parse_version(version) {
        Some((major, minor, patch)) => (major, minor, patch) >= MIN_NODE,
        None => false,
    }
}

/// 核心目录：外壳明确指定 → 用户目录里更新过的那份 → 安装包基线 → 源码运行（仓库根）。
pub fn core_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("DSH_CORE_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join("start.js").is_file() {
            return Some(dir);
        }
    }
    let current = prefs::app_dir().join("core").join("current");
    if current.join("start.js").is_file() {
        return Some(current);
    }
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    // 安装后的几个候选位置，按打包格式不同而不同：NSIS / AppImage 把 resources 放在可执行
    // 文件旁边（exe 同级的 core/），deb 放在 usr/lib，macOS 的 .app 放在 Contents/Resources。
    // 旧 Electron 安装包是 resources/core，保留一档省得老用户更新外壳后找不到核心。
    let candidates = [
        exe_dir.join("core"),
        exe_dir.join("resources").join("core"),
        exe_dir.join("..").join("Resources").join("core"),
        exe_dir.join("..").join("lib").join("core"),
    ];
    for candidate in candidates {
        if candidate.join("start.js").is_file() {
            return Some(candidate);
        }
    }
    // 源码运行（cargo run / cargo test）：可执行文件在 shell-rs/target/{debug,release} 或
    // 再下一层 deps，往上有几层就找几层——第一个带 start.js 的祖先就是仓库根。
    let mut current = Some(exe_dir);
    for _ in 0..5 {
        let Some(dir) = current else { break };
        if dir.join("start.js").is_file() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_url_only_from_dsh_line() {
        // 启动行前面可能有别的前缀（时间戳、日志级别），地址本身在最后。
        assert_eq!(
            parse_manager_url("dsh-versions: http://127.0.0.1:52341"),
            Some("http://127.0.0.1:52341".to_string())
        );
        assert_eq!(
            parse_manager_url("[12:00:00] dsh-versions: http://localhost:8080"),
            Some("http://localhost:8080".to_string())
        );
        // 别的日志里也会打印 127.0.0.1，不能把它当成管理页地址。
        assert_eq!(
            parse_manager_url("listening on http://127.0.0.1:52341"),
            None
        );
        assert_eq!(parse_manager_url("dsh-versions: https://example.com"), None);
        assert_eq!(parse_manager_url(""), None);
    }

    #[test]
    fn version_gate_matches_min_node() {
        assert!(version_ok("v22.18.0"));
        assert!(version_ok("v24.19.0"));
        assert!(version_ok("22.18.0-rc.1"));
        assert!(!version_ok("v22.17.9"));
        assert!(!version_ok("v20.11.1"));
        assert!(!version_ok("不是版本号"));
    }

    #[test]
    fn version_score_orders_like_versions() {
        assert!(version_score("v24.1.0") > version_score("v22.18.0"));
        assert!(version_score("v22.20.0") > version_score("v22.18.3"));
    }


}