//! Codex Switcher - Multi-account manager for Codex CLI

pub mod api;
#[cfg(desktop)]
pub mod app_menu;
pub mod auth;
pub mod commands;
#[cfg(desktop)]
pub mod tray;
pub mod types;
pub mod web;

use commands::{
    ack_close_behavior_prompt, add_account_from_file, cancel_login, check_codex_processes,
    complete_close_behavior, complete_login, delete_account, export_accounts_full_encrypted_file,
    export_accounts_slim_text, get_account_usage_stats, get_active_account_info,
    get_dock_display_mode, get_masked_account_ids, get_usage, hide_tray_window,
    import_accounts_full_encrypted_file, import_accounts_slim_text, kill_codex_processes,
    list_accounts, open_main_window, quit_app, refresh_account_metadata,
    refresh_all_accounts_usage, rename_account, report_usage, set_dock_display_mode,
    set_masked_account_ids, start_login, switch_account, warmup_account, warmup_all_accounts,
};
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                // Enforce single instance: if another copy is already running,
                // exit immediately. This prevents concurrent token refreshes
                // across multiple instances from causing refresh_token_reused.
                #[cfg(unix)]
                enforce_single_instance_unix(app)?;

                #[cfg(windows)]
                enforce_single_instance_windows()?;

                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                app_menu::setup(app.handle())?;
                tray::setup(app.handle())?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    #[cfg(target_os = "macos")]
                    if commands::should_prompt_for_close_behavior() {
                        let payload = commands::window::next_close_behavior_prompt_payload();
                        let app_handle = tauri::Manager::app_handle(window);
                        commands::window::schedule_close_behavior_prompt_fallback(
                            app_handle.clone(),
                            payload.request_id,
                        );
                        let _ =
                            window.emit(commands::window::CLOSE_BEHAVIOR_REQUESTED_EVENT, payload);
                        return;
                    }
                    commands::hide_main_window(&tauri::Manager::app_handle(window));
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_codex_app,
            // Account management
            list_accounts,
            get_active_account_info,
            add_account_from_file,
            switch_account,
            delete_account,
            rename_account,
            export_accounts_slim_text,
            import_accounts_slim_text,
            export_accounts_full_encrypted_file,
            import_accounts_full_encrypted_file,
            // Masked accounts
            get_masked_account_ids,
            set_masked_account_ids,
            // OAuth
            start_login,
            complete_login,
            cancel_login,
            // Usage
            get_usage,
            get_account_usage_stats,
            refresh_account_metadata,
            refresh_all_accounts_usage,
            warmup_account,
            warmup_all_accounts,
            // Process detection
            check_codex_processes,
            kill_codex_processes,
            // Tray window
            hide_tray_window,
            open_main_window,
            quit_app,
            report_usage,
            get_dock_display_mode,
            set_dock_display_mode,
            complete_close_behavior,
            ack_close_behavior_prompt,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                commands::restore_main_window(_app);
            }
        });
}

/// Unix single-instance enforcement using flock(2) on a lock file.
/// The lock is held for the entire process lifetime via a leaked file descriptor.
#[cfg(unix)]
fn enforce_single_instance_unix(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    use tauri::Manager;

    let lock_path = app
        .path()
        .app_data_dir()?
        .join("codex-switcher.lock");

    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock_path)?;

    let _ = write!(file, "{}", std::process::id());

    let locked = unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0
    };

    if !locked {
        println!("[App] Another instance is already running. Exiting.");
        std::process::exit(0);
    }

    // Leak the file so the fd stays open (and the lock held) for the process lifetime.
    std::mem::forget(file);
    Ok(())
}

/// Windows single-instance enforcement using a named mutex.
#[cfg(windows)]
fn enforce_single_instance_windows() -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::CString;

    let mutex_name = CString::new("Local\\CodexSwitcher_SingleInstance").unwrap();

    let handle = unsafe {
        windows_sys::Win32::System::Threading::CreateMutexA(
            std::ptr::null(),
            1, // bInitialOwner = TRUE
            mutex_name.as_ptr() as *const u8,
        )
    };

    if handle.is_null() {
        // Could not create mutex — something went wrong, allow startup.
        return Ok(());
    }

    let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };

    // ERROR_ALREADY_EXISTS (183) means another instance owns the mutex.
    if last_error == 183 {
        println!("[App] Another instance is already running. Exiting.");
        std::process::exit(0);
    }

    // Leak the handle so the mutex stays owned for the process lifetime.
    std::mem::forget(handle);
    Ok(())
}
