// 面板模型：核心 panelState()（server.js:1666）的原样投影。
//
// 为什么不在这里自己算一遍：起停、端口、装没装、失败原因全在核心，外壳再算一份只会漂。
// 所以外壳这边只有「解析 + 展示」——字段名与 server.js 一一对应，改一边必须改另一边
// （test/panel-model.test.mjs 与这里末尾的单测对的是同一份形状）。

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PanelModel {
    pub core: Core,
    pub shell: Shell,
    pub node: Node,
    pub prefs: Prefs,
    pub presets: Vec<Preset>,
    pub instances: Vec<Instance>,
    /// 安装 / 插件进度：形状跟着核心走（kind = install | plugin），这里不当结构化数据用。
    pub progress: Option<serde_json::Value>,
    pub failure: Option<Failure>,
    /// 核心侧生成时刻（毫秒）。外壳用它判断「这份模型是不是比手上的新」。
    pub updated_at: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Core {
    pub version: String,
    pub profile: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shell {
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Node {
    pub path: String,
    pub version: String,
}

/// 六个系统开关 + 界面语言。值的默认写法与 settings.js 的 DEFAULTS 一致：
/// 四项「不是明确 false 就算开」，两项「明确 true 才算开」。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub hardware_acceleration: bool,
    pub auto_update: bool,
    pub task_notifications: bool,
    pub notification_sound: bool,
    pub close_to_tray: bool,
    pub keep_awake: bool,
    pub node_runtime: String,
    pub system_node_path: String,
    pub open_mode: String,
    pub theme: String,
    pub lang: String,
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
            node_runtime: "system".to_string(),
            system_node_path: String::new(),
            open_mode: "tab".to_string(),
            theme: "auto".to_string(),
            lang: "zh".to_string(),
        }
    }
}

/// 面板里的一行：一个启动项或一个没有对应启动项的实例（核心在 trayLaunches() 里摊平）。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preset {
    pub id: String,
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub profile: String,
    pub port: u32,
    pub status: String,
    pub url: String,
    pub busy: bool,
    pub installed: bool,
    /// 动作回传用的目标标识：核心拿它挡「菜单过期后误操作另一个版本」。
    pub target_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Instance {
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub profile: String,
    pub home: String,
    pub status: String,
    pub url: String,
    pub started_at: f64,
    pub startup_ms: Option<f64>,
    pub startup_phase: String,
    pub diagnostic_warning: Option<serde_json::Value>,
}

/// 最近一次启动失败（publicFailure 的形状；多余字段忽略，这里只用面板要显示的几项）。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Failure {
    pub at: f64,
    pub version: String,
    pub profile: Option<String>,
    pub home: Option<String>,
    pub message: String,
    pub exit: Option<i32>,
    pub tail: Vec<String>,
}

impl PanelModel {
    /// 解析核心给的模型。
    ///
    /// 先删掉对象里的 null：核心用 null 表示「这项还不知道」（实例刚起来还没有 url、
    /// 失败记录没有 profile 与 home），而结构体上的 #[serde(default)] 只管「键不存在」，
    /// 管不了「键在但是 null」——一个 null 就会让整份模型解析失败，面板停在上一帧。
    /// 删掉等于没给，正好走默认值。
    pub fn parse(mut value: serde_json::Value) -> Result<Self, String> {
        prune_nulls(&mut value);
        serde_json::from_value(value).map_err(|error| error.to_string())
    }

    /// 有几行在跑：header 的「N 个实例运行中」用它，托盘菜单也用它。
    pub fn running(&self) -> usize {
        self.presets.iter().filter(|row| row.status == "running").count()
    }

    /// 失败提示的第一句：核心已经把密钥与 ANSI 洗过了，外壳直接显示。
    pub fn failure_text(&self) -> Option<&str> {
        self.failure.as_ref().map(|failure| failure.message.as_str())
    }

    #[cfg(test)]
    pub fn preset(&self, id: &str) -> Option<&Preset> {
        self.presets.iter().find(|row| row.id == id)
    }
}

/// 递归删掉对象里的 null（见 PanelModel::parse 的理由）。数组里的 null 一并删，
/// 免得 tail 之类的地方留一个空行。
fn prune_nulls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, item| !item.is_null());
            for item in map.values_mut() {
                prune_nulls(item);
            }
        }
        serde_json::Value::Array(items) => {
            items.retain(|item| !item.is_null());
            for item in items.iter_mut() {
                prune_nulls(item);
            }
        }
        _ => {}
    }
}

impl Preset {
    pub fn is_running(&self) -> bool {
        self.status == "running"
    }

