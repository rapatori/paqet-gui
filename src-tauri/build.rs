fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_snapshot",
            "create_profile",
            "update_profile",
            "delete_profile",
            "select_profile",
            "refresh_interfaces",
            "select_interface",
            "set_socks_port",
            "replace_advanced_settings",
            "connect",
            "disconnect",
            "subscribe_runtime_events",
            "subscribe_window_close_requests",
            "cancel_window_close",
            "confirm_window_close",
        ]),
    ))
    .expect("failed to build PaqetGUI");
}
