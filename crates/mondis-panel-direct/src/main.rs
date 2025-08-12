use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale, Separator, Button};
use tracing_subscriber::EnvFilter;
use glib::clone;
use std::rc::Rc;
use std::thread;

use std::time::Duration;
use std::process::Command;
use i2cdev::linux::LinuxI2CDevice;
use i2cdev::core::I2CDevice;

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

fn get_gpu_name_from_card(card_num: u8) -> String {
    // Try to get GPU name from sysfs
    let device_path = format!("/sys/class/drm/card{}/device", card_num);
    
    // Try to read vendor and device IDs
    if let (Ok(vendor), Ok(device)) = (
        std::fs::read_to_string(format!("{}/vendor", device_path)),
        std::fs::read_to_string(format!("{}/device", device_path))
    ) {
        let vendor_id = vendor.trim();
        let device_id = device.trim();
        
        // Map common vendor IDs to names
        let vendor_name = match vendor_id {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            _ => "Unknown GPU",
        };
        
        let result = format!("{} Card {}", vendor_name, card_num);
        println!("GPU info for card{}: {} (vendor: {}, device: {})", card_num, result, vendor_id, device_id);
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
        
        let port_name = match port_type {
            "HDMI" => format!("HDMI Port {}", port_num),
            "DP" => format!("DisplayPort {}", port_num),
            "DVI" => format!("DVI Port {}", port_num),
            "VGA" => format!("VGA Port {}", port_num),
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
        .title("Mondis: Прямое I2C управление (v0.1)")
        .default_width(640)
        .default_height(360)
        .build();

    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some("Прямое I2C управление мониторами"));
    let refresh_btn = Button::with_label("Обновить");
    header.append(&title);
    header.append(&refresh_btn);
    vbox.append(&header);
    vbox.append(&Separator::new(Orientation::Horizontal));

    let list_box = GtkBox::new(Orientation::Vertical, 12);
    vbox.append(&list_box);

    let list_box_for_populate = list_box.clone();
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
                                let row = GtkBox::new(Orientation::Horizontal, 12);
                                row.set_margin_start(20); // Indent under card header
                                
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
                                
                                let control_method = if d.supports_ddc {
                                    "аппаратно"
                                } else if d.xrandr_output.is_some() {
                                    "программно"
                                } else {
                                    "нет управления"
                                };
                                
                                let display_text = format!("{}: {} ({})", port_info, monitor_name, control_method);
                                let label = Label::new(Some(&display_text));
                                label.set_width_chars(40);
                            let scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
                            scale.set_hexpand(true);
                            scale.set_draw_value(true);
                            scale.set_value_pos(gtk::PositionType::Right);
                            
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
                                
                                glib::spawn_future_local(clone!(@strong scale, @strong label => async move {
                                    if let Ok(res) = s_rx.recv().await {
                                        match res {
                                            Ok(v) => {
                                                scale.set_value(v as f64);
                                                scale.set_sensitive(true);
                                            }
                                            Err(e) => {
                                                scale.set_sensitive(false);
                                                label.set_text(&format!("{} (ошибка: {})", label.text(), e));
                                            }
                                        }
                                    }
                                }));
                                
                                // Direct brightness control - immediate response
                                // Create a unique copy for this specific callback to avoid variable capture issues
                                // ВАЖНО: используем owned значение d, а не ссылку, чтобы избежать проблем с захватом
                                let display_info_for_callback = d.clone();
                                
                                println!("Creating callback for: {} (bus {}) [index: {}]", d.name, d.i2c_bus, display_index);
                                
                                scale.connect_value_changed(move |s| {
                                    let val = s.value() as u8;
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
                            
                                row.append(&label);
                                row.append(&scale);
                                list_box_target.append(&row);
                            }
                            
                            // Add separator after each card
                            let separator = Separator::new(Orientation::Horizontal);
                            separator.set_margin_top(8);
                            separator.set_margin_bottom(8);
                            list_box_target.append(&separator);
                        }
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