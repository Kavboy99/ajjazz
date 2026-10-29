//! Verified read-modify-write for the established Hall and keymap areas of AK820MAX.
use crate::{
    hardware::{self, Snapshot},
    model::keyboard_rows,
    protocol::HallKey,
    storage,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HallEdit {
    pub keys: BTreeSet<u8>,
    pub settings: HallKey,
}
#[derive(Clone, Debug, Serialize)]
pub struct KeyboardReceipt {
    pub backup_path: Option<PathBuf>,
    pub snapshot: Snapshot,
    pub changed: bool,
}
fn validate(snapshot: &Snapshot) -> Result<()> {
    ensure!(snapshot.schema == 1, "Unsupported keyboard snapshot");
    hardware::supported_lighting_revision(&snapshot.info)?;
    snapshot.is_wired()?;
    ensure!(
        snapshot.info.rt_precision == 0,
        "Unknown RT precision for this firmware"
    );
    for &(_, len, name) in crate::protocol::READ_BLOCKS.iter().skip(1) {
        ensure!(
            snapshot.blocks.get(name).is_some_and(|b| b.len() == len),
            "Invalid {name} block"
        );
    }
    Ok(())
}
fn same_identity(a: &Snapshot, b: &Snapshot) -> bool {
    a.info.profile == b.info.profile
        && a.info.rt_precision == b.info.rt_precision
        && a.endpoint.path == b.endpoint.path
}
fn equivalent(a: &Snapshot, b: &Snapshot) -> bool {
    same_identity(a, b) && a.blocks == b.blocks
}
pub fn hall_target(snapshot: &Snapshot, edit: &HallEdit) -> Result<Vec<u8>> {
    validate(snapshot)?;
    ensure!(!edit.keys.is_empty(), "Выберите клавиши на схеме");
    let ids: BTreeSet<_> = keyboard_rows()
        .into_iter()
        .flatten()
        .map(|k| k.id)
        .collect();
    ensure!(edit.keys.is_subset(&ids), "Unknown physical key");
    let mut target = snapshot.blocks["rapid_trigger"].clone();
    for &id in &edit.keys {
        let offset = usize::from(id) * 8;
        let record = edit
            .settings
            .patch(&target[offset..offset + 8], snapshot.info.rt_precision)?;
        target[offset..offset + 8].copy_from_slice(&record);
    }
    Ok(target)
}
#[derive(Clone, Copy)]
enum Block {
    Hall,
    Keymap,
    General,
    Dks,
}
impl Block {
    fn name(self) -> &'static str {
        match self {
            Self::Hall => "rapid_trigger",
            Self::Keymap => "keymap",
            Self::General => "game_mode",
            Self::Dks => "dks",
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Hall => 1024,
            Self::Keymap => 512,
            Self::General => 56,
            Self::Dks => 1024,
        }
    }
}
trait SettingsIo {
    fn read(&self) -> Result<Snapshot>;
    fn write(&self, block: Block, data: &[u8]) -> Result<()>;
}
#[cfg(windows)]
impl SettingsIo for hardware::Keyboard {
    fn read(&self) -> Result<Snapshot> {
        self.snapshot()
    }
    fn write(&self, block: Block, data: &[u8]) -> Result<()> {
        match block {
            Block::Hall => self.write_hall(data),
            Block::Keymap => self.write_keymap(data),
            Block::General => self.write_general(data),
            Block::Dks => self.write_dks(data),
        }
    }
}
fn commit(
    device: &impl SettingsIo,
    block: Block,
    expected: &Snapshot,
    target: &[u8],
    directory: &Path,
) -> Result<KeyboardReceipt> {
    commit_many(device, &[(block, target.to_vec())], expected, directory)
}
fn commit_many(
    device: &impl SettingsIo,
    targets: &[(Block, Vec<u8>)],
    expected: &Snapshot,
    directory: &Path,
) -> Result<KeyboardReceipt> {
    validate(expected)?;
    ensure!(!targets.is_empty(), "No settings targets");
    let mut names = BTreeSet::new();
    for (block, data) in targets {
        ensure!(
            data.len() == block.len() && names.insert(block.name()),
            "Invalid configuration targets"
        );
    }
    let before = device.read()?;
    validate(&before)?;
    ensure!(
        equivalent(&before, expected),
        "Настройки клавиатуры изменились. Прочитайте их заново."
    );
    if targets
        .iter()
        .all(|(block, data)| before.blocks[block.name()] == *data)
    {
        return Ok(KeyboardReceipt {
            backup_path: None,
            snapshot: before,
            changed: false,
        });
    }
    let path = storage::backup(
        directory,
        &format!("ak820max-{}", targets[0].0.name()),
        &before,
    )?;
    ensure!(
        equivalent(&device.read()?, &before),
        "Профиль или настройки изменились во время создания копии"
    );
    let mut attempted = Vec::new();
    let apply = (|| -> Result<Snapshot> {
        let mut intermediate = before.clone();
        for (block, data) in targets {
            if before.blocks[block.name()] != *data {
                if !attempted.is_empty() {
                    ensure!(
                        equivalent(&device.read()?, &intermediate),
                        "Профиль или настройки изменились между блоками; запись остановлена"
                    );
                }
                // Include a write whose ACK may be lost in the rollback set.
                attempted.push(*block);
                device.write(*block, data)?;
                intermediate
                    .blocks
                    .insert(block.name().into(), data.clone());
            }
        }
        let after = device.read()?;
        validate(&after)?;
        let mut wanted = before.clone();
        for (block, data) in targets {
            wanted.blocks.insert(block.name().into(), data.clone());
        }
        ensure!(
            equivalent(&after, &wanted),
            "Проверка записанных настроек не прошла"
        );
        Ok(after)
    })();
    match apply {
        Ok(snapshot) => Ok(KeyboardReceipt {
            backup_path: Some(path),
            snapshot,
            changed: true,
        }),
        Err(error) => {
            let restore = (|| -> Result<()> {
                let current = device.read()?;
                validate(&current)?;
                ensure!(
                    same_identity(&current, &before),
                    "Профиль или устройство изменились"
                );
                // A lost acknowledgement is resolved by reading back, even if write returned an error.
                let mut write_errors = Vec::new();
                for block in attempted.iter().rev() {
                    if let Err(error) = device.write(*block, &before.blocks[block.name()]) {
                        write_errors.push(format!("{}: {error:#}", block.name()));
                    }
                }
                let restored = device.read()?;
                ensure!(
                    equivalent(&restored, &before),
                    "Исходный снимок не совпал; {:?}",
                    write_errors
                );
                Ok(())
            })();
            match restore {
                Ok(()) => bail!(
                    "{error:#}. Исходные настройки восстановлены и проверены. Копия: {}",
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
pub fn apply(expected: &Snapshot, edit: &HallEdit) -> Result<KeyboardReceipt> {
    let target = hall_target(expected, edit)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(expected)?;
        commit(
            &device,
            Block::Hall,
            expected,
            &target,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        bail!("Windows only")
    }
}
pub fn restore(path: &Path) -> Result<KeyboardReceipt> {
    let saved: Snapshot = storage::read_json(path, 128 * 1024)?;
    validate(&saved)?;
    let target = saved.blocks.get("rapid_trigger").context("No Hall data")?;
    // Validate known encodings, including zero sentinels used by stock defaults.
    for record in target.as_chunks::<8>().0 {
        let k = HallKey::decode(record, saved.info.rt_precision)?;
        ensure!(
            k.axis <= 3
                && (k.actuation == 0.0 || (0.1..=3.3).contains(&k.actuation))
                && [k.press, k.release]
                    .iter()
                    .all(|v| *v == 0.0 || (0.01..=3.3).contains(v)),
            "Invalid Hall backup value"
        );
    }
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&saved)?;
        let current = device.snapshot()?;
        validate(&current)?;
        ensure!(
            same_identity(&current, &saved),
            "Выбран другой профиль или приёмник клавиатуры"
        );
        // Preserve reserved flags and all unused matrix positions from the live device.
        let mut safe = current.blocks["rapid_trigger"].clone();
        for key in keyboard_rows().into_iter().flatten() {
            let at = usize::from(key.id) * 8;
            safe[at..at + 8].copy_from_slice(&target[at..at + 8]);
            safe[at + 1] = (safe[at + 1] & 3) | (current.blocks["rapid_trigger"][at + 1] & !3);
        }
        commit(
            &device,
            Block::Hall,
            &current,
            &safe,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        bail!("Windows only")
    }
}

pub fn apply_keymap(
    expected: &Snapshot,
    edit: &crate::keymap::KeymapEdit,
) -> Result<KeyboardReceipt> {
    validate(expected)?;
    let target = crate::keymap::target(expected, edit)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(expected)?;
        commit(
            &device,
            Block::Keymap,
            expected,
            &target,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        bail!("Windows only")
    }
}
pub fn restore_keymap(path: &Path) -> Result<KeyboardReceipt> {
    let saved: Snapshot = storage::read_json(path, 128 * 1024)?;
    validate(&saved)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&saved)?;
        let current = device.snapshot()?;
        validate(&current)?;
        ensure!(
            same_identity(&saved, &current),
            "Выбран другой профиль или приёмник клавиатуры"
        );
        let mut target = current.blocks["keymap"].clone();
        for key in keyboard_rows().into_iter().flatten().filter(|k| k.hid != 0) {
            let at = usize::from(key.id) * 4;
            let value = &saved.blocks["keymap"][at..at + 4];
            if target[at..at + 4] == *value {
                continue;
            }
            ensure!(
                value[0] != 6,
                "Для возврата назначения макроса используйте копию из редактора макросов"
            );
            ensure!(
                crate::keymap::Assignment::decode(value).is_some()
                    && crate::keymap::Assignment::decode(&target[at..at + 4]).is_some(),
                "Копия содержит сложные назначения, которые ещё не поддерживаются"
            );
            target[at..at + 4].copy_from_slice(value);
        }
        crate::keymap::validate_pairs(&target)?;
        for record in target.as_chunks::<4>().0 {
            if let Some(crate::keymap::Assignment::Dks { slot }) =
                crate::keymap::Assignment::decode(record)
            {
                let at = usize::from(slot) * 16;
                ensure!(
                    saved.blocks["dks"][at..at + 16] == current.blocks["dks"][at..at + 16],
                    "Таблица DKS изменилась; используйте восстановление DKS вместе с назначениями"
                );
            }
        }
        let mut checked = current.clone();
        checked.blocks.insert("keymap".into(), target.clone());
        crate::keyboard_dks::validate_references(&checked)?;
        commit(
            &device,
            Block::Keymap,
            &current,
            &target,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        bail!("Windows only")
    }
}

pub fn apply_dks(
    expected: &Snapshot,
    key: u8,
    settings: &crate::keyboard_dks::DksConfig,
) -> Result<KeyboardReceipt> {
    validate(expected)?;
    let (bank, map) = crate::keyboard_dks::target(expected, key, settings)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(expected)?;
        commit_many(
            &device,
            &[(Block::Dks, bank), (Block::Keymap, map)],
            expected,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (bank, map);
        bail!("Windows only")
    }
}
pub fn restore_dks(path: &Path) -> Result<KeyboardReceipt> {
    let saved: Snapshot = storage::read_json(path, 128 * 1024)?;
    validate(&saved)?;
    crate::keyboard_dks::validate_references(&saved)?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&saved)?;
        let current = device.snapshot()?;
        validate(&current)?;
        ensure!(
            same_identity(&saved, &current),
            "Выбран другой профиль или интерфейс клавиатуры"
        );
        let mut map = current.blocks["keymap"].clone();
        for key in keyboard_rows().into_iter().flatten().filter(|k| k.hid != 0) {
            let at = usize::from(key.id) * 4;
            let value = &saved.blocks["keymap"][at..at + 4];
            if map[at..at + 4] != *value {
                ensure!(
                    value[0] != 6,
                    "Для возврата назначения макроса используйте копию из редактора макросов"
                );
                ensure!(
                    crate::keymap::Assignment::decode(value).is_some()
                        && crate::keymap::Assignment::decode(&map[at..at + 4]).is_some(),
                    "Неизвестное назначение в копии DKS"
                );
                map[at..at + 4].copy_from_slice(value);
            }
        }
        crate::keymap::validate_pairs(&map)?;
        commit_many(
            &device,
            &[
                (Block::Dks, saved.blocks["dks"].clone()),
                (Block::Keymap, map),
            ],
            &current,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        bail!("Windows only")
    }
}

pub fn apply_general(
    expected: &Snapshot,
    settings: &crate::keyboard_general::GeneralSettings,
) -> Result<KeyboardReceipt> {
    validate(expected)?;
    let target = settings.patch(&expected.blocks["game_mode"])?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(expected)?;
        commit(
            &device,
            Block::General,
            expected,
            &target,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        bail!("Windows only")
    }
}
/// Bounded engineering check: edit exactly one established field and always attempt restoration.
#[cfg(windows)]
pub fn diagnose_general_cycle(field: &str, output: &Path) -> Result<KeyboardReceipt> {
    ensure!(
        [
            "sleep",
            "delay",
            "top",
            "bottom",
            "stability",
            "calibration"
        ]
        .contains(&field),
        "Unknown diagnostic field"
    );
    let device = hardware::Keyboard::open()?;
    let before = device.snapshot()?;
    validate(&before)?;
    let raw = &before.blocks["game_mode"];
    let original = crate::keyboard_general::GeneralSettings::parse(raw)?;
    let mut edited = original.clone();
    match field {
        "sleep" => edited.sleep_minutes = if original.sleep_minutes == 5 { 6 } else { 5 },
        "delay" => edited.key_delay = if original.key_delay == 0 { 1 } else { 0 },
        "top" => edited.top_dead_zone = if original.top_dead_zone == 0 { 1 } else { 0 },
        "bottom" => edited.bottom_dead_zone = if original.bottom_dead_zone == 0 { 1 } else { 0 },
        "stability" => edited.stability = !original.stability,
        "calibration" => edited.auto_calibration = !original.auto_calibration,
        _ => unreachable!(),
    }
    let target = edited.patch(raw)?;
    before.save(output)?;
    let applied = commit(
        &device,
        Block::General,
        &before,
        &target,
        &storage::backup_directory()?,
    );
    let changed_record_error = applied.as_ref().ok().and_then(|receipt| {
        receipt
            .snapshot
            .save(&output.with_extension("changed.json"))
            .err()
    });
    let restored = (|| -> Result<KeyboardReceipt> {
        let current = device.snapshot()?;
        validate(&current)?;
        ensure!(
            same_identity(&current, &before),
            "Профиль изменился; диагностический возврат остановлен"
        );
        let target = original.patch(&current.blocks["game_mode"])?;
        let receipt = commit(
            &device,
            Block::General,
            &current,
            &target,
            &storage::backup_directory()?,
        )?;
        ensure!(
            equivalent(&receipt.snapshot, &before),
            "После диагностического возврата снимок отличается"
        );
        Ok(receipt)
    })();
    match (applied, restored) {
        (Ok(_), Ok(receipt)) => {
            let restored_record_error = receipt
                .snapshot
                .save(&output.with_extension("restored.json"))
                .err();
            if let Some(error) = changed_record_error.or(restored_record_error) {
                bail!(
                    "Исходные настройки восстановлены и проверены, но диагностический файл не сохранён: {error:#}"
                );
            }
            Ok(receipt)
        }
        (Err(error), Ok(_)) => bail!(
            "Диагностика {field} не прошла: {error:#}. Исходные настройки восстановлены и проверены."
        ),
        (_, Err(error)) => bail!(
            "Возврат после диагностики {field} не подтверждён: {error:#}. Исходная копия: {}",
            output.display()
        ),
    }
}
pub fn restore_general(path: &Path) -> Result<KeyboardReceipt> {
    let saved: Snapshot = storage::read_json(path, 128 * 1024)?;
    validate(&saved)?;
    let settings = crate::keyboard_general::GeneralSettings::parse(&saved.blocks["game_mode"])?;
    #[cfg(windows)]
    {
        let device = hardware::Keyboard::open_for_snapshot(&saved)?;
        let current = device.snapshot()?;
        validate(&current)?;
        ensure!(
            same_identity(&saved, &current),
            "Выбран другой профиль или приёмник клавиатуры"
        );
        let target = settings.patch(&current.blocks["game_mode"])?;
        commit(
            &device,
            Block::General,
            &current,
            &target,
            &storage::backup_directory()?,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = settings;
        bail!("Windows only")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_general_edit_is_rejected_before_opening_any_device() {
        let before = fixture();
        let mut settings =
            crate::keyboard_general::GeneralSettings::parse(&before.blocks["game_mode"]).unwrap();
        assert!(settings.patch(&before.blocks["game_mode"]).is_ok());
        settings.key_delay = 255;
        assert!(apply_general(&before, &settings).is_err());
    }
    use std::cell::{Cell, RefCell};
    fn fixture() -> Snapshot {
        let info = crate::protocol::KeyboardInfo {
            vid: 0x0c45,
            pid: 0x80a0,
            version: "1.30".into(),
            manufacturer: 358,
            product: 4100,
            work_mode: 2,
            battery: 31,
            charging: 0,
            profile: 0,
            rt_precision: 0,
            frame_version: 0,
            lighting_version: 0,
            firmware_status: 0,
            animation_limits: None,
        };
        Snapshot {
            schema: 1,
            endpoint: hardware::Endpoint {
                path: "test".into(),
                vid: 0x0c45,
                pid: 0xfefc,
                product: "AJAZZ AK820MAX".into(),
                usage_page: 0xff80,
                usage: 1,
                interface: 2,
                role: String::new(),
            },
            info,
            blocks: crate::protocol::READ_BLOCKS
                .iter()
                .skip(1)
                .map(|(_, len, name)| (name.to_string(), vec![0; *len]))
                .collect(),
        }
    }
    fn edit() -> HallEdit {
        HallEdit {
            keys: BTreeSet::from([34, 49, 50, 51]),
            settings: HallKey {
                axis: 0,
                rapid: true,
                whole_travel: false,
                continuous: false,
                actuation: 1.2,
                press: 0.1,
                release: 0.2,
            },
        }
    }
    struct Mock {
        state: RefCell<Snapshot>,
        writes: Cell<usize>,
        fail: Cell<bool>,
    }
    impl SettingsIo for Mock {
        fn read(&self) -> Result<Snapshot> {
            Ok(self.state.borrow().clone())
        }
        fn write(&self, block: Block, bytes: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            if self.fail.replace(false) {
                let partial = bytes.len().min(400);
                self.state
                    .borrow_mut()
                    .blocks
                    .get_mut(block.name())
                    .unwrap()[..partial]
                    .copy_from_slice(&bytes[..partial]);
                bail!("Interrupted write");
            }
            self.state
                .borrow_mut()
                .blocks
                .insert(block.name().into(), bytes.to_vec());
            Ok(())
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!("ajazz-keyboard-test-{}", storage::unique_suffix()))
    }

    #[test]
    fn usb_edit_and_restore_preserve_other_blocks_and_stop_on_endpoint_change() {
        let mut before = fixture();
        before.info.work_mode = 15;
        before.endpoint.pid = 0x80a0;
        before.endpoint.product = "AK820MAX".into();
        before.endpoint.usage_page = 0xff68;
        before.endpoint.usage = 97;
        assert!(before.is_wired().unwrap());
        let device = Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(false),
        };
        let target = hall_target(&before, &edit()).unwrap();
        let applied = commit(&device, Block::Hall, &before, &target, &directory()).unwrap();
        for (name, bytes) in &before.blocks {
            if name != "rapid_trigger" {
                assert_eq!(&applied.snapshot.blocks[name], bytes);
            }
        }
        let restored = commit(
            &device,
            Block::Hall,
            &applied.snapshot,
            &before.blocks["rapid_trigger"],
            &directory(),
        )
        .unwrap();
        assert!(equivalent(&restored.snapshot, &before));
        device.state.borrow_mut().endpoint.path = "different-usb-keyboard".into();
        let writes = device.writes.get();
        assert!(commit(&device, Block::Hall, &before, &target, &directory()).is_err());
        assert_eq!(device.writes.get(), writes);
    }
    #[test]
    fn dks_shared_slot_is_copied_and_second_block_failure_restores_both_blocks() {
        let mut before = fixture();
        let initial = crate::keyboard_dks::DksConfig::for_usage(68);
        before.blocks.get_mut("dks").unwrap()[..16]
            .copy_from_slice(&initial.patch(&[0; 16]).unwrap());
        before.blocks.get_mut("keymap").unwrap()[11 * 4..11 * 4 + 4].copy_from_slice(&[8, 0, 0, 0]);
        before.blocks.get_mut("keymap").unwrap()[12 * 4..12 * 4 + 4].copy_from_slice(&[8, 0, 0, 0]);
        let edited = crate::keyboard_dks::DksConfig::for_usage(69);
        let (bank, map) = crate::keyboard_dks::target(&before, 12, &edited).unwrap();
        assert_eq!(&map[12 * 4..12 * 4 + 4], &[8, 1, 0, 0]);
        assert_eq!(&bank[..16], &before.blocks["dks"][..16]);
        assert_eq!(&bank[32..], &before.blocks["dks"][32..]);
        struct SecondFailure(Mock);
        impl SettingsIo for SecondFailure {
            fn read(&self) -> Result<Snapshot> {
                self.0.read()
            }
            fn write(&self, b: Block, v: &[u8]) -> Result<()> {
                if self.0.writes.get() == 1 {
                    self.0.fail.set(true);
                }
                self.0.write(b, v)
            }
        }
        let device = SecondFailure(Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(false),
        });
        let error = commit_many(
            &device,
            &[(Block::Dks, bank.clone()), (Block::Keymap, map.clone())],
            &before,
            &directory(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("восстановлены и проверены"), "{error}");
        assert_eq!(device.0.writes.get(), 4);
        assert_eq!(device.0.state.borrow().blocks, before.blocks);
        let applied = commit_many(
            &device,
            &[(Block::Dks, bank), (Block::Keymap, map)],
            &before,
            &directory(),
        )
        .unwrap();
        crate::keyboard_dks::validate_references(&applied.snapshot).unwrap();
        for (name, bytes) in &before.blocks {
            if !["dks", "keymap"].contains(&name.as_str()) {
                assert_eq!(&applied.snapshot.blocks[name], bytes);
            }
        }
    }
    #[test]
    fn profile_switch_between_dks_and_keymap_prevents_writing_second_profile() {
        struct Switch(Mock);
        impl SettingsIo for Switch {
            fn read(&self) -> Result<Snapshot> {
                self.0.read()
            }
            fn write(&self, b: Block, v: &[u8]) -> Result<()> {
                self.0.write(b, v)?;
                self.0.state.borrow_mut().info.profile = 1;
                Ok(())
            }
        }
        let before = fixture();
        let settings = crate::keyboard_dks::DksConfig::for_usage(69);
        let (bank, map) = crate::keyboard_dks::target(&before, 12, &settings).unwrap();
        let device = Switch(Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(false),
        });
        let error = commit_many(
            &device,
            &[(Block::Dks, bank), (Block::Keymap, map)],
            &before,
            &directory(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Восстановление не подтверждено"));
        assert_eq!(device.0.writes.get(), 1);
        assert_eq!(
            device.0.state.borrow().blocks["keymap"],
            before.blocks["keymap"]
        );
    }
    #[test]
    fn editing_wasd_preserves_every_other_record_and_reserved_flag() {
        let mut before = fixture();
        before.blocks.get_mut("rapid_trigger").unwrap()[34 * 8 + 1] = 0xf8;
        let target = hall_target(&before, &edit()).unwrap();
        assert_eq!(target[34 * 8 + 1], 0xf8);
        for (i, record) in target.as_chunks::<8>().0.iter().enumerate() {
            if !edit().keys.contains(&(i as u8)) {
                assert_eq!(record, &before.blocks["rapid_trigger"][i * 8..i * 8 + 8]);
            }
        }
        let mut bad = edit();
        bad.keys.insert(127);
        assert!(hall_target(&before, &bad).is_err());
    }
    #[test]
    fn truncated_transfer_is_rolled_back_and_all_blocks_checked() {
        let before = fixture();
        let device = Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(true),
        };
        let error = commit(
            &device,
            Block::Hall,
            &before,
            &hall_target(&before, &edit()).unwrap(),
            &directory(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("восстановлены и проверены"), "{error}");
        assert_eq!(device.state.borrow().blocks, before.blocks);
        assert_eq!(device.writes.get(), 2);
    }
    #[test]
    fn successful_hall_write_is_verified_and_profile_conflict_prevents_writes() {
        let before = fixture();
        let target = hall_target(&before, &edit()).unwrap();
        let device = Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(false),
        };
        let receipt = commit(&device, Block::Hall, &before, &target, &directory()).unwrap();
        assert!(receipt.changed);
        assert_eq!(receipt.snapshot.blocks["rapid_trigger"], target);
        let restored: Snapshot =
            storage::read_json(receipt.backup_path.as_ref().unwrap(), 128 * 1024).unwrap();
        assert_eq!(restored.blocks, before.blocks);
        device.state.borrow_mut().info.profile = 1;
        assert!(commit(&device, Block::Hall, &before, &target, &directory()).is_err());
        assert_eq!(device.writes.get(), 1);
    }
    #[test]
    fn keymap_changes_only_selected_record_and_preserves_other_settings() {
        let mut before = fixture();
        // Existing advanced action must survive edits of another key.
        before.blocks.get_mut("keymap").unwrap()[34 * 4..34 * 4 + 4]
            .copy_from_slice(&[11, 0, 49, 50]);
        let edit = crate::keymap::KeymapEdit {
            keys: BTreeSet::from([12]),
            assignment: crate::keymap::Assignment::Key { usage: 68 },
        };
        let target = crate::keymap::target(&before, &edit).unwrap();
        assert_eq!(&target[48..52], &[2, 0, 68, 0]);
        assert_eq!(&target[34 * 4..34 * 4 + 4], &[11, 0, 49, 50]);
        let device = Mock {
            state: RefCell::new(before.clone()),
            writes: Cell::new(0),
            fail: Cell::new(false),
        };
        let receipt = commit(&device, Block::Keymap, &before, &target, &directory()).unwrap();
        assert!(receipt.changed);
        for (name, bytes) in &before.blocks {
            if name != "keymap" {
                assert_eq!(bytes, &receipt.snapshot.blocks[name]);
            }
        }
        let mut invalid = edit.clone();
        invalid.keys = BTreeSet::from([34]);
        assert!(crate::keymap::target(&before, &invalid).is_err());
        invalid.keys = BTreeSet::from([85]);
        assert!(crate::keymap::target(&before, &invalid).is_err());
    }
    #[test]
    fn general_write_and_interrupted_write_verify_all_other_blocks() {
        let before = fixture();
        let mut settings =
            crate::keyboard_general::GeneralSettings::parse(&before.blocks["game_mode"]).unwrap();
        settings.sleep_minutes = 6;
        settings.top_dead_zone = 1;
        let target = settings.patch(&before.blocks["game_mode"]).unwrap();
        for fail in [false, true] {
            let device = Mock {
                state: RefCell::new(before.clone()),
                writes: Cell::new(0),
                fail: Cell::new(fail),
            };
            let result = commit(&device, Block::General, &before, &target, &directory());
            if fail {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("восстановлены и проверены")
                );
                assert_eq!(device.state.borrow().blocks, before.blocks);
            } else {
                let after = result.unwrap().snapshot;
                assert_eq!(after.blocks["game_mode"], target);
                for (name, raw) in &before.blocks {
                    if name != "game_mode" {
                        assert_eq!(&after.blocks[name], raw);
                    }
                }
            }
        }
    }
}
