// Phase 0 tray app. At start it opens one webview window and closes it after 5s, so the idle memory
// reading is "window closed", then runs the probes and writes phase0-report.json to the app data dir.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

static RUNNING: AtomicBool = AtomicBool::new(false);

fn run_probes(app: &AppHandle) {
    // A second run would delete the worktrees under the live engines.
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Ok(data) = app.path().app_data_dir() {
            let _ = std::fs::create_dir_all(&data);
            let _ = std::fs::write(data.join("phase0-status"), "running");
            let report = prbots_core::run_phase0(&data);
            let json = serde_json::to_string_pretty(&report).unwrap_or_default();
            let _ = std::fs::write(data.join("phase0-report.json"), json);
            let _ = std::fs::write(data.join("phase0-status"), "done");
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

fn open_then_close_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("pr-bots phase 0")
        .build()?;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        let _ = window.close();
    });
    Ok(())
}

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let run = MenuItem::with_id(app, "run", "Run Phase 0 probes", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&run, &quit])?;
            let icon = app.default_window_icon().cloned().ok_or("no icon")?;
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("pr-bots phase 0")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "run" => run_probes(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            open_then_close_window(app.handle())?;
            run_probes(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("tauri app builds");
    // Closing the last window must not quit a tray app; only Quit ends it.
    app.run(|_, event| {
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
