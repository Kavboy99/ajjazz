//! Source-confirmed 64-slot DKS bank (0x18/0x28) and its keymap references.
use crate::{
    hardware::Snapshot,
    keymap::{self, Assignment, KeymapEdit},
    model::keyboard_rows,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DksConfig {
    /// Tenths of a millimetre: down1, down2, up1, up2.
    pub points: [u8; 4],
    pub actions: [u8; 4],
    /// [action][stage]: 0 off, 1 tap, 2 hold.
    pub states: [[u8; 4]; 4],
}
impl DksConfig {
    pub fn for_usage(usage: u8) -> Self {
        Self {
            points: [16, 30, 30, 16],
            actions: [usage, 0, 0, 0],
            states: [[1, 0, 0, 0], [0; 4], [0; 4], [0; 4]],
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.points.iter().all(|p| (1..=33).contains(p))
                && self.points[0] < self.points[1]
                && self.points[3] < self.points[2],
            "DKS: ход 0,1–3,3 мм; вторая точка нажатия глубже первой, точки отпускания идут обратно"
        );
        let usages: BTreeSet<_> = keyboard_rows()
            .into_iter()
            .flatten()
            .map(|k| k.hid)
            .collect();
        let mut active = false;
        for action in 0..4 {
            ensure!(
                usages.contains(&self.actions[action]),
                "Unknown DKS key usage"
            );
            ensure!(
                self.states[action].iter().all(|s| *s <= 2),
                "Invalid DKS action state"
            );
            if self.actions[action] == 0 {
                ensure!(
                    self.states[action] == [0; 4],
                    "Сначала выберите действие DKS"
                );
            } else if self.states[action].iter().any(|s| *s != 0) {
                active = true;
            }
        }
        ensure!(active, "DKS: включите хотя бы одно действие на одном этапе");
        Ok(())
    }
    pub fn patch(&self, original: &[u8]) -> Result<Vec<u8>> {
        self.validate()?;
        ensure!(original.len() == 16, "Invalid DKS record");
        ensure!(
            [4, 6, 8, 10].iter().all(|i| original[*i] == 0),
            "Неизвестный формат действия DKS; запись защищена"
        );
        let mut out = original.to_vec();
        out[..4].copy_from_slice(&self.points);
        for action in 0..4 {
            out[5 + action * 2] = self.actions[action];
        }
        for stage in 0..4 {
            out[12 + stage] = 0;
            for action in 0..4 {
                out[12 + stage] |= match self.states[action][stage] {
                    1 => 1 << action,
                    2 => 1 << (action + 4),
                    _ => 0,
                };
            }
        }
        Ok(out)
    }
    pub fn decode(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() == 16, "Invalid DKS record");
        ensure!(
            [4, 6, 8, 10].iter().all(|i| raw[*i] == 0),
            "Неизвестный формат действия DKS"
        );
        let mut out = Self {
            points: raw[..4].try_into()?,
            actions: [raw[5], raw[7], raw[9], raw[11]],
            states: [[0; 4]; 4],
        };
        for stage in 0..4 {
            for action in 0..4 {
                let tap = raw[12 + stage] & (1 << action) != 0;
                let hold = raw[12 + stage] & (1 << (action + 4)) != 0;
                ensure!(!(tap && hold), "Unknown overlapping DKS state");
                out.states[action][stage] = if tap {
                    1
                } else if hold {
                    2
                } else {
                    0
                };
            }
        }
        out.validate()?;
        Ok(out)
    }
}

