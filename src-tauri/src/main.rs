#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    // WebKitGTK's DMA-BUF renderer crashes with a Wayland protocol error on
    // NVIDIA; must be set before the webview initializes.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safety: called at the top of main, before any other threads exist.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_library_root,
            commands::add_scan_dir,
            commands::remove_scan_dir,
            commands::doctor,
            commands::scan_installers,
            commands::list_candidates,
            commands::list_games,
            commands::install_candidate,
            commands::launch_game,
            commands::remove_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running pezo");
}
