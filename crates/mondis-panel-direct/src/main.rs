use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale, Separator, Button};
use tracing_subscriber::EnvFilter;
use glib::clone;
use std::rc::Rc;
use std::thread;

use std::time::Duration;
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
                    if supports_ddc {
                        format!("{} {}{}", expanded_mfg, mdl, connector_info)
                    } else {
                        format!("{} {}{} - не поддерживает DDC", expanded_mfg, mdl, connector_info)
                    }
                }
                _ => {
                    if supports_ddc {
                        format!("I2C Monitor (bus {})", bus)
                    } else {
                        format!("I2C Device (bus {}) - не поддерживает DDC", bus)
                    }
                }
            };
            
            // Only show devices that have EDID info (actual monitors)
            if manufacturer.is_some() || supports_ddc {
                displays.push(DisplayInfo {
                    i2c_bus: bus,
                    name,
                    manufacturer,
                    model,
                    serial,
                    connector,
                    supports_ddc,
                });
            }
        }
    }
    
    Ok(displays)
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

        let ddc_header = Label::new(Some("I2C устройства"));
        ddc_header.set_xalign(0.0);
        list_box_for_populate.append(&ddc_header);
        
        let (tx, rx) = async_channel::unbounded::<Result<Vec<DisplayInfo>, String>>();
        thread::spawn(move || {
            let res = detect_i2c_displays();
            let _ = tx.send_blocking(res);
        });
        
        let list_box_target = list_box_for_populate.clone();
        glib::spawn_future_local(async move {
            if let Ok(msg) = rx.recv().await {
                match msg {
                    Ok(displays) if !displays.is_empty() => {
                        for d in displays {
                            let row = GtkBox::new(Orientation::Horizontal, 12);
                            let label = Label::new(Some(&d.name));
                            label.set_width_chars(28);
                            let scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
                            scale.set_hexpand(true);
                            scale.set_draw_value(true);
                            scale.set_value_pos(gtk::PositionType::Right);
                            
                            if !d.supports_ddc {
                                scale.set_sensitive(false);
                                scale.set_value(0.0);
                            } else {
                                scale.set_sensitive(false);
                                
                                // Get current brightness
                                let (s_tx, s_rx) = async_channel::unbounded::<Result<u8, String>>();
                                let bus = d.i2c_bus;
                                thread::spawn(move || {
                                    let res = ddc_get_brightness(bus);
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
                                scale.connect_value_changed(move |s| {
                                    let val = s.value() as u8;
                                    let bus = d.i2c_bus;
                                    
                                    // Set brightness immediately in background thread
                                    thread::spawn(move || {
                                        let _ = ddc_set_brightness(bus, val);
                                    });
                                });
                            }
                            
                            row.append(&label);
                            row.append(&scale);
                            list_box_target.append(&row);
                        }
                    }
                    Ok(_) => {
                        let lbl = Label::new(Some("(I2C устройств не найдено)"));
                        lbl.set_xalign(0.0);
                        list_box_target.append(&lbl);
                    }
                    Err(err) => {
                        let lbl = Label::new(Some(&format!("Ошибка I2C: {}", err)));
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