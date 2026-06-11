use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config::ServiceConfig;
use crate::store::{self, CheckRecord, CheckResult};

/// 历史最大条数保护
const MAX_HISTORY: usize = 5000;

/// 全局共享的 HTTP 客户端（惰性初始化，连接池复用）
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn get_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(false)
            .user_agent(concat!("Pulse/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                // 重定向目标也必须通过 SSRF 校验
                if is_safe_url(attempt.url().as_str()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .unwrap_or_default()
    })
}

/// SSRF 基础防护：仅允许 http/https，拒绝内网/回环/链路本地地址
pub fn is_safe_url(url: &str) -> bool {
    // 解析失败默认拒绝（防御性编程）
    let parsed = match url::Url::parse(url) {
        Ok(p) => p,
        Err(_) => return false,
    };
    // 仅允许 http 和 https
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return false,
    }
    if let Some(host) = parsed.host_str() {
        // 拒绝常见内网地址
        if host == "localhost"
            || host == "127.0.0.1"
            || host.starts_with("127.")
            || host == "::1"
            || host == "0.0.0.0"
            || host == "::"
            || host == "::ffff:7f00:1"
            || host == "::ffff:127.0.0.1"
            || host == "[::1]"
            || host == "[::]"
            || host == "[::ffff:7f00:1]"
            || host == "[::ffff:127.0.0.1]"
            || host.starts_with("10.")
            || host.starts_with("192.168.")
            || host.starts_with("169.254.")
            || host.ends_with(".local")
            || host.ends_with(".internal")
        {
            return false;
        }
        // 拒绝 172.16-31.x.x
        if let Some(rest) = host.strip_prefix("172.") {
            if let Some(second) = rest.split('.').next() {
                if let Ok(n) = second.parse::<u8>() {
                    if (16..=31).contains(&n) {
                        return false;
                    }
                }
            }
        }
        // 拒绝 IPv6 ULA (fc00::/7) 和链路本地 (fe80::/10)
        let host_lower = host.to_lowercase();
        let ipv6 = host_lower.strip_prefix('[').unwrap_or(&host_lower);
        if ipv6.starts_with("fc") || ipv6.starts_with("fd") {
            return false;
        }
        if ipv6.starts_with("fe8") || ipv6.starts_with("fe9")
            || ipv6.starts_with("fea") || ipv6.starts_with("feb")
        {
            return false;
        }
    }
    true
}

/// 告警信息结构体
struct AlertInfo {
    name: String,
    error: String,
    service_id: String,
    checked_at: String,
    is_down: bool,
}

/// 告警事件的有效载荷（可序列化）
#[derive(Serialize)]
struct ServiceAlertPayload {
    #[serde(rename = "type")]
    alert_type: String,
    service_id: String,
    name: String,
    error: Option<String>,
    checked_at: String,
}

/// 构造 CheckResult 的辅助函数
fn make_result(
    svc: &ServiceConfig,
    status: u16,
    healthy: bool,
    elapsed_ms: u64,
    error: Option<String>,
) -> CheckResult {
    CheckResult {
        service_id: svc.id.clone(),
        name: svc.name.clone(),
        url: svc.url.clone(),
        status,
        healthy,
        response_time_ms: elapsed_ms,
        error,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// 执行单个服务的健康检查（支持 per-service timeout）
async fn check_service(svc: &ServiceConfig) -> CheckResult {
    let start = Instant::now();

    // SSRF 检查
    if !is_safe_url(&svc.url) {
        return make_result(svc, 0, false, 0, Some("不允许监控内网地址".to_string()));
    }

    let client = get_client();
    let timeout = Duration::from_secs(svc.timeout_secs.max(1));

    let mut req = match svc.method.as_str() {
        "POST" => client.post(&svc.url),
        "PUT" => client.put(&svc.url),
        "HEAD" => client.head(&svc.url),
        "DELETE" => client.delete(&svc.url),
        "PATCH" => client.patch(&svc.url),
        "OPTIONS" => client.request(reqwest::Method::OPTIONS, &svc.url),
        _ => client.get(&svc.url),
    };

    req = req.timeout(timeout);

    for header in &svc.headers {
        req = req.header(&header.key, &header.value);
    }

    if let Some(body) = &svc.body {
        req = req.body(body.clone());
    }

    match req.send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_millis() as u64;
            let status = resp.status().as_u16();
            let healthy = status == svc.expected_status;
            let error = if !healthy {
                Some(format!("期望状态 {}, 实际 {}", svc.expected_status, status))
            } else {
                None
            };
            make_result(svc, status, healthy, elapsed, error)
        }
        Err(e) => {
            let elapsed = start.elapsed().as_millis() as u64;
            make_result(svc, 0, false, elapsed, Some(e.to_string()))
        }
    }
}

/// 发送 OS 级系统通知
fn send_os_notification(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}

