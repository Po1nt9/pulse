use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::ServiceConfig;

/// 单次检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub service_id: String,
    pub name: String,
    pub url: String,
    pub status: u16,
    pub healthy: bool,
    pub response_time_ms: u64,
    pub error: Option<String>,
    pub checked_at: String,
}

/// 服务最新状态（用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub service_id: String,
    pub name: String,
    pub url: String,
    pub last_check: Option<CheckResult>,
    pub uptime_pct: f64,
    pub avg_response_ms: u64,
    pub total_checks: u64,
    pub failed_checks: u64,
}

/// 检查历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRecord {
    pub service_id: String,
    pub healthy: bool,
    pub response_time_ms: u64,
    pub status: u16,
    pub checked_at: String,
}

/// 持久化存储结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentStore {
    #[serde(default)]
    pub history: Vec<CheckRecord>,
}

impl Default for PersistentStore {
    fn default() -> Self {
        Self {
            history: Vec::new(),
        }
    }
}

/// 历史索引条目（用于 all_statuses O(n+m) 优化）
struct HistoryIndexEntry {
    total: u64,
    failed: u64,
    sum_ms: u64,
}

/// 历史索引类型
type ServiceHistoryIndex = std::collections::HashMap<String, HistoryIndexEntry>;

/// 运行时应用状态
pub struct AppState {
    pub services: Vec<ServiceConfig>,
    pub latest_results: HashMap<String, CheckResult>,
    pub history: Vec<CheckRecord>,
    pub settings: crate::config::AppSettings,
}

impl AppState {
    pub fn new(config: crate::config::AppConfig) -> Self {
        Self {
            services: config.services,
            latest_results: HashMap::new(),
            history: Vec::new(),
            settings: config.settings,
        }
    }

    /// 使用可选的预计算索引生成状态摘要
    fn service_status_with(&self, svc: &ServiceConfig, index: Option<&ServiceHistoryIndex>) -> ServiceStatus {
        let (total, failed, sum_ms) = if let Some(idx) = index {
            idx.get(&svc.id)
                .map(|e| (e.total, e.failed, e.sum_ms))
                .unwrap_or((0, 0, 0))
        } else {
            // 回退路径：线性扫描
            let records: Vec<_> = self.history.iter()
                .filter(|r| r.service_id == svc.id)
                .collect();
            let total = records.len() as u64;
            let failed = records.iter().filter(|r| !r.healthy).count() as u64;
            let sum_ms: u64 = records.iter().map(|r| r.response_time_ms).sum();
            (total, failed, sum_ms)
        };

        ServiceStatus {
            service_id: svc.id.clone(),
            name: svc.name.clone(),
            url: svc.url.clone(),
            last_check: self.latest_results.get(&svc.id).cloned(),
            uptime_pct: if total == 0 { 100.0 } else { ((total - failed) as f64 / total as f64) * 100.0 },
            avg_response_ms: if total == 0 { 0 } else { sum_ms / total },
            total_checks: total,
            failed_checks: failed,
        }
    }

    /// 获取所有服务的状态摘要（O(history + services) 而非 O(services * history)）
    pub fn all_statuses(&self) -> Vec<ServiceStatus> {
        // 单次遍历历史构建索引
        let mut index: ServiceHistoryIndex = std::collections::HashMap::new();
        for r in &self.history {
            let entry = index.entry(r.service_id.clone())
                .or_insert_with(|| HistoryIndexEntry { total: 0, failed: 0, sum_ms: 0 });
            entry.total += 1;
            if !r.healthy {
                entry.failed += 1;
            }
            entry.sum_ms += r.response_time_ms;
        }

        self.services.iter()
            .map(|svc| self.service_status_with(svc, Some(&index)))
            .collect()
    }

    /// 全局健康状态：所有服务都有检查结果且全部健康
    pub fn all_healthy(&self) -> bool {
        if self.services.is_empty() {
            return true;
        }
        // 所有服务都必须有结果，且结果全部健康
        self.services.iter().all(|svc| {
            self.latest_results.get(&svc.id).is_some_and(|r| r.healthy)
        })
    }

    /// 健康服务数量 / 总服务数量
    pub fn health_summary(&self) -> (usize, usize) {
        let total = self.services.len();
        let healthy = self.services.iter()
            .filter(|svc| self.latest_results.get(&svc.id).is_some_and(|r| r.healthy))
            .count();
        (healthy, total)
    }

    /// 清理超过保留天数的历史记录（解析失败的记录保留而非删除）
    pub fn prune_history(&mut self, days: u32) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days.min(365) as i64);
        self.history.retain(|r| {
            match chrono::DateTime::parse_from_rfc3339(&r.checked_at) {
                Ok(dt) => dt > cutoff,
                Err(e) => {
                    log::warn!("历史记录日期解析失败 ({}), 保留记录: {}", e, r.checked_at);
                    true // 保留无法解析的记录
                }
            }
        });
    }
}

