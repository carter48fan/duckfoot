use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveNode {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rgb4Way {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawLevelsParams {
    pub black: f32,
    pub white: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhiteBalanceParams {
    pub temperature_k: f32,
    pub tint: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExposureParams {
    pub ev: f32,
    pub black_level: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToneCurveParams {
    pub channel: String, // "rgb" | "r" | "g" | "b" | "l"
    pub nodes: Vec<CurveNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmicRgbParams {
    pub white_rel_ev: f32,
    pub black_rel_ev: f32,
    pub latitude: f32,
    pub contrast: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorBalanceRgbParams {
    pub lift: Rgb4Way,
    pub gamma: Rgb4Way,
    pub gain: Rgb4Way,
    pub offset: Rgb4Way,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropRotateParams {
    pub aspect: Option<String>,
    pub angle_deg: f32,
    pub rect: Option<CropRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputProfileParams {
    pub profile: String,
    pub bit_depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInstance<T> {
    pub id: String,
    pub enabled: bool,
    pub params: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawStackParams {
    pub raw_levels: Option<ModuleInstance<RawLevelsParams>>,
    pub white_balance: Option<ModuleInstance<WhiteBalanceParams>>,
    pub exposure: Option<ModuleInstance<ExposureParams>>,
    pub tone_curve: Option<ModuleInstance<ToneCurveParams>>,
    pub filmic_rgb: Option<ModuleInstance<FilmicRgbParams>>,
    pub color_balance_rgb: Option<ModuleInstance<ColorBalanceRgbParams>>,
    pub crop_rotate: Option<ModuleInstance<CropRotateParams>>,
    pub output_profile: Option<ModuleInstance<OutputProfileParams>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramResult {
    pub r: Vec<u32>,
    pub g: Vec<u32>,
    pub b: Vec<u32>,
    pub l: Vec<u32>,
    pub max_bin: u32,
    pub clipped_low_fraction: f32,
    pub clipped_high_fraction: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPhotoResult {
    pub width: usize,
    pub height: usize,
    pub rgba_bytes: Vec<u8>,
    pub histogram: HistogramResult,
    pub clipped_high_percent: f32,
    pub render_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMetadata {
    pub make: String,
    pub model: String,
    pub width: usize,
    pub height: usize,
    pub default_black: u16,
    pub default_white: u16,
    pub as_shot_kelvin: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDecodeResult {
    pub metadata: RawMetadata,
    pub width: usize,
    pub height: usize,
    pub scene_data: Vec<f32>, // Decimated or full float RGB (3 floats per pixel)
}
