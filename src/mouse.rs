//! Read-modify-write transactions for AJ159 APEX. Files contain state, never opcodes to execute.
use crate::{
    protocol::{MouseDpi, MouseOptions, MouseSleep},
    storage,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MouseSnapshot {
    pub captured_utc: String,
    pub source: String,
    pub mouse_id: u32,
    pub last_battery_percent: Option<u8>,
    pub options: MouseOptions,
    pub polling_hz: u32,
    pub dpi: MouseDpi,
    pub raw_options: Vec<u8>,
    pub raw_dpi: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MouseSettings {
    pub options: MouseOptions,
    pub dpi: MouseDpi,
    #[serde(default)]
    pub sleep: Option<MouseSleep>,
}
impl From<&MouseSnapshot> for MouseSettings {
    fn from(snapshot: &MouseSnapshot) -> Self {
        Self {
            options: snapshot.options.clone(),
            dpi: snapshot.dpi.clone(),
            sleep: MouseSleep::parse(&snapshot.raw_options).ok(),
        }
    }
}
impl MouseSnapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.mouse_id == 0x800006db, "Unsupported mouse model");
        ensure!(
            self.options == MouseOptions::parse(&self.raw_options)?,
            "Inconsistent options snapshot"
        );
        ensure!(
            self.dpi == MouseDpi::parse(&self.raw_dpi)?,
            "Inconsistent DPI snapshot"
        );
        ensure!(
            self.options.profile <= 3 && self.raw_dpi[1] == self.options.profile,
            "Mouse profile mismatch"
        );
        ensure!(
            self.polling_hz == self.options.polling_hz()?,
            "Inconsistent polling rate"
        );
        Ok(())
    }
    pub fn equivalent(&self, other: &Self) -> bool {
        self.mouse_id == other.mouse_id
            && self.options.profile == other.options.profile
            && block_equal(&self.raw_options, &other.raw_options)
            && block_equal(&self.raw_dpi, &other.raw_dpi)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let snapshot: Self = storage::read_json(path, 64 * 1024)?;
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn target(&self, options: &MouseOptions, dpi: &MouseDpi) -> Result<Self> {
        self.validate()?;
        let mut target = self.clone();
        target.raw_options = options.patch(&self.raw_options)?;
        target.raw_dpi = dpi.patch(&self.raw_dpi)?;
        target.options = options.clone();
        target.polling_hz = options.polling_hz()?;
        target.dpi = dpi.clone();
        target.validate()?;
        Ok(target)
    }
}

impl MouseSnapshot {
    pub fn target_with_sleep(&self, settings: &MouseSettings) -> Result<Self> {
        let mut target = self.target(&settings.options, &settings.dpi)?;
        if let Some(sleep) = &settings.sleep {
            target.raw_options = sleep.patch(&target.raw_options)?;
        }
        target.validate()?;
        Ok(target)
    }
}
// BIT7 is transport checksum; it changes with the GET/SET header and is not a setting.
fn block_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == 64 && b.len() == 64 && (0..64).filter(|i| *i != 7).all(|i| a[i] == b[i])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Options,
    Dpi,
}
impl Block {
    fn bytes(self, snapshot: &MouseSnapshot) -> &[u8] {
        match self {
            Self::Options => &snapshot.raw_options,
            Self::Dpi => &snapshot.raw_dpi,
        }
    }
}

pub(crate) trait MouseIo {
    fn read(&self) -> Result<MouseSnapshot>;
    fn write(&self, block: Block, state: &[u8]) -> Result<()>;
}

#[derive(Clone, Debug, Serialize)]
pub struct MouseReceipt {
    pub backup_path: Option<PathBuf>,
    pub snapshot: MouseSnapshot,
    pub changed: bool,
}

