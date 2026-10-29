//! Keyboard macro configuration; vendor 0x15/0x25 bank, not MCU flash.
use crate::{
    hardware::{self, Snapshot},
    keymap::{self, Assignment, KeymapEdit},
    storage,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub const INDEX_BYTES: usize = 400;
pub const DATA_BYTES: usize = 512;
pub const BANK_BYTES: usize = INDEX_BYTES + DATA_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub delay_ms: u16,
    pub usage: u8,
    pub pressed: bool,
    pub mouse: bool,
}
impl Action {
    fn decode(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() == 4, "Invalid macro action");
        ensure!(
            [0x90, 0x10, 0xb0, 0x30].contains(&raw[3]),
            "Unknown macro action flags"
        );
        Ok(Self {
            delay_ms: u16::from_le_bytes([raw[0], raw[1]]),
            usage: raw[2],
            pressed: raw[3] & 0x80 != 0,
            mouse: raw[3] & 0x70 == 0x30,
        })
    }
    fn encode(&self) -> [u8; 4] {
        let [lo, hi] = self.delay_ms.to_le_bytes();
        [
            lo,
            hi,
            self.usage,
            if self.mouse { 0x30 } else { 0x10 } | if self.pressed { 0x80 } else { 0 },
        ]
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Macro {
    pub slot: u8,
    pub actions: Vec<Action>,
}
impl Macro {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.slot < 100 && !self.actions.is_empty(),
            "Пустой макрос или неверная ячейка"
        );
        let known: BTreeSet<_> = crate::model::keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|k| k.hid != 0)
            .map(|k| k.hid)
            .collect();
        let mut down = BTreeSet::new();
        for action in &self.actions {
            ensure!(
                if action.mouse {
                    [1, 2, 4, 8, 16].contains(&action.usage)
                } else {
                    known.contains(&action.usage)
                },
                "Неизвестное действие макроса"
            );
            let key = (action.mouse, action.usage);
            ensure!(
                if action.pressed {
                    down.insert(key)
                } else {
                    down.remove(&key)
                },
                "Повторное нажатие или отпускание ненажатой клавиши"
            );
        }
        ensure!(
            down.is_empty(),
            "В конце макроса все нажатые клавиши должны быть отпущены"
        );
        Ok(())
    }
}

pub fn validate_map(map: &[u8], macros: &[Macro]) -> Result<()> {
    ensure!(map.len() == 512, "Invalid keymap");
    for record in map.as_chunks::<4>().0 {
        if record[0] == 6 {
            let Some(Assignment::Macro { slot, .. }) = Assignment::decode(record) else {
                bail!("Неизвестный режим макроса в назначениях");
            };
            ensure!(
                macros.iter().any(|m| m.slot == slot),
                "Макрос {} назначен клавише; сначала снимите назначение",
                slot + 1
            );
        }
    }
    Ok(())
}

pub fn decode(raw: &[u8]) -> Result<Vec<Macro>> {
    ensure!(raw.len() == BANK_BYTES, "Invalid macro bank");
    let mut macros = Vec::new();
    let mut occupied = BTreeSet::new();
    for slot in 0..100u8 {
        let at = usize::from(slot) * 4;
        let address = u32::from_le_bytes(raw[at..at + 4].try_into()?) as usize;
        if address == 0 {
            continue;
        }
        ensure!(
            (INDEX_BYTES..=BANK_BYTES - 4).contains(&address) && address.is_multiple_of(4),
            "Macro pointer outside known bank"
        );
        let units = u16::from_le_bytes([raw[address], raw[address + 1]]) as usize;
        ensure!(
            units > 0 && units.is_multiple_of(2) && raw[address + 2..address + 4] == [0, 0],
            "Unknown macro header"
        );
        let end = address + 4 + units * 2;
        ensure!(end <= BANK_BYTES, "Macro actions outside known bank");
        for byte in address..end {
            ensure!(occupied.insert(byte), "Overlapping macro records");
        }
        let actions = raw[address + 4..end]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|r| Action::decode(r))
            .collect::<Result<Vec<_>>>()?;
        macros.push(Macro { slot, actions });
    }
    Ok(macros)
}

