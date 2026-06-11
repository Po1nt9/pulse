use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use crate::config::{self, AppConfig, AppSettings, ServiceConfig};
use crate::store::{AppState, CheckResult, ServiceStatus};

type SharedState = Arc<Mutex<AppState>>;

/// 校验 ServiceConfig 输入
fn validate_service(svc: &ServiceConfig) -> Result<(), String> {
    if svc.name.trim().is_empty() {
        return Err("服务名称不能为空".to_string());
    }
    if svc.url.trim().is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if url::Url::parse(&svc.url).is_err() {
        return Err(format!("URL 格式无效: {}", svc.url));
    }
    // SSRF 基础防护
    if !crate::monitor::is_safe_url(&svc.url) {
        return Err("不允许监控内网地址".to_string());
    }
    const VALID_METHODS: &[&str] = &["GET", "POST", "PUT", "HEAD", "DELETE", "PATCH", "OPTIONS"];
    if !VALID_METHODS.contains(&svc.method.as_str()) {
        return Err(format!("不支持的 HTTP 方法: {}", svc.method));
    }
    if svc.interval_secs < 5 {
        return Err("检测间隔不能小于 5 秒".to_string());
    }
    if svc.timeout_secs < 1 || svc.timeout_secs > 120 {
        return Err("超时时间须在 1-120 秒之间".to_string());
    }
    // 期望状态码范围校验
    if !(100..=599).contains(&svc.expected_status) {
        return Err("期望状态码须在 100-599 之间".to_string());
    }
    // Header 注入防护
    for h in &svc.headers {
        if h.key.trim().is_empty() {
            return Err("Header key 不能为空".to_string());
        }
        if h.key.contains('\r') || h.key.contains('\n') || h.value.contains('\r') || h.value.contains('\n') {
            return Err("Header 不能包含换行符".to_string());
        }
    }
    // Body 大小限制（最大 1MB）
    if let Some(ref body) = svc.body {
        if body.len() > 1_048_576 {
            return Err("请求体不能超过 1MB".to_string());
        }
    }
    Ok(())
}

/// 将当前状态保存为配置（返回错误以便传播给前端）
fn persist_config(sg: &AppState) -> Result<(), String> {
    let config = AppConfig {
        services: sg.services.clone(),
        settings: sg.settings.clone(),
    };
    config::save_config(&config).map_err(|e| format!("保存配置文件失败: {}", e))
}

/// 获取所有服务的当前状态
#[tauri::command]
pub fn get_all_status(state: State<'_, SharedState>) -> Result<Vec<ServiceStatus>, String> {
    let sg = state.lock().map_err(|e| e.to_string())?;
    Ok(sg.all_statuses())
}

/// 获取所有服务配置
#[tauri::command]
pub fn get_services(state: State<'_, SharedState>) -> Result<Vec<ServiceConfig>, String> {
    let sg = state.lock().map_err(|e| e.to_string())?;
    Ok(sg.services.clone())
}

/// 添加新服务
#[tauri::command]
pub fn add_service(
    state: State<'_, SharedState>,
    service: ServiceConfig,
) -> Result<ServiceConfig, String> {
    validate_service(&service)?;

    let mut sg = state.lock().map_err(|e| e.to_string())?;

    // 服务数量上限保护
    const MAX_SERVICES: usize = 100;
    if sg.services.len() >= MAX_SERVICES {
        return Err(format!("服务数量已达上限 ({})", MAX_SERVICES));
    }

    let svc = if service.id.is_empty() {
        ServiceConfig {
            id: uuid::Uuid::new_v4().to_string(),
            ..service
        }
    } else {
        service
    };
    sg.services.push(svc.clone());
    persist_config(&sg)?;

    Ok(svc)
}

/// 更新服务配置
#[tauri::command]
pub fn update_service(
    state: State<'_, SharedState>,
    service: ServiceConfig,
) -> Result<(), String> {
    validate_service(&service)?;

    let mut sg = state.lock().map_err(|e| e.to_string())?;
    if let Some(existing) = sg.services.iter_mut().find(|s| s.id == service.id) {
        *existing = service;
    } else {
        return Err("服务不存在".to_string());
    }
    persist_config(&sg)?;

    Ok(())
}