pub(crate) fn commit(
    device: &impl MouseIo,
    expected: &MouseSnapshot,
    target: &MouseSnapshot,
    directory: &Path,
) -> Result<MouseReceipt> {
    expected.validate()?;
    // Reconstruct from editable fields to reject changes to unknown/reserved bytes.
    let allowed = expected.target_with_sleep(&MouseSettings::from(target))?;
    ensure!(allowed.equivalent(target), "Изменены неизвестные поля мыши");
    let before = device.read()?;
    before.validate()?;
    ensure!(
        before.equivalent(expected),
        "Настройки мыши изменились. Прочитайте их заново перед применением."
    );
    let blocks: Vec<_> = [Block::Options, Block::Dpi]
        .into_iter()
        .filter(|b| !block_equal(b.bytes(&before), b.bytes(target)))
        .collect();
    if blocks.is_empty() {
        return Ok(MouseReceipt {
            backup_path: None,
            snapshot: before,
            changed: false,
        });
    }
    let backup_path = storage::backup(directory, "aj159-apex-settings", &before)?;
    ensure!(
        device.read()?.equivalent(&before),
        "Настройки изменились во время создания копии. Повторите чтение."
    );
    let mut attempted = Vec::new();
    let apply = (|| -> Result<MouseSnapshot> {
        for &block in &blocks {
            ensure!(
                device.read()?.options.profile == before.options.profile,
                "Mouse profile changed"
            );
            // A transport failure can occur after the device accepted a write.
            attempted.push(block);
            device.write(block, block.bytes(target))?;
        }
        let after = device.read()?;
        after.validate()?;
        ensure!(
            after.equivalent(target),
            "Проверка записанных параметров мыши не прошла"
        );
        Ok(after)
    })();
    match apply {
        Ok(snapshot) => Ok(MouseReceipt {
            backup_path: Some(backup_path),
            snapshot,
            changed: true,
        }),
        Err(error) => {
            let restore = (|| -> Result<()> {
                let current = device.read()?;
                ensure!(
                    current.mouse_id == before.mouse_id
                        && current.options.profile == before.options.profile,
                    "Профиль изменился; автоматическое восстановление остановлено"
                );
                let mut failures = Vec::new();
                for &block in attempted.iter().rev() {
                    if let Err(e) = device.write(block, block.bytes(&before)) {
                        failures.push(format!("{block:?}: {e:#}"));
                    }
                }
                let verified = device.read()?.equivalent(&before);
                ensure!(
                    verified,
                    "Восстановление не подтверждено; {}",
                    failures.join("; ")
                );
                Ok(())
            })();
            match restore {
                Ok(()) => bail!(
                    "{error:#}. Исходные параметры восстановлены и проверены. Копия: {}",
                    backup_path.display()
                ),
                Err(e) => bail!(
                    "{error:#}. Восстановление не подтверждено: {e:#}. Копия: {}",
                    backup_path.display()
                ),
            }
        }
    }
}

pub fn apply(
    expected: &MouseSnapshot,
    options: &MouseOptions,
    dpi: &MouseDpi,
) -> Result<MouseReceipt> {
    let target = expected.target(options, dpi)?;
    let device = crate::mouse_hid::MouseSession::open()?;
    commit(&device, expected, &target, &storage::backup_directory()?)
}