pub fn encode(macros: &[Macro], original: &[u8]) -> Result<Vec<u8>> {
    ensure!(original.len() == BANK_BYTES, "Invalid macro bank");
    let mut raw = original.to_vec();
    raw[..INDEX_BYTES].fill(0);
    let mut offset = INDEX_BYTES;
    let mut slots = BTreeSet::new();
    for item in macros {
        ensure!(
            item.slot < 100 && slots.insert(item.slot),
            "Invalid or duplicate macro slot"
        );
        ensure!(!item.actions.is_empty(), "Empty macro");
        let length = 4 + item.actions.len() * 4;
        ensure!(
            offset + length <= BANK_BYTES,
            "Макросы превышают 512 байт памяти этой ревизии"
        );
        let ptr = usize::from(item.slot) * 4;
        raw[ptr..ptr + 4].copy_from_slice(&(offset as u32).to_le_bytes());
        raw[offset..offset + 2].copy_from_slice(&((item.actions.len() * 2) as u16).to_le_bytes());
        raw[offset + 2..offset + 4].fill(0);
        for (i, action) in item.actions.iter().enumerate() {
            raw[offset + 4 + i * 4..offset + 8 + i * 4].copy_from_slice(&action.encode());
        }
        offset += length;
    }
    Ok(raw)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacroSnapshot {
    pub schema: u32,
    pub configuration: Snapshot,
    pub raw: Vec<u8>,
    pub macros: Vec<Macro>,
}
impl MacroSnapshot {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.configuration.schema == 1,
            "Unsupported macro snapshot"
        );
        hardware::supported_lighting_revision(&self.configuration.info)?;
        self.configuration.is_wired()?;
        ensure!(
            decode(&self.raw)? == self.macros,
            "Macro snapshot contains inconsistent data"
        );
        for &(_, len, name) in crate::protocol::READ_BLOCKS.iter().skip(1) {
            ensure!(
                self.configuration
                    .blocks
                    .get(name)
                    .is_some_and(|b| b.len() == len),
                "Invalid {name} block"
            );
        }
        Ok(())
    }
}
fn same_identity(a: &MacroSnapshot, b: &MacroSnapshot) -> bool {
    a.configuration.endpoint.path == b.configuration.endpoint.path
        && a.configuration.info.profile == b.configuration.info.profile
}
fn equivalent(a: &MacroSnapshot, b: &MacroSnapshot) -> bool {
    same_identity(a, b) && a.raw == b.raw && a.configuration.blocks == b.configuration.blocks
}
trait MacroIo {
    fn read(&self) -> Result<MacroSnapshot>;
    fn configuration(&self) -> Result<Snapshot>;
    fn write_bank(&self, raw: &[u8]) -> Result<()>;
    fn write_map(&self, raw: &[u8]) -> Result<()>;
}
#[cfg(windows)]
impl MacroIo for hardware::Keyboard {
    fn read(&self) -> Result<MacroSnapshot> {
        read_device(self)
    }
    fn configuration(&self) -> Result<Snapshot> {
        self.snapshot()
    }
    fn write_bank(&self, raw: &[u8]) -> Result<()> {
        self.write_macro_bank(raw)
    }
    fn write_map(&self, raw: &[u8]) -> Result<()> {
        self.write_keymap(raw)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct MacroReceipt {
    pub backup_path: Option<PathBuf>,
    pub snapshot: MacroSnapshot,
    pub changed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub key: u8,
    pub slot: u8,
    pub mode: u8,
    pub repeats: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub macros: Vec<Macro>,
    pub binding: Option<Binding>,
}
pub fn target(expected: &MacroSnapshot, edit: &Edit) -> Result<MacroSnapshot> {
    expected.validate()?;
    // Preserve untouched vendor actions, including types not exposed by this editor.
    for item in &edit.macros {
        if expected.macros.iter().find(|m| m.slot == item.slot) != Some(item) {
            item.validate()?;
        }
    }
    let mut wanted = expected.clone();
    wanted.raw = if edit.macros == expected.macros {
        expected.raw.clone()
    } else {
        encode(&edit.macros, &expected.raw)?
    };
    wanted.macros = decode(&wanted.raw)?;
    if let Some(binding) = &edit.binding {
        ensure!(
            wanted.macros.iter().any(|m| m.slot == binding.slot),
            "Выберите существующий макрос"
        );
        let mut map = keymap::target(
            &expected.configuration,
            &KeymapEdit {
                keys: BTreeSet::from([binding.key]),
                assignment: Assignment::Default,
            },
        )?;
        let at = usize::from(binding.key) * 4;
        map[at..at + 4].copy_from_slice(
            &Assignment::Macro {
                slot: binding.slot,
                mode: binding.mode,
                repeats: binding.repeats,
            }
            .encode()?,
        );
        wanted.configuration.blocks.insert("keymap".into(), map);
    }
    validate_map(&wanted.configuration.blocks["keymap"], &wanted.macros)?;
    Ok(wanted)
}
fn commit(
    device: &impl MacroIo,
    expected: &MacroSnapshot,
    wanted: &MacroSnapshot,
    directory: &Path,
) -> Result<MacroReceipt> {
    expected.validate()?;
    wanted.validate()?;
    validate_map(&wanted.configuration.blocks["keymap"], &wanted.macros)?;
    ensure!(
        same_identity(expected, wanted),
        "Macro target identity mismatch"
    );
    let mut allowed = expected.configuration.blocks.clone();
    allowed.insert(
        "keymap".into(),
        wanted.configuration.blocks["keymap"].clone(),
    );
    ensure!(
        allowed == wanted.configuration.blocks,
        "Macro target changes unrelated settings"
    );
    ensure!(
        equivalent(&device.read()?, expected),
        "Настройки изменились; прочитайте макросы заново"
    );
    if equivalent(expected, wanted) {
        return Ok(MacroReceipt {
            backup_path: None,
            snapshot: expected.clone(),
            changed: false,
        });
    }
    let path = storage::backup(directory, "ak820max-macros", expected)?;
    ensure!(
        equivalent(&device.read()?, expected),
        "Настройки изменились во время создания копии"
    );
    let mut bank_attempted = false;
    let mut map_attempted = false;
    let result = (|| -> Result<MacroSnapshot> {
        let mut intermediate = expected.clone();
        if wanted.raw != expected.raw {
            bank_attempted = true;
            device.write_bank(&wanted.raw)?;
            intermediate.raw = wanted.raw.clone();
            intermediate.macros = wanted.macros.clone();
            ensure!(
                equivalent(&device.read()?, &intermediate),
                "Банк макросов не совпал после записи"
            );
        }
        if wanted.configuration.blocks["keymap"] != expected.configuration.blocks["keymap"] {
            map_attempted = true;
            device.write_map(&wanted.configuration.blocks["keymap"])?;
        }
        let after = device.read()?;
        ensure!(
            equivalent(&after, wanted),
            "Макросы или назначения не совпали после записи"
        );
        Ok(after)
    })();
    match result {
        Ok(snapshot) => Ok(MacroReceipt {
            backup_path: Some(path),
            snapshot,
            changed: true,
        }),
        Err(error) => {
            let restore = (|| -> Result<()> {
                let current = device.configuration()?;
                hardware::supported_lighting_revision(&current.info)?;
                ensure!(
                    current.endpoint.path == expected.configuration.endpoint.path
                        && current.info.profile == expected.configuration.info.profile,
                    "Профиль или устройство изменились"
                );
                // Resolve even a lost ACK by final readback, and attempt both areas.
                let mut errors = Vec::new();
                if bank_attempted && let Err(e) = device.write_bank(&expected.raw) {
                    errors.push(format!("банк: {e:#}"));
                }
                if map_attempted
                    && let Err(e) = device.write_map(&expected.configuration.blocks["keymap"])
                {
                    errors.push(format!("назначения: {e:#}"));
                }
                ensure!(
                    equivalent(&device.read()?, expected),
                    "Исходные данные не совпали: {errors:?}"
                );
                Ok(())
            })();
            match restore {
                Ok(()) => bail!(
                    "{error:#}. Исходные данные восстановлены и проверены. Копия: {}",
                    path.display()
                ),
                Err(e) => bail!(
                    "{error:#}. Восстановление не подтверждено: {e:#}. Копия: {}",
                    path.display()
                ),
            }
        }
    }
}
pub fn apply(expected: &MacroSnapshot, edit: &Edit) -> Result<MacroReceipt> {
    let wanted = target(expected, edit)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&expected.configuration)?;
        commit(&device, expected, &wanted, &storage::backup_directory()?)
    }
    #[cfg(not(windows))]
    {
        let _ = wanted;
        bail!("Windows only")
    }
}
pub fn restore(path: &Path) -> Result<MacroReceipt> {
    let saved: MacroSnapshot = storage::read_json(path, 128 * 1024)?;
    saved.validate()?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&saved.configuration)?;
        let current = read_device(&device)?;
        ensure!(
            same_identity(&saved, &current),
            "Выбран другой профиль или интерфейс"
        );
        let mut wanted = current.clone();
        wanted.raw = saved.raw.clone();
        wanted.macros = saved.macros.clone();
        let mut map = current.configuration.blocks["keymap"].clone();
        for key in crate::model::keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|k| k.hid != 0)
        {
            let at = usize::from(key.id) * 4;
            let a = &map[at..at + 4];
            let b = &saved.configuration.blocks["keymap"][at..at + 4];
            if a != b {
                ensure!(
                    Assignment::decode(a).is_some() && Assignment::decode(b).is_some(),
                    "Неизвестное назначение в копии"
                );
                if let Some(Assignment::Dks { slot }) = Assignment::decode(b) {
                    let d = usize::from(slot) * 16;
                    ensure!(
                        current.configuration.blocks["dks"][d..d + 16]
                            == saved.configuration.blocks["dks"][d..d + 16],
                        "Таблица DKS изменилась"
                    );
                }
                map[at..at + 4].copy_from_slice(b);
            }
        }
        keymap::validate_pairs(&map)?;
        wanted.configuration.blocks.insert("keymap".into(), map);
        commit(&device, &current, &wanted, &storage::backup_directory()?)
    }
    #[cfg(not(windows))]
    {
        bail!("Windows only")
    }
}
#[cfg(windows)]
fn read_device(device: &hardware::Keyboard) -> Result<MacroSnapshot> {
    let configuration = device.snapshot()?;
    let raw = device.read_macro_bank()?;
    let macros =
        decode(&raw).context("Формат макросов требует исследования; запись не выполнялась")?;
    let after = device.snapshot()?;
    ensure!(
        configuration.endpoint.path == after.endpoint.path
            && configuration.info.profile == after.info.profile
            && configuration.blocks == after.blocks,
        "Настройки изменились во время чтения макросов"
    );
    let result = MacroSnapshot {
        schema: 1,
        configuration,
        raw,
        macros,
    };
    result.validate()?;
    Ok(result)
}
pub fn read(wired: bool) -> Result<MacroSnapshot> {
    #[cfg(windows)]
    {
        let device = if wired {
            hardware::Keyboard::open_usb()?
        } else {
            hardware::Keyboard::open()?
        };
        let result = read_device(&device)?;
        ensure!(
            result.configuration.is_wired()? == wired,
            "Macro channel mismatch"
        );
        Ok(result)
    }
    #[cfg(not(windows))]
    {
        let _ = wired;
        anyhow::bail!("Windows only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn fixture() -> MacroSnapshot {
        let mut raw = [0; 48];
        raw[4..8].copy_from_slice(&[0x45, 0x0c, 0xa0, 0x80]);
        raw[8..10].copy_from_slice(&[0x30, 1]);
        raw[12..16].copy_from_slice(&[0x66, 1, 4, 0x10]);
        raw[16] = 15;
        MacroSnapshot {
            schema: 1,
            configuration: Snapshot {
                schema: 1,
                endpoint: hardware::Endpoint {
                    path: "test-usb".into(),
                    vid: 0x0c45,
                    pid: 0x80a0,
                    product: "AK820MAX".into(),
                    usage_page: 0xff68,
                    usage: 97,
                    interface: 2,
                    role: String::new(),
                },
                info: crate::protocol::KeyboardInfo::parse(&raw).unwrap(),
                blocks: crate::protocol::READ_BLOCKS
                    .iter()
                    .skip(1)
                    .map(|(_, n, name)| (name.to_string(), vec![0; *n]))
                    .collect(),
            },
            raw: vec![0; BANK_BYTES],
            macros: Vec::new(),
        }
    }
    fn edit() -> Edit {
        Edit {
            macros: vec![Macro {
                slot: 3,
                actions: vec![
                    Action {
                        delay_ms: 20,
                        usage: 68,
                        pressed: true,
                        mouse: false,
                    },
                    Action {
                        delay_ms: 20,
                        usage: 68,
                        pressed: false,
                        mouse: false,
                    },
                ],
            }],
            binding: Some(Binding {
                key: 12,
                slot: 3,
                mode: 1,
                repeats: 2,
            }),
        }
    }
    struct Mock {
        state: RefCell<MacroSnapshot>,
        writes: Cell<usize>,
        fail_on: usize,
        corrupt: bool,
        switch_profile: bool,
    }
    impl MacroIo for Mock {
        fn read(&self) -> Result<MacroSnapshot> {
            let s = self.state.borrow().clone();
            s.validate()?;
            Ok(s)
        }
        fn configuration(&self) -> Result<Snapshot> {
            Ok(self.state.borrow().configuration.clone())
        }
        fn write_bank(&self, raw: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            let mut s = self.state.borrow_mut();
            s.raw = raw.to_vec();
            s.macros = decode(raw)?;
            if self.switch_profile {
                s.configuration.info.profile += 1;
            }
            if self.writes.get() == self.fail_on {
                if self.corrupt {
                    s.raw[..4].copy_from_slice(&[1, 0, 0, 0]);
                }
                bail!("lost bank ACK");
            }
            Ok(())
        }
        fn write_map(&self, raw: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            self.state
                .borrow_mut()
                .configuration
                .blocks
                .insert("keymap".into(), raw.to_vec());
            if self.writes.get() == self.fail_on {
                bail!("lost map ACK");
            }
            Ok(())
        }
    }
    fn mock(before: &MacroSnapshot, fail_on: usize, corrupt: bool) -> Mock {
        Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail_on,
            corrupt,
            switch_profile: false,
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!("ajazz-macro-tests-{}", storage::unique_suffix()))
    }
    #[test]
    fn bank_and_binding_commit_preserves_other_areas_and_rolls_back_lost_ack() {
        let before = fixture();
        let wanted = target(&before, &edit()).unwrap();
        assert_eq!(
            &wanted.configuration.blocks["keymap"][48..52],
            &[6, 3, 1, 2]
        );
        let device = mock(&before, 0, false);
        let receipt = commit(&device, &before, &wanted, &directory()).unwrap();
        assert!(equivalent(&receipt.snapshot, &wanted));
        assert_eq!(device.writes.get(), 2);
        let unchanged = commit(&device, &wanted, &wanted, &directory()).unwrap();
        assert!(!unchanged.changed);
        assert_eq!(device.writes.get(), 2);
        for (failure, corrupt) in [(1, true), (2, false)] {
            let device = mock(&before, failure, corrupt);
            let error = commit(&device, &before, &wanted, &directory())
                .unwrap_err()
                .to_string();
            assert!(error.contains("восстановлены и проверены"), "{error}");
            assert!(equivalent(&device.read().unwrap(), &before));
        }
    }
    #[test]
    fn profile_change_and_failed_backup_do_not_write_other_profile() {
        let before = fixture();
        let wanted = target(&before, &edit()).unwrap();
        let mut device = mock(&before, 0, false);
        device.switch_profile = true;
        assert!(commit(&device, &before, &wanted, &directory()).is_err());
        assert_eq!(device.writes.get(), 1);
        let device = mock(&before, 0, false);
        let path = directory();
        std::fs::write(&path, b"not a directory").unwrap();
        assert!(commit(&device, &before, &wanted, &path).is_err());
        assert_eq!(device.writes.get(), 0);
        let mut stale = before.clone();
        stale.configuration.info.profile = 1;
        assert!(commit(&device, &stale, &wanted, &directory()).is_err());
        assert_eq!(device.writes.get(), 0);
    }
    #[test]
    fn dangling_bindings_unbalanced_keys_and_invalid_repeats_are_rejected() {
        let before = fixture();
        let mut change = edit();
        change.macros[0].actions.pop();
        assert!(target(&before, &change).is_err());
        let assigned = target(&before, &edit()).unwrap();
        assert!(
            target(
                &assigned,
                &Edit {
                    macros: Vec::new(),
                    binding: None
                }
            )
            .is_err()
        );
        change = edit();
        change.binding.as_mut().unwrap().repeats = 0;
        assert!(target(&before, &change).is_err());
        change = edit();
        change.macros[0].actions.reverse();
        assert!(target(&before, &change).is_err());
    }
    #[test]
    fn macro_bank_keeps_unused_tail_and_rejects_bad_pointers_and_overlap() {
        let original = vec![0xa5; BANK_BYTES];
        let macros = vec![Macro {
            slot: 3,
            actions: vec![
                Action {
                    delay_ms: 20,
                    usage: 4,
                    pressed: true,
                    mouse: false,
                },
                Action {
                    delay_ms: 40,
                    usage: 4,
                    pressed: false,
                    mouse: false,
                },
            ],
        }];
        let encoded = encode(&macros, &original).unwrap();
        assert_eq!(&encoded[12..16], &[0x90, 1, 0, 0]);
        assert_eq!(
            &encoded[400..412],
            &[4, 0, 0, 0, 20, 0, 4, 0x90, 40, 0, 4, 0x10]
        );
        assert_eq!(&encoded[412..], &original[412..]);
        assert_eq!(decode(&encoded).unwrap(), macros);
        let mut bad = encoded.clone();
        bad[..4].copy_from_slice(&[0x90, 1, 0, 0]);
        assert!(decode(&bad).is_err());
        bad[..4].copy_from_slice(&[4, 0, 0, 0]);
        assert!(decode(&bad).is_err());
        let oversized = vec![Macro {
            slot: 0,
            actions: vec![macros[0].actions[0].clone(); 128],
        }];
        assert!(encode(&oversized, &original).is_err());
    }
}
