//! Bounded legacy RGB probe derived from the unpacked vendor MusicPlug binary.
#[cfg(windows)]
use crate::hardware::LightingIo;
use anyhow::{Result, ensure};
use std::path::Path;

fn marker_packets(rgb: &[u8]) -> Result<Vec<[u8; 32]>> {
    ensure!(rgb.len() == 512, "Invalid RGB matrix");
    let mut bytes = rgb.to_vec();
    bytes.extend_from_slice(&[0xaa, 0x55]);
    Ok(bytes
        .chunks(24)
        .enumerate()
        .map(|(index, chunk)| {
            let offset = index * 24;
            let mut packet = [0; 32];
            packet[0] = 0xaa;
            packet[1] = 0x32;
            packet[2] = rgb.len().saturating_sub(offset).min(24) as u8;
            packet[3..5].copy_from_slice(&(offset as u16).to_le_bytes());
            packet[8..8 + chunk.len()].copy_from_slice(chunk);
            packet
        })
        .collect())
}
fn music_packet(level: u8) -> [u8; 32] {
    let mut packet = [0; 32];
    packet[..6].copy_from_slice(&[0xaa, 0x35, 0, 100, 0, 0]);
    packet[6..27].fill(level);
    packet[31] = packet[..31]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    packet
}
fn usb_packets(rgb: &[u8], legacy: bool) -> Result<Vec<Vec<u8>>> {
    ensure!(rgb.len() == 512, "Invalid USB RGB matrix");
    if !legacy {
        return crate::protocol::keyboard_packets(0x32, 0, 512, Some(rgb), 64);
    }
    let mut bytes = rgb.to_vec();
    bytes.extend_from_slice(&[0xaa, 0x55]);
    Ok(bytes
        .chunks(56)
        .enumerate()
        .map(|(index, chunk)| {
            let offset = index * 56;
            let mut packet = vec![0; 64];
            packet[0] = 0xaa;
            packet[1] = 0x32;
            packet[2] = rgb.len().saturating_sub(offset).min(56) as u8;
            packet[3..5].copy_from_slice(&(offset as u16).to_le_bytes());
            packet[8..8 + chunk.len()].copy_from_slice(chunk);
            packet
        })
        .collect())
}
/// Reproduce Lighting Lab's proven path: mode 128 followed by full 0x24 tables.
/// This diagnostic is bounded because persistence/endurance of 0x24 is unknown.
pub fn diagnose_table_animation(usb: bool, output: &Path) -> Result<serde_json::Value> {
    #[cfg(windows)]
    {
        let device = if usb {
            crate::hardware::Keyboard::open_usb()?
        } else {
            crate::hardware::Keyboard::open()?
        };
        let before = device.snapshot()?;
        crate::hardware::supported_keyboard_identity(&before.info)?;
        ensure!(
            before.info.work_mode == if usb { 15 } else { 2 },
            "Unexpected connection mode"
        );
        before.save(output)?;
        let mut probes = Vec::new();
        let tested = (|| -> Result<()> {
            let mut effect = before.blocks["lighting"].clone();
            effect[0] = 128;
            device.write_effect(&effect)?;
            let selected_mode = device.read_area(crate::protocol::KeyboardArea::Lighting)?;
            println!(
                "Lighting Lab sequence: mode 128 requested, mode {} read; 8 red/blue tables, 5 seconds each",
                selected_mode[0]
            );
            for frame in 0..8 {
                let color = if frame % 2 == 0 {
                    [255, 0, 0]
                } else {
                    [0, 0, 255]
                };
                let mut rgb = before.blocks["custom_rgb"].clone();
                for (index, record) in rgb.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    record[0] = index as u8;
                    record[1..].copy_from_slice(&color);
                }
                device.write_rgb(&rgb)?;
                let readback = device.read_area(crate::protocol::KeyboardArea::CustomRgb)?;
                ensure!(
                    readback == rgb,
                    "Frame {frame}: 0x14 readback differs from 0x24 table"
                );
                println!("Frame {frame}: {color:?}, table readback matches");
                probes.push(serde_json::json!({"frame":frame,"color":color,"table_matches":true}));
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Ok(())
        })();
        let restored = (|| -> Result<()> {
            let info = device.read_info()?;
            ensure!(
                info.profile == before.info.profile,
                "Profile changed; restoration stopped"
            );
            let rgb_error = device.write_rgb(&before.blocks["custom_rgb"]).err();
            let effect_error = device.write_effect(&before.blocks["lighting"]).err();
            let after = device.snapshot()?;
            ensure!(
                after.info.profile == before.info.profile && after.blocks == before.blocks,
                "Original configuration differs: RGB={rgb_error:?}, mode={effect_error:?}"
            );
            after.save(&output.with_extension("restored.json"))?;
            Ok(())
        })();
        let record = serde_json::json!({"channel":if usb {"USB"} else {"2.4G"},"format":"Lighting Lab mode 128, command 0x24","frames":probes,"visual_confirmation":null,"test_error":tested.as_ref().err().map(|e|format!("{e:#}")),"restored":restored.is_ok(),"restore_error":restored.as_ref().err().map(|e|format!("{e:#}"))});
        crate::storage::write_json_atomic(&output.with_extension("result.json"), &record)?;
        restored?;
        tested?;
        Ok(record)
    }
    #[cfg(not(windows))]
    {
        let _ = (usb, output);
        anyhow::bail!("Windows only")
    }
}
pub fn diagnose_usb(legacy: bool, output: &Path) -> Result<serde_json::Value> {
    #[cfg(windows)]
    {
        let device = crate::hardware::Keyboard::open_usb()?;
        let before = device.snapshot()?;
        crate::hardware::supported_keyboard_identity(&before.info)?;
        ensure!(before.info.work_mode == 15, "Unknown wired work mode");
        before.save(output)?;
        let original = before.blocks["custom_rgb"].clone();
        let original_live = device.usb_live_rgb()?;
        let mut probes = Vec::new();
        let tested = (|| -> Result<()> {
            // Earlier probes never selected the vendor's custom SET mode.
            // Read-back mode 20 is not the SET value; write_effect maps it to128.
            device.write_effect(&crate::protocol::static_lighting(
                &before.blocks["lighting"],
            )?)?;
            ensure!(
                device.read_area(crate::protocol::KeyboardArea::Lighting)?[0] == 20,
                "Custom mode was not selected before temporary RGB test"
            );
            println!("USB red/blue temporary frames running for approximately 50 seconds");
            for frame in 0..40 {
                ensure!(
                    device.read_info()?.profile == before.info.profile,
                    "Profile changed; temporary RGB test stopped"
                );
                let color = if frame % 2 == 0 {
                    [0, 0, 255]
                } else {
                    [255, 0, 0]
                };
                let mut rgb = original.clone();
                for key in crate::model::keyboard_rows().into_iter().flatten() {
                    let offset = usize::from(key.id) * 4;
                    rgb[offset + 1..offset + 4].copy_from_slice(&color);
                }
                for packet in usb_packets(&rgb, legacy)? {
                    device.temporary_exchange(&packet)?;
                }
                let current = device.usb_live_rgb()?;
                if frame < 2 {
                    probes.push(serde_json::json!({"requested":color,"live_rgb":current,"changed":current!=original_live}));
                }
                std::thread::sleep(std::time::Duration::from_millis(1000));
            }
            Ok(())
        })();
        let restored = (|| -> Result<()> {
            ensure!(
                device.read_info()?.profile == before.info.profile,
                "Profile changed; restoration stopped"
            );
            for packet in usb_packets(&original, legacy)? {
                let _ = device.temporary_exchange(&packet);
            }
            let write = device.write_effect(&before.blocks["lighting"]);
            let after = device.snapshot()?;
            ensure!(
                after.info.profile == before.info.profile && after.blocks == before.blocks,
                "Исходные настройки USB не совпали: {write:?}"
            );
            after.save(&output.with_extension("restored.json"))?;
            Ok(())
        })();
        let record = serde_json::json!({"channel":"USB FF68","legacy_marker":legacy,"probes":probes,"original_live_rgb":original_live,"test_error":tested.as_ref().err().map(|e|format!("{e:#}")),"restored":restored.is_ok(),"restore_error":restored.as_ref().err().map(|e|format!("{e:#}"))});
        crate::storage::write_json_atomic(&output.with_extension("result.json"), &record)?;
        restored?;
        tested?;
        Ok(record)
    }
    #[cfg(not(windows))]
    {
        let _ = (legacy, output);
        anyhow::bail!("Windows only")
    }
}
pub fn diagnose_music(output: &Path) -> Result<serde_json::Value> {
    #[cfg(windows)]
    {
        let device = crate::hardware::Keyboard::open()?;
        let before = device.snapshot()?;
        crate::hardware::supported_lighting_revision(&before.info)?;
        before.save(output)?;
        let mut replies = Vec::new();
        let tested = (|| -> Result<()> {
            println!("Legacy 0x35 checksum test running for 40 seconds");
            for frame in 0..40 {
                let packet = music_packet(if frame % 2 == 0 { 7 } else { 0 });
                let reply = device.temporary_exchange(&packet)?;
                if frame < 2 {
                    replies.push(reply);
                }
                std::thread::sleep(std::time::Duration::from_millis(950));
            }
            Ok(())
        })();
        let restored = (|| -> Result<()> {
            let write = device.write_effect(&before.blocks["lighting"]);
            let after = device.snapshot()?;
            ensure!(
                after.info.profile == before.info.profile && after.blocks == before.blocks,
                "Возврат после 0x35 не подтверждён: {write:?}"
            );
            after.save(&output.with_extension("restored.json"))?;
            Ok(())
        })();
        let record = serde_json::json!({"format":"legacy 0x35 amplitude with additive checksum","replies":replies,"test_error":tested.as_ref().err().map(|e|format!("{e:#}")),"restored":restored.is_ok(),"restore_error":restored.as_ref().err().map(|e|format!("{e:#}"))});
        crate::storage::write_json_atomic(&output.with_extension("result.json"), &record)?;
        restored?;
        tested?;
        Ok(record)
    }
    #[cfg(not(windows))]
    {
        let _ = output;
        anyhow::bail!("Windows only")
    }
}
#[cfg(windows)]
fn current_rgb(device: &crate::hardware::Keyboard) -> Result<Vec<u8>> {
    let mut colors = Vec::new();
    for index in 0..6 {
        let mut packet = [0; 32];
        packet[..3].copy_from_slice(&[0xaa, 0x37, index]);
        let reply = device.temporary_exchange(&packet)?;
        colors.extend_from_slice(&reply[4..]);
    }
    colors.truncate(164);
    Ok(colors)
}
pub fn diagnose_marker(output: &Path) -> Result<serde_json::Value> {
    #[cfg(windows)]
    {
        let device = crate::hardware::Keyboard::open()?;
        let before = device.snapshot()?;
        crate::hardware::supported_lighting_revision(&before.info)?;
        ensure!(
            before.blocks["lighting"][0] == 20,
            "Для диагностики нужен сохранённый статический кадр (режим 20)"
        );
        before.save(output)?;
        let original = before.blocks["custom_rgb"].clone();
        let baseline_rgb = current_rgb(&device)?;
        let mut probes = Vec::new();
        let tested = (|| -> Result<()> {
            for color in [[0, 0, 255], [255, 0, 0]] {
                let mut rgb = original.clone();
                for key in crate::model::keyboard_rows().into_iter().flatten() {
                    let offset = usize::from(key.id) * 4;
                    rgb[offset + 1..offset + 4].copy_from_slice(&color);
                }
                for packet in marker_packets(&rgb)? {
                    device.temporary_exchange(&packet)?;
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
                let current = current_rgb(&device)?;
                probes.push(serde_json::json!({"requested":color,"rgb565":current,"differs_from_original":current!=baseline_rgb}));
            }
            Ok(())
        })();
        // Reapply original RAM colors, then the known saved effect. Never stream 0x24.
        let restored = (|| -> Result<()> {
            let mut ram_error = None;
            for packet in marker_packets(&original)? {
                if let Err(error) = device.temporary_exchange(&packet) {
                    ram_error = Some(error);
                    break;
                }
            }
            let effect_error = device.write_effect(&before.blocks["lighting"]).err();
            let after = device.snapshot()?;
            ensure!(
                after.info.profile == before.info.profile && after.blocks == before.blocks,
                "Исходные настройки после теста отличаются"
            );
            ensure!(
                current_rgb(&device)? == baseline_rgb,
                "Исходный RGB после возврата не совпал: RAM={ram_error:?}, mode={effect_error:?}"
            );
            after.save(&output.with_extension("restored.json"))?;
            Ok(())
        })();
        let record = serde_json::json!({"format":"legacy 0x32, RGB matrix + AA55, no final flag","original_rgb565":baseline_rgb,"probes":probes,"test_error":tested.as_ref().err().map(|e|format!("{e:#}")),"all_configuration_restored":restored.is_ok(),"restore_error":restored.as_ref().err().map(|e|format!("{e:#}"))});
        crate::storage::write_json_atomic(&output.with_extension("result.json"), &record)?;
        restored?;
        tested?;
        Ok(record)
    }
    #[cfg(not(windows))]
    {
        let _ = output;
        anyhow::bail!("Windows only")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_marker_is_transmitted_but_excluded_from_length() {
        let rgb = vec![123; 512];
        let packets = marker_packets(&rgb).unwrap();
        assert_eq!(packets.len(), 22);
        let last = packets.last().unwrap();
        assert_eq!(&last[..8], &[0xaa, 0x32, 8, 248, 1, 0, 0, 0]);
        assert_eq!(
            &last[8..18],
            &[123, 123, 123, 123, 123, 123, 123, 123, 0xaa, 0x55]
        );
        assert!(marker_packets(&rgb[..511]).is_err());
    }
    #[test]
    fn legacy_music_uses_amplitudes_and_additive_checksum() {
        let packet = music_packet(7);
        assert_eq!(&packet[..6], &[0xaa, 0x35, 0, 100, 0, 0]);
        assert!(packet[6..27].iter().all(|&level| level == 7));
        assert_eq!(packet[31], 0xd6);
    }
    #[test]
    fn usb_modern_and_legacy_framing_are_separate() {
        let modern = usb_packets(&vec![123; 512], false).unwrap();
        assert_eq!(modern.last().unwrap()[6], 1);
        let legacy = usb_packets(&vec![123; 512], true).unwrap();
        let last = legacy.last().unwrap();
        assert_eq!(&last[..8], &[0xaa, 0x32, 8, 248, 1, 0, 0, 0]);
        assert_eq!(&last[16..18], &[0xaa, 0x55]);
    }
}
