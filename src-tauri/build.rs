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
            "replace_advanced_settings",
            "connect",
            "disconnect",
            "subscribe_runtime_events",
        ]),
    ))
    .expect("failed to build PaqetGUI");
}