/// 删除服务
#[tauri::command]
pub fn remove_service(
    state: State<'_, SharedState>,
    app: AppHandle,
    service_id: String,
) -> Result<(), String> {
    let mut sg = state.lock().map_err(|e| e.to_string())?;
    sg.services.retain(|s| s.id != service_id);
    sg.latest_results.remove(&service_id);
    sg.history.retain(|r| r.service_id != service_id);
    persist_config(&sg)?;

    // 即时刷新托盘状态
    let (all_healthy, healthy_count, total_count) = {
        let ah = sg.all_healthy();
        let (hc, tc) = sg.health_summary();
        (ah, hc, tc)
    };
    drop(sg); // 释放锁后再更新托盘
    crate::tray::update_tray_status(&app, all_healthy, healthy_count, total_count);

    Ok(())
}

/// 手动触发所有服务检查，返回完整的 ServiceStatus 列表
#[tauri::command]
pub async fn check_all(
    state: State<'_, SharedState>,
    app: AppHandle,
) -> Result<Vec<ServiceStatus>, String> {
    let state_arc = state.inner().clone();
    crate::monitor::manual_check_all(state_arc.clone(), app).await;
    // 检查完毕后重新计算聚合状态
    let sg = state_arc.lock().map_err(|e| e.to_string())?;
    Ok(sg.all_statuses())
}

/// 手动检查单个服务
#[tauri::command]
pub async fn check_one(
    state: State<'_, SharedState>,
    app: AppHandle,
    service_id: String,
) -> Result<Option<CheckResult>, String> {
    let state_arc = state.inner().clone();
    let result = crate::monitor::manual_check_one(state_arc, app, &service_id).await;
    Ok(result)
}

/// 获取全局检查历史（按时间倒序）
#[tauri::command]
pub fn get_all_history(
    state: State<'_, SharedState>,
    limit: Option<usize>,
) -> Result<Vec<crate::store::CheckRecord>, String> {
    let sg = state.lock().map_err(|e| e.to_string())?;
    let records: Vec<_> = sg.history.iter().rev().cloned().collect();
    if let Some(n) = limit {
        Ok(records.into_iter().take(n).collect())
    } else {
        Ok(records)
    }
}

/// 获取应用设置
#[tauri::command]
pub fn get_settings(state: State<'_, SharedState>) -> Result<AppSettings, String> {
    let sg = state.lock().map_err(|e| e.to_string())?;
    Ok(sg.settings.clone())
}

/// 更新应用设置
#[tauri::command]
pub fn update_settings(
    state: State<'_, SharedState>,
    app: AppHandle,
    settings: AppSettings,
) -> Result<(), String> {
    let mut sg = state.lock().map_err(|e| e.to_string())?;
    sg.settings = settings.clone();
    persist_config(&sg)?;
    sg.prune_history(settings.history_days);
    drop(sg);

    // 连通 autostart 插件
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if settings.auto_start {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }

    Ok(())
}

