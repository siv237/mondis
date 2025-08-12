use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale, Separator, Button, HeaderBar, CssProvider, Image};
use gtk::gdk::Display as GdkDisplay;
use tracing_subscriber::EnvFilter;
use glib::clone;
use std::rc::Rc;
use std::cell::Cell;
use std::thread;

use std::time::Duration;
use std::process::Command;
use i2cdev::linux::LinuxI2CDevice;
use i2cdev::core::I2CDevice;

fn setup_styles() {
    // Современные стили: карточки, отступы, скругления
    let css = r#"
    .root { background-color: @theme_bg_color; }
    .card { 
        padding: 12px; 
        border-radius: 10px; 
        border: 1px solid alpha(@theme_fg_color, 0.08);
        background-color: alpha(@theme_base_color, 0.6);
    }
    .section-title { font-weight: 700; opacity: 0.9; }
    .gpu-title { font-weight: 700; margin-top: 8px; }
    .row { spacing: 12px; }
    .info-box { spacing: 8px; }
    .badge { 
        padding: 2px 8px; 
        border-radius: 999px; 
        font-size: 11px; 
        color: rgba(0,0,0,0.85);
        background-color: rgba(0,0,0,0.06);
        margin-left: 6px;
    }
    .badge-ddc { background-color: rgba(46,160,67,0.18); }
    .badge-xrandr { background-color: rgba(56,139,253,0.16); }
    .badge-none { background-color: rgba(0,0,0,0.08); }
    .port { opacity: 0.8; }
    .type { opacity: 0.8; }
    .monitor { font-weight: 600; }
    "#;

    let provider = CssProvider::new();
    provider.load_from_data(css);
    if let Some(display) = GdkDisplay::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}
fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let app = Application::builder()
        .application_id("com.mondis.panel.direct")
        .build();

    app.connect_activate(move |app| {
        build_ui(app);
    });

    app.run();
    Ok(())
}

#[derive(Clone, Debug)]
struct DisplayInfo { 
    i2c_bus: u8,
    name: String,
    manufacturer: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    connector: Option<String>,
    supports_ddc: bool,
    xrandr_output: Option<String>,
    card_name: Option<String>,
    port_name: Option<String>,
}

#[derive(Clone, Debug)]
struct VideoCard {
    name: String,
    displays: Vec<DisplayInfo>,
}

// DDC/CI constants
const DDC_ADDR: u16 = 0x37;
const EDID_ADDR: u16 = 0x50;
const VCP_BRIGHTNESS: u8 = 0x10;

fn ddc_get_brightness(i2c_bus: u8) -> Result<u8, String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    let mut dev = LinuxI2CDevice::new(&device_path, DDC_ADDR)
        .map_err(|e| format!("Failed to open {}: {}", device_path, e))?;
    
    // DDC Get VCP request: [source_addr, length, type, vcp_code, checksum]
    let request = [0x51, 0x02, 0x01, VCP_BRIGHTNESS];
    let checksum = 0x6E ^ request.iter().fold(0u8, |acc, &x| acc ^ x);
    let full_request = [request[0], request[1], request[2], request[3], checksum];
    
    dev.write(&full_request)
        .map_err(|e| format!("Failed to write DDC request: {}", e))?;
    
    // Small delay for DDC response
    thread::sleep(Duration::from_millis(50));
    
    // Read response: [dest, length, type, result, vcp_code, type_flag, max_hi, max_lo, cur_hi, cur_lo, checksum]
    let mut response = [0u8; 12];
    dev.read(&mut response)
        .map_err(|e| format!("Failed to read DDC response: {}", e))?;
    
    // Parse current brightness value (bytes 8-9)
    if response.len() >= 10 && response[4] == VCP_BRIGHTNESS {
        let current_value = ((response[8] as u16) << 8) | (response[9] as u16);
        Ok((current_value & 0xFF) as u8)
    } else {
        Err("Invalid DDC response".to_string())
    }
}

fn parse_edid_manufacturer(edid_data: &[u8]) -> String {
    if edid_data.len() < 10 {
        return "Unknown".to_string();
    }
    
    // Manufacturer ID is at bytes 8-9 (big endian)
    let mfg_id = ((edid_data[8] as u16) << 8) | (edid_data[9] as u16);
    
    // Extract 3 letters (5 bits each)
    let letter1 = ((mfg_id >> 10) & 0x1F) as u8;
    let letter2 = ((mfg_id >> 5) & 0x1F) as u8;
    let letter3 = (mfg_id & 0x1F) as u8;
    
    // Convert to ASCII (A=1, B=2, etc.)
    if letter1 > 0 && letter2 > 0 && letter3 > 0 {
        format!("{}{}{}", 
            (letter1 + b'A' - 1) as char,
            (letter2 + b'A' - 1) as char,
            (letter3 + b'A' - 1) as char
        )
    } else {
        "Unknown".to_string()
    }
}

