use std::sync::Mutex;
use std::time::Instant;
use tauri::State;
use super::pipeline::{process_scene, scene_to_rgba_and_histogram};
use super::raw_decode::decode_camera_raw_file;
use super::types::*;

#[derive(Clone)]
pub struct SceneBufferState {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
}

#[derive(Default)]
pub struct PhotoSession {
    pub current_scene: Mutex<Option<SceneBufferState>>,
}

#[tauri::command]
pub fn set_photo_scene(
    pixels: Vec<f32>,
    width: usize,
    height: usize,
    session: State<PhotoSession>,
) -> Result<bool, String> {
    let mut guard = session
        .current_scene
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    *guard = Some(SceneBufferState {
        width,
        height,
        pixels,
    });
    Ok(true)
}

#[tauri::command]
pub fn render_current_photo(
    stack: RawStackParams,
    session: State<PhotoSession>,
) -> Result<RenderPhotoResult, String> {
    let guard = session
        .current_scene
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;

    let scene_state = guard
        .as_ref()
        .ok_or_else(|| "No photo loaded in current session".to_string())?;

    let start = Instant::now();
    let width = scene_state.width;
    let height = scene_state.height;
    let mut working_buffer = scene_state.pixels.clone();

    process_scene(&mut working_buffer, width, height, &stack);
    let (rgba_bytes, histogram, clipped_high_percent) =
        scene_to_rgba_and_histogram(&working_buffer, width, height);

    let render_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    Ok(RenderPhotoResult {
        width,
        height,
        rgba_bytes,
        histogram,
        clipped_high_percent,
        render_time_ms,
    })
}

#[tauri::command]
pub fn load_and_decode_camera_raw(
    file_path: String,
    max_edge: Option<usize>,
    session: State<PhotoSession>,
) -> Result<RawMetadata, String> {
    let decoded = decode_camera_raw_file(&file_path, max_edge.unwrap_or(1600))?;
    let metadata = decoded.metadata.clone();

    let mut guard = session
        .current_scene
        .lock()
        .map_err(|e| format!("Lock poisoned: {}", e))?;
    *guard = Some(SceneBufferState {
        width: decoded.width,
        height: decoded.height,
        pixels: decoded.scene_data,
    });

    Ok(metadata)
}

#[tauri::command]
pub fn render_photo_stack(
    pixels: Vec<f32>,
    width: usize,
    height: usize,
    stack: RawStackParams,
) -> RenderPhotoResult {
    let start = Instant::now();
    let mut working_buffer = pixels;

    process_scene(&mut working_buffer, width, height, &stack);
    let (rgba_bytes, histogram, clipped_high_percent) =
        scene_to_rgba_and_histogram(&working_buffer, width, height);

    let render_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    RenderPhotoResult {
        width,
        height,
        rgba_bytes,
        histogram,
        clipped_high_percent,
        render_time_ms,
    }
}

#[tauri::command]
pub fn decode_camera_raw(
    file_path: String,
    max_edge: Option<usize>,
) -> Result<RawDecodeResult, String> {
    decode_camera_raw_file(&file_path, max_edge.unwrap_or(1600))
}
