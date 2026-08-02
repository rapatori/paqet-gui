pub mod config;
pub mod ipc;
pub mod network;
pub mod process;
pub mod profiles;
pub mod settings;
pub mod state;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::{Manager, RunEvent, WindowEvent};

use ipc::IpcError;
use state::AppState;

pub const APP_NAME: &str = "PaqetGUI";
pub const MAIN_WINDOW_LABEL: &str = "main";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let state = AppState::from_app_handle(app.handle()).map_err(IpcError::from);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_app_snapshot,
            ipc::create_profile,
            ipc::update_profile,
            ipc::delete_profile,
            ipc::select_profile,
            ipc::refresh_interfaces,
            ipc::select_interface,
            ipc::set_socks_port,
            ipc::replace_advanced_settings,
            ipc::connect,
            ipc::disconnect,
            ipc::subscribe_runtime_events,
            ipc::subscribe_window_close_requests,
            ipc::cancel_window_close,
            ipc::confirm_window_close,
        ])
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            let WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let Some(managed) = window.try_state::<ipc::ManagedAppState>() else {
                return;
            };
            let Ok(state) = managed.as_ref() else {
                return;
            };
            match state.request_window_close() {
                Ok(state::WindowCloseDecision::Allow) => {}
                Ok(state::WindowCloseDecision::Confirm(_)) => {
                    api.prevent_close();
                }
                Ok(state::WindowCloseDecision::Shutdown) | Err(_) => {
                    api.prevent_close();
                    let state = state.clone();
                    let window = window.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if state.shutdown().is_ok() {
                            let _ = window.destroy();
                        }
                    });
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build PaqetGUI");
    let shutdown_started = Arc::new(AtomicBool::new(false));
    app.run(move |handle, event| {
        let RunEvent::ExitRequested { api, .. } = event else {
            return;
        };
        let Some(managed) = handle.try_state::<ipc::ManagedAppState>() else {
            return;
        };
        let Ok(state) = managed.as_ref() else {
            return;
        };
        match state.begin_application_exit() {
            Ok(state::ApplicationExitDecision::Allow) => return,
            Ok(state::ApplicationExitDecision::Shutdown) | Err(_) => api.prevent_exit(),
        }
        if shutdown_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let state = state.clone();
        let handle = handle.clone();
        let shutdown_started = Arc::clone(&shutdown_started);
        tauri::async_runtime::spawn_blocking(move || {
            if state.shutdown().is_ok() {
                handle.exit(0);
            } else {
                shutdown_started.store(false, Ordering::Release);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{APP_NAME, MAIN_WINDOW_LABEL};

    #[test]
    fn application_name_is_stable() {
        assert_eq!(APP_NAME, "PaqetGUI");
        assert_eq!(MAIN_WINDOW_LABEL, "main");
    }
}