fn parse_edid_model(edid_data: &[u8]) -> String {
    if edid_data.len() < 128 {
        return "Unknown Monitor".to_string();
    }
    
    // Look for descriptor blocks starting at byte 54
    for i in (54..126).step_by(18) {
        if edid_data[i] == 0x00 && edid_data[i+1] == 0x00 && edid_data[i+2] == 0x00 {
            // This is a display descriptor
            if edid_data[i+3] == 0xFC { // Monitor name descriptor
                let name_bytes = &edid_data[i+5..i+18];
                let name = String::from_utf8_lossy(name_bytes)
                    .trim_end_matches('\0')
                    .trim_end_matches('\n')
                    .trim()
                    .to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    
    "Monitor".to_string()
}

fn get_all_edids_from_sysfs() -> Vec<(String, String, String, String)> {
    let mut edids = Vec::new();
    let drm_path = "/sys/class/drm";
    
    if let Ok(entries) = std::fs::read_dir(drm_path) {
        for entry in entries.flatten() {
            let connector_path = entry.path();
            if let Some(name) = connector_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("card") && (name.contains("DP") || name.contains("HDMI") || name.contains("DVI")) {
                    let edid_path = connector_path.join("edid");
                    if let Ok(edid_data) = std::fs::read(&edid_path) {
                        if edid_data.len() >= 128 && edid_data[0] == 0x00 && edid_data[1] == 0xFF {
                            let manufacturer = parse_edid_manufacturer(&edid_data);
                            let model = parse_edid_model(&edid_data);
                            let serial = format!("{:08X}", 
                                ((edid_data[12] as u32) << 24) | 
                                ((edid_data[13] as u32) << 16) | 
                                ((edid_data[14] as u32) << 8) | 
                                (edid_data[15] as u32)
                            );
                            println!("Found EDID in {}: {} {}", name, manufacturer, model);
                            edids.push((manufacturer, model, serial, name.to_string()));
                        }
                    }
                }
            }
        }
    }
    edids
}

fn read_edid(i2c_bus: u8) -> Result<(String, String, String), String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    let mut dev = LinuxI2CDevice::new(&device_path, EDID_ADDR)
        .map_err(|e| format!("Failed to open {} for EDID: {}", device_path, e))?;
    
    // Write EDID offset (0x00) first, like ddcutil does
    dev.write(&[0x00])
        .map_err(|e| format!("Failed to write EDID offset: {}", e))?;
    
    // Small delay
    thread::sleep(Duration::from_millis(10));
    
    // Read EDID (128 bytes minimum)
    let mut edid_data = [0u8; 128];
    dev.read(&mut edid_data)
        .map_err(|e| format!("Failed to read EDID: {}", e))?;
    
    // Debug: print first few bytes
    println!("Bus {}: EDID header: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}", 
        i2c_bus, edid_data[0], edid_data[1], edid_data[2], edid_data[3], 
        edid_data[4], edid_data[5], edid_data[6], edid_data[7]);
    
    // Check EDID header (should start with 00 FF FF FF FF FF FF 00)
    if edid_data[0] != 0x00 || edid_data[1] != 0xFF || edid_data[7] != 0x00 {
        return Err(format!("Invalid EDID header: {:02X} {:02X} ... {:02X}", 
            edid_data[0], edid_data[1], edid_data[7]));
    }
    
    let manufacturer = parse_edid_manufacturer(&edid_data);
    let model = parse_edid_model(&edid_data);
    let serial = format!("{:08X}", 
        ((edid_data[12] as u32) << 24) | 
        ((edid_data[13] as u32) << 16) | 
        ((edid_data[14] as u32) << 8) | 
        (edid_data[15] as u32)
    );
    
    println!("Bus {}: Parsed - Manufacturer: {}, Model: {}, Serial: {}", 
        i2c_bus, manufacturer, model, serial);
    
    Ok((manufacturer, model, serial))
}

fn get_gpu_model_name(vendor_id: &str, device_id: &str) -> Option<String> {
    // Mapping common GPU device IDs to model names
    match (vendor_id, device_id) {
        // NVIDIA RTX 40 series
        ("0x10de", "0x2684") => Some("RTX 4090".to_string()),
        ("0x10de", "0x2782") => Some("RTX 4070 Ti".to_string()),
        ("0x10de", "0x2786") => Some("RTX 4070".to_string()),
        ("0x10de", "0x2788") => Some("RTX 4060 Ti".to_string()),
        ("0x10de", "0x28e0") => Some("RTX 4060".to_string()),
        
        // NVIDIA RTX 30 series
        ("0x10de", "0x2204") => Some("RTX 3090".to_string()),
        ("0x10de", "0x2206") => Some("RTX 3080".to_string()),
        ("0x10de", "0x2484") => Some("RTX 3070".to_string()),
        ("0x10de", "0x2504") => Some("RTX 3060".to_string()),
        ("0x10de", "0x2487") => Some("RTX 3060 Ti".to_string()),
        
        // NVIDIA RTX 20 series
        ("0x10de", "0x1e04") => Some("RTX 2080 Ti".to_string()),
        ("0x10de", "0x1e07") => Some("RTX 2080".to_string()),
        ("0x10de", "0x1f02") => Some("RTX 2070".to_string()),
        ("0x10de", "0x1f06") => Some("RTX 2060".to_string()),
        
        // NVIDIA GTX 16 series
        ("0x10de", "0x2182") => Some("GTX 1660 Ti".to_string()),
        ("0x10de", "0x21c4") => Some("GTX 1660".to_string()),
        ("0x10de", "0x1f82") => Some("GTX 1650".to_string()),
        
        // AMD RX 7000 series
        ("0x1002", "0x744c") => Some("RX 7900 XTX".to_string()),
        ("0x1002", "0x7448") => Some("RX 7900 XT".to_string()),
        ("0x1002", "0x747e") => Some("RX 7800 XT".to_string()),
        ("0x1002", "0x7479") => Some("RX 7700 XT".to_string()),
        
        // AMD RX 6000 series
        ("0x1002", "0x73bf") => Some("RX 6900 XT".to_string()),
        ("0x1002", "0x73df") => Some("RX 6800 XT".to_string()),
        ("0x1002", "0x73ef") => Some("RX 6800".to_string()),
        ("0x1002", "0x73ff") => Some("RX 6700 XT".to_string()),
        ("0x1002", "0x7421") => Some("RX 6600 XT".to_string()),
        ("0x1002", "0x73e3") => Some("RX 6600".to_string()),
        
        // Intel Arc
        ("0x8086", "0x56a0") => Some("Arc A770".to_string()),
        ("0x8086", "0x56a1") => Some("Arc A750".to_string()),
        ("0x8086", "0x56a5") => Some("Arc A380".to_string()),
        
        _ => None,
    }
}

fn get_gpu_name_from_lspci() -> Option<String> {
    // Try to get GPU name from lspci command
    let output = Command::new("lspci")
        .arg("-nn")
        .output()
        .ok()?;
    
    if !output.status.success() {
        return None;
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse lspci output to find VGA/Display controller
    for line in stdout.lines() {
        if line.contains("VGA compatible controller") || line.contains("Display controller") {
            // Extract GPU name from line like:
            // "01:00.0 VGA compatible controller [0300]: NVIDIA Corporation GA106 [GeForce RTX 3060 Lite Hash Rate] [10de:2504] (rev a1)"
            if let Some(colon_pos) = line.find(": ") {
                let gpu_part = &line[colon_pos + 2..];
                // Remove the final PCI ID part [10de:2504] and revision
                let gpu_name = gpu_part
                    .split(" [")
                    .last()
                    .unwrap_or(gpu_part)
                    .split(']')
                    .next()
                    .unwrap_or("")
                    .trim();
                
                if !gpu_name.is_empty() {
                    // Extract just the model name from full string
                    // "NVIDIA Corporation GA106 [GeForce RTX 3060 Lite Hash Rate]" -> "GeForce RTX 3060 Lite Hash Rate"
                    if let Some(bracket_start) = gpu_part.find('[') {
                        if let Some(bracket_end) = gpu_part.find(']') {
                            let model_name = &gpu_part[bracket_start + 1..bracket_end];
                            if !model_name.is_empty() {
                                // Get vendor from the beginning
                                let vendor_part = &gpu_part[..bracket_start].trim();
                                let vendor = if vendor_part.contains("NVIDIA") {
                                    "NVIDIA"
                                } else if vendor_part.contains("AMD") || vendor_part.contains("ATI") {
                                    "AMD"
                                } else if vendor_part.contains("Intel") {
                                    "Intel"
                                } else {
                                    "Unknown"
                                };
                                return Some(format!("{} {}", vendor, model_name));
                            }
                        }
                    }
                }
            }
        }
    }
    
    None
}

fn get_gpu_name_from_card(card_num: u8) -> String {
    // First try to get name from lspci (most accurate)
    if let Some(lspci_name) = get_gpu_name_from_lspci() {
        println!("GPU info for card{}: {} (from lspci)", card_num, lspci_name);
        return lspci_name;
    }
    
    // Fallback to sysfs PCI ID lookup
    let device_path = format!("/sys/class/drm/card{}/device", card_num);
    
    // Try to read vendor and device IDs
    if let (Ok(vendor), Ok(device)) = (
        std::fs::read_to_string(format!("{}/vendor", device_path)),
        std::fs::read_to_string(format!("{}/device", device_path))
    ) {
        let vendor_id = vendor.trim();
        let device_id = device.trim();
        
        // Try to get specific model name from our table
        if let Some(model) = get_gpu_model_name(vendor_id, device_id) {
            let vendor_name = match vendor_id {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Unknown",
            };
            let result = format!("{} {}", vendor_name, model);
            println!("GPU info for card{}: {} (from PCI ID table)", card_num, result);
            return result;
        }
        
        // Fallback to generic vendor name
        let vendor_name = match vendor_id {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD", 
            "0x8086" => "Intel",
            _ => "Unknown GPU",
        };
        
        let result = format!("{} Card {}", vendor_name, card_num);
        println!("GPU info for card{}: {} (generic fallback)", card_num, result);
        result
    } else {
        let result = format!("Card {}", card_num);
        println!("GPU info for card{}: {} (no vendor/device info)", card_num, result);
        result
    }
}

fn parse_connector_info(connector: &str) -> (Option<String>, Option<String>, u8) {
    // Parse "card1-DP-3" -> (card_name, port_name, card_num)
    println!("Parsing connector: {}", connector);
    let parts: Vec<&str> = connector.split('-').collect();
    if parts.len() >= 3 {
        let card_part = parts[0]; // "card1"
        let port_type = parts[1]; // "DP"
        let port_num = parts[2];  // "3"
        
        let card_num = card_part.replace("card", "").parse::<u8>().unwrap_or(0);
        let card_name = get_gpu_name_from_card(card_num);
        
        // Use full descriptive port names instead of simplified numbering
        let port_name = match port_type {
            "HDMI" => {
                if port_num == "A" {
                    format!("HDMI Port A")
                } else {
                    format!("HDMI Port {}", port_num)
                }
            },
            "DP" => format!("DisplayPort {}", port_num),
            "DVI" => format!("DVI Port {}", port_num),
            "VGA" => format!("VGA Port {}", port_num),
            "eDP" => format!("eDP Port {}", port_num),
            "LVDS" => format!("LVDS Port {}", port_num),
            _ => format!("{} Port {}", port_type, port_num),
        };
        
        println!("Parsed: card_name={}, port_name={}, card_num={}", card_name, port_name, card_num);
        (Some(card_name), Some(port_name), card_num)
    } else {
        println!("Failed to parse connector: {}", connector);
        (None, None, 0)
    }
}

fn get_xrandr_outputs() -> Vec<(String, Vec<u8>)> {
    // Получаем список xrandr выводов с их EDID
    let output = match Command::new("xrandr")
        .arg("--verbose")
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
        Err(_) => return Vec::new(),
    };
    
    let mut outputs = Vec::new();
    let mut current_output: Option<String> = None;
    let mut edid_lines = Vec::new();
    
    for line in output.lines() {
        // Ищем строки вида "DP-3 connected primary 2560x1440+0+0"
        if line.contains("connected") && !line.contains("disconnected") {
            // Сохраняем предыдущий EDID если есть
            if let Some(ref output_name) = current_output {
                if !edid_lines.is_empty() {
                    let edid_hex = edid_lines.join("");
                    if let Ok(edid_bytes) = hex::decode(&edid_hex) {
                        outputs.push((output_name.clone(), edid_bytes));
                    }
                }
            }
            
            if let Some(output_name) = line.split_whitespace().next() {
                current_output = Some(output_name.to_string());
                edid_lines.clear();
            }
        }
        
        // Ищем EDID в hex формате (строки только из hex символов)
        if current_output.is_some() {
            let trimmed = line.trim();
            if trimmed.len() == 32 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                edid_lines.push(trimmed.to_string());
            }
        }
    }
    
    // Обрабатываем последний EDID
    if let Some(ref output_name) = current_output {
        if !edid_lines.is_empty() {
            let edid_hex = edid_lines.join("");
            if let Ok(edid_bytes) = hex::decode(&edid_hex) {
                outputs.push((output_name.clone(), edid_bytes));
            }
        }
    }
    
    outputs
}

