//! Vendor legacy Hall calibration (64/65, FB reports), not firmware programming.
//! Never commit incomplete coverage: native65 substitutes defaults for untouched keys.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::mpsc};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub id: u8,
    pub status: u8,
    pub maximum: u16,
    pub minimum: u16,
    pub current: u16,
    pub stroke: u16,
}
pub fn sample(raw: &[u8]) -> Option<Sample> {
    let p = if raw.len() == 65 && raw[0] == 0 {
        &raw[1..]
    } else {
        raw
    };
    if p.len() < 14 || p[..2] != [0x55, 0xfb] || p[2] >= 128 || p[3] > 1 {
        return None;
    }
    let w = |i| u16::from_le_bytes([p[i], p[i + 1]]);
    let s = Sample {
        id: p[2],
        status: p[3],
        maximum: w(4),
        minimum: w(6) & 0x7fff,
        current: w(8),
        stroke: w(10),
    };
    if s.maximum > 4095 || s.minimum > 4095 || s.current > 4095 {
        return None;
    }
    Some(s)
}
pub fn command(op: u8) -> Result<Vec<u8>> {
    ensure!([0x64, 0x65].contains(&op), "Неверная команда калибровки");
    let mut p = vec![0; 64];
    p[0] = 0xaa;
    p[1] = op;
    p[6] = 1;
    Ok(p)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Progress {
    pub completed: BTreeSet<u8>,
    pub last: Option<Sample>,
}
impl Progress {
    pub fn observe(&mut self, s: Sample) -> bool {
        let physical = crate::model::keyboard_rows()
            .into_iter()
            .flatten()
            .any(|k| k.id == s.id);
        let changed = physical
            && s.status == 1
            && s.maximum > s.minimum.saturating_add(16)
            && self.completed.insert(s.id);
        self.last = Some(s);
        changed
    }
    pub fn missing(&self) -> Vec<u8> {
        crate::model::keyboard_rows()
            .into_iter()
            .flatten()
            .map(|k| k.id)
            .filter(|id| !self.completed.contains(id))
            .collect()
    }
    pub fn may_commit(&self) -> bool {
        self.missing().is_empty()
    }
}
pub enum Command {
    Finish,
    Cancel,
}
#[derive(Clone, Debug)]
pub enum Update {
    Started,
    Progress(Progress),
    Notice(String),
}

#[cfg(windows)]
pub fn run(receiver: mpsc::Receiver<Command>, mut notify: impl FnMut(Update)) -> Result<Progress> {
    let keyboard = crate::hardware::Keyboard::open_usb()?;
    let info = keyboard.read_info()?;
    crate::hardware::supported_lighting_revision(&info)?;
    ensure!(
        info.version == "1.36" && info.frame_version == 0,
        "Мастер поддерживает проводную AJKS1.36 с legacy Hall-протоколом"
    );
    let mut seq = 0u8;
    let mut ajks = |op, p: &[u8]| -> Result<Vec<u8>> {
        seq = seq.wrapping_add(1);
        keyboard.memory_exchange(&crate::keyboard_memory::request(op, seq, p)?)
    };
    let cap = ajks(0, &[])?;
    ensure!(&cap[..6] == b"AJKS\x01\x08", "Нет подтверждения AJKS");
    let active = cap[6];
    let before = keyboard.snapshot()?;
    let backup = crate::storage::backup(
        &crate::storage::backup_directory()?,
        "ak820max-before-calibration",
        &before,
    )?;
    notify(Update::Notice(format!(
        "Копия настроек: {}",
        backup.display()
    )));
    ajks(5, &[255])?;
    if let Err(error) = keyboard.calibration_control(0x64) {
        let _ = ajks(5, &[active]);
        return Err(error);
    }
    notify(Update::Started);
    let mut progress = Progress::default();
    loop {
        match receiver.try_recv() {
            Ok(Command::Finish) => {
                if !progress.may_commit() {
                    notify(Update::Notice(format!(
                        "Ещё не пройдены {} клавиш; запись не выполнена",
                        progress.missing().len()
                    )));
                    continue;
                }
                keyboard.calibration_control(0x65)?;
                ajks(5, &[active])?;
                let after = keyboard.snapshot()?;
                for block in ["keymap", "rapid_trigger", "dks", "game_mode", "custom_rgb"] {
                    ensure!(
                        before.blocks.get(block) == after.blocks.get(block),
                        "Калибровка неожиданно изменила {block}; копия {}",
                        backup.display()
                    );
                }
                crate::storage::backup(
                    &crate::storage::backup_directory()?,
                    "ak820max-calibration-complete",
                    &progress,
                )?;
                return Ok(progress);
            }
            Ok(Command::Cancel) | Err(mpsc::TryRecvError::Disconnected) => {
                // No documented non-writing legacy cancel. Never send65 here.
                anyhow::bail!(
                    "Калибровка прервана без записи. Переподключите USB клавиатуры, чтобы выйти из режима калибровки"
                );
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let report = keyboard.calibration_read()?;
        if let Some(s) = sample(&report)
            && progress.observe(s)
        {
            notify(Update::Progress(progress.clone()));
        }
    }
}
#[cfg(not(windows))]
pub fn run(_receiver: mpsc::Receiver<Command>, _notify: impl FnMut(Update)) -> Result<Progress> {
    anyhow::bail!("Windows only")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(id: u8, status: u8) -> Vec<u8> {
        let mut p = vec![0; 64];
        p[..4].copy_from_slice(&[0x55, 0xfb, id, status]);
        p[4..6].copy_from_slice(&2400u16.to_le_bytes());
        p[6..8].copy_from_slice(&0x8708u16.to_le_bytes());
        p[8..10].copy_from_slice(&1900u16.to_le_bytes());
        p
    }
    #[test]
    fn vendor_report_masks_calibration_marker_and_validates_framing() {
        let p = packet(34, 1);
        let s = sample(&p).unwrap();
        assert_eq!(s.minimum, 1800);
        assert_eq!(s.maximum, 2400);
        assert_eq!(s.id, 34);
        assert!(sample(&p[..13]).is_none());
        let mut q = p;
        q[1] = 0x65;
        assert!(sample(&q).is_none());
    }
    #[test]
    fn only_complete_physical_coverage_can_commit() {
        let mut p = Progress::default();
        for k in crate::model::keyboard_rows().into_iter().flatten() {
            assert!(!p.may_commit());
            p.observe(sample(&packet(k.id, 1)).unwrap());
        }
        assert_eq!(p.completed.len(), 82);
        assert!(p.may_commit());
    }
    #[test]
    fn repeats_nonphysical_and_incomplete_press_do_not_complete() {
        let mut p = Progress::default();
        assert!(!p.observe(sample(&packet(14, 1)).unwrap()));
        assert!(!p.observe(sample(&packet(34, 0)).unwrap()));
        assert!(p.observe(sample(&packet(34, 1)).unwrap()));
        assert!(!p.observe(sample(&packet(34, 1)).unwrap()));
        assert_eq!(p.completed.len(), 1);
        assert!(!p.may_commit());
        assert!(command(0x6f).is_err());
        assert_eq!(
            &command(0x64).unwrap()[..8],
            &[0xaa, 0x64, 0, 0, 0, 0, 1, 0]
        );
    }
}