pub fn restore(path: &Path) -> Result<MouseReceipt> {
    let saved = MouseSnapshot::load(path)?;
    let device = crate::mouse_hid::MouseSession::open()?;
    let current = device.read()?;
    ensure!(
        current.options.profile == saved.options.profile,
        "Выбран другой профиль мыши"
    );
    // Only established editable fields are restored; current unknown fields stay intact.
    let target = current
        .target_with_sleep(&MouseSettings::from(&saved))
        .context("Резервная копия несовместима")?;
    commit(&device, &current, &target, &storage::backup_directory()?)
}
pub fn apply_settings(expected: &MouseSnapshot, settings: &MouseSettings) -> Result<MouseReceipt> {
    let target = expected.target_with_sleep(settings)?;
    commit(
        &crate::mouse_hid::MouseSession::open()?,
        expected,
        &target,
        &storage::backup_directory()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    #[test]
    fn sleep_edits_touch_only_radio_timeout_and_old_settings_still_load() {
        let before = fixture();
        assert_eq!(
            MouseSleep::parse(&before.raw_options)
                .unwrap()
                .radio_seconds,
            20
        );
        let mut settings = MouseSettings::from(&before);
        settings.sleep = Some(MouseSleep { radio_seconds: 300 });
        let target = before.target_with_sleep(&settings).unwrap();
        for i in (0..64).filter(|i| ![44, 45].contains(i)) {
            assert_eq!(target.raw_options[i], before.raw_options[i]);
        }
        let device = Mock::new();
        let receipt = commit(&device, &before, &target, &directory()).unwrap();
        assert_eq!(
            MouseSleep::parse(&receipt.snapshot.raw_options)
                .unwrap()
                .radio_seconds,
            300
        );
        let mut legacy = serde_json::to_value(&settings).unwrap();
        legacy.as_object_mut().unwrap().remove("sleep");
        let old: MouseSettings = serde_json::from_value(legacy).unwrap();
        assert!(old.sleep.is_none());
        assert!(before.target_with_sleep(&old).unwrap().equivalent(&before));
        settings.sleep = Some(MouseSleep {
            radio_seconds: 3601,
        });
        assert!(before.target_with_sleep(&settings).is_err());
    }
    fn fixture() -> MouseSnapshot {
        #[derive(Deserialize)]
        struct Raw {
            option0: Vec<u8>,
            option1: Vec<u8>,
        }
        let raw: Raw =
            serde_json::from_str(include_str!("../tests/fixtures/aj159-apex-read.json")).unwrap();
        let options = MouseOptions::parse(&raw.option0).unwrap();
        MouseSnapshot {
            captured_utc: String::new(),
            source: "test".into(),
            mouse_id: 0x800006db,
            last_battery_percent: None,
            polling_hz: options.polling_hz().unwrap(),
            options,
            dpi: MouseDpi::parse(&raw.option1).unwrap(),
            raw_options: raw.option0,
            raw_dpi: raw.option1,
        }
    }
    struct Mock {
        state: RefCell<MouseSnapshot>,
        writes: RefCell<Vec<Block>>,
        fail_at: Cell<usize>,
        corrupt: Cell<bool>,
    }
    impl Mock {
        fn new() -> Self {
            Self {
                state: RefCell::new(fixture()),
                writes: RefCell::new(Vec::new()),
                fail_at: Cell::new(0),
                corrupt: Cell::new(false),
            }
        }
    }
    impl MouseIo for Mock {
        fn read(&self) -> Result<MouseSnapshot> {
            Ok(self.state.borrow().clone())
        }
        fn write(&self, block: Block, raw: &[u8]) -> Result<()> {
            self.writes.borrow_mut().push(block);
            let mut state = self.state.borrow_mut();
            match block {
                Block::Options => {
                    state.raw_options = raw.to_vec();
                    state.options = MouseOptions::parse(raw)?;
                    state.polling_hz = state.options.polling_hz()?;
                }
                Block::Dpi => {
                    state.raw_dpi = raw.to_vec();
                    state.dpi = MouseDpi::parse(raw)?;
                }
            }
            if self.writes.borrow().len() == 2 && self.corrupt.replace(false) {
                state.raw_dpi[40] ^= 1;
                state.dpi.colors[0][0] ^= 1;
            }
            ensure!(
                self.writes.borrow().len() != self.fail_at.get(),
                "Simulated failure after device acceptance"
            );
            Ok(())
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!("ajazz-mouse-test-{}", storage::unique_suffix()))
    }

    fn target(before: &MouseSnapshot) -> MouseSnapshot {
        let mut options = before.options.clone();
        options.debounce_ms = 3;
        let mut dpi = before.dpi.clone();
        dpi.x[0] = 450;
        dpi.y[0] = 450;
        before.target(&options, &dpi).unwrap()
    }
    #[test]
    fn transaction_writes_verifies_and_saves_the_original() {
        let device = Mock::new();
        let before = fixture();
        let target = target(&before);
        let receipt = commit(&device, &before, &target, &directory()).unwrap();
        assert!(receipt.changed && receipt.snapshot.equivalent(&target));
        assert!(
            MouseSnapshot::load(receipt.backup_path.as_ref().unwrap())
                .unwrap()
                .equivalent(&before)
        );
        assert_eq!(*device.writes.borrow(), [Block::Options, Block::Dpi]);
    }
    #[test]
    fn partial_failure_restores_both_attempted_blocks_and_checks_them() {
        let device = Mock::new();
        device.fail_at.set(2);
        let before = fixture();
        let error = commit(&device, &before, &target(&before), &directory())
            .unwrap_err()
            .to_string();
        assert!(error.contains("восстановлены и проверены"), "{error}");
        assert!(device.state.borrow().equivalent(&before));
        assert_eq!(
            *device.writes.borrow(),
            [Block::Options, Block::Dpi, Block::Dpi, Block::Options]
        );
    }
    #[test]
    fn failed_rollback_ack_does_not_prevent_other_block_restore() {
        let device = Mock::new();
        device.corrupt.set(true);
        device.fail_at.set(3);
        let before = fixture();
        let error = commit(&device, &before, &target(&before), &directory())
            .unwrap_err()
            .to_string();
        assert!(error.contains("восстановлены и проверены"), "{error}");
        assert!(device.state.borrow().equivalent(&before));
        assert_eq!(device.writes.borrow().len(), 4);
    }
    #[test]
    fn stale_snapshot_wrong_profile_and_unknown_fields_write_nothing() {
        let device = Mock::new();
        let before = fixture();
        let mut bad = target(&before);
        bad.raw_options[20] ^= 1;
        assert!(commit(&device, &before, &bad, &directory()).is_err());
        device.state.borrow_mut().raw_options[20] ^= 1;
        assert!(commit(&device, &before, &target(&before), &directory()).is_err());
        device.state.borrow_mut().options.profile = 1;
        assert!(commit(&device, &before, &target(&before), &directory()).is_err());
        assert!(device.writes.borrow().is_empty());
    }
    #[test]
    fn backup_failure_and_no_changes_never_write_device() {
        let device = Mock::new();
        let before = fixture();
        let path = directory();
        std::fs::write(&path, b"not a directory").unwrap();
        assert!(commit(&device, &before, &target(&before), &path).is_err());
        let receipt = commit(&device, &before, &before, &path).unwrap();
        assert!(!receipt.changed && receipt.backup_path.is_none());
        assert!(device.writes.borrow().is_empty());
    }
    #[test]
    fn parameter_edits_preserve_every_unknown_byte_and_enforce_hardware_limits() {
        let before = fixture();
        let mut options = before.options.clone();
        options.debounce_ms = 3;
        let raw = options.patch(&before.raw_options).unwrap();
        for i in (0..64).filter(|i| *i != 10) {
            assert_eq!(raw[i], before.raw_options[i]);
        }
        options.debounce_ms = 30;
        assert!(options.patch(&before.raw_options).is_err());
        options.debounce_ms = 2;
        options.lift_off = 3;
        assert!(options.patch(&before.raw_options).is_err());
        let mut dpi = before.dpi.clone();
        dpi.x[2] = 0;
        dpi.y[2] = 0;
        assert!(dpi.patch(&before.raw_dpi).is_err());
        dpi.x[2] = 42050;
        dpi.y[2] = 42050;
        assert!(dpi.patch(&before.raw_dpi).is_err());
    }
}
