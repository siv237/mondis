use anyhow::{anyhow, Context, Result};
use regex::Regex;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub index: u8,          // ddcutil display index (1..)
    pub model: Option<String>,
    pub mfg: Option<String>,
}

pub async fn list_displays() -> Result<Vec<DisplayInfo>> {
    let mut cmd = Command::new("ddcutil");
    cmd.arg("detect").arg("--terse");
    cmd.stdout(Stdio::piped());
    let out = cmd.output().await.context("failed to run ddcutil detect")?;
    if !out.status.success() {
        return Err(anyhow!("ddcutil detect failed: status {:?}", out.status));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    // Parse lines like: Display 1
    // Following lines may include:   Mfg: XXX Model: YYY
    let mut res = Vec::new();
    let re_disp = Regex::new(r"^Display\s+(\d+)").unwrap();
    let re_info = Regex::new(r"Mfg:\s*([^\s]+)\s+Model:\s*(.+)$").unwrap();
    let mut cur: Option<DisplayInfo> = None;
    for line in s.lines() {
        if let Some(c) = re_disp.captures(line) {
            if let Some(d) = cur.take() { res.push(d); }
            let idx: u8 = c[1].parse().unwrap_or(0);
            cur = Some(DisplayInfo { index: idx, model: None, mfg: None });
            continue;
        }
        if let Some(c) = re_info.captures(line) {
            if let Some(ref mut d) = cur {
                d.mfg = Some(c[1].to_string());
                d.model = Some(c[2].trim().to_string());
            }
        }
    }
    if let Some(d) = cur.take() { res.push(d); }
    Ok(res)
}

pub async fn get_brightness(bus: Option<u8>, display: Option<u8>) -> Result<u8> {
    // Use ddcutil getvcp 0x10 [--bus N] [--display N]
    let mut cmd = Command::new("ddcutil");
    cmd.arg("getvcp").arg("0x10");
    if let Some(b) = bus { cmd.arg("--bus").arg(b.to_string()); }
    if let Some(d) = display { cmd.arg("--display").arg(d.to_string()); }
    cmd.stdout(Stdio::piped());
    let out = cmd.output().await.context("failed to run ddcutil")?;
    if !out.status.success() {
        return Err(anyhow!("ddcutil failed: status {:?}", out.status));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    // Example: VCP code 0x10 (Brightness): current value = 50, max value = 100
    let re = Regex::new(r"current value = (\d+)").unwrap();
    if let Some(caps) = re.captures(&s) {
        let v: u8 = caps[1].parse().unwrap_or(0);
        return Ok(v);
    }
    Err(anyhow!("failed to parse ddcutil output: {}", s))
}

pub async fn set_brightness(value: u8, bus: Option<u8>, display: Option<u8>) -> Result<()> {
    let mut cmd = Command::new("ddcutil");
    cmd.arg("setvcp").arg("0x10").arg(value.to_string());
    if let Some(b) = bus { cmd.arg("--bus").arg(b.to_string()); }
    if let Some(d) = display { cmd.arg("--display").arg(d.to_string()); }
    cmd.stdout(Stdio::piped());
    let out = cmd.output().await.context("failed to run ddcutil")?;
    if !out.status.success() {
        return Err(anyhow!("ddcutil failed: status {:?}", out.status));
    }
    Ok(())
}