/// 处理检查结果：单次加锁完成所有状态更新，释放锁后执行 I/O
fn process_results(
    state: &Arc<Mutex<store::AppState>>,
    app: &AppHandle,
    results: &[CheckResult],
) {
    // ── 加锁阶段 ──
    let (was_all_healthy, all_healthy, healthy_count, total_count, prev_hc, prev_tc, store_snapshot, alerts) = {
        let mut sg = match state.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("Mutex 加锁失败: {}", e);
                return;
            }
        };
        let notifications_enabled = sg.settings.notifications_enabled;
        let was_all_healthy = sg.all_healthy();
        let (prev_healthy_count, prev_total_count) = sg.health_summary();

        let mut alerts: Vec<AlertInfo> = Vec::new();

        for result in results {
            let was_healthy = sg.latest_results
                .get(&result.service_id)
                .map(|r| r.healthy)
                .unwrap_or(true);
            let now_unhealthy = !result.healthy;

            sg.history.push(CheckRecord {
                service_id: result.service_id.clone(),
                healthy: result.healthy,
                response_time_ms: result.response_time_ms,
                status: result.status,
                checked_at: result.checked_at.clone(),
            });

            sg.latest_results.insert(result.service_id.clone(), result.clone());

            if notifications_enabled && was_healthy && now_unhealthy {
                alerts.push(AlertInfo {
                    name: result.name.clone(),
                    error: result.error.clone().unwrap_or_default(),
                    service_id: result.service_id.clone(),
                    checked_at: result.checked_at.clone(),
                    is_down: true,
                });
            }
            if notifications_enabled && !was_healthy && result.healthy {
                alerts.push(AlertInfo {
                    name: result.name.clone(),
                    error: String::new(),
                    service_id: result.service_id.clone(),
                    checked_at: result.checked_at.clone(),
                    is_down: false,
                });
            }
        }

        // 裁剪历史 + 上限保护
        let history_days = sg.settings.history_days;
        sg.prune_history(history_days);
        if sg.history.len() > MAX_HISTORY {
            let excess = sg.history.len() - MAX_HISTORY;
            sg.history.drain(..excess);
        }

        let snapshot = store::PersistentStore { history: sg.history.clone() };
        let all_healthy = sg.all_healthy();
        let (hc, tc) = sg.health_summary();

        (was_all_healthy, all_healthy, hc, tc, prev_healthy_count, prev_total_count, snapshot, alerts)
    }; // ── 锁释放 ──

    // ── I/O 阶段 ──

    for result in results {
        let _ = app.emit("check-result", result);
    }

    // 告警：同时发送前端事件和 OS 通知
    for alert in &alerts {
        let alert_type = if alert.is_down { "down" } else { "recovered" };
        let _ = app.emit("service-alert", &ServiceAlertPayload {
            alert_type: alert_type.to_string(),
            service_id: alert.service_id.clone(),
            name: alert.name.clone(),
            error: if alert.error.is_empty() { None } else { Some(alert.error.clone()) },
            checked_at: alert.checked_at.clone(),
        });
        let os_title = if alert.is_down {
            format!("{} 已宕机", alert.name)
        } else {
            format!("{} 已恢复", alert.name)
        };
        send_os_notification(app, &os_title, &alert.error);
    }

    crate::tray::update_tray_status(app, all_healthy, healthy_count, total_count);
    if let Err(e) = store::save_store(&store_snapshot) {
        log::error!("保存存储数据失败: {}", e);
    }

    if was_all_healthy != all_healthy || healthy_count != prev_hc || total_count != prev_tc {
        let _ = app.emit("health-changed", &serde_json::json!({
            "all_healthy": all_healthy,
            "healthy_count": healthy_count,
            "total_count": total_count,
        }));
    }
}

/// 启动后台监控循环
pub fn start_monitor(app: AppHandle, state: Arc<Mutex<store::AppState>>) {
    let handle = tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut last_check_times: std::collections::HashMap<String, Instant> =
            std::collections::HashMap::new();

        loop {
            tick.tick().await;

            let services_to_check: Vec<ServiceConfig> = {
                let state_guard = match state.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        log::error!("监控循环加锁失败: {}", e);
                        continue;
                    }
                };
                // 清理已移除服务的计时记录
                last_check_times.retain(|id, _| state_guard.services.iter().any(|s| &s.id == id));
                state_guard.services.iter()
                    .filter(|svc| {
                        let interval = Duration::from_secs(svc.interval_secs.max(5));
                        match last_check_times.get(&svc.id) {
                            Some(last) => last.elapsed() >= interval,
                            None => true,
                        }
                    })
                    .cloned()
                    .collect()
            };

            if services_to_check.is_empty() {
                continue;
            }

            let mut futures = Vec::new();
            for svc in &services_to_check {
                last_check_times.insert(svc.id.clone(), Instant::now());
                futures.push(check_service(svc));
            }

            let results = stream::iter(futures)
                .buffer_unordered(10)
                .collect::<Vec<_>>()
                .await;
            process_results(&state, &app, &results);
        }
    });
    // 防止 JoinHandle 被 drop 导致任务取消
    std::mem::forget(handle);
}