/// 清除所有历史数据
#[tauri::command]
pub fn clear_history(state: State<'_, SharedState>) -> Result<(), String> {
    // 先持久化空历史
    crate::store::save_store(&crate::store::PersistentStore { history: Vec::new() })?;
    // 持久化成功后清内存
    let mut sg = state.lock().map_err(|e| e.to_string())?;
    sg.history.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HeaderPair, ServiceConfig};

    /// 构造一个通过所有校验的 ServiceConfig
    fn valid_service() -> ServiceConfig {
        ServiceConfig {
            id: "test-id".to_string(),
            name: "Test Service".to_string(),
            url: "https://example.com".to_string(),
            method: "GET".to_string(),
            interval_secs: 30,
            timeout_secs: 10,
            expected_status: 200,
            headers: Vec::new(),
            body: None,
        }
    }

    // ==================== validate_service 测试 ====================

    #[test]
    fn validate_service_valid_input() {
        let svc = valid_service();
        assert!(validate_service(&svc).is_ok());
    }

    #[test]
    fn validate_service_valid_with_post_and_body() {
        let svc = ServiceConfig {
            method: "POST".to_string(),
            body: Some(r#"{"key":"value"}"#.to_string()),
            headers: vec![HeaderPair {
                key: "Content-Type".to_string(),
                value: "application/json".to_string(),
            }],
            ..valid_service()
        };
        assert!(validate_service(&svc).is_ok());
    }

    #[test]
    fn validate_service_empty_name_rejected() {
        let svc = ServiceConfig {
            name: "".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_whitespace_name_rejected() {
        let svc = ServiceConfig {
            name: "   ".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_empty_url_rejected() {
        let svc = ServiceConfig {
            url: "".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_invalid_url_format_rejected() {
        let svc = ServiceConfig {
            url: "not-a-url".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_localhost_rejected() {
        let svc = ServiceConfig {
            url: "http://localhost/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_loopback_127_rejected() {
        let svc = ServiceConfig {
            url: "http://127.0.0.1/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_10_network_rejected() {
        let svc = ServiceConfig {
            url: "http://10.0.0.1/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_192_168_network_rejected() {
        let svc = ServiceConfig {
            url: "http://192.168.1.1/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_172_16_network_rejected() {
        let svc = ServiceConfig {
            url: "http://172.16.0.1/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_172_31_network_rejected() {
        let svc = ServiceConfig {
            url: "http://172.31.255.255/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_172_32_network_allowed() {
        let svc = ServiceConfig {
            url: "http://172.32.0.1/api".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_ok());
    }

    #[test]
    fn validate_service_invalid_method_rejected() {
        let svc = ServiceConfig {
            method: "TRACE".to_string(),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_all_valid_methods_accepted() {
        for method in &["GET", "POST", "PUT", "HEAD", "DELETE", "PATCH", "OPTIONS"] {
            let svc = ServiceConfig {
                method: method.to_string(),
                ..valid_service()
            };
            assert!(
                validate_service(&svc).is_ok(),
                "method {} should be valid",
                method
            );
        }
    }

    #[test]
    fn validate_service_interval_below_5_rejected() {
        let svc = ServiceConfig {
            interval_secs: 4,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_interval_at_5_accepted() {
        let svc = ServiceConfig {
            interval_secs: 5,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_ok());
    }

    #[test]
    fn validate_service_timeout_zero_rejected() {
        let svc = ServiceConfig {
            timeout_secs: 0,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_timeout_121_rejected() {
        let svc = ServiceConfig {
            timeout_secs: 121,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_timeout_boundaries_accepted() {
        let svc1 = ServiceConfig {
            timeout_secs: 1,
            ..valid_service()
        };
        assert!(validate_service(&svc1).is_ok());

        let svc120 = ServiceConfig {
            timeout_secs: 120,
            ..valid_service()
        };
        assert!(validate_service(&svc120).is_ok());
    }

    #[test]
    fn validate_service_expected_status_below_100_rejected() {
        let svc = ServiceConfig {
            expected_status: 99,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_expected_status_above_599_rejected() {
        let svc = ServiceConfig {
            expected_status: 600,
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_expected_status_boundaries_accepted() {
        let svc100 = ServiceConfig {
            expected_status: 100,
            ..valid_service()
        };
        assert!(validate_service(&svc100).is_ok());

        let svc599 = ServiceConfig {
            expected_status: 599,
            ..valid_service()
        };
        assert!(validate_service(&svc599).is_ok());
    }

    #[test]
    fn validate_service_header_newline_in_value_rejected() {
        let svc = ServiceConfig {
            headers: vec![HeaderPair {
                key: "X-Custom".to_string(),
                value: "value\ninjection".to_string(),
            }],
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_empty_header_key_rejected() {
        let svc = ServiceConfig {
            headers: vec![HeaderPair {
                key: "".to_string(),
                value: "value".to_string(),
            }],
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_whitespace_header_key_rejected() {
        let svc = ServiceConfig {
            headers: vec![HeaderPair {
                key: "   ".to_string(),
                value: "value".to_string(),
            }],
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_header_cr_in_key_rejected() {
        let svc = ServiceConfig {
            headers: vec![HeaderPair {
                key: "X-Custom\r".to_string(),
                value: "value".to_string(),
            }],
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_body_exceeds_1mb_rejected() {
        let svc = ServiceConfig {
            body: Some("x".repeat(1_048_577)),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_err());
    }

    #[test]
    fn validate_service_body_exactly_1mb_accepted() {
        let svc = ServiceConfig {
            body: Some("x".repeat(1_048_576)),
            ..valid_service()
        };
        assert!(validate_service(&svc).is_ok());
    }
}
