//! Offline inspection only. A plausible vector table does not identify an MCU.
use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct VectorCandidate {
    pub offset: usize,
    pub stack: u32,
    pub reset: u32,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub bytes: usize,
    pub sha256: String,
    pub entropy: f64,
    pub container: String,
    pub vectors: Vec<VectorCandidate>,
    pub strings: Vec<String>,
    pub flashable: bool,
    pub note: String,
}
pub fn inspect(path: &Path) -> Result<Inspection> {
    ensure!(
        path.metadata()?.len() <= 64 * 1024 * 1024,
        "Файл больше 64 МБ"
    );
    inspect_bytes(&std::fs::read(path)?)
}
pub fn inspect_bytes(data: &[u8]) -> Result<Inspection> {
    ensure!(!data.is_empty(), "Пустой файл");
    let mut counts = [0usize; 256];
    for &b in data {
        counts[usize::from(b)] += 1;
    }
    let entropy = counts
        .iter()
        .filter(|&&n| n > 0)
        .map(|&n| {
            let p = n as f64 / data.len() as f64;
            -p * p.log2()
        })
        .sum();
    let text_prefix = data
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(data)
        .trim_ascii_start();
    let html = text_prefix[..text_prefix.len().min(256)].to_ascii_lowercase();
    let is_html = html.starts_with(b"<!doctype html") || html.starts_with(b"<html");
    let container = if is_html {
        "HTML page, not firmware or archive"
    } else if data.starts_with(b"Rar!\x1a\x07") {
        "RAR container"
    } else if data.starts_with(b"7z\xbc\xaf\x27\x1c") {
        "7z container"
    } else if data.starts_with(b"MZ") {
        "Windows executable / updater"
    } else if data.starts_with(b"PK\x03\x04") {
        "ZIP container"
    } else if data.starts_with(b"\x7fELF") {
        "ELF executable"
    } else if data.starts_with(b":") {
        "Possible Intel HEX text"
    } else {
        "Unknown / raw binary"
    }
    .to_string();
    let mut vectors = Vec::new();
    for offset in (0..data.len().saturating_sub(7).min(65536)).step_by(4) {
        let stack = u32::from_le_bytes(data[offset..offset + 4].try_into()?);
        let reset = u32::from_le_bytes(data[offset + 4..offset + 8].try_into()?);
        if (0x20000000..0x20100000).contains(&stack)
            && stack % 4 == 0
            && reset & 1 == 1
            && (0x08000000..0x08200000).contains(&(reset & !1))
        {
            vectors.push(VectorCandidate {
                offset,
                stack,
                reset,
            });
            if vectors.len() == 32 {
                break;
            }
        }
    }
    let strings = data
        .split(|b| !(32..127).contains(b))
        .filter(|s| s.len() >= 7 && s.len() <= 160)
        .take(80)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Ok(Inspection{bytes:data.len(),sha256:format!("{:x}",Sha256::digest(data)),entropy,container,vectors,strings,flashable:false,note:if is_html {"Вместо прошивки скачана веб-страница. Имя файла или расширение RAR не меняют его содержимое."} else {"Эвристики не подтверждают контроллер, адреса загрузки, подпись и совместимость. Функции записи прошивки нет."}.into()})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updater_is_not_treated_as_raw_firmware() {
        let i = inspect_bytes(b"MZexample updater").unwrap();
        assert!(i.container.contains("updater"));
        assert!(!i.flashable);
    }
    #[test]
    fn empty_image_is_rejected() {
        assert!(inspect_bytes(&[]).is_err());
    }
    #[test]
    fn browser_challenge_cannot_be_mistaken_for_a_firmware_container() {
        let report = inspect_bytes(
            b"\xef\xbb\xbf \r\n<!DOCTYPE HTML><html><title>One moment</title></html>",
        )
        .unwrap();
        assert!(report.container.starts_with("HTML"));
        assert!(!report.flashable);
        assert!(report.vectors.is_empty());
        assert_eq!(
            inspect_bytes(b"Rar!\x1a\x07\x01\x00").unwrap().container,
            "RAR container"
        );
    }
}
