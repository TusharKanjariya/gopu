//! Updates (release ticket 05): a check a minute after start, then once a day, against `latest.json` on GitHub
//! Releases. Nothing installs until the user picks "Update and restart". The check is Gopu's only background request,
//! and the About page's "Check for updates once a day" turns it off. `GOPU_UPDATE_URL` points it elsewhere, for testing.

use gopu_core::settings::Settings;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// The update found by the last check, waiting for the user.
static READY: Mutex<Option<Update>> = Mutex::new(None);
/// Why the last install failed, for the About page.
static FAILED: Mutex<Option<String>> = Mutex::new(None);

/// The version waiting to install, if any.
pub fn ready() -> Option<String> {
    READY.lock().unwrap().as_ref().map(|u| u.version.clone())
}

async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    let mut b = app.updater_builder();
    if let Some(url) = gopu_core::setting("GOPU_UPDATE_URL") {
        b = b.endpoints(vec![url.parse().map_err(|_| "GOPU_UPDATE_URL isn't a URL")?]).map_err(|e| e.to_string())?;
    }
    let found = b.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let version = found.as_ref().map(|u| u.version.clone());
    *READY.lock().unwrap() = found;
    crate::memory_ui::refresh_tray(app);
    Ok(version)
}

/// The daily check. Dev builds only check when `GOPU_UPDATE_URL` is set.
pub fn start(app: AppHandle) {
    if cfg!(debug_assertions) && gopu_core::setting("GOPU_UPDATE_URL").is_none() {
        return;
    }
    // ponytail: a sleeping thread, so a PC asleep for a day checks a day after waking, not at once; fine for daily
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            if !Settings::load().updates_off && ready().is_none() {
                if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
                    println!("update check: {e}");
                }
            }
            std::thread::sleep(Duration::from_secs(86_400));
        }
    });
}

/// Downloads and runs the installer (with its progress bar); on Windows the installer closes Gopu and starts the new one.
pub fn install(app: &AppHandle) {
    let Some(update) = READY.lock().unwrap().clone() else { return };
    let app = app.clone();
    std::thread::spawn(move || match tauri::async_runtime::block_on(update.download_and_install(|_, _| {}, || {})) {
        Ok(()) => app.restart(),
        Err(e) => {
            *FAILED.lock().unwrap() = Some(e.to_string());
            crate::open_settings(&app, Some("about"));
        }
    });
}

/// The About page's state: the current version, any update waiting, and a failed install's reason.
#[tauri::command]
pub fn update_state() -> serde_json::Value {
    serde_json::json!({ "ready": ready(), "failed": FAILED.lock().unwrap().take() })
}

/// The About page's "Check now", and its check on opening.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    check(&app).await
}

#[tauri::command]
pub fn update_install(app: AppHandle) {
    install(&app);
}
