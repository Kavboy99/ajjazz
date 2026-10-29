//! Read-only identification for the exact AJ159 APEX mouse/dock pair.
//! Device IDs, version words and an updater UI do not establish flash compatibility.
use anyhow::{Result, ensure};
use serde::Serialize;

pub const MOUSE_ID: u32 = 1755;
pub const DOCK_ID: u32 = 1759;
pub const RADIO_MOUSE_ID: u32 = 0x800006db;

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub captured_utc: String,
    pub source: String,
    pub dock_oled_version: u16,
    pub dock_flash_version: u16,
    pub service_mouse_id: u32,
    pub mouse_id_verified: bool,
    pub mouse_version: Option<u16>,
    pub mouse_read_error: Option<String>,
    pub raw_dock: Vec<u8>,
    pub raw_mouse_identity: Option<Vec<u8>>,
    pub raw_mouse_version: Option<Vec<u8>>,
    pub firmware_programming_available: bool,
}

pub(crate) fn version_word(command: u8, raw: &[u8]) -> Result<u16> {
    ensure!(
        raw.len() == 64 && raw[0] == command,
        "Неверный ответ версии"
    );
    let version = u16::from_le_bytes([raw[1], raw[2]]);
    ensure!(
        version != 0 && version != u16::MAX,
        "Версия не получена: пустой ответ"
    );
    Ok(version)
}

pub(crate) fn verify_mouse(raw: &[u8]) -> Result<()> {
    ensure!(raw.len() == 64 && raw[0] == 0x8f, "Неверный ответ ID мыши");
    ensure!(
        u32::from_le_bytes(raw[1..5].try_into()?) == MOUSE_ID,
        "Другая ревизия мыши; требуется ID1755"
    );
    Ok(())
}

pub fn read_via_vendor_service() -> Result<Identity> {
    crate::local_bridge::read_firmware_identity()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_apex_label_cannot_replace_exact_mouse_identity() {
        let mut raw = [0; 64];
        raw[0] = 0x8f;
        raw[1..5].copy_from_slice(&MOUSE_ID.to_le_bytes());
        assert!(verify_mouse(&raw).is_ok());
        raw[1..5].copy_from_slice(&2349u32.to_le_bytes());
        assert!(verify_mouse(&raw).is_err());
        raw[1..5].copy_from_slice(&RADIO_MOUSE_ID.to_le_bytes());
        assert!(verify_mouse(&raw).is_err());
        assert!(verify_mouse(&raw[..5]).is_err());
    }

    #[test]
    fn version_is_a_raw_little_endian_word_not_a_chip_identity() {
        let mut raw = [0; 64];
        raw[..3].copy_from_slice(&[0xad, 4, 1]);
        assert_eq!(version_word(0xad, &raw).unwrap(), 0x104);
        assert!(version_word(0x80, &raw).is_err());
        assert!(version_word(0xad, &raw[..63]).is_err());
        raw[1..3].fill(0);
        assert!(version_word(0xad, &raw).is_err());
        raw[1..3].fill(255);
        assert!(version_word(0xad, &raw).is_err());
    }
}
