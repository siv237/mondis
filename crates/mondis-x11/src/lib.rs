use anyhow::{anyhow, Result};
use mondis_core::model::{MonitorId, MonitorInfo};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{ConnectionExt as RandrConnectionExt, GetOutputInfoReply};
use x11rb::protocol::xproto::ConnectionExt as XprotoConnectionExt;
use x11rb::rust_connection::RustConnection;

pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let window = screen.root;
    let _ver = conn.randr_query_version(1, 5)?.reply()?;
    let resources = conn.randr_get_screen_resources_current(window)?.reply()?;
    let mut list = Vec::new();
    // Pre-fetch EDID atom id
    let edid_atom = conn.intern_atom(false, b"EDID")?.reply()?.atom;
    for output in resources.outputs {
        let info: GetOutputInfoReply = conn
            .randr_get_output_info(output, resources.config_timestamp)?
            .reply()?;
        if info.connection != x11rb::protocol::randr::Connection::CONNECTED {
            continue;
        }
        let name = String::from_utf8_lossy(&info.name).to_string();

        // Try to read EDID bytes
        let mut manufacturer = None;
        let mut model = None;
        let mut serial = None;
        let mut edid_hash: Option<String> = None;
        if let Ok(cookie) = conn.randr_get_output_property(
            output,
            edid_atom,
            x11rb::NONE,
            0,
            u32::MAX,
            false,
            false,
        ) {
            if let Ok(prop) = cookie.reply() {
            if prop.format == 8 {
                let bytes = prop.data.as_slice();
                if !bytes.is_empty() {
                    let n = bytes.len().min(16);
                    edid_hash = Some(bytes[..n].iter().map(|b| format!("{:02X}", b)).collect());
                    if let Ok((mfg, mdl, ser)) = parse_edid(bytes) {
                        manufacturer = mfg;
                        model = mdl;
                        serial = ser;
                    }
                }
            }
            }
        }
        let mon = MonitorInfo {
            id: MonitorId { name, edid_hash },
            manufacturer,
            model,
            serial,
            size_mm: Some((info.mm_width as u16, info.mm_height as u16)),
            current_mode: None,
        };
        list.push(mon);
    }
    Ok(list)
}

fn parse_edid(edid: &[u8]) -> Result<(Option<String>, Option<String>, Option<String>)> {
    if edid.len() < 128 { return Err(anyhow!("EDID too short")); }
    // Manufacturer ID: bytes 8-9 (big-endian, 5-bit letters)
    let mfg_id = u16::from_be_bytes([edid[8], edid[9]]);
    let c1 = (((mfg_id >> 10) & 0x1F) as u8 + 0x40) as char;
    let c2 = (((mfg_id >> 5) & 0x1F) as u8 + 0x40) as char;
    let c3 = ((mfg_id & 0x1F) as u8 + 0x40) as char;
    let mfg = if c1.is_ascii_uppercase() && c2.is_ascii_uppercase() && c3.is_ascii_uppercase() {
        Some(format!("{}{}{}", c1, c2, c3))
    } else { None };

    // Search descriptor blocks for model name (type 0xFC) and serial string (0xFF)
    let mut model: Option<String> = None;
    let mut serial: Option<String> = None;
    // Detailed timing/descriptor blocks from 54 to 126 in 18-byte chunks
    let mut i = 54usize;
    while i + 18 <= edid.len() {
        let block = &edid[i..i + 18];
        if block[0] == 0 && block[1] == 0 {
            // Descriptor
            match block[3] {
                0xFC => { // Monitor name
                    let text = parse_descriptor_text(&block[5..18]);
                    if !text.is_empty() { model = Some(text); }
                }
                0xFF => { // Serial string
                    let text = parse_descriptor_text(&block[5..18]);
                    if !text.is_empty() { serial = Some(text); }
                }
                _ => {}
            }
        }
        i += 18;
        if i >= 126 { break; }
    }
    Ok((mfg, model, serial))
}

fn parse_descriptor_text(bytes: &[u8]) -> String {
    let mut s: Vec<u8> = bytes.iter().copied().take_while(|&b| b != 0x0A && b != 0x00).collect();
    // Trim trailing spaces
    while let Some(b) = s.last() { if *b == b' ' { s.pop(); } else { break; } }
    String::from_utf8_lossy(&s).trim().to_string()
}