/// 获取存储文件路径
pub fn store_path() -> std::path::PathBuf {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let pulse_dir = data_dir.join("pulse");
    if let Err(e) = std::fs::create_dir_all(&pulse_dir) {
        log::error!("创建存储目录失败: {}", e);
    }
    pulse_dir.join("store.json")
}

/// 原子写入（公开供 config 模块复用）
pub fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)
        .map_err(|e| format!("写入临时文件失败: {}", e))?;
    // Windows: rename 不支持覆盖已有文件，需先删除目标
    if cfg!(windows) && path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path)
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("重命名文件失败: {}", e)
        })?;
    Ok(())
}

/// 加载持久化存储
pub fn load_store() -> PersistentStore {
    let path = store_path();
    if !path.exists() {
        return PersistentStore::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            log::warn!("存储文件解析失败 ({}), 使用空历史", e);
            PersistentStore::default()
        }),
        Err(e) => {
            log::warn!("读取存储文件失败 ({})", e);
            PersistentStore::default()
        }
    }
}

/// 保存持久化存储（原子写入）
pub fn save_store(store: &PersistentStore) -> Result<(), String> {
    let path = store_path();
    let content = serde_json::to_string(store)
        .map_err(|e| format!("序列化存储数据失败: {}", e))?;
    atomic_write(&path, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, AppSettings, ServiceConfig};

    fn test_config_with_services(services: Vec<ServiceConfig>) -> AppConfig {
        AppConfig {
            services,
            settings: AppSettings::default(),
        }
    }

    fn make_service(id: &str, name: &str, url: &str) -> ServiceConfig {
        ServiceConfig {
            id: id.to_string(),
            name: name.to_string(),
            url: url.to_string(),
            method: "GET".to_string(),
            interval_secs: 30,
            timeout_secs: 10,
            expected_status: 200,
            headers: Vec::new(),
            body: None,
        }
    }

    fn make_check_record(service_id: &str, healthy: bool, response_time_ms: u64) -> CheckRecord {
        CheckRecord {
            service_id: service_id.to_string(),
            healthy,
            response_time_ms,
            status: if healthy { 200 } else { 500 },
            checked_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn make_check_record_with_time(
        service_id: &str,
        healthy: bool,
        response_time_ms: u64,
        checked_at: &str,
    ) -> CheckRecord {
        CheckRecord {
            service_id: service_id.to_string(),
            healthy,
            response_time_ms,
            status: if healthy { 200 } else { 500 },
            checked_at: checked_at.to_string(),
        }
    }

    fn make_check_result(service_id: &str, healthy: bool) -> CheckResult {
        CheckResult {
            service_id: service_id.to_string(),
            name: "Test".to_string(),
            url: "https://example.com".to_string(),
            status: if healthy { 200 } else { 500 },
            healthy,
            response_time_ms: 100,
            error: if healthy { None } else { Some("error".to_string()) },
            checked_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    // ==================== all_statuses 测试 ====================

    #[test]
    fn all_statuses_empty_services_empty_history() {
        let state = AppState::new(test_config_with_services(vec![]));
        let statuses = state.all_statuses();
        assert!(statuses.is_empty());
    }

    #[test]
    fn all_statuses_single_service_multiple_records() {
        let svc = make_service("s1", "Service 1", "https://example.com");
        let mut state = AppState::new(test_config_with_services(vec![svc]));

        state.history = vec![
            make_check_record("s1", true, 100),
            make_check_record("s1", true, 200),
            make_check_record("s1", false, 300),
            make_check_record("s1", true, 400),
        ];

        let statuses = state.all_statuses();
        assert_eq!(statuses.len(), 1);

        let s = &statuses[0];
        assert_eq!(s.service_id, "s1");
        assert_eq!(s.name, "Service 1");
        assert_eq!(s.url, "https://example.com");
        assert_eq!(s.total_checks, 4);
        assert_eq!(s.failed_checks, 1);
        // (4 - 1) / 4 * 100 = 75.0
        assert!((s.uptime_pct - 75.0).abs() < 0.001);
        // (100 + 200 + 300 + 400) / 4 = 250
        assert_eq!(s.avg_response_ms, 250);
    }

    #[test]
    fn all_statuses_multiple_services_independent() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2]));

        state.history = vec![
            make_check_record("s1", true, 100),
            make_check_record("s1", true, 200),
            make_check_record("s2", false, 50),
            make_check_record("s2", true, 150),
            make_check_record("s2", true, 250),
        ];

        let statuses = state.all_statuses();
        assert_eq!(statuses.len(), 2);

        let s1 = statuses.iter().find(|s| s.service_id == "s1").unwrap();
        assert_eq!(s1.total_checks, 2);
        assert_eq!(s1.failed_checks, 0);
        assert!((s1.uptime_pct - 100.0).abs() < 0.001);
        assert_eq!(s1.avg_response_ms, 150);

        let s2 = statuses.iter().find(|s| s.service_id == "s2").unwrap();
        assert_eq!(s2.total_checks, 3);
        assert_eq!(s2.failed_checks, 1);
        // (3 - 1) / 3 * 100 ≈ 66.667
        assert!((s2.uptime_pct - 66.666).abs() < 0.1);
        assert_eq!(s2.avg_response_ms, 150);
    }

    #[test]
    fn all_statuses_service_no_history() {
        let svc = make_service("s1", "Service 1", "https://example.com");
        let state = AppState::new(test_config_with_services(vec![svc]));

        let statuses = state.all_statuses();
        assert_eq!(statuses.len(), 1);

        let s = &statuses[0];
        assert_eq!(s.total_checks, 0);
        assert_eq!(s.failed_checks, 0);
        assert_eq!(s.uptime_pct, 100.0);
        assert_eq!(s.avg_response_ms, 0);
    }

    // ==================== prune_history 测试 ====================

    #[test]
    fn prune_history_removes_old_records() {
        let svc = make_service("s1", "Service 1", "https://example.com");
        let mut state = AppState::new(test_config_with_services(vec![svc]));

        let old_date = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        let recent_date = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();

        state.history = vec![
            make_check_record_with_time("s1", true, 100, &old_date),
            make_check_record_with_time("s1", true, 200, &recent_date),
        ];

        state.prune_history(7);

        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].response_time_ms, 200);
    }

    #[test]
    fn prune_history_days_capped_at_365() {
        let svc = make_service("s1", "Service 1", "https://example.com");
        let mut state = AppState::new(test_config_with_services(vec![svc]));

        let date_500_days_ago =
            (chrono::Utc::now() - chrono::Duration::days(500)).to_rfc3339();

        state.history = vec![make_check_record_with_time(
            "s1",
            true,
            100,
            &date_500_days_ago,
        )];

        // 请求保留 1000 天，但上限为 365 天，500 天前的记录仍会被删除
        state.prune_history(1000);
        assert_eq!(state.history.len(), 0);

        // 再来验证：300 天前的记录在 365 天上限下会被保留
        state.history = vec![make_check_record_with_time(
            "s1",
            true,
            100,
            &(chrono::Utc::now() - chrono::Duration::days(300)).to_rfc3339(),
        )];
        state.prune_history(1000);
        assert_eq!(state.history.len(), 1);
    }

    #[test]
    fn prune_history_keeps_unparseable_records() {
        let svc = make_service("s1", "Service 1", "https://example.com");
        let mut state = AppState::new(test_config_with_services(vec![svc]));

        state.history = vec![
            CheckRecord {
                service_id: "s1".to_string(),
                healthy: true,
                response_time_ms: 100,
                status: 200,
                checked_at: "not-a-valid-date".to_string(),
            },
            make_check_record_with_time(
                "s1",
                true,
                200,
                &(chrono::Utc::now() - chrono::Duration::days(100)).to_rfc3339(),
            ),
        ];

        state.prune_history(7);

        // 解析失败的记录被保留，超期的记录被删除
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].checked_at, "not-a-valid-date");
    }

    // ==================== health_summary 测试 ====================

    #[test]
    fn health_summary_empty_services() {
        let state = AppState::new(test_config_with_services(vec![]));
        let (healthy, total) = state.health_summary();
        assert_eq!(healthy, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn health_summary_all_healthy() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2]));

        state.latest_results.insert("s1".to_string(), make_check_result("s1", true));
        state.latest_results.insert("s2".to_string(), make_check_result("s2", true));

        let (healthy, total) = state.health_summary();
        assert_eq!(healthy, 2);
        assert_eq!(total, 2);
    }

    #[test]
    fn health_summary_partial_healthy() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let svc3 = make_service("s3", "Service 3", "https://example.net");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2, svc3]));

        state.latest_results.insert("s1".to_string(), make_check_result("s1", true));
        state.latest_results.insert("s2".to_string(), make_check_result("s2", false));
        // s3 无结果

        let (healthy, total) = state.health_summary();
        assert_eq!(healthy, 1);
        assert_eq!(total, 3);
    }

    // ==================== all_healthy 测试 ====================

    #[test]
    fn all_healthy_empty_services() {
        let state = AppState::new(test_config_with_services(vec![]));
        assert!(state.all_healthy());
    }

    #[test]
    fn all_healthy_all_have_healthy_results() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2]));

        state.latest_results.insert("s1".to_string(), make_check_result("s1", true));
        state.latest_results.insert("s2".to_string(), make_check_result("s2", true));

        assert!(state.all_healthy());
    }

    #[test]
    fn all_healthy_some_missing_results() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2]));

        state.latest_results.insert("s1".to_string(), make_check_result("s1", true));
        // s2 没有结果

        assert!(!state.all_healthy());
    }

    #[test]
    fn all_healthy_some_unhealthy() {
        let svc1 = make_service("s1", "Service 1", "https://example.com");
        let svc2 = make_service("s2", "Service 2", "https://example.org");
        let mut state = AppState::new(test_config_with_services(vec![svc1, svc2]));

        state.latest_results.insert("s1".to_string(), make_check_result("s1", true));
        state.latest_results.insert("s2".to_string(), make_check_result("s2", false));

        assert!(!state.all_healthy());
    }
}