fn blocks(snapshot: &Snapshot) -> Result<(&[u8], &[u8])> {
    let map = snapshot.blocks.get("keymap").context("No keymap")?;
    let bank = snapshot.blocks.get("dks").context("No DKS bank")?;
    ensure!(
        map.len() == 512 && bank.len() == 1024,
        "Invalid DKS snapshot"
    );
    Ok((map, bank))
}
pub fn for_key(snapshot: &Snapshot, key: u8) -> Result<DksConfig> {
    let (map, bank) = blocks(snapshot)?;
    let physical = keyboard_rows()
        .into_iter()
        .flatten()
        .find(|k| k.id == key && k.hid != 0)
        .context("Unknown DKS key")?;
    if let Some(Assignment::Dks { slot }) =
        Assignment::decode(&map[usize::from(key) * 4..usize::from(key) * 4 + 4])
    {
        DksConfig::decode(&bank[usize::from(slot) * 16..usize::from(slot) * 16 + 16])
    } else {
        Ok(DksConfig::for_usage(physical.hid))
    }
}

pub fn target(snapshot: &Snapshot, key: u8, settings: &DksConfig) -> Result<(Vec<u8>, Vec<u8>)> {
    settings.validate()?;
    let (map, bank) = blocks(snapshot)?;
    // This also rejects Fn, unknown assignments and half of an RS/SOCD pair.
    let mut next_map = keymap::target(
        snapshot,
        &KeymapEdit {
            keys: BTreeSet::from([key]),
            assignment: Assignment::Default,
        },
    )?;
    let offset = usize::from(key) * 4;
    let existing = match Assignment::decode(&map[offset..offset + 4]) {
        Some(Assignment::Dks { slot }) => Some(slot),
        _ => None,
    };
    let slot = existing.filter(|slot| map.as_chunks::<4>().0.iter().filter(|r| r.as_slice()==[8,*slot,0,0]).count()==1)
        .or_else(|| (0..64u8).find(|slot| {
            bank[usize::from(*slot)*16..usize::from(*slot)*16+16].iter().all(|b| *b==0)
                && !map.as_chunks::<4>().0.iter().any(|r| r.as_slice()==[8,*slot,0,0])
        })).context("Нет свободной ячейки DKS; чужие и неиспользуемые сохранённые записи не перезаписываются")?;
    let at = usize::from(slot) * 16;
    let mut next_bank = bank.to_vec();
    next_bank[at..at + 16].copy_from_slice(&settings.patch(&bank[at..at + 16])?);
    next_map[offset..offset + 4].copy_from_slice(&Assignment::Dks { slot }.encode()?);
    Ok((next_bank, next_map))
}

pub fn validate_references(snapshot: &Snapshot) -> Result<()> {
    let (map, bank) = blocks(snapshot)?;
    for key in keyboard_rows().into_iter().flatten().filter(|k| k.hid != 0) {
        let record = &map[usize::from(key.id) * 4..usize::from(key.id) * 4 + 4];
        if record[0] == 8 {
            ensure!(
                record[1] < 64 && record[2..] == [0, 0],
                "Invalid DKS reference"
            );
            let at = usize::from(record[1]) * 16;
            DksConfig::decode(&bank[at..at + 16])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dks_vendor_bitfields_roundtrip_and_invalid_points_are_rejected() {
        let settings = DksConfig {
            points: [16, 30, 30, 16],
            actions: [4, 7, 22, 26],
            states: [[1, 2, 0, 0], [0, 1, 2, 0], [0, 0, 1, 2], [2, 0, 0, 1]],
        };
        let mut raw = vec![0; 16];
        let encoded = settings.patch(&raw).unwrap();
        assert_eq!(
            encoded,
            [16, 30, 30, 16, 0, 4, 0, 7, 0, 22, 0, 26, 129, 18, 36, 72]
        );
        assert_eq!(DksConfig::decode(&encoded).unwrap(), settings);
        let mut bad = settings.clone();
        bad.points[0] = 31;
        assert!(bad.patch(&raw).is_err());
        bad = settings.clone();
        bad.actions[0] = 0;
        assert!(bad.patch(&raw).is_err());
        let mut overlapping = encoded;
        overlapping[12] |= 16;
        assert!(DksConfig::decode(&overlapping).is_err());
        raw[4] = 0x7e;
        assert!(settings.patch(&raw).is_err());
    }
}
