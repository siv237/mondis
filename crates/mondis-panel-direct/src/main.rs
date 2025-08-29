#![allow(dead_code)]
#![allow(unused_imports, unused_variables, unused_mut)]

use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale, Separator, Button, HeaderBar, CssProvider, Image, Window, Notebook, ScrolledWindow, TextView, TextBuffer, GestureClick, WrapMode, PolicyType};
use gtk::gdk::Display as GdkDisplay;
use tracing_subscriber::EnvFilter;
use glib::clone;
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::thread;
use std::collections::HashMap;

use std::time::Duration;
use std::process::Command;
use i2cdev::linux::LinuxI2CDevice;
use i2cdev::core::I2CDevice;
use std::fs;
use std::path::PathBuf;

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
    .badge-button { cursor: pointer; transition: background-color 120ms ease, box-shadow 120ms ease; }
    .badge-button:hover { box-shadow: inset 0 0 0 1px alpha(@theme_selected_bg_color, 0.25); }
    .port { opacity: 0.8; }
    .type { opacity: 0.8; }
    .monitor { font-weight: 600; }
    .confirm-bar { 
        padding: 12px; 
        border-radius: 8px; 
        background: alpha(@warning_color, 0.1);
        border: 1px solid alpha(@warning_color, 0.3);
        margin-bottom: 12px;
    }
    .confirm-button { 
        background: @success_color; 
        color: white; 
        border-radius: 6px; 
        padding: 8px 16px;
        font-weight: 600;
        border: none;
    }
    .cancel-button { 
        background: @error_color; 
        color: white; 
        border-radius: 6px; 
        padding: 8px 16px;
        font-weight: 600;
        margin-left: 8px;
        border: none;
    }
    .timer-label { 
        font-weight: 700; 
        color: @warning_color;
        margin-right: 12px;
        font-size: 14px;
    }
    
    /* Адаптивные цвета для светлой темы */
    @define-color warning_color_light #047857; /* темно-зеленый для светлой темы */
    @define-color success_color_light #059669;
    @define-color error_color_light #dc2626;
    
    /* Адаптивные цвета для темной темы */
    @define-color warning_color_dark #34d399; /* светло-зеленый для темной темы */
    @define-color success_color_dark #10b981;
    @define-color error_color_dark #ef4444;
    
    /* Автоматический выбор цветов в зависимости от темы */
    .timer-label {
        color: mix(@warning_color_light, @warning_color_dark, 0.5);
    }
    
    /* Для светлой темы */
    window:not(.dark) .timer-label {
        color: @warning_color_light;
    }
    window:not(.dark) .confirm-button {
        background: @success_color_light;
    }
    window:not(.dark) .cancel-button {
        background: @error_color_light;
    }
    window:not(.dark) .confirm-bar {
        background: alpha(@warning_color_light, 0.1);
        border-color: alpha(@warning_color_light, 0.3);
    }
    
    /* Для темной темы */
    window.dark .timer-label {
        color: @warning_color_dark;
    }
    window.dark .confirm-button {
        background: @success_color_dark;
    }
    window.dark .cancel-button {
        background: @error_color_dark;
    }
    window.dark .confirm-bar {
        background: alpha(@warning_color_dark, 0.1);
        border-color: alpha(@warning_color_dark, 0.3);
    }
    
    /* Стили для кликабельных карточек */
    .clickable-card {
        cursor: pointer;
        transition: all 200ms ease;
    }
    .clickable-card:hover {
        background-color: alpha(@theme_selected_bg_color, 0.1);
        border-color: alpha(@theme_selected_bg_color, 0.3);
        transform: translateY(-1px);
        box-shadow: 0 4px 8px alpha(@theme_fg_color, 0.1);
    }
    .clickable-card:active {
        transform: translateY(0px);
        box-shadow: 0 2px 4px alpha(@theme_fg_color, 0.1);
    }

    /* Кнопка-иконка монитора: подсветка при наведении */
    .icon-button {
        border-radius: 6px;
        padding: 4px;
        transition: background-color 150ms ease, box-shadow 150ms ease, transform 150ms ease;
    }
    .icon-button:hover {
        background-color: alpha(@theme_selected_bg_color, 0.12);
        box-shadow: inset 0 0 0 1px alpha(@theme_selected_bg_color, 0.25);
    }
    .icon-button:active {
        background-color: alpha(@theme_selected_bg_color, 0.18);
        transform: scale(0.98);
    }
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

#[derive(Clone, Debug)]
struct MonitorDetails {
    // EDID информация
    manufacturer: String,
    model: String,
    serial_number: Option<String>,
    manufacture_year: Option<u16>,
    manufacture_week: Option<u8>,
    edid_version: Option<String>,
    video_input: Option<String>,
    color_space: Option<String>,
    resolution: Option<String>,
    physical_size: Option<String>, // размер в мм
    aspect_ratio: Option<String>,
    
    // DDC/CI возможности
    mccs_version: Option<String>,
    controller_mfg: Option<String>,
    firmware_version: Option<String>,
    supported_vcp_codes: Vec<u8>,
    capabilities_string: Option<String>, // полная строка capabilities
    
    // Текущие настройки
    current_brightness: Option<u8>,
    current_contrast: Option<u8>,
    current_color_temp: Option<String>,
    current_input_source: Option<String>,
    current_volume: Option<u8>,
    current_power_state: Option<String>,
    
    // Дополнительные VCP значения
    red_gain: Option<u8>,
    green_gain: Option<u8>,
    blue_gain: Option<u8>,
    backlight_control: Option<u8>,
    osd_language: Option<String>,
    
    // Технические детали
    i2c_bus: String,
    drm_connector: Option<String>,
    driver: Option<String>,
    pci_path: Option<String>,
    device_path: Option<String>,
    
    // EDID hex dump
    edid_hex: Option<String>,
    
    // Статистика
    read_errors: u32,
    last_update: Option<String>,
}

#[derive(Clone, Debug)]
struct BrightnessState {
    original_values: HashMap<u8, u8>, // i2c_bus -> brightness value
    current_values: HashMap<u8, u8>,
    has_changes: bool,
    timer_active: bool,
}

// Структура для хранения ссылок на UI элементы
struct SliderRefs {
    sliders: HashMap<u8, (Scale, Label)>, // i2c_bus -> (slider, value_label)
    programmatic_update: HashMap<u8, bool>, // флаг программного обновления
    // Последние значения ползунка для каждого метода отдельно
    last_values_ddc: HashMap<u8, u8>,
    last_values_xrandr: HashMap<u8, u8>,
}

impl BrightnessState {
    fn new() -> Self {
        Self {
            original_values: HashMap::new(),
            current_values: HashMap::new(),
            has_changes: false,
            timer_active: false,
        }
    }
    
    fn save_original(&mut self, bus: u8, value: u8) {
        if !self.original_values.contains_key(&bus) {
            self.original_values.insert(bus, value);
        }
        self.current_values.insert(bus, value);
    }
    
    fn update_current(&mut self, bus: u8, value: u8) {
        self.current_values.insert(bus, value);
        if let Some(&original) = self.original_values.get(&bus) {
            self.has_changes = original != value || self.has_changes_for_other_buses(bus);
        }
    }
    
    fn has_changes_for_other_buses(&self, exclude_bus: u8) -> bool {
        for (&bus, &current) in &self.current_values {
            if bus != exclude_bus {
                if let Some(&original) = self.original_values.get(&bus) {
                    if original != current {
                        return true;
                    }
                }
            }
        }
        false
    }
    
    fn reset_to_original(&mut self) {
        self.current_values = self.original_values.clone();
        self.has_changes = false;
        self.timer_active = false;
    }
    
    fn confirm_changes(&mut self) {
        self.original_values = self.current_values.clone();
        self.has_changes = false;
        self.timer_active = false;
    }
}

impl SliderRefs {
    fn new() -> Self {
        Self {
            sliders: HashMap::new(),
            programmatic_update: HashMap::new(),
            last_values_ddc: HashMap::new(),
            last_values_xrandr: HashMap::new(),
        }
    }
    
    fn add_slider(&mut self, bus: u8, slider: Scale, label: Label) {
        self.sliders.insert(bus, (slider, label));
        self.programmatic_update.insert(bus, false);
    }
    
    fn update_slider_value(&mut self, bus: u8, value: u8) {
        if let Some((slider, label)) = self.sliders.get(&bus) {
            // Устанавливаем флаг программного обновления
            self.programmatic_update.insert(bus, true);
            slider.set_value(value as f64);
            label.set_text(&format!("{}%", value));
        }
    }
    
    fn restore_all_sliders(&mut self, original_values: &HashMap<u8, u8>) {
        for (&bus, &value) in original_values {
            self.update_slider_value(bus, value);
        }
    }
    
    fn is_programmatic_update(&mut self, bus: u8) -> bool {
        if let Some(&is_programmatic) = self.programmatic_update.get(&bus) {
            if is_programmatic {
                self.programmatic_update.insert(bus, false); // Сбрасываем флаг
                return true;
            }
        }
        false
    }

    fn remember_value(&mut self, bus: u8, method: ControlMethodPref, value: u8) {
        match method {
            ControlMethodPref::Ddc => { self.last_values_ddc.insert(bus, value); }
            ControlMethodPref::Xrandr => { self.last_values_xrandr.insert(bus, value); }
        }
    }

    fn get_last_value(&self, bus: u8, method: ControlMethodPref) -> Option<u8> {
        match method {
            ControlMethodPref::Ddc => self.last_values_ddc.get(&bus).copied(),
            ControlMethodPref::Xrandr => self.last_values_xrandr.get(&bus).copied(),
        }
    }
}

// DDC/CI constants
const DDC_ADDR: u16 = 0x37;
const EDID_ADDR: u16 = 0x50;
const VCP_BRIGHTNESS: u8 = 0x10;

fn get_profile_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set")?;
    let config_dir = PathBuf::from(home).join(".config").join("mondis");
    
    // Создаем директорию если не существует
    if let Err(e) = fs::create_dir_all(&config_dir) {
        return Err(format!("Failed to create config directory: {}", e));
    }
    
    Ok(config_dir.join("brightness_profile.xml"))
}