fn get_drm_connector_edid(connector: &str) -> Option<Vec<u8>> {
    let edid_path = format!("/sys/class/drm/{}/edid", connector);
    std::fs::read(&edid_path).ok()
}

fn edid_matches(edid1: &[u8], edid2: &[u8]) -> bool {
    if edid1.len() < 128 || edid2.len() < 128 {
        return false;
    }
    
    // Сравниваем первые 128 байт EDID (основной блок)
    edid1[..128] == edid2[..128]
}

fn get_xrandr_output_for_connector(connector: &str) -> Option<String> {
    println!("Mapping connector: {}", connector);
    
    // Получаем EDID для DRM коннектора
    let drm_edid = get_drm_connector_edid(connector)?;
    if drm_edid.len() < 128 {
        println!("DRM connector {} has invalid EDID", connector);
        return None;
    }
    
    // Получаем все xrandr выводы с их EDID
    let xrandr_outputs = get_xrandr_outputs();
    
    // Ищем соответствие по EDID
    for (output_name, xrandr_edid) in xrandr_outputs {
        if edid_matches(&drm_edid, &xrandr_edid) {
            println!("Matched {} -> {} by EDID", connector, output_name);
            return Some(output_name);
        }
    }
    
    println!("No EDID match found for connector: {}", connector);
    
    // Fallback: простое соответствие по типу порта
    if connector.contains("HDMI") {
        Some("HDMI-0".to_string())
    } else if connector.contains("DP") {
        // Попробуем угадать по номеру порта
        if let Some(port_num) = connector.split('-').last() {
            Some(format!("DP-{}", port_num))
        } else {
            None
        }
    } else {
        None
    }
}

