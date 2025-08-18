use anyhow::{Context, Result};
use std::process::Command;
use std::{env, path::PathBuf};
use which::which;

#[cfg(not(feature = "xfce-gtk-tray"))]
use ksni::{self, menu::MenuItem};
#[cfg(not(feature = "xfce-gtk-tray"))]
use ksni::menu::StandardItem;
#[cfg(not(feature = "xfce-gtk-tray"))]
use tracing_subscriber::EnvFilter;
#[cfg(feature = "xfce-gtk-tray")]
use gtk::prelude::*;

#[cfg(not(feature = "xfce-gtk-tray"))]
struct MondisTray;

#[cfg(not(feature = "xfce-gtk-tray"))]
impl ksni::Tray for MondisTray {
    fn id(&self) -> String { "com.mondis.tray".into() }
    fn title(&self) -> String { "Mondis".into() }
    fn icon_name(&self) -> String { "display-brightness".into() }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if panel_running() {
            items.push(MenuItem::Standard(StandardItem {
                label: "Закрыть Mondis".into(),
                activate: Box::new(|_this: &mut MondisTray| {
                    let _ = close_panel().map_err(|e| eprintln!("failed to close mondis panel: {e:#}"));
                }),
                ..Default::default()
            }));
        } else {
            items.push(MenuItem::Standard(StandardItem {
                label: "Открыть Mondis".into(),
                activate: Box::new(|_this: &mut MondisTray| {
                    let _ = open_mondis(None).map_err(|e| eprintln!("failed to open mondis: {e:#}"));
                }),
                ..Default::default()
            }));
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::Standard(StandardItem {
            label: "Выход".into(),
            activate: Box::new(|_this: &mut MondisTray| std::process::exit(0)),
            ..Default::default()
        }));
        items
    }

    // Клик по значку: открываем панель. Координаты (x, y) можно позже использовать
    // для позиционирования окна, если добавим поддержку.
    fn activate(&mut self, x: i32, y: i32) {
        eprintln!("mondis-tray: activate() click at {},{}", x, y);
        if panel_running() {
            if let Err(e) = close_panel() { eprintln!("failed to close mondis: {e:#}"); }
        } else if let Err(e) = open_mondis(None) {
            eprintln!("failed to open mondis: {e:#}");
        }
    }

    // Альтернативная активация (например, средняя кнопка)
    fn secondary_activate(&mut self, x: i32, y: i32) {
        eprintln!("mondis-tray: secondary_activate() click at {},{}", x, y);
        if panel_running() {
            if let Err(e) = close_panel() { eprintln!("failed to close mondis: {e:#}"); }
        } else if let Err(e) = open_mondis(None) {
            eprintln!("failed to open mondis: {e:#}");
        }
    }

}

fn open_mondis(pos: Option<(i32, i32)>) -> Result<()> {
    // 1) Try binaries located next to the current tray executable (useful when running from target/release)
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            // try mondis-panel-direct then mondis-panel
            let mut candidates: Vec<PathBuf> = Vec::new();
            candidates.push(dir.join("mondis-panel-direct"));
            candidates.push(dir.join("mondis-panel"));
            for cand in candidates {
                if cand.is_file() {
                    let mut cmd = Command::new(&cand);
                    if let Some((x, y)) = pos { cmd.arg("--position").arg(x.to_string()).arg(y.to_string()); }
                    cmd.spawn().with_context(|| format!("spawn {}", cand.display()))?;
                    return Ok(());
                }
            }
        }
    }

    // 2) Try PATH via which
    if let Ok(path) = which("mondis-panel-direct") {
        let mut cmd = Command::new(&path);
        if let Some((x, y)) = pos { cmd.arg("--position").arg(x.to_string()).arg(y.to_string()); }
        cmd.spawn().context("spawn mondis-panel-direct")?;
        return Ok(());
    }
    if let Ok(path) = which("mondis-panel") {
        let mut cmd = Command::new(&path);
        if let Some((x, y)) = pos { cmd.arg("--position").arg(x.to_string()).arg(y.to_string()); }
        cmd.spawn().context("spawn mondis-panel")?;
        return Ok(());
    }

    // 3) Nothing found
    anyhow::bail!("не найден бинарник mondis-panel-direct или mondis-panel ни рядом с трейем, ни в PATH")
}

fn panel_running() -> bool {
    // Check for either binary name
    let names = ["mondis-panel-direct", "mondis-panel"];
    for name in names.iter() {
        if Command::new("pgrep").arg("-x").arg(name).status().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
        if Command::new("pidof").arg(name).status().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn close_panel() -> Result<()> {
    // Try to gracefully terminate both possible binaries
    for name in ["mondis-panel-direct", "mondis-panel"].iter() {
        let _ = Command::new("pkill").arg("-x").arg(name).status();
        let _ = Command::new("killall").arg("-q").arg(name).status();
    }
    Ok(())
}

#[cfg(not(feature = "xfce-gtk-tray"))]
fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let service = ksni::TrayService::new(MondisTray);
    let _handle = service.spawn();
    // Block forever
    loop { std::thread::park(); }
}

#[cfg(feature = "xfce-gtk-tray")]
fn main() -> Result<()> {
    // GTK-based tray (StatusIcon) for XFCE to avoid forced menus
    gtk::init()?;
    let icon = gtk::StatusIcon::new();
    icon.set_from_icon_name(Some("display-brightness"));
    icon.set_visible(true);
    icon.set_has_tooltip(true);
    icon.set_tooltip_text(Some("Mondis"));
    icon.connect_activate(|_| {
        eprintln!("mondis-tray(gtk): activate()");
        if let Err(e) = open_mondis(None) { eprintln!("failed to open mondis: {e:#}"); }
    });
    // Keep running main loop
    gtk::main();
    Ok(())
}
