#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod queue;

fn main() {
    // Desktop-entry launches: `pezo-app --launch <slug>` runs the game
    // headless (no window) and exits with its status.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let ["--launch", slug] = args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        std::process::exit(headless_launch(slug));
    }

    // WebKitGTK's DMA-BUF renderer crashes with a Wayland protocol error on
    // NVIDIA; must be set before the webview initializes.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safety: called at the top of main, before any other threads exist.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    tauri::Builder::default()
        // Must be first: a second launch focuses the existing window instead
        // of racing it for the library database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            // Never restore DECORATIONS: the config says CSD, and restoring a
            // stale `decorated: true` from disk causes a double titlebar.
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        - tauri_plugin_window_state::StateFlags::DECORATIONS
                        - tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            queue::spawn_worker(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_library_root,
            commands::add_scan_dir,
            commands::remove_scan_dir,
            commands::set_adopt_portables,
            commands::doctor,
            commands::desktop_layout,
            commands::installable_tools,
            commands::install_tool,
            commands::library_fs_info,
            commands::scan_installers,
            commands::list_candidates,
            commands::list_candidate_archives,
            commands::list_games,
            commands::queue_install,
            commands::queue_state,
            commands::queue_cancel,
            commands::queue_clear_finished,
            commands::get_cover,
            commands::fetch_missing_covers,
            commands::update_launch_options,
            commands::run_winetricks,
            commands::launch_game,
            commands::remove_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running pezo");
}

fn headless_launch(slug: &str) -> i32 {
    let result = (|| -> pezo_core::Result<pezo_core::launch::LaunchOutcome> {
        let config = pezo_core::Config::load()?;
        let (_, db) = pezo_core::open_library(&config)?;
        let game = db
            .find_game_by_slug(slug)?
            .ok_or_else(|| pezo_core::CoreError::NotFound(format!("game `{slug}`")))?;
        pezo_core::launch::launch_game(&db, game.id)
    })();
    match result {
        Ok(outcome) => outcome.exit_code.unwrap_or(0),
        Err(e) => {
            eprintln!("pezo: failed to launch `{slug}`: {e}");
            1
        }
    }
}
