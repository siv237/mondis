use anyhow::Result;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale, Separator, Button};
use tracing_subscriber::EnvFilter;
use glib::clone;
use std::rc::Rc;
use std::process::{Command, Stdio};
use std::thread;
use regex::Regex;
use std::cell::RefCell;
use std::time::Duration;

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let app = Application::builder()
        .application_id("com.mondis.panel")
        .build();

    app.connect_activate(move |app| {
        build_ui(app);
    });

    app.run();
    Ok(())
}

fn build_ui(app: &Application) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("Mondis: Панель яркости (прототип)")
        .default_width(640)
        .default_height(360)
        .build();

    let vbox = GtkBox::new(Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some("Обнаруженные мониторы"));
    let refresh_btn = Button::with_label("Обновить");
    header.append(&title);
    header.append(&refresh_btn);
    vbox.append(&header);
    vbox.append(&Separator::new(Orientation::Horizontal));

    let list_box = GtkBox::new(Orientation::Vertical, 12);
    vbox.append(&list_box);

    let list_box_for_populate = list_box.clone();
    let populate: Rc<dyn Fn()> = Rc::new(move || {
        // Очистить список
        while let Some(child) = list_box_for_populate.first_child() {
            list_box_for_populate.remove(&child);
        }

        // Только DDC-дисплеи (рабочие слайдеры)
        let ddc_header = Label::new(Some("Мониторы (DDC)"));
        ddc_header.set_xalign(0.0);
        list_box_for_populate.append(&ddc_header);
        let (tx, rx) = async_channel::unbounded::<Result<Vec<DisplayInfo>, String>>();
        thread::spawn(move || {
            let res = detect_displays_sync();
            let _ = tx.send_blocking(res);
        });
        let list_box_target = list_box_for_populate.clone();
        glib::spawn_future_local(async move {
            if let Ok(msg) = rx.recv().await {
                match msg {
                Ok(disps) if !disps.is_empty() => {
                    for d in disps {
                        let row = GtkBox::new(Orientation::Horizontal, 12);
                        let title = match (&d.mfg, &d.model) {
                            (Some(mfg), Some(model)) => {
                                // Expand common manufacturer codes
                                let mfg_name = match mfg.as_str() {
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
                                if d.supports_ddc {
                                    format!("{} {} (Display {})", mfg_name, model, d.index)
                                } else {
                                    format!("{} {} (не поддерживается DDC)", mfg_name, model)
                                }
                            }
                            _ => {
                                if d.supports_ddc {
                                    format!("Display {}", d.index)
                                } else {
                                    format!("Неизвестный монитор (не поддерживается DDC)")
                                }
                            }
                        };
                        let label = Label::new(Some(&title));
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

                            // Для каждого дисплея подтягиваем текущее значение яркости
                            let (s_tx, s_rx) = async_channel::unbounded::<Result<u8, String>>();
                            let d_idx = d.index;
                            thread::spawn(move || {
                                match get_brightness_sync(d_idx) {
                                    Ok(v) => { let _ = s_tx.send_blocking(Ok(v)); }
                                    Err(e) => { let _ = s_tx.send_blocking(Err(e)); }
                                }
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
                                            label.set_text(&format!("{} (ошибка DDC: {})", label.text(), e));
                                        }
                                    }
                                }
                            }));
                        }

                        // Дебаунсинг для слайдера, чтобы не спамить ddcutil (только для поддерживаемых)
                        if d.supports_ddc {
                            let d_idx = d.index;
                            let debounce_counter = Rc::new(RefCell::new(0u32));
                            scale.connect_value_changed(clone!(@strong debounce_counter => move |s| {
                                // Increment counter to invalidate previous timers
                                let current_id = {
                                    let mut counter = debounce_counter.borrow_mut();
                                    *counter += 1;
                                    *counter
                                };
                                
                                let val = s.value() as u8;
                                let idx = d_idx;
                                let debounce_counter_clone = debounce_counter.clone();
                                
                                glib::timeout_add_local_once(Duration::from_millis(200), move || {
                                    // Check if this timer is still valid
                                    if *debounce_counter_clone.borrow() == current_id {
                                        thread::spawn(move || {
                                            let _ = set_brightness_sync(idx, val);
                                        });
                                    }
                                });
                            }));
                        }

                        row.append(&label);
                        row.append(&scale);
                        list_box_target.append(&row);
                    }
                }
                Ok(_) => {
                    let lbl = Label::new(Some("(DDC-дисплеев не найдено)"));
                    lbl.set_xalign(0.0);
                    list_box_target.append(&lbl);
                }
                Err(err) => {
                    let lbl = Label::new(Some(&format!("Ошибка ddcutil: {}", err)));
                    lbl.set_xalign(0.0);
                    list_box_target.append(&lbl);
                }
                }
            }
        });
    });

    // Initial population
    populate();

    // Refresh button action
    let populate_btn = populate.clone();
    refresh_btn.connect_clicked(move |_| {
        populate_btn();
    });

    win.set_child(Some(&vbox));
    win.present();
}