/// 手动触发所有服务的一次检查
pub async fn manual_check_all(
    state: Arc<Mutex<store::AppState>>,
    app: AppHandle,
) {
    let services: Vec<ServiceConfig> = {
        let sg = match state.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("manual_check_all 加锁失败: {}", e);
                return;
            }
        };
        sg.services.clone()
    };

    let mut futures = Vec::new();
    for svc in &services {
        futures.push(check_service(svc));
    }

    let results = stream::iter(futures)
        .buffer_unordered(10)
        .collect::<Vec<_>>()
        .await;
    process_results(&state, &app, &results);
}

/// 手动检查单个服务
pub async fn manual_check_one(
    state: Arc<Mutex<store::AppState>>,
    app: AppHandle,
    service_id: &str,
) -> Option<CheckResult> {
    let svc = {
        let sg = match state.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("manual_check_one 加锁失败: {}", e);
                return None;
            }
        };
        sg.services.iter().find(|s| s.id == service_id).cloned()
    }?;

    let result = check_service(&svc).await;
    process_results(&state, &app, &[result.clone()]);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_safe_url 测试 ──

    #[test]
    fn safe_url_allows_https() {
        assert!(is_safe_url("https://api.github.com"));
        assert!(is_safe_url("https://www.google.com/health"));
        assert!(is_safe_url("http://example.com/api"));
    }

    #[test]
    fn safe_url_blocks_localhost() {
        assert!(!is_safe_url("http://localhost"));
        assert!(!is_safe_url("http://localhost:8080"));
        assert!(!is_safe_url("https://localhost/api"));
    }

    #[test]
    fn safe_url_blocks_loopback_ip() {
        assert!(!is_safe_url("http://127.0.0.1"));
        assert!(!is_safe_url("http://127.0.0.1:3000"));
        assert!(!is_safe_url("http://127.0.0.255"));
    }

    #[test]
    fn safe_url_blocks_private_networks() {
        assert!(!is_safe_url("http://10.0.0.1"));
        assert!(!is_safe_url("http://10.255.255.255"));
        assert!(!is_safe_url("http://192.168.0.1"));
        assert!(!is_safe_url("http://192.168.1.100"));
        assert!(!is_safe_url("http://172.16.0.1"));
        assert!(!is_safe_url("http://172.31.255.255"));
    }

    #[test]
    fn safe_url_allows_public_172() {
        assert!(is_safe_url("http://172.15.0.1"));
        assert!(is_safe_url("http://172.32.0.1"));
    }

    #[test]
    fn safe_url_blocks_ipv6_loopback() {
        assert!(!is_safe_url("http://[::1]"));
        assert!(!is_safe_url("http://[::]"));
        assert!(!is_safe_url("http://[::ffff:127.0.0.1]"));
    }

    #[test]
    fn safe_url_blocks_non_http_schemes() {
        assert!(!is_safe_url("ftp://example.com"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("data:text/html,<h1>hi</h1>"));
    }

    #[test]
    fn safe_url_blocks_invalid_input() {
        assert!(!is_safe_url(""));
        assert!(!is_safe_url("not a url"));
        assert!(!is_safe_url("://missing-scheme"));
    }

    #[test]
    fn safe_url_blocks_link_local() {
        assert!(!is_safe_url("http://169.254.1.1"));
        assert!(!is_safe_url("http://169.254.169.254"));
    }

    #[test]
    fn safe_url_blocks_internal_tlds() {
        assert!(!is_safe_url("http://myserver.local"));
        assert!(!is_safe_url("http://service.internal"));
    }

    #[test]
    fn safe_url_blocks_all_zeros() {
        assert!(!is_safe_url("http://0.0.0.0"));
    }

    #[test]
    fn safe_url_blocks_ipv6_ula() {
        assert!(!is_safe_url("http://[fc00::1]"));
        assert!(!is_safe_url("http://[fd00::1]"));
        assert!(!is_safe_url("http://[fc00:abcd:1234::1]"));
        assert!(!is_safe_url("http://[FD12:3456::1]"));
    }

    #[test]
    fn safe_url_blocks_ipv6_link_local() {
        assert!(!is_safe_url("http://[fe80::1]"));
        assert!(!is_safe_url("http://[fe80::1%25eth0]"));
        assert!(!is_safe_url("http://[fe90::abcd]"));
        assert!(!is_safe_url("http://[fea0::1234]"));
        assert!(!is_safe_url("http://[feb0::5678]"));
        assert!(!is_safe_url("http://[FE80::1]"));
    }
}
