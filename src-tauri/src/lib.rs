mod commands;
mod config;
mod monitor;
mod store;
mod tray;

use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    // 加载配置和持久化数据
    let app_config = config::load_config();
    let persistent = store::load_store();

    // 初始化运行时状态
    let mut app_state = store::AppState::new(app_config);
    app_state.history = persistent.history;
    // 启动前读取 start_minimized 设置
    let should_start_minimized = app_state.settings.start_minimized;
    let state = Arc::new(Mutex::new(app_state));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(state.clone())
        .setup(move |app| {
            // 创建系统托盘
            tray::create_tray(app.handle())?;

            // 启动后台监控
            let app_handle = app.handle().clone();
            monitor::start_monitor(app_handle, state.clone());

            // 检测 --minimized 启动参数（autostart 插件使用）或 start_minimized 设置
            let minimized = std::env::args().any(|a| a == "--minimized");

            // 拦截窗口关闭 -> 隐藏到托盘；失焦自动隐藏
            let window = app.get_webview_window("main")
                .expect("main window should exist per tauri.conf.json");

            // visible 为 false，窗口创建后不可见；非最小化启动时主动 show
            if !(minimized || should_start_minimized) {
                let _ = window.show();
            }

            let win_close = window.clone();
            let win_blur = window.clone();
            window.on_window_event(move |event| {
                match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = win_close.hide();
                    }
                    tauri::WindowEvent::Focused(false) => {
                        let _ = win_blur.hide();
                    }
                    _ => {}
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_all_status,
            commands::get_services,
            commands::add_service,
            commands::update_service,
            commands::remove_service,
            commands::check_all,
            commands::check_one,
            commands::get_all_history,
            commands::get_settings,
            commands::update_settings,
            commands::clear_history,
        ])
        .run(tauri::generate_context!())
        .expect("Failed to start Pulse");
}
