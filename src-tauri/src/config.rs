use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 单个 API 服务的监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub method: String,
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub expected_status: u16,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderPair {
    pub key: String,
    pub value: String,
}

/// 应用全局设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_refresh_interval")]
    pub default_interval_secs: u64,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default = "default_history_days")]
    pub history_days: u32,
    #[serde(default)]
    pub start_minimized: bool,
}

fn default_refresh_interval() -> u64 { 30 }
fn default_true() -> bool { true }
fn default_history_days() -> u32 { 7 }

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_interval_secs: 30,
            notifications_enabled: true,
            auto_start: false,
            history_days: 7,
            start_minimized: false,
        }
    }
}

/// 完整配置文件结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub services: Vec<ServiceConfig>,
    pub settings: AppSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            services: vec![
                ServiceConfig {
                    id: Uuid::new_v4().to_string(),
                    name: "百度".to_string(),
                    url: "https://www.baidu.com".to_string(),
                    method: "GET".to_string(),
                    interval_secs: 30,
                    timeout_secs: 10,
                    expected_status: 200,
                    headers: Vec::new(),
                    body: None,
                },
                ServiceConfig {
                    id: Uuid::new_v4().to_string(),
                    name: "哔哩哔哩".to_string(),
                    url: "https://www.bilibili.com".to_string(),
                    method: "GET".to_string(),
                    interval_secs: 30,
                    timeout_secs: 10,
                    expected_status: 200,
                    headers: Vec::new(),
                    body: None,
                },
            ],
            settings: AppSettings::default(),
        }
    }
}

/// 获取配置文件路径
pub fn config_path() -> std::path::PathBuf {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let pulse_dir = data_dir.join("pulse");
    if let Err(e) = std::fs::create_dir_all(&pulse_dir) {
        log::error!("创建配置目录失败: {}", e);
    }
    pulse_dir.join("config.json")
}

/// 加载配置
pub fn load_config() -> AppConfig {
    let path = config_path();
    if !path.exists() {
        let config = AppConfig::default();
        let _ = save_config(&config);
        return config;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            log::warn!("配置文件解析失败 ({}), 回退到默认配置", e);
            let config = AppConfig::default();
            let _ = save_config(&config);
            config
        }),
        Err(e) => {
            log::warn!("读取配置文件失败 ({}), 回退到默认配置", e);
            let config = AppConfig::default();
            let _ = save_config(&config);
            config
        }
    }
}

/// 保存配置（原子写入，复用 store::atomic_write）
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("序列化配置失败: {}", e))?;
    crate::store::atomic_write(&path, &content)
}
