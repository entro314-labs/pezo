#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_library_root,
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