fn set_xrandr_brightness(output: &str, brightness: f64) -> Result<(), String> {
    let output = Command::new("xrandr")
        .arg("--output")
        .arg(output)
        .arg("--brightness")
        .arg(brightness.to_string())
        .output()
        .map_err(|e| format!("Failed to run xrandr: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("xrandr failed: {}", stderr));
    }
    
    Ok(())
}

fn get_xrandr_brightness(output_name: &str) -> Result<f64, String> {
    let output = Command::new("xrandr")
        .arg("--verbose")
        .output()
        .map_err(|e| format!("Failed to run xrandr: {}", e))?;
    
    if !output.status.success() {
        return Err("xrandr failed".to_string());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found_output = false;
    
    for line in stdout.lines() {
        if line.contains(output_name) && line.contains("connected") {
            found_output = true;
            continue;
        }
        if found_output && line.contains("Brightness:") {
            if let Some(brightness_str) = line.split("Brightness:").nth(1) {
                if let Ok(brightness) = brightness_str.trim().parse::<f64>() {
                    return Ok(brightness);
                }
            }
        }
        // Stop if we hit another output
        if found_output && line.contains("connected") {
            break;
        }
    }
    
    Ok(1.0) // Default brightness
}

fn set_brightness_any_method(display: &DisplayInfo, value: u8) -> Result<(), String> {
    if display.supports_ddc {
        ddc_set_brightness(display.i2c_bus, value)
    } else if let Some(ref output) = display.xrandr_output {
        let brightness = value as f64 / 100.0;
        set_xrandr_brightness(output, brightness)
    } else {
        Err("No brightness control method available".to_string())
    }
}

fn get_brightness_any_method(display: &DisplayInfo) -> Result<u8, String> {
    if display.supports_ddc {
        ddc_get_brightness(display.i2c_bus)
    } else if let Some(ref output) = display.xrandr_output {
        let brightness = get_xrandr_brightness(output)?;
        Ok((brightness * 100.0) as u8)
    } else {
        Err("No brightness control method available".to_string())
    }
}

fn ddc_set_brightness(i2c_bus: u8, value: u8) -> Result<(), String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    let mut dev = LinuxI2CDevice::new(&device_path, DDC_ADDR)
        .map_err(|e| format!("Failed to open {}: {}", device_path, e))?;
    
    // DDC Set VCP request: [source_addr, length, type, vcp_code, hi_byte, lo_byte]
    let request = [0x51, 0x04, 0x03, VCP_BRIGHTNESS, 0x00, value];
    let checksum = 0x6E ^ request.iter().fold(0u8, |acc, &x| acc ^ x);
    let full_request = [request[0], request[1], request[2], request[3], request[4], request[5], checksum];
    
    dev.write(&full_request)
        .map_err(|e| format!("Failed to write DDC set request: {}", e))?;
    
    Ok(())
}

fn detect_i2c_displays() -> Result<Vec<DisplayInfo>, String> {
    let mut displays = Vec::new();
    
    // Get all EDIDs from sysfs first
    let all_edids = get_all_edids_from_sysfs();
    
    // Map known buses to EDID info based on ddcutil output
    let bus_edid_map = std::collections::HashMap::from([
        (3u8, 0usize), // GSM LG TV
        (5u8, 1usize), // HSI HiTV  
        (6u8, 2usize), // ACR VG270U
    ]);
    
    // Scan common I2C buses (typically 0-10)
    for bus in 0..=10 {
        let device_path = format!("/dev/i2c-{}", bus);
        if std::path::Path::new(&device_path).exists() {
            // Try to detect DDC capability first
            let supports_ddc = ddc_get_brightness(bus).is_ok();
            
            // Assign EDID info based on known mapping
            let (manufacturer, model, serial, connector) = if let Some(&edid_idx) = bus_edid_map.get(&bus) {
                if edid_idx < all_edids.len() {
                    let edid = &all_edids[edid_idx];
                    (Some(edid.0.clone()), Some(edid.1.clone()), Some(edid.2.clone()), Some(edid.3.clone()))
                } else {
                    (None, None, None, None)
                }
            } else {
                (None, None, None, None)
            };
            
            // Get xrandr output name for fallback brightness control
            let xrandr_output = connector.as_ref().and_then(|c| get_xrandr_output_for_connector(c));
            if let Some(ref xrandr_out) = xrandr_output {
                println!("Bus {}: xrandr output = {}", bus, xrandr_out);
            }
            
            // Parse connector info for card and port names
            let (card_name, port_name, _card_num) = if let Some(ref conn) = connector {
                parse_connector_info(conn)
            } else {
                (Some("Unknown GPU".to_string()), Some("Unknown Port".to_string()), 0)
            };
            
            let name = match (&manufacturer, &model) {
                (Some(mfg), Some(mdl)) => {
                    let expanded_mfg = match mfg.as_str() {
                        "ACR" => "Acer",
                        "GSM" => "LG",
                        "SAM" => "Samsung", 
                        "DEL" => "Dell",
                        "AUS" => "ASUS",
                        "BNQ" => "BenQ",
                        "AOC" => "AOC",
                        "HPN" => "HP",
                        "LEN" => "Lenovo",
                        "MSI" => "MSI",
                        _ => mfg,
                    };
                    let connector_info = connector.as_ref()
                        .map(|c| format!(" • {}", c))
                        .unwrap_or_default();
                    let control_method = if supports_ddc {
                        "DDC"
                    } else if xrandr_output.is_some() {
                        "xrandr"
                    } else {
                        "нет управления"
                    };
                    format!("{} {}{} ({})", expanded_mfg, mdl, connector_info, control_method)
                }
                _ => {
                    let control_method = if supports_ddc {
                        "DDC"
                    } else if xrandr_output.is_some() {
                        "xrandr"
                    } else {
                        "нет управления"
                    };
                    format!("I2C Device (bus {}) ({})", bus, control_method)
                }
            };
            
            // Show devices that have EDID info or any brightness control
            if manufacturer.is_some() || supports_ddc || xrandr_output.is_some() {
                displays.push(DisplayInfo {
                    i2c_bus: bus,
                    name,
                    manufacturer,
                    model,
                    serial,
                    connector,
                    supports_ddc,
                    xrandr_output,
                    card_name,
                    port_name,
                });
            }
        }
    }
    
    Ok(displays)
}

fn group_displays_by_card(displays: Vec<DisplayInfo>) -> Vec<VideoCard> {
    if displays.is_empty() {
        return vec![];
    }
    
    let mut cards: std::collections::HashMap<String, Vec<DisplayInfo>> = std::collections::HashMap::new();
    
    for display in displays {
        let card_key = display.card_name.clone().unwrap_or_else(|| "Unknown GPU".to_string());
        cards.entry(card_key).or_insert_with(Vec::new).push(display);
    }
    
    let mut result: Vec<VideoCard> = cards.into_iter()
        .map(|(name, displays)| VideoCard { name, displays })
        .collect();
    
    // Sort by card name for consistent ordering
    result.sort_by(|a, b| a.name.cmp(&b.name));
    
    // Debug output
    for card in &result {
        println!("Card: {} with {} displays", card.name, card.displays.len());
    }
    
    result
}

fn build_ui(app: &Application) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("Mondis Panel")
        .build();
    // Пусть окно подстраивается под естественный размер контента
    win.set_resizable(true);

    // По умолчанию светлая тема
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(false);
    }

    setup_styles();

    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    vbox.add_css_class("root");

    // Современный заголовок окна
    let headerbar = HeaderBar::new();
    let title_lbl = Label::new(Some("Mondis"));
    title_lbl.add_css_class("section-title");
    headerbar.set_title_widget(Some(&title_lbl));
    let refresh_btn = Button::with_label("Обновить");
    refresh_btn.add_css_class("suggested-action");

    // Кнопка переключения темы (день/ночь)
    let theme_toggle = Button::new();
    theme_toggle.set_tooltip_text(Some("Светлая/тёмная тема"));
    let is_dark = Cell::new(false); // состояние темы
    let icon_sun = Image::from_icon_name("weather-clear-symbolic");
    theme_toggle.set_child(Some(&icon_sun));

    // Захватываем ссылку на Settings для переключения темы
    if let Some(settings) = gtk::Settings::default() {
        let settings_clone = settings.clone();
        let is_dark_cell = is_dark; // перемещаем в замыкание
        theme_toggle.connect_clicked(move |btn| {
            let new_val = !is_dark_cell.get();
            is_dark_cell.set(new_val);
            settings_clone.set_gtk_application_prefer_dark_theme(new_val);
            // Меняем иконку
            let new_icon = if new_val {
                Image::from_icon_name("weather-clear-night-symbolic")
            } else {
                Image::from_icon_name("weather-clear-symbolic")
            };
            btn.set_child(Some(&new_icon));
        });
    }

    headerbar.pack_end(&refresh_btn);
    headerbar.pack_end(&theme_toggle);
    win.set_titlebar(Some(&headerbar));

    let list_box = GtkBox::new(Orientation::Vertical, 12);
    list_box.set_hexpand(true);
    vbox.append(&list_box);

    let list_box_for_populate = list_box.clone();
    // Используем слабую ссылку на окно и ссылку на корневой контейнер для измерения
    let win_weak_for_measure = win.downgrade();
    let content_for_measure = vbox.clone();
    let populate: Rc<dyn Fn()> = Rc::new(move || {
        // Clear list
        while let Some(child) = list_box_for_populate.first_child() {
            list_box_for_populate.remove(&child);
        }

        let header = Label::new(Some("Видеокарты и мониторы"));
        header.set_xalign(0.0);
        list_box_for_populate.append(&header);
        
        let (tx, rx) = async_channel::unbounded::<Result<Vec<VideoCard>, String>>();
        thread::spawn(move || {
            let displays_result = detect_i2c_displays();
            let cards_result = displays_result.map(|displays| group_displays_by_card(displays));
            let _ = tx.send_blocking(cards_result);
        });
        
        let list_box_target = list_box_for_populate.clone();
        // Клонируем ссылки для измерения, чтобы не тащить исходные и избежать move-проблем
        let win_weak_for_measure_outer = win_weak_for_measure.clone();
        let content_for_measure_outer = content_for_measure.clone();
        glib::spawn_future_local(async move {
            if let Ok(msg) = rx.recv().await {
                match msg {
                    Ok(cards) if !cards.is_empty() => {
                        for card in cards {
                            // Add card header
                            let card_header = Label::new(Some(&card.name));
                            card_header.set_xalign(0.0);
                            card_header.set_markup(&format!("<b>{}</b>", card.name));
                            list_box_target.append(&card_header);
                            
                            // Add displays for this card
                            for (display_index, d) in card.displays.into_iter().enumerate() {
                                // Grid-ряд для ровных колонок: Порт | Тип | Название | Иконка | Бейдж | Слайдер | Значение
                                let grid = gtk::Grid::new();
                                grid.set_margin_start(20);
                                grid.set_column_spacing(12);
                                grid.set_row_spacing(6);
                                grid.add_css_class("row");
                                grid.set_hexpand(true);
                                
                                // Create port and monitor info
                                let default_port = "Unknown Port".to_string();
                                let port_info = d.port_name.as_ref().unwrap_or(&default_port);
                                let monitor_name = match (&d.manufacturer, &d.model) {
                                    (Some(mfg), Some(model)) => {
                                        let expanded_mfg = match mfg.as_str() {
                                            "ACR" => "Acer",
                                            "GSM" => "LG", 
                                            "SAM" => "Samsung",
                                            "DEL" => "Dell",
                                            "AUS" => "ASUS",
                                            "BNQ" => "BenQ",
                                            "AOC" => "AOC",
                                            "HPN" => "HP",
                                            "LEN" => "Lenovo",
                                            "MSI" => "MSI",
                                            _ => mfg,
                                        };
                                        format!("{} {}", expanded_mfg, model)
                                    }
                                    _ => "Unknown Monitor".to_string(),
                                };
                                
                                let (control_method, badge_class, tooltip) = if d.supports_ddc {
                                    (
                                        "аппаратно",
                                        "badge-ddc",
                                        format!(
                                            "Управление: аппаратно (DDC/CI)\nШина: /dev/i2c-{}{}",
                                            d.i2c_bus,
                                            d.connector.as_ref().map(|c| format!("\nКоннектор: {}", c)).unwrap_or_default()
                                        ),
                                    )
                                } else if let Some(ref out) = d.xrandr_output {
                                    (
                                        "программно",
                                        "badge-xrandr",
                                        format!(
                                            "Управление: программно (xrandr)\nВывод: {}{}",
                                            out,
                                            d.connector.as_ref().map(|c| format!("\nКоннектор: {}", c)).unwrap_or_default()
                                        ),
                                    )
                                } else {
                                    (
                                        "нет управления",
                                        "badge-none",
                                        d.connector.as_ref().map(|c| format!("Коннектор: {}", c)).unwrap_or_else(|| "Нет доступного метода".to_string())
                                    )
                                };
                                
                                // Determine port type from connector info
                                let port_type = if let Some(ref connector) = d.connector {
                                    if connector.contains("HDMI") {
                                        "HDMI"
                                    } else if connector.contains("DP") {
                                        "DisplayPort"
                                    } else if connector.contains("DVI") {
                                        "DVI"
                                    } else if connector.contains("VGA") {
                                        "VGA"
                                    } else {
                                        "Unknown"
                                    }
                                } else {
                                    "Unknown"
                                };
                                
                                // Колонки: порт, тип, название, иконка, бейдж
                                let port_lbl = Label::new(Some(port_info));
                                port_lbl.add_css_class("port");
                                port_lbl.set_xalign(0.0);
                                port_lbl.set_width_chars(14);
                                let type_lbl = Label::new(Some(port_type));
                                type_lbl.add_css_class("type");
                                type_lbl.set_xalign(0.0);
                                type_lbl.set_width_chars(12);
                                let monitor_lbl = Label::new(Some(&monitor_name));
                                monitor_lbl.add_css_class("monitor");
                                monitor_lbl.set_xalign(0.0);
                                monitor_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
                                monitor_lbl.set_hexpand(true);
                                let badge_icon = match badge_class {
                                    "badge-none" => Image::from_icon_name("dialog-warning-symbolic"),
                                    _ => Image::from_icon_name("video-display-symbolic"),
                                };
                                let badge = Label::new(Some(control_method));
                                badge.add_css_class("badge");
                                badge.add_css_class(badge_class);
                                badge.set_tooltip_text(Some(&tooltip));

                                grid.attach(&port_lbl, 0, 0, 1, 1);
                                grid.attach(&type_lbl, 1, 0, 1, 1);
                                grid.attach(&monitor_lbl, 2, 0, 1, 1);
                                grid.attach(&badge_icon, 3, 0, 1, 1);
                                grid.attach(&badge, 4, 0, 1, 1);

                                // Правый слайдер и метка значения
                                let scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
                                scale.set_hexpand(false);
                                scale.set_width_request(320);
                                scale.set_halign(gtk::Align::End);
                                scale.set_draw_value(false);
                                let value_lbl = Label::new(Some("0%"));
                                value_lbl.set_width_chars(4); // фиксированная ширина 
                                value_lbl.set_xalign(1.0);
                                grid.attach(&scale, 5, 0, 1, 1);
                                grid.attach(&value_lbl, 6, 0, 1, 1);
                            
                            // Try to get brightness using any available method
                            if !d.supports_ddc && d.xrandr_output.is_none() {
                                scale.set_sensitive(false);
                                scale.set_value(0.0);
                            } else {
                                scale.set_sensitive(false);
                                
                                // Get current brightness using any available method
                                let (s_tx, s_rx) = async_channel::unbounded::<Result<u8, String>>();
                                let display_for_brightness = d.clone();
                                thread::spawn(move || {
                                    let res = get_brightness_any_method(&display_for_brightness);
                                    let _ = s_tx.send_blocking(res);
                                });
                                
                                let grid_for_tooltip = grid.clone();
                                glib::spawn_future_local(clone!(@strong scale, @strong grid_for_tooltip, @strong value_lbl => async move {
                                    if let Ok(res) = s_rx.recv().await {
                                        match res {
                                            Ok(v) => {
                                                scale.set_value(v as f64);
                                                scale.set_sensitive(true);
                                                value_lbl.set_text(&format!("{}%", v));
                                            }
                                            Err(e) => {
                                                scale.set_sensitive(false);
                                                // Показываем ошибку в подсказке ряда
                                                grid_for_tooltip.set_tooltip_text(Some(&format!("Ошибка чтения яркости: {}", e)));
                                            }
                                        }
                                    }
                                }));
                                
                                // Direct brightness control - immediate response
                                // Create a unique copy for this specific callback to avoid variable capture issues
                                // ВАЖНО: используем owned значение d, а не ссылку, чтобы избежать проблем с захватом
                                let display_info_for_callback = d.clone();
                                
                                println!("Creating callback for: {} (bus {}) [index: {}]", d.name, d.i2c_bus, display_index);
                                
                                let value_lbl_for_change = value_lbl.clone();
                                scale.connect_value_changed(move |s| {
                                    let val = s.value() as u8;
                                    value_lbl_for_change.set_text(&format!("{}%", val));
                                    let display_clone = display_info_for_callback.clone();
                                    let callback_id = format!("{}_{}_bus{}", display_clone.name, display_index, display_clone.i2c_bus);
                                    
                                    println!("CALLBACK {}: Setting brightness {} for display: {} (bus {})", 
                                        callback_id, val, display_clone.name, display_clone.i2c_bus);
                                    
                                    // Set brightness immediately in background thread
                                    thread::spawn(move || {
                                        if let Err(e) = set_brightness_any_method(&display_clone, val) {
                                            println!("Failed to set brightness for {}: {}", display_clone.name, e);
                                        } else {
                                            println!("Successfully set brightness {} for {}", val, display_clone.name);
                                        }
                                    });
                                });
                            }
                            
                                // Оборачиваем в "карточку"
                                let frame = gtk::Frame::new(None);
                                frame.add_css_class("card");
                                frame.set_hexpand(true);
                                frame.set_child(Some(&grid));
                                list_box_target.append(&frame);
                            }
                            
                            // Add separator after each card
                            let separator = Separator::new(Orientation::Horizontal);
                            separator.set_margin_top(8);
                            separator.set_margin_bottom(8);
                            list_box_target.append(&separator);
                        }

                        // После полного наполнения списка — один раз подстроим окно под контент
                        let win_weak_local = win_weak_for_measure_outer.clone();
                        let content_for_measure_local = content_for_measure_outer.clone();
                        glib::idle_add_local_once(move || {
                            if let Some(win) = win_weak_local.upgrade() {
                                let (_, nat_w, _, _) = content_for_measure_local.measure(Orientation::Horizontal, -1);
                                let (_, nat_h, _, _) = content_for_measure_local.measure(Orientation::Vertical, -1);
                                // небольшой запас под заголовок/паддинги
                                win.set_size_request(nat_w + 48, nat_h + 64);
                            }
                        });
                    }
                    Ok(_) => {
                        let lbl = Label::new(Some("(Видеокарт с мониторами не найдено)"));
                        lbl.set_xalign(0.0);
                        list_box_target.append(&lbl);
                    }
                    Err(err) => {
                        let lbl = Label::new(Some(&format!("Ошибка обнаружения: {}", err)));
                        lbl.set_xalign(0.0);
                        list_box_target.append(&lbl);
                    }
                }
            }
        });
    });

    // Initial population
    populate();

    // Refresh button
    let populate_btn = populate.clone();
    refresh_btn.connect_clicked(move |_| {
        populate_btn();
    });

    win.set_child(Some(&vbox));
    win.present();
}