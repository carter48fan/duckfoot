// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod photo;

fn main() {
    tauri::Builder::default()
        .manage(photo::PhotoSession::default())
        .invoke_handler(tauri::generate_handler![
            photo::render_photo_stack,
            photo::decode_camera_raw,
            photo::set_photo_scene,
            photo::render_current_photo,
            photo::load_and_decode_camera_raw,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Duckfoot desktop application");
}