    /// 停在标题右侧的一句话：运行中显示端口，否则显示版本。
    pub fn caption(&self) -> String {
        if self.is_running() && self.port > 0 {
            format!("{}:{} · {}", self.profile, self.port, self.version)
        } else {
            format!("{} · {}", self.profile, self.version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 形状抄自 server.js 的 panelState()：字段名改了一边，这里就会红。
    const SAMPLE: &str = r#"
    {
      "core": { "version": "0.1.24", "profile": "web" },
      "shell": { "version": "0.1.24" },
      "node": { "path": "C:\\Program Files\\nodejs\\node.exe", "version": "24.19.0" },
      "prefs": {
        "hardwareAcceleration": true, "autoUpdate": false, "taskNotifications": true,
        "notificationSound": false, "closeToTray": true, "keepAwake": false,
        "nodeRuntime": "system", "systemNodePath": "C:\\nodejs\\node.exe",
        "openMode": "tab", "theme": "light", "lang": "en"
      },
      "presets": [
        { "id": "a", "instanceId": "k1", "name": "DSH", "version": "0.2.1-rc.4",
          "profile": "web", "port": 3799, "status": "running", "url": "http://127.0.0.1:3799",
          "busy": false, "installed": true, "targetId": "t1" },
        { "id": "b", "instanceId": "k2", "name": "工作", "version": "0.2.0",
          "profile": "desk", "port": 0, "status": "stopped", "url": "",
          "busy": false, "installed": false, "targetId": "" }
      ],
      "instances": [
        { "instanceId": "k1", "name": "DSH", "version": "0.2.1-rc.4", "profile": "web",
          "home": "C:\\Users\\u\\.dsh", "status": "running", "url": "http://127.0.0.1:3799",
          "startedAt": 1.0, "startupMs": 2400.0, "startupPhase": "", "diagnosticWarning": null }
      ],
      "progress": { "kind": "install", "version": "0.2.1-rc.4", "percent": 40 },
      "failure": { "at": 2.0, "version": "0.2.1-rc.4", "profile": "web", "home": null,
        "message": "端口被占用", "exit": 1, "tail": ["EADDRINUSE"], "diagnosis": null },
      "updatedAt": 1700000000000
    }
    "#;

    #[test]
    fn parses_the_core_shape() {
        let model: PanelModel = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(model.core.version, "0.1.24");
        assert_eq!(model.core.profile, "web");
        assert_eq!(model.shell.version, "0.1.24");
        assert_eq!(model.node.version, "24.19.0");
        assert!(model.prefs.hardware_acceleration);
        assert!(!model.prefs.notification_sound);
        assert_eq!(model.prefs.lang, "en");
        assert_eq!(model.presets.len(), 2);
        assert_eq!(model.presets[0].port, 3799);
        assert_eq!(model.presets[0].target_id, "t1");
        assert_eq!(model.instances.len(), 1);
        assert_eq!(model.instances[0].startup_ms, Some(2400.0));
        assert!(model.progress.is_some());
        assert_eq!(model.failure_text(), Some("端口被占用"));
        assert_eq!(model.updated_at, 1_700_000_000_000.0);
        assert_eq!(model.running(), 1);
    }

    #[test]
    fn null_strings_fall_back_to_defaults() {
        // 核心给 null 表示「还不知道」：实例刚起来还没有 url、失败记录没有 profile/home。
        // 一个 null 不能让整份模型作废（面板会停在上一帧，连启动项都点不动）。
        let raw = r#"{
            "core": { "version": "0.1.24", "profile": null },
            "node": { "path": null, "version": null },
            "presets": [{ "id": "p1", "name": "DSH", "url": null, "port": null }],
            "instances": [{ "instanceId": "i1", "url": null, "profile": null }],
            "failure": { "message": "端口被占用", "profile": null, "home": null }
        }"#;
        let model = PanelModel::parse(serde_json::from_str(raw).unwrap()).unwrap();
        assert_eq!(model.core.version, "0.1.24");
        assert_eq!(model.core.profile, "");
        assert_eq!(model.node.path, "");
        assert_eq!(model.presets.len(), 1);
        assert_eq!(model.presets[0].port, 0);
        assert_eq!(model.instances.len(), 1);
        assert_eq!(model.failure_text(), Some("端口被占用"));
    }

    #[test]
    fn missing_fields_fall_back_to_settings_defaults() {
        // 旧核心只回部分字段时面板不该整块空掉：开关按 settings.js 的默认值补。
        let model: PanelModel = serde_json::from_str(r#"{ "core": { "version": "0.1.24" } }"#).unwrap();
        assert!(model.prefs.task_notifications);
        assert!(model.prefs.close_to_tray);
        assert!(!model.prefs.auto_update);
        assert!(model.presets.is_empty());
        assert!(model.failure_text().is_none());
    }

    #[test]
    fn preset_caption_shows_port_only_when_running() {
        let model: PanelModel = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(model.preset("a").unwrap().caption(), "web:3799 · 0.2.1-rc.4");
        assert_eq!(model.preset("b").unwrap().caption(), "desk · 0.2.0");
        assert!(model.preset("nope").is_none());
    }
}