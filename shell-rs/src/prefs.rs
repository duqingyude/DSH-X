// 外壳直接读 settings.json，不走管理页接口：外壳和核心分开更新，读同一份文件比让两边
// 商量协议版本省事得多（和 Electron 外壳的 shell/prefs.mjs 同一套约定）。
// 默认值必须和 settings.js 的 DEFAULTS 一致——test/prefs-parity 会拿两边比对。

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Prefs {
    /// 关掉能规避显卡/驱动导致的白屏，代价是动画更吃 CPU；只有这项要重启外壳才生效。
    pub hardware_acceleration: bool,
    pub auto_update: bool,
    pub task_notifications: bool,
    pub notification_sound: bool,
    pub close_to_tray: bool,
    pub keep_awake: bool,
    /// bundled = 用启动器自带的那份（现在指系统里找到的 node，见 §5）；system = 用户指的那个。
    pub node_runtime: String,
    pub system_node_path: String,
    pub open_mode: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            hardware_acceleration: true,
            auto_update: false,
            task_notifications: true,
            notification_sound: true,
            close_to_tray: true,
            keep_awake: false,
            node_runtime: "bundled".to_string(),
            system_node_path: String::new(),
            open_mode: "tab".to_string(),
        }
    }
}

/// 每用户数据目录：%APPDATA%\DSH、~/Library/Application Support/DSH、$XDG_CONFIG_HOME/DSH。
pub fn app_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("AppData/Roaming")
        });
        return base.join("DSH");
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap_or_default();
        return PathBuf::from(home).join("Library/Application Support/DSH");
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("DSH")
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

impl Prefs {
    /// 读一份；文件不在、坏掉、字段类型不对都退回默认值——外壳不能因为设置文件坏掉就打不开。
    pub fn load() -> Self {
        read_prefs(&settings_path())
    }
}

fn read_prefs(path: &PathBuf) -> Prefs {
    let defaults = Prefs::default();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return defaults,
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return defaults,
    };
    let flag = |key: &str, fallback: bool| -> bool {
        // 走的是 settings.js 的同款语义：这类开关是「只要不是明确关掉就算开」，
        // 另一些是「必须明确打开才算开」，别把两种写法混成一个 truthy 判断。
        match value.get(key) {
            Some(serde_json::Value::Bool(flag)) => *flag,
            _ => fallback,
        }
    };
    let text_field = |key: &str, fallback: &str| -> String {
        value
            .get(key)
            .and_then(|item| item.as_str())
            .unwrap_or(fallback)
            .to_string()
    };
    Prefs {
        hardware_acceleration: flag("hardwareAcceleration", defaults.hardware_acceleration),
        auto_update: flag("autoUpdate", defaults.auto_update),
        task_notifications: flag("taskNotifications", defaults.task_notifications),
        notification_sound: flag("notificationSound", defaults.notification_sound),
        close_to_tray: flag("closeToTray", defaults.close_to_tray),
        keep_awake: flag("keepAwake", defaults.keep_awake),
        node_runtime: text_field("nodeRuntime", &defaults.node_runtime),
        system_node_path: text_field("systemNodePath", &defaults.system_node_path),
        open_mode: text_field("openMode", &defaults.open_mode),
    }
}