fn save_brightness_profile(brightness_values: &HashMap<u8, u8>, displays: &[DisplayInfo]) -> Result<(), String> {
    let profile_path = get_profile_path()?;
    
    let mut xml_content = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mondis_profile>\n");
    xml_content.push_str("  <brightness_settings>\n");
    
    for (&bus, &brightness) in brightness_values {
        // Найдем информацию о дисплее для этого bus
        if let Some(display) = displays.iter().find(|d| d.i2c_bus == bus) {
            xml_content.push_str(&format!(
                "    <display bus=\"{}\" brightness=\"{}\" name=\"{}\" />\n",
                bus, brightness, 
                display.name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            ));
        } else {
            xml_content.push_str(&format!(
                "    <display bus=\"{}\" brightness=\"{}\" name=\"Unknown\" />\n",
                bus, brightness
            ));
        }
    }
    
    xml_content.push_str("  </brightness_settings>\n");
    xml_content.push_str(&format!("  <timestamp>{}</timestamp>\n", 
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    xml_content.push_str("</mondis_profile>\n");
    
    fs::write(&profile_path, xml_content)
        .map_err(|e| format!("Failed to write profile to {:?}: {}", profile_path, e))?;
    
    println!("Brightness profile saved to: {:?}", profile_path);
    Ok(())
}

fn read_edid_directly(i2c_bus: u8) -> Result<Vec<u8>, String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    println!("    Opening I2C device: {}", device_path);
    
    let mut dev = LinuxI2CDevice::new(&device_path, 0x50)
        .map_err(|e| format!("Failed to open {} for EDID: {}", device_path, e))?;
    
    println!("    Writing EDID offset 0x00...");
    // Записываем offset 0x00
    dev.write(&[0x00])
        .map_err(|e| format!("Failed to write EDID offset: {}", e))?;
    
    thread::sleep(Duration::from_millis(10));
    
    println!("    Reading 128 bytes of EDID data...");
    // Читаем EDID (128 байт базовый блок)
    let mut edid_data = vec![0u8; 128];
    dev.read(&mut edid_data)
        .map_err(|e| format!("Failed to read EDID: {}", e))?;
    
    println!("    Validating EDID header...");
    // Проверяем заголовок EDID
    if edid_data.len() >= 8 && edid_data[0] == 0x00 && edid_data[1] == 0xFF && edid_data[7] == 0x00 {
        println!("    EDID header is valid");
        Ok(edid_data)
    } else {
        Err(format!("Invalid EDID header: {:02X} {:02X} ... {:02X}", 
            edid_data[0], edid_data[1], edid_data[7]))
    }
}

fn parse_edid_detailed(edid_data: &[u8]) -> (Option<String>, Option<String>, Option<u16>, Option<u8>, Option<String>, Option<String>, Option<String>, Option<String>) {
    if edid_data.len() < 128 {
        return (None, None, None, None, None, None, None, None);
    }
    
    // Manufacturer ID (bytes 8-9)
    let mfg_id = u16::from_be_bytes([edid_data[8], edid_data[9]]);
    let c1 = (((mfg_id >> 10) & 0x1F) as u8 + 0x40) as char;
    let c2 = (((mfg_id >> 5) & 0x1F) as u8 + 0x40) as char;
    let c3 = ((mfg_id & 0x1F) as u8 + 0x40) as char;
    let manufacturer = if c1.is_ascii_uppercase() && c2.is_ascii_uppercase() && c3.is_ascii_uppercase() {
        Some(format!("{}{}{}", c1, c2, c3))
    } else { 
        None 
    };
    
    // Год производства (byte 17) + 1990
    let manufacture_year = if edid_data[17] > 0 {
        Some(edid_data[17] as u16 + 1990)
    } else {
        None
    };
    
    // Неделя производства (byte 16)
    let manufacture_week = if edid_data[16] > 0 && edid_data[16] <= 54 {
        Some(edid_data[16])
    } else {
        None
    };
    
    // EDID версия (bytes 18-19)
    let edid_version = Some(format!("{}.{}", edid_data[18], edid_data[19]));
    
    // Физический размер (bytes 21-22) в см
    let physical_size = if edid_data[21] > 0 && edid_data[22] > 0 {
        let width_cm = edid_data[21];
        let height_cm = edid_data[22];
        Some(format!("{} x {} см", width_cm, height_cm))
    } else {
        None
    };
    
    // Разрешение из первого detailed timing descriptor (bytes 54-71)
    let resolution = if edid_data.len() >= 72 {
        let h_active = ((edid_data[58] as u16) << 4) | ((edid_data[62] as u16 >> 4) & 0x0F);
        let v_active = ((edid_data[61] as u16) << 4) | (edid_data[62] as u16 & 0x0F);
        if h_active > 0 && v_active > 0 {
            Some(format!("{} x {}", h_active, v_active))
        } else {
            None
        }
    } else {
        None
    };
    
    // Соотношение сторон
    let aspect_ratio = if let Some(ref _size) = physical_size {
        if edid_data[21] > 0 && edid_data[22] > 0 {
            let ratio = edid_data[21] as f32 / edid_data[22] as f32;
            if (ratio - 16.0/9.0).abs() < 0.1 {
                Some("16:9".to_string())
            } else if (ratio - 16.0/10.0).abs() < 0.1 {
                Some("16:10".to_string())
            } else if (ratio - 4.0/3.0).abs() < 0.1 {
                Some("4:3".to_string())
            } else if (ratio - 21.0/9.0).abs() < 0.1 {
                Some("21:9".to_string())
            } else {
                Some(format!("{:.2}:1", ratio))
            }
        } else {
            None
        }
    } else {
        None
    };
    
    // Ищем название модели в дескрипторах (bytes 54-125)
    let mut model_name = None;
    for i in (54..126).step_by(18) {
        if edid_data[i] == 0x00 && edid_data[i+1] == 0x00 && edid_data[i+2] == 0x00 && edid_data[i+3] == 0xFC {
            // Monitor name descriptor
            let name_bytes = &edid_data[i+5..i+18];
            let name = String::from_utf8_lossy(name_bytes)
                .trim_end_matches('\0')
                .trim_end_matches('\n')
                .trim()
                .to_string();
            if !name.is_empty() {
                model_name = Some(name);
                break;
            }
        }
    }
    
    (manufacturer, model_name, manufacture_year, manufacture_week, edid_version, physical_size, resolution, aspect_ratio)
}

fn read_ddc_capabilities(i2c_bus: u8) -> Result<String, String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    println!("    Opening DDC device: {}", device_path);
    
    let mut dev = LinuxI2CDevice::new(&device_path, 0x37)
        .map_err(|e| format!("Failed to open {} for DDC: {}", device_path, e))?;
    
    // DDC Get Capabilities request: [source_addr, length, type]
    let request = [0x51, 0x01, 0xF3]; // F3 = Capabilities Request
    let checksum = 0x6E ^ request.iter().fold(0u8, |acc, &x| acc ^ x);
    let full_request = [request[0], request[1], request[2], checksum];
    
    println!("    Sending capabilities request: {:02X?}", full_request);
    dev.write(&full_request)
        .map_err(|e| format!("Failed to write DDC capabilities request: {}", e))?;
    
    println!("    Waiting 200ms for response...");
    thread::sleep(Duration::from_millis(200)); // Больше времени для capabilities
    
    // Читаем ответ (простое чтение, не multi-part)
    let mut response = vec![0u8; 256];
    dev.read(&mut response)
        .map_err(|e| format!("Failed to read DDC capabilities response: {}", e))?;
    
    println!("    Received {} bytes: {:02X?}", response.len(), &response[..std::cmp::min(16, response.len())]);
    
    // Проверяем заголовок ответа
    if response.len() < 4 || response[0] != 0x6E {
        return Err(format!("Invalid DDC response header: 0x{:02X}", response.get(0).unwrap_or(&0)));
    }
    
    let data_len = response[1] as usize;
    if data_len == 0 || data_len + 3 > response.len() {
        return Err(format!("Invalid capabilities response length: {} (buffer size: {})", data_len, response.len()));
    }
    
    // Извлекаем данные (пропускаем заголовок и checksum)
    let data_start = 3;
    let data_end = std::cmp::min(data_start + data_len - 1, response.len() - 1); // -1 для checksum
    
    let mut all_data = Vec::new();
    if data_end > data_start {
        all_data.extend_from_slice(&response[data_start..data_end]);
    }
    
    if all_data.is_empty() {
        return Err("No capabilities data received".to_string());
    }
    
    // Очищаем данные от мусора
    let mut clean_data = Vec::new();
    for &byte in &all_data {
        if byte >= 0x20 && byte <= 0x7E { // Печатные ASCII символы
            clean_data.push(byte);
        } else if byte == 0x00 {
            break; // Конец строки
        }
    }
    
    let caps_string = String::from_utf8_lossy(&clean_data).trim().to_string();
    println!("    Parsed capabilities string ({} chars): {}", caps_string.len(), caps_string);
    
    if caps_string.is_empty() {
        Err("Empty capabilities string".to_string())
    } else {
        Ok(caps_string)
    }
}

fn read_vcp_value(i2c_bus: u8, vcp_code: u8) -> Result<(u8, u8), String> {
    let device_path = format!("/dev/i2c-{}", i2c_bus);
    let mut dev = LinuxI2CDevice::new(&device_path, 0x37)
        .map_err(|e| format!("Failed to open {} for DDC: {}", device_path, e))?;
    
    // DDC Get VCP request: [source_addr, length, type, vcp_code]
    let request = [0x51, 0x02, 0x01, vcp_code];
    let checksum = 0x6E ^ request.iter().fold(0u8, |acc, &x| acc ^ x);
    let full_request = [request[0], request[1], request[2], request[3], checksum];
    
    dev.write(&full_request)
        .map_err(|e| format!("Failed to write DDC VCP request: {}", e))?;
    
    thread::sleep(Duration::from_millis(50));
    
    // Читаем ответ: [dest, length, type, result, vcp_code, type_flag, max_hi, max_lo, cur_hi, cur_lo, checksum]
    let mut response = [0u8; 12];
    dev.read(&mut response)
        .map_err(|e| format!("Failed to read DDC VCP response: {}", e))?;
    
    if response.len() >= 10 && response[4] == vcp_code {
        let current_value = ((response[8] as u16) << 8) | (response[9] as u16);
        let max_value = ((response[6] as u16) << 8) | (response[7] as u16);
        Ok((current_value as u8, max_value as u8))
    } else {
        Err(format!("Invalid VCP response: expected code 0x{:02X}, got 0x{:02X} (response: {:02X?})", 
            vcp_code, response[4], &response[..std::cmp::min(12, response.len())]))
    }
}

