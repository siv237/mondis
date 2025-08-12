use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorId {
    pub name: String,       // X11 output name or Wayland identifier
    pub edid_hash: Option<String>, // hash of EDID for stable identity
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size_mm: Option<(u16, u16)>,
    pub current_mode: Option<(u32, u32, u32)>, // width, height, refresh*1000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrightnessInfo {
    pub supported: bool,
    pub value: Option<u8>, // 0..100
}
