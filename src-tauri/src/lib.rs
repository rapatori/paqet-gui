pub mod config;
pub mod ipc;
pub mod network;
pub mod process;
pub mod profiles;
pub mod state;

use tauri::Manager;

use ipc::IpcError;
use state::AppState;

pub const APP_NAME: &str = "paqet";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
            ipc::replace_advanced_settings,
            ipc::connect,
            ipc::disconnect,
            ipc::subscribe_runtime_events,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run paqet");
}

#[cfg(test)]
mod tests {
    use super::APP_NAME;

    #[test]
    fn application_name_is_stable() {
        assert_eq!(APP_NAME, "paqet");
    }
}
