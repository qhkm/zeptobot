mod commands;
mod services;
pub mod tools;

use commands::{clear_history, execute_automation, get_status, send_message, AgentState};
use services::agent::build_agent;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // ── Build ZeptoAgent (persists across all commands) ────
            match build_agent() {
                Ok(agent) => {
                    app.manage(AgentState(agent));
                }
                Err(e) => {
                    eprintln!("Warning: Agent not available: {e}");
                    // Build a dummy agent so the app still starts —
                    // commands will fail gracefully when called.
                    // For now, we require an API key.
                    return Err(e.into());
                }
            }

            // ── System tray ──────────────────────────────────────
            let show_item = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .tooltip("ZeptoBot")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            clear_history,
            get_status,
            execute_automation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
