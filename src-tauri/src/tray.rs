use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition,
};

/// 弹窗尺寸常量（与 tauri.conf.json 保持同步）
const WIN_W: i32 = 356;
const WIN_H: i32 = 560;

/// 根据托盘图标位置定位弹窗，支持任务栏在不同位置
fn position_popup_near_tray(
    tray: &tauri::tray::TrayIcon,
    window: &tauri::WebviewWindow,
) {
    if let Ok(Some(rect)) = tray.rect() {
        let px = match rect.position {
            tauri::Position::Physical(p) => p.x,
            tauri::Position::Logical(p) => p.x as i32,
        };
        let py = match rect.position {
            tauri::Position::Physical(p) => p.y,
            tauri::Position::Logical(p) => p.y as i32,
        };
        let sw = match rect.size {
            tauri::Size::Physical(s) => s.width as i32,
            tauri::Size::Logical(s) => s.width as i32,
        };
        let sh = match rect.size {
            tauri::Size::Physical(s) => s.height as i32,
            tauri::Size::Logical(s) => s.height as i32,
        };

        // 获取屏幕尺寸（使用窗口所在 monitor）
        let monitor = window.current_monitor().ok().flatten();
        let screen_w = monitor.as_ref().map(|m| m.size().width as i32).unwrap_or(1920);
        let screen_h = monitor.as_ref().map(|m| m.size().height as i32).unwrap_or(1080);

        let center_x = px + sw / 2;
        // 水平位置钳位到屏幕边界
        let x = (center_x - WIN_W / 2).max(8).min(screen_w - WIN_W - 8);

        // 根据托盘图标在屏幕中的位置决定弹窗在上方还是下方
        let y = if py > screen_h / 2 {
            // 任务栏在底部：弹窗出现在图标上方
            py - WIN_H - 8
        } else {
            // 任务栏在顶部：弹窗出现在图标下方
            py + sh + 8
        };

        let _ = window.set_position(tauri::Position::Physical(
            PhysicalPosition { x, y },
        ));
    }
}

/// 创建系统托盘图标和菜单
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示面板", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "立即检测全部", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 Pulse", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &refresh_item, &separator, &quit_item])?;

    let icon = app.default_window_icon()
        .cloned()
        .expect("default_window_icon should be set from bundle icons");

    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Pulse - 接口监控")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                toggle_popup(app);
            }
            "refresh" => {
                // emit 是同步方法，无需 spawn async task
                let _ = app.emit("tray-refresh", ());
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        position_popup_near_tray(tray, &window);
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// 切换弹窗显隐（含定位）
fn toggle_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            // 通过托盘图标重新定位
            if let Some(tray) = app.tray_by_id("main") {
                position_popup_near_tray(&tray, &window);
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// 根据监控状态更新托盘提示
pub fn update_tray_status(app: &AppHandle, all_healthy: bool, healthy: usize, total: usize) {
    let tooltip = if total == 0 {
        "Pulse - 暂无服务".to_string()
    } else if all_healthy {
        format!("Pulse - {}/{} 正常 ✓", healthy, total)
    } else {
        format!("Pulse - {}/{} 正常 ⚠", healthy, total)
    };

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
}
