// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod queue;
mod settings;

use std::sync::Arc;

use tauri::Manager;

fn main() {
    scribe_core::asr::whisper::handle_gpu_probe();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    scribe_core::asr::whisper::install_logging();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            commands::prune_old_playback_copies(app.handle());
            let queue = Arc::new(queue::Queue::default());
            queue::spawn_worker(app.handle().clone(), queue.clone());
            app.manage(queue);
            app.manage(commands::Downloads::default());
            // `tiroscribe interview.scribe.json` opens the transcript in the editor.
            let initial = std::env::args().nth(1).filter(|a| a.ends_with(".json"));
            app.manage(commands::InitialFile(initial.map(|p| {
                dunce::canonicalize(&p)
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or(p)
            })));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::initial_file,
            commands::list_models,
            commands::download_model,
            commands::cancel_download,
            commands::probe_npu,
            commands::enqueue_job,
            commands::list_jobs,
            commands::cancel_job,
            commands::remove_job,
            commands::default_output_path,
            commands::load_transcript,
            commands::save_transcript,
            commands::export_transcript,
            commands::prepare_playback,
            commands::playback_cache_size,
            commands::clear_playback_cache,
            settings::get_settings,
            settings::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tiroScribe");
}