fn get_monitor_details(display: &DisplayInfo) -> Result<MonitorDetails, String> {
    let mut details = MonitorDetails {
        manufacturer: display.manufacturer.clone().unwrap_or_default(),
        model: display.model.clone().unwrap_or_default(),
        serial_number: display.serial.clone(),
        manufacture_year: None,
        manufacture_week: None,
        edid_version: None,
        video_input: None,
        color_space: None,
        resolution: None,
        physical_size: None,
        aspect_ratio: None,
        mccs_version: None,
        controller_mfg: None,
        firmware_version: None,
        supported_vcp_codes: Vec::new(),
        capabilities_string: None,
        current_brightness: None,
        current_contrast: None,
        current_color_temp: None,
        current_input_source: None,
        current_volume: None,
        current_power_state: None,
        red_gain: None,
        green_gain: None,
        blue_gain: None,
        backlight_control: None,
        osd_language: None,
        i2c_bus: format!("/dev/i2c-{}", display.i2c_bus),
        drm_connector: display.connector.clone(),
        driver: None,
        pci_path: None,
        device_path: None,
        edid_hex: None,
        read_errors: 0,
        last_update: Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()),
    };
    
    println!("=== Getting detailed info for bus {} using direct I2C access ===", display.i2c_bus);
    
    // Читаем EDID напрямую
    println!("Step 1: Reading EDID from /dev/i2c-{} at address 0x50...", display.i2c_bus);
    match read_edid_directly(display.i2c_bus) {
        Ok(edid_data) => {
            println!("✅ Successfully read EDID ({} bytes)", edid_data.len());
            println!("EDID header: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}", 
                edid_data[0], edid_data[1], edid_data[2], edid_data[3], 
                edid_data[4], edid_data[5], edid_data[6], edid_data[7]);
        
            // Парсим детальную информацию из EDID
            println!("Step 2: Parsing EDID data...");
            let (mfg, model, year, week, version, physical_size, resolution, aspect_ratio) = parse_edid_detailed(&edid_data);
            
            println!("Parsed EDID info:");
            if let Some(ref mfg) = mfg {
                println!("  - Manufacturer: {}", mfg);
                details.manufacturer = mfg.clone();
            } else {
                println!("  - Manufacturer: Not found");
            }
            
            if let Some(ref model) = model {
                println!("  - Model: {}", model);
                details.model = model.clone();
            } else {
                println!("  - Model: Not found");
            }
            
            if let Some(year) = year {
                println!("  - Year: {}", year);
                details.manufacture_year = Some(year);
            } else {
                println!("  - Year: Not found");
            }
            
            if let Some(week) = week {
                println!("  - Week: {}", week);
                details.manufacture_week = Some(week);
            } else {
                println!("  - Week: Not found");
            }
            
            if let Some(ref version) = version {
                println!("  - EDID Version: {}", version);
                details.edid_version = Some(version.clone());
            } else {
                println!("  - EDID Version: Not found");
                details.edid_version = version;
            }
            
            if let Some(ref size) = physical_size {
                println!("  - Physical Size: {}", size);
                details.physical_size = Some(size.clone());
            }
            
            if let Some(ref res) = resolution {
                println!("  - Resolution: {}", res);
                details.resolution = Some(res.clone());
            }
            
            if let Some(ref ratio) = aspect_ratio {
                println!("  - Aspect Ratio: {}", ratio);
                details.aspect_ratio = Some(ratio.clone());
            }
        
            // Определяем тип входа из EDID
            if edid_data.len() > 20 {
                let video_input = edid_data[20];
                let input_type = if video_input & 0x80 != 0 {
                    format!("Digital Input (0x{:02X})", video_input)
                } else {
                    format!("Analog Input (0x{:02X})", video_input)
                };
                println!("  - Video Input: {}", input_type);
                details.video_input = Some(input_type);
            }
        
        // Создаем hex dump
        let hex_dump = edid_data.chunks(16)
            .enumerate()
            .map(|(i, chunk)| {
                let hex_part = chunk.iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii_part = chunk.iter()
                    .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
                    .collect::<String>();
                format!("  +{:04x}   {:<47} {}", i * 16, hex_part, ascii_part)
            })
            .collect::<Vec<_>>()
            .join("\n");
        
            details.edid_hex = Some(format!("          +0          +4          +8          +c            0   4   8   c\n{}", hex_dump));
            println!("  - EDID Hex dump: {} lines generated", hex_dump.lines().count());
        }
        Err(e) => {
            println!("❌ Failed to read EDID directly: {}", e);
        }
    }
    
    // Получаем DDC/CI информацию если поддерживается
    if display.supports_ddc {
        println!("\nStep 3: Getting DDC/CI capabilities for bus {}...", display.i2c_bus);
        
        // Читаем capabilities напрямую
        match read_ddc_capabilities(display.i2c_bus) {
            Ok(caps_string) => {
                println!("✅ Successfully read capabilities ({} chars): {}", caps_string.len(), caps_string);
                details.capabilities_string = Some(caps_string.clone());
            
                // Парсим capabilities string для извлечения информации
                println!("Parsing capabilities string...");
                
                // MCCS версия
                if caps_string.contains("mccs_ver(") {
                    if let Some(start) = caps_string.find("mccs_ver(") {
                        if let Some(end) = caps_string[start..].find(')') {
                            let version = &caps_string[start+9..start+end];
                            println!("  - MCCS Version: {}", version);
                            details.mccs_version = Some(version.to_string());
                        }
                    }
                } else {
                    println!("  - MCCS Version: Not found in capabilities");
                }
                
                // Модель из capabilities
                if caps_string.contains("model(") {
                    if let Some(start) = caps_string.find("model(") {
                        if let Some(end) = caps_string[start..].find(')') {
                            let model = &caps_string[start+6..start+end];
                            println!("  - Model from capabilities: {}", model);
                            if details.model.is_empty() {
                                details.model = model.to_string();
                            }
                        }
                    }
                }
                
                // Тип монитора
                if caps_string.contains("type(") {
                    if let Some(start) = caps_string.find("type(") {
                        if let Some(end) = caps_string[start..].find(')') {
                            let monitor_type = &caps_string[start+5..start+end];
                            println!("  - Monitor Type: {}", monitor_type);
                            details.color_space = Some(monitor_type.to_string());
                        }
                    }
                }
                
                // Извлекаем поддерживаемые VCP коды из vcp() секции
                let mut vcp_codes = Vec::new();
                if let Some(vcp_start) = caps_string.find("vcp(") {
                    if let Some(vcp_end) = caps_string[vcp_start..].find(')') {
                        let vcp_section = &caps_string[vcp_start+4..vcp_start+vcp_end];
                        
                        // Парсим VCP коды (могут быть в формате "10 12 14(05 06 08)")
                        for part in vcp_section.split_whitespace() {
                            let code_part = part.split('(').next().unwrap_or(part);
                            if code_part.len() == 2 {
                                if let Ok(code) = u8::from_str_radix(code_part, 16) {
                                    vcp_codes.push(code);
                                }
                            }
                        }
                    }
                }
                
                println!("  - Found {} VCP codes: {:02X?}", vcp_codes.len(), vcp_codes);
                details.supported_vcp_codes = vcp_codes;
            }
            Err(e) => {
                println!("❌ Failed to read DDC capabilities: {}", e);
                details.read_errors += 1;
            }
        }
        
        // Читаем текущие значения VCP кодов
        println!("\nStep 4: Reading current VCP values...");
        
        // Яркость (0x10)
        print!("  - Reading brightness (VCP 0x10)... ");
        match read_vcp_value(display.i2c_bus, 0x10) {
            Ok((current, max_val)) => {
                println!("✅ {}/{}", current, max_val);
                details.current_brightness = Some(current);
            }
            Err(e) => println!("❌ {}", e),
        }
        
        // Контраст (0x12)
        print!("  - Reading contrast (VCP 0x12)... ");
        match read_vcp_value(display.i2c_bus, 0x12) {
            Ok((current, max_val)) => {
                println!("✅ {}/{}", current, max_val);
                details.current_contrast = Some(current);
            }
            Err(e) => println!("❌ {}", e),
        }
        
        // Источник входа (0x60)
        print!("  - Reading input source (VCP 0x60)... ");
        match read_vcp_value(display.i2c_bus, 0x60) {
            Ok((current, _max)) => {
                let input_name = match current {
                    0x0F => "DisplayPort-1",
                    0x11 => "HDMI-1",
                    0x12 => "HDMI-2",
                    _ => "Unknown",
                };
                println!("✅ {} (0x{:02X})", input_name, current);
                details.current_input_source = Some(format!("{} (0x{:02X})", input_name, current));
            }
            Err(e) => println!("❌ {}", e),
        }
        
        // Громкость (0x62)
        print!("  - Reading volume (VCP 0x62)... ");
        match read_vcp_value(display.i2c_bus, 0x62) {
            Ok((current, max_val)) => {
                println!("✅ {}/{}", current, max_val);
                details.current_volume = Some(current);
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
        
        // Дополнительные VCP коды
        
        // Красный канал (0x16)
        print!("  - Reading red gain (VCP 0x16)... ");
        match read_vcp_value(display.i2c_bus, 0x16) {
            Ok((current, _max)) => {
                println!("✅ {}", current);
                details.red_gain = Some(current);
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
        
        // Зеленый канал (0x18)
        print!("  - Reading green gain (VCP 0x18)... ");
        match read_vcp_value(display.i2c_bus, 0x18) {
            Ok((current, _max)) => {
                println!("✅ {}", current);
                details.green_gain = Some(current);
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
        
        // Синий канал (0x1A)
        print!("  - Reading blue gain (VCP 0x1A)... ");
        match read_vcp_value(display.i2c_bus, 0x1A) {
            Ok((current, _max)) => {
                println!("✅ {}", current);
                details.blue_gain = Some(current);
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
        
        // Управление подсветкой (0x13)
        print!("  - Reading backlight control (VCP 0x13)... ");
        match read_vcp_value(display.i2c_bus, 0x13) {
            Ok((current, _max)) => {
                println!("✅ {}", current);
                details.backlight_control = Some(current);
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
        
        // Состояние питания (0xD6)
        print!("  - Reading power state (VCP 0xD6)... ");
        match read_vcp_value(display.i2c_bus, 0xD6) {
            Ok((current, _max)) => {
                let power_state = match current {
                    0x01 => "On",
                    0x02 => "Standby",
                    0x03 => "Suspend",
                    0x04 => "Off (Soft)",
                    0x05 => "Off (Hard)",
                    _ => "Unknown",
                };
                println!("✅ {} (0x{:02X})", power_state, current);
                details.current_power_state = Some(format!("{} (0x{:02X})", power_state, current));
            }
            Err(e) => {
                println!("❌ {}", e);
                details.read_errors += 1;
            }
        }
    } else {
        println!("\nStep 3: DDC not supported for bus {}, skipping DDC/CI info", display.i2c_bus);
    }
    
    // Получаем системную информацию
    println!("\nStep 5: Getting system information...");
    
    // Сохраняем путь к устройству I2C
    details.device_path = Some(format!("/dev/i2c-{}", display.i2c_bus));
    
    if let Some(ref connector) = display.connector {
        println!("  - Connector: {}", connector);
        details.drm_connector = Some(connector.clone());
        
        // Извлекаем информацию о драйвере из sysfs
        let card_num = if connector.starts_with("card") {
            connector.chars().nth(4).and_then(|c| c.to_digit(10)).unwrap_or(0) as u8
        } else {
            0
        };
        
        let device_path = format!("/sys/class/drm/card{}/device", card_num);
        println!("  - Device path: {}", device_path);
        
        // Читаем драйвер
        match std::fs::read_to_string(format!("{}/driver/module/name", device_path)) {
            Ok(driver_name) => {
                let driver = driver_name.trim().to_string();
                println!("  - Driver: {}", driver);
                details.driver = Some(driver);
            }
            Err(e) => {
                println!("  - Driver: Failed to read ({})", e);
                details.read_errors += 1;
            }
        }
        
        // Читаем PCI путь
        match std::fs::read_link(&device_path) {
            Ok(pci_path) => {
                let pci_path_str = format!("/sys/devices{}", pci_path.to_string_lossy());
                println!("  - PCI path: {}", pci_path_str);
                details.pci_path = Some(pci_path_str);
            }
            Err(e) => {
                println!("  - PCI path: Failed to read ({})", e);
                details.read_errors += 1;
            }
        }
        
        // Читаем vendor и device ID
        if let Ok(vendor_id) = std::fs::read_to_string(format!("{}/vendor", device_path)) {
            if let Ok(device_id) = std::fs::read_to_string(format!("{}/device", device_path)) {
                let vendor = vendor_id.trim();
                let device = device_id.trim();
                println!("  - PCI ID: {}:{}", vendor, device);
                
                // Добавляем к информации о контроллере
                if details.controller_mfg.is_none() {
                    details.controller_mfg = Some(format!("PCI {}:{}", vendor, device));
                }
            }
        }
        
        // Читаем состояние подключения
        if let Ok(status) = std::fs::read_to_string(format!("/sys/class/drm/{}/status", connector)) {
            let status = status.trim();
            println!("  - Connection Status: {}", status);
        }
        
        // Читаем DPMS состояние
        if let Ok(dpms) = std::fs::read_to_string(format!("/sys/class/drm/{}/dpms", connector)) {
            let dpms = dpms.trim();
            println!("  - DPMS State: {}", dpms);
            if details.current_power_state.is_none() {
                details.current_power_state = Some(format!("DPMS: {}", dpms));
            }
        }
        
    } else {
        println!("  - No connector information available");
        details.read_errors += 1;
    }
    
    println!("\n=== Monitor details collection completed ===");
    
    Ok(details)
}

fn parse_ddcutil_detect_output(output: &str, details: &mut MonitorDetails) {
    for line in output.lines() {
        let line = line.trim();
        
        if line.starts_with("Driver:") {
            details.driver = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("PCI device path:") {
            details.pci_path = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Manufacture year:") {
            if let Some(year_part) = line.split(':').nth(1) {
                if let Some(year_str) = year_part.split(',').next() {
                    details.manufacture_year = year_str.trim().parse().ok();
                }
                if let Some(week_part) = year_part.split("Week:").nth(1) {
                    details.manufacture_week = week_part.trim().parse().ok();
                }
            }
        } else if line.starts_with("EDID version:") {
            details.edid_version = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Video input definition:") {
            details.video_input = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Standard sRGB color space:") {
            let srgb = line.split(':').nth(1).map(|s| s.trim()).unwrap_or("Unknown");
            details.color_space = Some(format!("sRGB: {}", srgb));
        }
    }
    
    // Извлекаем EDID hex dump
    if let Some(hex_start) = output.find("EDID hex dump:") {
        let hex_section = &output[hex_start..];
        let mut hex_lines = Vec::new();
        
        for line in hex_section.lines().skip(2) { // Пропускаем заголовок
            if line.trim().is_empty() || !line.contains("+") {
                break;
            }
            hex_lines.push(line.to_string());
        }
        
        if !hex_lines.is_empty() {
            details.edid_hex = Some(hex_lines.join("\n"));
        }
    }
}

fn parse_ddcutil_capabilities(output: &str, details: &mut MonitorDetails) {
    for line in output.lines() {
        let line = line.trim();
        
        if line.starts_with("MCCS version:") {
            details.mccs_version = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("VCP version:") {
            details.mccs_version = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Controller mfg:") {
            details.controller_mfg = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Firmware version:") {
            details.firmware_version = line.split(':').nth(1).map(|s| s.trim().to_string());
        } else if line.starts_with("Feature:") {
            // Парсим VCP коды: "Feature: 10 (Brightness)"
            if let Some(code_part) = line.split(':').nth(1) {
                if let Some(code_str) = code_part.split('(').next() {
                    if let Ok(code) = u8::from_str_radix(code_str.trim(), 16) {
                        details.supported_vcp_codes.push(code);
                    }
                }
            }
        }
    }
}

fn get_current_vcp_values(bus: u8, details: &mut MonitorDetails) {
    // Получаем яркость (VCP 0x10)
    if let Ok(brightness) = ddc_get_brightness(bus) {
        details.current_brightness = Some(brightness);
    }
    
    // Получаем другие параметры через ddcutil getvcp
    let vcp_codes = vec![
        (0x12, "contrast"),
        (0x14, "color_temp"),
        (0x60, "input_source"),
        (0x62, "volume"),
    ];
    
    for (code, param_type) in vcp_codes {
        if let Ok(output) = Command::new("ddcutil")
            .arg("getvcp")
            .arg(format!("{:02x}", code))
            .arg("--bus")
            .arg(bus.to_string())
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_vcp_value(&stdout, param_type, details);
        }
    }
}

fn parse_vcp_value(output: &str, param_type: &str, details: &mut MonitorDetails) {
    for line in output.lines() {
        if line.contains("current value") {
            match param_type {
                "contrast" => {
                    if let Some(value) = extract_current_value(line) {
                        details.current_contrast = Some(value);
                    }
                }
                "color_temp" => {
                    // Парсим цветовую температуру: "6500 K (0x05)"
                    if let Some(temp_part) = line.split(':').nth(1) {
                        if let Some(temp_str) = temp_part.split('(').next() {
                            details.current_color_temp = Some(temp_str.trim().to_string());
                        }
                    }
                }
                "input_source" => {
                    if let Some(source_part) = line.split(':').nth(1) {
                        if let Some(source_str) = source_part.split('(').next() {
                            details.current_input_source = Some(source_str.trim().to_string());
                        }
                    }
                }
                "volume" => {
                    if let Some(value) = extract_current_value(line) {
                        details.current_volume = Some(value);
                    }
                }
                _ => {}
            }
        }
    }
}

fn extract_current_value(line: &str) -> Option<u8> {
    if let Some(value_part) = line.split("current value =").nth(1) {
        if let Some(value_str) = value_part.split(',').next() {
            return value_str.trim().parse().ok();
        }
    }
    None
}

fn show_monitor_details_dialog(parent: &ApplicationWindow, display: &DisplayInfo) {
    let dialog = Window::builder()
        .title(&format!("Подробности: {}", display.name))
        .transient_for(parent)
        .modal(false)  // Делаем немодальным для лучшего UX
        .default_width(1000)
        .default_height(800)
        .resizable(true)  // Разрешаем изменение размера
        .build();
    
    // Создаем основной контейнер
    let vbox = GtkBox::new(Orientation::Vertical, 0);
    
    // Создаем notebook с вкладками
    let notebook = Notebook::new();
    notebook.set_margin_top(12);
    notebook.set_margin_bottom(12);
    notebook.set_margin_start(12);
    notebook.set_margin_end(12);
    notebook.set_vexpand(true);
    notebook.set_hexpand(true);
    
    // Показываем индикатор загрузки
    let loading_label = Label::new(Some("Загрузка информации..."));
    loading_label.set_margin_top(50);
    loading_label.set_margin_bottom(50);
    
    vbox.append(&loading_label);
    dialog.set_child(Some(&vbox));
    
    // Получаем детальную информацию асинхронно
    let display_clone = display.clone();
    let _dialog_clone = dialog.clone();
    let notebook_clone = notebook.clone();
    let vbox_clone = vbox.clone();
    let loading_label_clone = loading_label.clone();
    
    glib::timeout_add_local_once(Duration::from_millis(100), move || {
        let details = match get_monitor_details(&display_clone) {
            Ok(details) => details,
            Err(e) => {
                loading_label_clone.set_text(&format!("Ошибка загрузки: {}", e));
                return;
            }
        };
        
        // Удаляем индикатор загрузки
        vbox_clone.remove(&loading_label_clone);
        
        // Создаем вкладки с информацией
        create_monitor_info_tabs(&notebook_clone, &details);
        
        // Добавляем notebook
        vbox_clone.prepend(&notebook_clone);
    });
    
    dialog.present();
}

fn create_monitor_info_tabs(notebook: &Notebook, details: &MonitorDetails) {
    // Вкладка "Основное"
    let basic_page = create_basic_info_page(details);
    notebook.append_page(&basic_page, Some(&Label::new(Some("Основное"))));
    
    // Вкладка "Настройки"
    let settings_page = create_settings_page(details);
    notebook.append_page(&settings_page, Some(&Label::new(Some("Настройки"))));
    
    // Вкладка "Capabilities" (если есть данные)
    if details.capabilities_string.is_some() {
        let caps_page = create_capabilities_page(details);
        notebook.append_page(&caps_page, Some(&Label::new(Some("Capabilities"))));
    }
    
    // Вкладка "Технические"
    let tech_page = create_technical_page(details);
    notebook.append_page(&tech_page, Some(&Label::new(Some("Технические"))));
    
    // Вкладка "EDID" (если есть данные)
    if details.edid_hex.is_some() {
        let edid_page = create_edid_page(details);
        notebook.append_page(&edid_page, Some(&Label::new(Some("EDID"))));
    }
}

fn create_basic_info_page(details: &MonitorDetails) -> ScrolledWindow {
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    
    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    
    // Основная информация
    add_info_row(&vbox, "Производитель:", &details.manufacturer);
    add_info_row(&vbox, "Модель:", &details.model);
    
    if let Some(ref serial) = details.serial_number {
        add_info_row(&vbox, "Серийный номер:", serial);
    }
    
    if let (Some(year), Some(week)) = (details.manufacture_year, details.manufacture_week) {
        add_info_row(&vbox, "Дата производства:", &format!("{} год, {} неделя", year, week));
    }
    
    if let Some(ref version) = details.edid_version {
        add_info_row(&vbox, "Версия EDID:", version);
    }
    
    if let Some(ref input) = details.video_input {
        add_info_row(&vbox, "Тип входа:", input);
    }
    
    if let Some(ref resolution) = details.resolution {
        add_info_row(&vbox, "Разрешение:", resolution);
    }
    
    if let Some(ref size) = details.physical_size {
        add_info_row(&vbox, "Физический размер:", size);
    }
    
    if let Some(ref ratio) = details.aspect_ratio {
        add_info_row(&vbox, "Соотношение сторон:", ratio);
    }
    
    if let Some(ref color_space) = details.color_space {
        add_info_row(&vbox, "Тип монитора:", color_space);
    }
    
    // Статистика
    let separator = Separator::new(Orientation::Horizontal);
    separator.set_margin_top(12);
    separator.set_margin_bottom(12);
    vbox.append(&separator);
    
    if let Some(ref last_update) = details.last_update {
        add_info_row(&vbox, "Последнее обновление:", last_update);
    }
    
    if details.read_errors > 0 {
        add_info_row(&vbox, "Ошибки чтения:", &format!("{}", details.read_errors));
    }
    
    scrolled.set_child(Some(&vbox));
    scrolled
}

fn create_settings_page(details: &MonitorDetails) -> ScrolledWindow {
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    
    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    
    // DDC/CI информация
    if let Some(ref mccs) = details.mccs_version {
        add_info_row(&vbox, "Версия MCCS:", mccs);
    }
    
    if let Some(ref controller) = details.controller_mfg {
        add_info_row(&vbox, "Контроллер:", controller);
    }
    
    if let Some(ref firmware) = details.firmware_version {
        add_info_row(&vbox, "Версия прошивки:", firmware);
    }
    
    // Текущие настройки
    let separator1 = Separator::new(Orientation::Horizontal);
    separator1.set_margin_top(12);
    separator1.set_margin_bottom(12);
    vbox.append(&separator1);
    
    let settings_label = Label::new(Some("Текущие настройки:"));
    settings_label.set_xalign(0.0);
    settings_label.set_markup("<b>Текущие настройки:</b>");
    vbox.append(&settings_label);
    
    if let Some(brightness) = details.current_brightness {
        add_info_row(&vbox, "Яркость:", &format!("{}/100", brightness));
    }
    
    if let Some(contrast) = details.current_contrast {
        add_info_row(&vbox, "Контраст:", &format!("{}/100", contrast));
    }
    
    if let Some(ref input_source) = details.current_input_source {
        add_info_row(&vbox, "Источник входа:", input_source);
    }
    
    if let Some(volume) = details.current_volume {
        add_info_row(&vbox, "Громкость:", &format!("{}/100", volume));
    }
    
    if let Some(ref power_state) = details.current_power_state {
        add_info_row(&vbox, "Состояние питания:", power_state);
    }
    
    // Цветовые каналы
    if details.red_gain.is_some() || details.green_gain.is_some() || details.blue_gain.is_some() {
        let separator2 = Separator::new(Orientation::Horizontal);
        separator2.set_margin_top(12);
        separator2.set_margin_bottom(12);
        vbox.append(&separator2);
        
        let color_label = Label::new(Some("Цветовые каналы:"));
        color_label.set_xalign(0.0);
        color_label.set_markup("<b>Цветовые каналы:</b>");
        vbox.append(&color_label);
        
        if let Some(red) = details.red_gain {
            add_info_row(&vbox, "Красный канал:", &format!("{}", red));
        }
        
        if let Some(green) = details.green_gain {
            add_info_row(&vbox, "Зеленый канал:", &format!("{}", green));
        }
        
        if let Some(blue) = details.blue_gain {
            add_info_row(&vbox, "Синий канал:", &format!("{}", blue));
        }
    }
    
    if let Some(backlight) = details.backlight_control {
        add_info_row(&vbox, "Управление подсветкой:", &format!("{}", backlight));
    }
    
    if let Some(ref osd_lang) = details.osd_language {
        add_info_row(&vbox, "Язык OSD:", osd_lang);
    }
    
    // Поддерживаемые VCP коды
    if !details.supported_vcp_codes.is_empty() {
        let separator = Separator::new(Orientation::Horizontal);
        separator.set_margin_top(12);
        separator.set_margin_bottom(12);
        vbox.append(&separator);
        
        let vcp_label = Label::new(Some("Поддерживаемые VCP коды:"));
        vcp_label.set_xalign(0.0);
        vcp_label.set_markup("<b>Поддерживаемые VCP коды:</b>");
        vbox.append(&vcp_label);
        
        let mut vcp_codes = details.supported_vcp_codes.clone();
        vcp_codes.sort();
        let vcp_text = vcp_codes.iter()
            .map(|code| format!("0x{:02X}", code))
            .collect::<Vec<_>>()
            .join(", ");
        
        let vcp_value_label = Label::new(Some(&vcp_text));
        vcp_value_label.set_xalign(0.0);
        vcp_value_label.set_wrap(true);
        vcp_value_label.set_selectable(true);
        vbox.append(&vcp_value_label);
    }
    
    scrolled.set_child(Some(&vbox));
    scrolled
}

fn create_capabilities_page(details: &MonitorDetails) -> ScrolledWindow {
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    
    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    
    if let Some(ref caps_string) = details.capabilities_string {
        // Заголовок
        let title_label = Label::new(Some("DDC/CI Capabilities String:"));
        title_label.set_xalign(0.0);
        title_label.set_markup("<b>DDC/CI Capabilities String:</b>");
        vbox.append(&title_label);
        
        // Текстовое поле с capabilities string
        let text_view = TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(false);
        text_view.set_wrap_mode(WrapMode::Word);
        text_view.set_monospace(true);
        
        let buffer = text_view.buffer();
        buffer.set_text(caps_string);
        
        let scrolled_text = ScrolledWindow::new();
        scrolled_text.set_policy(PolicyType::Automatic, PolicyType::Automatic);
        scrolled_text.set_min_content_height(300);
        scrolled_text.set_child(Some(&text_view));
        vbox.append(&scrolled_text);
        
        // Парсинг capabilities
        let separator = Separator::new(Orientation::Horizontal);
        separator.set_margin_top(12);
        separator.set_margin_bottom(12);
        vbox.append(&separator);
        
        let parsed_label = Label::new(Some("Распознанная информация:"));
        parsed_label.set_xalign(0.0);
        parsed_label.set_markup("<b>Распознанная информация:</b>");
        vbox.append(&parsed_label);
        
        // Извлекаем и показываем информацию из capabilities
        if caps_string.contains("mccs_ver(") {
            if let Some(start) = caps_string.find("mccs_ver(") {
                if let Some(end) = caps_string[start..].find(')') {
                    let version = &caps_string[start+9..start+end];
                    add_info_row(&vbox, "MCCS версия:", version);
                }
            }
        }
        
        if caps_string.contains("model(") {
            if let Some(start) = caps_string.find("model(") {
                if let Some(end) = caps_string[start..].find(')') {
                    let model = &caps_string[start+6..start+end];
                    add_info_row(&vbox, "Модель:", model);
                }
            }
        }
        
        if caps_string.contains("type(") {
            if let Some(start) = caps_string.find("type(") {
                if let Some(end) = caps_string[start..].find(')') {
                    let monitor_type = &caps_string[start+5..start+end];
                    add_info_row(&vbox, "Тип монитора:", monitor_type);
                }
            }
        }
        
        if caps_string.contains("cmds(") {
            if let Some(start) = caps_string.find("cmds(") {
                if let Some(end) = caps_string[start..].find(')') {
                    let commands = &caps_string[start+5..start+end];
                    add_info_row(&vbox, "Поддерживаемые команды:", commands);
                }
            }
        }
        
        // Показываем VCP коды более детально
        if !details.supported_vcp_codes.is_empty() {
            let vcp_separator = Separator::new(Orientation::Horizontal);
            vcp_separator.set_margin_top(12);
            vcp_separator.set_margin_bottom(12);
            vbox.append(&vcp_separator);
            
            let vcp_label = Label::new(Some("Поддерживаемые VCP коды:"));
            vcp_label.set_xalign(0.0);
            vcp_label.set_markup("<b>Поддерживаемые VCP коды:</b>");
            vbox.append(&vcp_label);
            
            let mut vcp_codes = details.supported_vcp_codes.clone();
            vcp_codes.sort();
            
            let codes_text = vcp_codes.iter()
                .map(|code| {
                    let description = match *code {
                        0x10 => "Яркость",
                        0x12 => "Контраст",
                        0x13 => "Управление подсветкой",
                        0x14 => "Цветовая температура",
                        0x16 => "Красный канал",
                        0x18 => "Зеленый канал",
                        0x1A => "Синий канал",
                        0x60 => "Источник входа",
                        0x62 => "Громкость",
                        0xD6 => "Состояние питания",
                        _ => "Неизвестно",
                    };
                    format!("0x{:02X} - {}", code, description)
                })
                .collect::<Vec<_>>()
                .join("\n");
            
            let vcp_text_view = TextView::new();
            vcp_text_view.set_editable(false);
            vcp_text_view.set_cursor_visible(false);
            vcp_text_view.set_monospace(true);
            
            let buffer = vcp_text_view.buffer();
            buffer.set_text(&codes_text);
            
            let vcp_scrolled = ScrolledWindow::new();
            vcp_scrolled.set_policy(PolicyType::Automatic, PolicyType::Automatic);
            vcp_scrolled.set_min_content_height(200);
            vcp_scrolled.set_child(Some(&vcp_text_view));
            vbox.append(&vcp_scrolled);
        }
    } else {
        let no_data_label = Label::new(Some("Capabilities данные недоступны"));
        no_data_label.set_xalign(0.5);
        no_data_label.set_yalign(0.5);
        vbox.append(&no_data_label);
    }
    
    scrolled.set_child(Some(&vbox));
    scrolled
}

fn create_technical_page(details: &MonitorDetails) -> ScrolledWindow {
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    
    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    
    // Системная информация
    let system_label = Label::new(Some("Системная информация:"));
    system_label.set_xalign(0.0);
    system_label.set_markup("<b>Системная информация:</b>");
    vbox.append(&system_label);
    
    add_info_row(&vbox, "I2C шина:", &details.i2c_bus);
    
    if let Some(ref device_path) = details.device_path {
        add_info_row(&vbox, "Путь к устройству:", device_path);
    }
    
    if let Some(ref connector) = details.drm_connector {
        add_info_row(&vbox, "DRM коннектор:", connector);
    }
    
    if let Some(ref driver) = details.driver {
        add_info_row(&vbox, "Драйвер:", driver);
    }
    
    if let Some(ref pci_path) = details.pci_path {
        add_info_row(&vbox, "PCI путь:", pci_path);
    }
    
    // DDC/CI статистика
    let separator = Separator::new(Orientation::Horizontal);
    separator.set_margin_top(12);
    separator.set_margin_bottom(12);
    vbox.append(&separator);
    
    let ddc_label = Label::new(Some("DDC/CI статистика:"));
    ddc_label.set_xalign(0.0);
    ddc_label.set_markup("<b>DDC/CI статистика:</b>");
    vbox.append(&ddc_label);
    
    add_info_row(&vbox, "Ошибки чтения:", &format!("{}", details.read_errors));
    
    if let Some(ref last_update) = details.last_update {
        add_info_row(&vbox, "Последнее обновление:", last_update);
    }
    
    add_info_row(&vbox, "Поддерживаемых VCP кодов:", &format!("{}", details.supported_vcp_codes.len()));
    
    // Дополнительная информация
    if details.capabilities_string.is_some() {
        add_info_row(&vbox, "Capabilities доступны:", "Да");
    } else {
        add_info_row(&vbox, "Capabilities доступны:", "Нет");
    }
    
    if details.edid_hex.is_some() {
        add_info_row(&vbox, "EDID доступен:", "Да");
    } else {
        add_info_row(&vbox, "EDID доступен:", "Нет");
    }
    
    scrolled.set_child(Some(&vbox));
    scrolled
}

fn create_edid_page(details: &MonitorDetails) -> ScrolledWindow {
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    
    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    
    let label = Label::new(Some("EDID Hex Dump:"));
    label.set_xalign(0.0);
    label.set_markup("<b>EDID Hex Dump:</b>");
    vbox.append(&label);
    
    if let Some(ref edid_hex) = details.edid_hex {
        let text_view = TextView::new();
        let buffer = TextBuffer::new(None);
        buffer.set_text(edid_hex);
        text_view.set_buffer(Some(&buffer));
        text_view.set_editable(false);
        text_view.set_monospace(true);
        text_view.set_margin_top(8);
        
        let scrolled_text = ScrolledWindow::new();
        scrolled_text.set_height_request(400);
        scrolled_text.set_child(Some(&text_view));
        vbox.append(&scrolled_text);
        
        // Кнопка копирования
        let copy_button = Button::with_label("Копировать в буфер обмена");
        copy_button.set_margin_top(8);
        
        let edid_hex_for_copy = edid_hex.clone();
        copy_button.connect_clicked(move |_| {
            if let Some(display) = gtk::gdk::Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&edid_hex_for_copy);
            }
        });
        
        vbox.append(&copy_button);
    }
    
    scrolled.set_child(Some(&vbox));
    scrolled
}

fn add_info_row(container: &GtkBox, label: &str, value: &str) {
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    
    let label_widget = Label::new(Some(label));
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(20);
    label_widget.set_markup(&format!("<b>{}</b>", label));
    
    let value_widget = Label::new(Some(value));
    value_widget.set_xalign(0.0);
    value_widget.set_selectable(true);
    value_widget.set_wrap(true);
    value_widget.set_hexpand(true);
    
    hbox.append(&label_widget);
    hbox.append(&value_widget);
    container.append(&hbox);
}

fn load_brightness_profile() -> Result<HashMap<u8, u8>, String> {
    let profile_path = get_profile_path()?;
    
    if !profile_path.exists() {
        return Ok(HashMap::new()); // Пустой профиль если файл не существует
    }
    
    let content = fs::read_to_string(&profile_path)
        .map_err(|e| format!("Failed to read profile: {}", e))?;
    
    let mut brightness_values = HashMap::new();
    
    // Простой парсинг XML (можно заменить на полноценный XML парсер)
    for line in content.lines() {
        if line.trim().starts_with("<display") {
            if let (Some(bus_start), Some(brightness_start)) = (
                line.find("bus=\"").map(|i| i + 5),
                line.find("brightness=\"").map(|i| i + 12)
            ) {
                if let (Some(bus_end), Some(brightness_end)) = (
                    line[bus_start..].find('"').map(|i| i + bus_start),
                    line[brightness_start..].find('"').map(|i| i + brightness_start)
                ) {
                    if let (Ok(bus), Ok(brightness)) = (
                        line[bus_start..bus_end].parse::<u8>(),
                        line[brightness_start..brightness_end].parse::<u8>()
                    ) {
                        brightness_values.insert(bus, brightness);
                    }
                }
            }
        }
    }
    
    println!("Loaded brightness profile: {:?}", brightness_values);
    Ok(brightness_values)
}

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
        xrandr_set_brightness(output, value)
    } else {
        Err("No available method to set brightness".to_string())
    }
}

fn get_brightness_any_method(display: &DisplayInfo) -> Result<u8, String> {
    if display.supports_ddc {
        ddc_get_brightness(display.i2c_bus)
    } else if let Some(ref output) = display.xrandr_output {
        xrandr_get_brightness(output)
    } else {
        Err("No available method to get brightness".to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlMethodPref { Ddc, Xrandr }

// Simple xrandr helpers as a fallback software brightness control
fn xrandr_set_brightness(output: &str, value: u8) -> Result<(), String> {
    let factor = (value as f32) / 100.0;
    let status = Command::new("xrandr")
        .arg("--output").arg(output)
        .arg("--brightness").arg(format!("{:.2}", factor))
        .status()
        .map_err(|e| format!("Failed to run xrandr: {}", e))?;
    if status.success() { Ok(()) } else { Err(format!("xrandr exited with status: {:?}", status.code())) }
}

fn xrandr_get_brightness(output: &str) -> Result<u8, String> {
    let out = Command::new("xrandr")
        .arg("--verbose").arg("--output").arg(output)
        .output()
        .map_err(|e| format!("Failed to run xrandr --verbose: {}", e))?;
    if !out.status.success() {
        return Err(format!("xrandr --verbose failed: {:?}", out.status.code()));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.trim().strip_prefix("Brightness:") {
            if let Ok(f) = rest.trim().parse::<f32>() {
                let v = (f * 100.0).round().clamp(0.0, 100.0) as u8;
                return Ok(v);
            }
        }
    }
    // Default if not found
    Ok(100)
}

fn set_brightness_with_pref(display: &DisplayInfo, value: u8, pref: Option<ControlMethodPref>) -> Result<(), String> {
    match pref {
        Some(ControlMethodPref::Ddc) if display.supports_ddc => ddc_set_brightness(display.i2c_bus, value),
        Some(ControlMethodPref::Xrandr) => {
            if let Some(ref out) = display.xrandr_output { xrandr_set_brightness(out, value) } else { Err("XRandR not available".into()) }
        }
        _ => set_brightness_any_method(display, value),
    }
}

fn get_brightness_with_pref(display: &DisplayInfo, pref: Option<ControlMethodPref>) -> Result<u8, String> {
    match pref {
        Some(ControlMethodPref::Ddc) if display.supports_ddc => ddc_get_brightness(display.i2c_bus),
        Some(ControlMethodPref::Xrandr) => {
            if let Some(ref out) = display.xrandr_output { xrandr_get_brightness(out) } else { Err("XRandR not available".into()) }
        }
        _ => get_brightness_any_method(display),
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

    // Определяем текущую тему и добавляем соответствующий CSS класс
    if let Some(settings) = gtk::Settings::default() {
        let is_dark_theme = settings.is_gtk_application_prefer_dark_theme();
        if is_dark_theme {
            win.add_css_class("dark");
        } else {
            win.add_css_class("light");
        }
        
        // Отслеживаем изменения темы
        let win_for_theme = win.clone();
        settings.connect_gtk_application_prefer_dark_theme_notify(move |settings| {
            let is_dark = settings.is_gtk_application_prefer_dark_theme();
            if is_dark {
                win_for_theme.remove_css_class("light");
                win_for_theme.add_css_class("dark");
            } else {
                win_for_theme.remove_css_class("dark");
                win_for_theme.add_css_class("light");
            }
        });
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

    // Панель подтверждения изменений (скрыта по умолчанию)
    let confirm_bar = GtkBox::new(Orientation::Horizontal, 12);
    confirm_bar.add_css_class("confirm-bar");
    confirm_bar.set_visible(false);
    
    let timer_label = Label::new(Some("Изменения будут отменены через 20 сек"));
    timer_label.add_css_class("timer-label");
    
    let confirm_button = Button::with_label("Подтвердить изменения");
    confirm_button.add_css_class("confirm-button");
    
    let cancel_button = Button::with_label("Отменить");
    cancel_button.add_css_class("cancel-button");
    
    confirm_bar.append(&timer_label);
    confirm_bar.append(&confirm_button);
    confirm_bar.append(&cancel_button);
    vbox.append(&confirm_bar);

    let list_box = GtkBox::new(Orientation::Vertical, 12);
    list_box.set_hexpand(true);
    vbox.append(&list_box);
    
    // Состояние яркости для отслеживания изменений
    let brightness_state = Rc::new(RefCell::new(BrightnessState::new()));
    let slider_refs = Rc::new(RefCell::new(SliderRefs::new()));
    // Предпочитаемый метод управления яркостью по шине I2C (если монитор поддерживает оба)
    let control_pref_map: Rc<RefCell<HashMap<u8, ControlMethodPref>>> = Rc::new(RefCell::new(HashMap::new()));
    let timer_source_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    
    // Функция для запуска таймера обратного отсчета
    let start_confirmation_timer = {
        let brightness_state = brightness_state.clone();
        let slider_refs = slider_refs.clone();
        let timer_source_id = timer_source_id.clone();
        let confirm_bar = confirm_bar.clone();
        let timer_label = timer_label.clone();
        
        move |displays: Vec<DisplayInfo>| {
            // Отменяем предыдущий таймер если есть
            if let Some(source_id) = timer_source_id.borrow_mut().take() {
                source_id.remove();
            }
            
            brightness_state.borrow_mut().timer_active = true;
            confirm_bar.set_visible(true);
            
            let countdown = Rc::new(Cell::new(20));
            let brightness_state_timer = brightness_state.clone();
            let slider_refs_timer = slider_refs.clone();
            let timer_source_id_timer = timer_source_id.clone();
            let confirm_bar_timer = confirm_bar.clone();
            let timer_label_timer = timer_label.clone();
            let displays_for_timer = displays.clone();
            
            let source_id = glib::timeout_add_seconds_local(1, move || {
                let remaining = countdown.get();
                if remaining > 0 {
                    timer_label_timer.set_text(&format!("Изменения будут отменены через {} сек", remaining));
                    countdown.set(remaining - 1);
                    glib::ControlFlow::Continue
                } else {
                    // Время вышло - откатываем изменения
                    println!("Timer expired - restoring original brightness values");
                    
                    let state = brightness_state_timer.borrow();
                    let original_values = state.original_values.clone();
                    drop(state); // Освобождаем borrow раньше
                    
                    // Безопасно восстанавливаем позиции слайдеров
                    if let Ok(mut slider_refs) = slider_refs_timer.try_borrow_mut() {
                        slider_refs.restore_all_sliders(&original_values);
                    } else {
                        println!("Warning: Could not restore slider positions - slider_refs is borrowed");
                    }
                    
                    // Применяем исходные значения к мониторам
                    for (&bus, &brightness) in &original_values {
                        if let Some(display) = displays_for_timer.iter().find(|d| d.i2c_bus == bus) {
                            let display_clone = display.clone();
                            thread::spawn(move || {
                                if let Err(e) = set_brightness_any_method(&display_clone, brightness) {
                                    println!("Failed to restore brightness for {}: {}", display_clone.name, e);
                                } else {
                                    println!("Restored brightness {} for {}", brightness, display_clone.name);
                                }
                            });
                        }
                    }
                    
                    // Обновляем состояние
                    if let Ok(mut state) = brightness_state_timer.try_borrow_mut() {
                        state.reset_to_original();
                    }
                    
                    // Скрываем панель подтверждения
                    confirm_bar_timer.set_visible(false);
                    
                    // Очищаем ID таймера
                    if let Ok(mut timer_id) = timer_source_id_timer.try_borrow_mut() {
                        timer_id.take();
                    }
                    
                    println!("Timer cleanup completed");
                    glib::ControlFlow::Break
                }
            });
            
            *timer_source_id.borrow_mut() = Some(source_id);
        }
    };
    
    // Обработчик кнопки подтверждения
    let confirm_handler = {
        let brightness_state = brightness_state.clone();
        let timer_source_id = timer_source_id.clone();
        let confirm_bar = confirm_bar.clone();
        
        move |displays: Vec<DisplayInfo>| {
            println!("Confirm button clicked - saving brightness profile");
            
            // Отменяем таймер
            if let Ok(mut timer_id) = timer_source_id.try_borrow_mut() {
                if let Some(source_id) = timer_id.take() {
                    source_id.remove();
                }
            }
            
            let current_values = if let Ok(state) = brightness_state.try_borrow() {
                state.current_values.clone()
            } else {
                println!("Warning: Could not access brightness state");
                return;
            };
            
            // Сохраняем профиль в XML
            if let Err(e) = save_brightness_profile(&current_values, &displays) {
                println!("Failed to save brightness profile: {}", e);
            } else {
                println!("Brightness profile saved successfully");
            }
            
            // Подтверждаем изменения
            if let Ok(mut state) = brightness_state.try_borrow_mut() {
                state.confirm_changes();
            }
            
            confirm_bar.set_visible(false);
            println!("Confirm operation completed");
        }
    };
    
    // Обработчик кнопки отмены
    let cancel_handler = {
        let brightness_state = brightness_state.clone();
        let slider_refs = slider_refs.clone();
        let timer_source_id = timer_source_id.clone();
        let confirm_bar = confirm_bar.clone();
        
        move |displays: Vec<DisplayInfo>| {
            println!("Cancel button clicked - restoring original brightness values");
            
            // Отменяем таймер
            if let Ok(mut timer_id) = timer_source_id.try_borrow_mut() {
                if let Some(source_id) = timer_id.take() {
                    source_id.remove();
                }
            }
            
            let original_values = if let Ok(state) = brightness_state.try_borrow() {
                state.original_values.clone()
            } else {
                println!("Warning: Could not access brightness state");
                return;
            };
            
            // Восстанавливаем позиции слайдеров
            if let Ok(mut slider_refs_mut) = slider_refs.try_borrow_mut() {
                slider_refs_mut.restore_all_sliders(&original_values);
            } else {
                println!("Warning: Could not restore slider positions");
            }
            
            // Применяем исходные значения к мониторам
            for (&bus, &brightness) in &original_values {
                if let Some(display) = displays.iter().find(|d| d.i2c_bus == bus) {
                    let display_clone = display.clone();
                    thread::spawn(move || {
                        if let Err(e) = set_brightness_any_method(&display_clone, brightness) {
                            println!("Failed to restore brightness for {}: {}", display_clone.name, e);
                        } else {
                            println!("Restored brightness {} for {}", brightness, display_clone.name);
                        }
                    });
                }
            }
            
            // Обновляем состояние
            if let Ok(mut state) = brightness_state.try_borrow_mut() {
                state.reset_to_original();
            }
            
            confirm_bar.set_visible(false);
            println!("Cancel operation completed");
        }
    };

    let list_box_for_populate = list_box.clone();
    // Используем слабую ссылку на окно и ссылку на корневой контейнер для измерения
    let win_weak_for_measure = win.downgrade();
    let content_for_measure = vbox.clone();
    
    // Обработчики кнопок подтверждения (нужно подключить после создания дисплеев)
    let confirm_handler_rc = Rc::new(RefCell::new(None::<Box<dyn Fn()>>));
    let cancel_handler_rc = Rc::new(RefCell::new(None::<Box<dyn Fn()>>));
    
    // Клонируем переменные для использования в populate
    let brightness_state_for_populate = brightness_state.clone();
    let slider_refs_for_populate = slider_refs.clone();
    let control_pref_map_for_populate = control_pref_map.clone();
    let start_confirmation_timer_for_populate = start_confirmation_timer.clone();
    let confirm_handler_rc_for_populate = confirm_handler_rc.clone();
    let cancel_handler_rc_for_populate = cancel_handler_rc.clone();
    let confirm_handler_for_populate = confirm_handler.clone();
    let cancel_handler_for_populate = cancel_handler.clone();
    
    let populate: Rc<dyn Fn()> = Rc::new(move || {
        // Clear list and reset state
        while let Some(child) = list_box_for_populate.first_child() {
            list_box_for_populate.remove(&child);
        }
        
        // Очищаем состояние при обновлении
        brightness_state_for_populate.borrow_mut().original_values.clear();
        brightness_state_for_populate.borrow_mut().current_values.clear();
        brightness_state_for_populate.borrow_mut().has_changes = false;
        brightness_state_for_populate.borrow_mut().timer_active = false;
        slider_refs_for_populate.borrow_mut().sliders.clear();

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
        
        // Клонируем переменные для async блока
        let brightness_state_for_async = brightness_state_for_populate.clone();
        let slider_refs_for_async = slider_refs_for_populate.clone();
        let control_pref_map_for_async = control_pref_map_for_populate.clone();
        let start_timer_for_async = start_confirmation_timer_for_populate.clone();
        let confirm_handler_rc_for_async = confirm_handler_rc_for_populate.clone();
        let cancel_handler_rc_for_async = cancel_handler_rc_for_populate.clone();
        let confirm_handler_for_async = confirm_handler_for_populate.clone();
        let cancel_handler_for_async = cancel_handler_for_populate.clone();
        
        glib::spawn_future_local(async move {
            if let Ok(msg) = rx.recv().await {
                match msg {
                    Ok(cards) if !cards.is_empty() => {
                        // Собираем все дисплеи для использования в callbacks
                        let all_displays: Vec<DisplayInfo> = cards.iter()
                            .flat_map(|card| card.displays.iter())
                            .cloned()
                            .collect();
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
                                // Инициализируем предпочтение метода
                                {
                                    let mut pref_map = control_pref_map_for_async.borrow_mut();
                                    let default_pref = if d.supports_ddc { ControlMethodPref::Ddc } else { ControlMethodPref::Xrandr };
                                    pref_map.entry(d.i2c_bus).or_insert(default_pref);
                                }
                                
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
                                // Выравнивание и стили для иконки-кнопки
                                badge_icon.set_halign(gtk::Align::Center);
                                badge_icon.set_valign(gtk::Align::Center);
                                badge_icon.set_margin_start(6);
                                badge_icon.set_margin_end(6);
                                badge_icon.set_pixel_size(18);
                                badge_icon.add_css_class("icon-button");
                                badge_icon.set_tooltip_text(Some("Открыть карточку монитора"));
                                let badge = Button::with_label(control_method);
                                badge.add_css_class("badge");
                                badge.add_css_class("badge-button");
                                badge.add_css_class(badge_class);
                                badge.add_css_class("flat");
                                badge.set_can_focus(false);
                                badge.set_tooltip_text(Some(&tooltip));
                                // Если доступны оба метода — делаем кнопку переключаемой
                                if d.supports_ddc && d.xrandr_output.is_some() {
                                    let badge_btn = badge.clone();
                                    let control_pref_map_for_toggle = control_pref_map_for_async.clone();
                                    let slider_refs_for_toggle = slider_refs_for_async.clone();
                                    let brightness_state_for_toggle = brightness_state_for_async.clone();
                                    let d_for_toggle = d.clone();
                                    badge.connect_clicked(move |_| {
                                        // Переключаем предпочтение
                                        let new_pref = {
                                            let mut map = control_pref_map_for_toggle.borrow_mut();
                                            let entry = map.entry(d_for_toggle.i2c_bus).or_insert(ControlMethodPref::Ddc);
                                            *entry = match *entry { ControlMethodPref::Ddc => ControlMethodPref::Xrandr, ControlMethodPref::Xrandr => ControlMethodPref::Ddc };
                                            *entry
                                        };
                                        // Обновляем визуал
                                        badge_btn.remove_css_class("badge-ddc");
                                        badge_btn.remove_css_class("badge-xrandr");
                                        match new_pref {
                                            ControlMethodPref::Ddc => {
                                                badge_btn.add_css_class("badge-ddc");
                                                badge_btn.set_label("аппаратно");
                                                badge_btn.set_tooltip_text(Some(&format!("Управление: аппаратно (DDC/CI)\nШина: /dev/i2c-{}", d_for_toggle.i2c_bus)));
                                            }
                                            ControlMethodPref::Xrandr => {
                                                badge_btn.add_css_class("badge-xrandr");
                                                badge_btn.set_label("программно");
                                                if let Some(ref out) = d_for_toggle.xrandr_output {
                                                    badge_btn.set_tooltip_text(Some(&format!("Управление: программно (xrandr)\nВывод: {}", out)));
                                                }
                                            }
                                        }
                                        // Независимые значения ползунка для каждого метода:
                                        // 1) Берём сохранённое значение для НОВОГО метода, если есть, иначе текущее
                                        let (target_val, have_stored) = if let Ok(refs) = slider_refs_for_toggle.try_borrow() {
                                            let cur = if let Some((scale_ref, _)) = refs.sliders.get(&d_for_toggle.i2c_bus) { scale_ref.value() as u8 } else { 0 };
                                            if let Some(v) = refs.get_last_value(d_for_toggle.i2c_bus, new_pref) { (v, true) } else { (cur, false) }
                                        } else { (0, false) };
                                        // 2) Если есть сохранённое — программно выставляем ползунок в него
                                        if have_stored {
                                            if let Ok(mut refs) = slider_refs_for_toggle.try_borrow_mut() {
                                                refs.update_slider_value(d_for_toggle.i2c_bus, target_val);
                                            }
                                        }
                                        // 3) Применяем яркость выбранным методом
                                        let d_set = d_for_toggle.clone();
                                        thread::spawn(move || {
                                            let _ = set_brightness_with_pref(&d_set, target_val, Some(new_pref));
                                        });
                                        // 4) Обновляем состояние и запоминаем это значение для нового метода
                                        let brightness_state_ui = brightness_state_for_toggle.clone();
                                        let slider_refs_ui2 = slider_refs_for_toggle.clone();
                                        glib::spawn_future_local(clone!(@strong d_for_toggle => async move {
                                            if let Ok(mut st) = brightness_state_ui.try_borrow_mut() {
                                                st.update_current(d_for_toggle.i2c_bus, target_val);
                                            }
                                            if let Ok(mut refs) = slider_refs_ui2.try_borrow_mut() {
                                                refs.remember_value(d_for_toggle.i2c_bus, new_pref, target_val);
                                            }
                                        }));
                                    });
                                }

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
                                let bus_for_pref = d.i2c_bus;
                                let pref_for_init = control_pref_map_for_async.borrow().get(&bus_for_pref).copied();
                                thread::spawn(move || {
                                    let res = get_brightness_with_pref(&display_for_brightness, pref_for_init);
                                    let _ = s_tx.send_blocking(res);
                                });
                                
                                let grid_for_tooltip = grid.clone();
                                let brightness_state_for_init = brightness_state_for_async.clone();
                                let slider_refs_for_init = slider_refs_for_async.clone();
                                let control_pref_map_for_init = control_pref_map_for_async.clone();
                                glib::spawn_future_local(clone!(@strong scale, @strong grid_for_tooltip, @strong value_lbl, @strong control_pref_map_for_init => async move {
                                    if let Ok(res) = s_rx.recv().await {
                                        match res {
                                            Ok(v) => {
                                                scale.set_value(v as f64);
                                                scale.set_sensitive(true);
                                                value_lbl.set_text(&format!("{}%", v));
                                                
                                                // Регистрируем слайдер в SliderRefs
                                                slider_refs_for_init.borrow_mut().add_slider(bus_for_pref, scale.clone(), value_lbl.clone());
                                                
                                                // Сохраняем исходное значение после получения реального значения
                                                brightness_state_for_init.borrow_mut().save_original(bus_for_pref, v);
                                                // Запоминаем значение для текущего предпочитаемого метода
                                                if let Ok(pref_map) = control_pref_map_for_init.try_borrow() {
                                                    if let Some(&pref_m) = pref_map.get(&bus_for_pref) {
                                                        if let Ok(mut refs) = slider_refs_for_init.try_borrow_mut() {
                                                            refs.remember_value(bus_for_pref, pref_m, v);
                                                        }
                                                    }
                                                }
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
                                let brightness_state_for_callback = brightness_state_for_async.clone();
                                let control_pref_map_for_callback = control_pref_map_for_async.clone();
                                let slider_refs_for_callback = slider_refs_for_async.clone();
                                let start_timer_for_callback = start_timer_for_async.clone();
                                let all_displays_for_callback = all_displays.clone();
                                
                                scale.connect_value_changed(move |s| {
                                    let val = s.value() as u8;
                                    let display_clone = display_info_for_callback.clone();
                                    
                                    // Проверяем, является ли это программным обновлением
                                    if let Ok(mut slider_refs_mut) = slider_refs_for_callback.try_borrow_mut() {
                                        if slider_refs_mut.is_programmatic_update(display_clone.i2c_bus) {
                                            // Это программное обновление - просто обновляем метку и выходим
                                            value_lbl_for_change.set_text(&format!("{}%", val));
                                            return;
                                        }
                                    } else {
                                        // Если не можем получить мутабельную ссылку, значит идет восстановление
                                        // Просто обновляем метку и выходим
                                        value_lbl_for_change.set_text(&format!("{}%", val));
                                        return;
                                    }
                                    
                                    value_lbl_for_change.set_text(&format!("{}%", val));
                                    let callback_id = format!("{}_{}_bus{}", display_clone.name, display_index, display_clone.i2c_bus);
                                    
                                    println!("CALLBACK {}: Setting brightness {} for display: {} (bus {})", 
                                        callback_id, val, display_clone.name, display_clone.i2c_bus);
                                    
                                    // Обновляем состояние
                                    let (has_changes, _timer_active) = if let Ok(mut state) = brightness_state_for_callback.try_borrow_mut() {
                                        state.update_current(display_clone.i2c_bus, val);
                                        (state.has_changes, state.timer_active)
                                    } else {
                                        println!("Warning: Could not update brightness state");
                                        (false, false)
                                    };
                                    // Запоминаем значение для текущего метода
                                    if let Ok(pref_map) = control_pref_map_for_callback.try_borrow() {
                                        if let Some(&pref_m) = pref_map.get(&display_clone.i2c_bus) {
                                            if let Ok(mut refs) = slider_refs_for_callback.try_borrow_mut() {
                                                refs.remember_value(display_clone.i2c_bus, pref_m, val);
                                            }
                                        }
                                    }
                                    
                                    // Set brightness immediately in background thread, учитывая предпочтение
                                    let pref = control_pref_map_for_callback.borrow().get(&display_clone.i2c_bus).copied();
                                    thread::spawn(move || {
                                        if let Err(e) = set_brightness_with_pref(&display_clone, val, pref) {
                                            println!("Failed to set brightness for {}: {}", display_clone.name, e);
                                        } else {
                                            println!("Successfully set brightness {} for {}", val, display_clone.name);
                                        }
                                    });
                                    
                                    // Перезапускаем таймер подтверждения при каждом изменении
                                    if has_changes {
                                        start_timer_for_callback(all_displays_for_callback.clone());
                                    }
                                });
                            }
                            
                                // Оборачиваем в "карточку"
                                let frame = gtk::Frame::new(None);
                                frame.add_css_class("card");
                                frame.set_hexpand(true);
                                frame.set_child(Some(&grid));
                                
                                // Добавляем обработчик клика ТОЛЬКО на иконку монитора, чтобы не была кликабельной вся строка
                                let click_gesture = GestureClick::new();
                                let display_for_click = d.clone();
                                let win_for_dialog = win_weak_for_measure_outer.clone();
                                
                                click_gesture.connect_pressed(move |_, _, _, _| {
                                    if let Some(win) = win_for_dialog.upgrade() {
                                        show_monitor_details_dialog(&win, &display_for_click);
                                    }
                                });
                                
                                // Вешаем контроллер клика на иконку монитора
                                badge_icon.add_controller(click_gesture);
                                
                                list_box_target.append(&frame);
                            }
                            
                            // Add separator after each card
                            let separator = Separator::new(Orientation::Horizontal);
                            separator.set_margin_top(8);
                            separator.set_margin_bottom(8);
                            list_box_target.append(&separator);
                        }

                        // Устанавливаем обработчики кнопок подтверждения
                        *confirm_handler_rc_for_async.borrow_mut() = Some(Box::new({
                            let all_displays = all_displays.clone();
                            let confirm_handler = confirm_handler_for_async.clone();
                            move || confirm_handler(all_displays.clone())
                        }));
                        
                        *cancel_handler_rc_for_async.borrow_mut() = Some(Box::new({
                            let all_displays = all_displays.clone();
                            let cancel_handler = cancel_handler_for_async.clone();
                            move || cancel_handler(all_displays.clone())
                        }));

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

    let confirm_handler_for_button = confirm_handler_rc.clone();
    confirm_button.connect_clicked(move |_| {
        if let Some(ref handler) = *confirm_handler_for_button.borrow() {
            handler();
        }
    });
    
    let cancel_handler_for_button = cancel_handler_rc.clone();
    cancel_button.connect_clicked(move |_| {
        if let Some(ref handler) = *cancel_handler_for_button.borrow() {
            handler();
        }
    });

    // Refresh button
    let populate_btn = populate.clone();
    refresh_btn.connect_clicked(move |_| {
        populate_btn();
    });

    win.set_child(Some(&vbox));
    win.present();
}