#[derive(Clone, Debug)]
struct DisplayInfo { 
    index: u8, 
    mfg: Option<String>, 
    model: Option<String>,
    supports_ddc: bool,
}

fn detect_displays_sync() -> Result<Vec<DisplayInfo>, String> {
    let out = Command::new("ddcutil").arg("detect").arg("--terse").stdout(Stdio::piped()).output()
        .map_err(|e| format!("failed to run ddcutil detect: {}", e))?;
    if !out.status.success() {
        return Err(format!("ddcutil detect failed: status {:?}", out.status));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut res = Vec::new();
    let re_disp = Regex::new(r"^Display\s+(\d+)").unwrap();
    let re_invalid = Regex::new(r"^Invalid display").unwrap();
    let re_monitor = Regex::new(r"^\s*Monitor:\s*([^:]+):([^:]+):").unwrap();
    let mut cur: Option<DisplayInfo> = None;
    let mut invalid_count = 0u8;
    
    for line in s.lines() {
        if let Some(c) = re_disp.captures(line) {
            if let Some(d) = cur.take() { res.push(d); }
            let idx: u8 = c[1].parse().unwrap_or(0);
            cur = Some(DisplayInfo { index: idx, model: None, mfg: None, supports_ddc: true });
            continue;
        }
        if re_invalid.is_match(line) {
            if let Some(d) = cur.take() { res.push(d); }
            invalid_count += 1;
            // Use negative index to mark invalid displays
            cur = Some(DisplayInfo { index: 200 + invalid_count, model: None, mfg: None, supports_ddc: false });
            continue;
        }
        if let Some(c) = re_monitor.captures(line) {
            if let Some(ref mut d) = cur {
                d.mfg = Some(c[1].trim().to_string());
                d.model = Some(c[2].trim().to_string());
            }
        }
    }
    if let Some(d) = cur.take() { res.push(d); }
    Ok(res)
}

fn get_brightness_sync(display: u8) -> Result<u8, String> {
    let out = Command::new("ddcutil").arg("getvcp").arg("0x10").arg("--display").arg(display.to_string())
        .stdout(Stdio::piped()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ddcutil getvcp failed: status {:?}", out.status));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let re = Regex::new(r"current value =\s+(\d+)").unwrap();
    if let Some(caps) = re.captures(&s) {
        let v: u8 = caps[1].parse().unwrap_or(0);
        return Ok(v);
    }
    Err(format!("failed to parse ddcutil output: {}", s))
}

fn set_brightness_sync(display: u8, value: u8) -> Result<(), String> {
    let out = Command::new("ddcutil").arg("setvcp").arg("0x10").arg(value.to_string()).arg("--display").arg(display.to_string())
        .stdout(Stdio::piped()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ddcutil setvcp failed: status {:?}", out.status));
    }
    Ok(())
}
