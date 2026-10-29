//! AJ159 APEX main button matrix. D0 replies have no transport header.
use crate::storage;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BUTTONS: &[(u8, &str)] = &[
    (0, "Левая кнопка"),
    (1, "Правая кнопка"),
    (2, "Нажатие колеса"),
    (3, "Боковая: вперёд"),
    (4, "Боковая: назад"),
    (5, "Кнопка DPI"),
    (14, "Прокрутка вверх"),
    (15, "Прокрутка вниз"),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    None,
    Ctrl,
    Shift,
    Alt,
    Win,
}
impl Modifier {
    pub const ALL: [Self; 5] = [Self::None, Self::Ctrl, Self::Shift, Self::Alt, Self::Win];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Без модификатора",
            Self::Ctrl => "Ctrl",
            Self::Shift => "Shift",
            Self::Alt => "Alt",
            Self::Win => "Win",
        }
    }
    fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Ctrl => 224,
            Self::Shift => 225,
            Self::Alt => 226,
            Self::Win => 227,
        }
    }
    fn decode(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.code() == value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    LeftClick,
    RightClick,
    MiddleClick,
    Forward,
    Back,
    DpiCycle,
    ScrollUp,
    ScrollDown,
    Disabled,
    Key { usage: u8, modifier: Modifier },
}
fn known_key(usage: u8) -> bool {
    usage != 0
        && crate::model::keyboard_rows()
            .iter()
            .flatten()
            .any(|k| k.hid == usage)
}
impl Action {
    pub fn choices() -> Vec<Self> {
        vec![
            Self::LeftClick,
            Self::RightClick,
            Self::MiddleClick,
            Self::Forward,
            Self::Back,
            Self::DpiCycle,
            Self::ScrollUp,
            Self::ScrollDown,
            Self::Disabled,
            Self::Key {
                usage: 4,
                modifier: Modifier::None,
            },
        ]
    }
    pub fn label(self) -> String {
        match self {
            Self::LeftClick => "Левый клик".into(),
            Self::RightClick => "Правый клик".into(),
            Self::MiddleClick => "Средний клик".into(),
            Self::Forward => "Вперёд".into(),
            Self::Back => "Назад".into(),
            Self::DpiCycle => "Следующая ступень DPI".into(),
            Self::ScrollUp => "Прокрутка вверх".into(),
            Self::ScrollDown => "Прокрутка вниз".into(),
            Self::Disabled => "Отключена".into(),
            Self::Key { usage, modifier } => {
                let key = crate::model::keyboard_rows()
                    .into_iter()
                    .flatten()
                    .find(|k| k.hid == usage)
                    .map(|k| k.physical)
                    .unwrap_or_else(|| format!("HID {usage}"));
                if modifier == Modifier::None {
                    key
                } else {
                    format!("{}+{key}", modifier.label())
                }
            }
        }
    }
    pub fn encode(self) -> Result<[u8; 4]> {
        Ok(match self {
            Self::LeftClick => [1, 0, 240, 0],
            Self::RightClick => [1, 0, 241, 0],
            Self::MiddleClick => [1, 0, 242, 0],
            Self::Forward => [1, 0, 244, 0],
            Self::Back => [1, 0, 243, 0],
            Self::DpiCycle => [20, 0, 0, 0],
            Self::ScrollUp => [1, 0, 245, 1],
            Self::ScrollDown => [1, 0, 245, 255],
            Self::Disabled => [0; 4],
            Self::Key { usage, modifier } => {
                ensure!(known_key(usage), "Неизвестная клавиша HID");
                [0, modifier.code(), usage, 0]
            }
        })
    }
    pub fn decode(raw: &[u8]) -> Option<Self> {
        Self::choices()
            .into_iter()
            .filter(|a| !matches!(a, Self::Key { .. }))
            .find(|a| a.encode().ok().as_ref().map(|v| v.as_slice()) == Some(raw))
            .or_else(|| {
                if raw.len() == 4 && raw[0] == 0 && raw[3] == 0 && known_key(raw[2]) {
                    Some(Self::Key {
                        usage: raw[2],
                        modifier: Modifier::decode(raw[1])?,
                    })
                } else {
                    None
                }
            })
    }
    pub fn default_for(slot: u8) -> Option<Self> {
        Some(match slot {
            0 => Self::LeftClick,
            1 => Self::RightClick,
            2 => Self::MiddleClick,
            3 => Self::Forward,
            4 => Self::Back,
            5 => Self::DpiCycle,
            14 => Self::ScrollUp,
            15 => Self::ScrollDown,
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonEdit {
    pub slot: u8,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ButtonSnapshot {
    pub schema: u32,
    pub captured_utc: String,
    pub mouse_id: u32,
    pub profile: u8,
    pub raw_matrix: Vec<u8>,
}
impl ButtonSnapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.mouse_id == 0x800006db && self.profile <= 3,
            "Unsupported button snapshot"
        );
        ensure!(
            self.raw_matrix.len() == 64,
            "Mouse button matrix must contain 16 four-byte records"
        );
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = storage::read_json(path, 16 * 1024)?;
        value.validate()?;
        Ok(value)
    }
    pub fn equivalent(&self, other: &Self) -> bool {
        self.mouse_id == other.mouse_id
            && self.profile == other.profile
            && self.raw_matrix == other.raw_matrix
    }
    pub fn action(&self, slot: u8) -> Option<Action> {
        self.raw_matrix
            .get(usize::from(slot) * 4..usize::from(slot) * 4 + 4)
            .and_then(Action::decode)
    }
    pub fn target(&self, edit: &ButtonEdit) -> Result<Self> {
        self.validate()?;
        ensure!(
            BUTTONS.iter().any(|(s, _)| *s == edit.slot),
            "Служебная запись недоступна для изменения"
        );
        ensure!(
            self.action(edit.slot).is_some(),
            "Текущее сложное назначение пока не поддерживается редактором"
        );
        let mut target = self.clone();
        let start = usize::from(edit.slot) * 4;
        target.raw_matrix[start..start + 4].copy_from_slice(&edit.action.encode()?);
        validate_target(self, &target)?;
        Ok(target)
    }
}
fn validate_target(before: &ButtonSnapshot, target: &ButtonSnapshot) -> Result<()> {
    before.validate()?;
    target.validate()?;
    ensure!(
        before.mouse_id == target.mouse_id && before.profile == target.profile,
        "Другой профиль мыши"
    );
    for slot in 0..16u8 {
        let start = usize::from(slot) * 4;
        if before.raw_matrix[start..start + 4] != target.raw_matrix[start..start + 4] {
            ensure!(
                BUTTONS.iter().any(|(s, _)| *s == slot)
                    && before.action(slot).is_some()
                    && target.action(slot).is_some(),
                "Изменены неизвестные записи кнопок"
            );
        }
    }
    ensure!(
        (0..6).any(|slot| target.action(slot) == Some(Action::LeftClick)),
        "Хотя бы одна физическая кнопка должна сохранить левый клик"
    );
    Ok(())
}
// The vendor's _setKeyConfig uses generic command 09, not unused mouse command 50.
pub(crate) fn packets(profile: u8, matrix: &[u8]) -> Result<Vec<[u8; 64]>> {
    ensure!(profile <= 3 && matrix.len() == 64, "Invalid button write");
    Ok(matrix
        .chunks(56)
        .enumerate()
        .map(|(index, chunk)| {
            let mut packet = [0; 64];
            packet[..8].copy_from_slice(&[9, profile, 64, 0, index as u8, 0, 0, 0]);
            packet[7] =
                255u8.wrapping_sub(packet[..7].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)));
            packet[8..8 + chunk.len()].copy_from_slice(chunk);
            packet
        })
        .collect())
}
pub(crate) trait ButtonsIo {
    fn read_buttons(&self) -> Result<ButtonSnapshot>;
    fn write_buttons(&self, profile: u8, matrix: &[u8]) -> Result<()>;
}
#[derive(Clone, Debug, Serialize)]
pub struct ButtonReceipt {
    pub changed: bool,
    pub backup_path: Option<PathBuf>,
    pub snapshot: ButtonSnapshot,
}
fn commit(
    device: &impl ButtonsIo,
    expected: &ButtonSnapshot,
    target: &ButtonSnapshot,
    directory: &Path,
) -> Result<ButtonReceipt> {
    validate_target(expected, target)?;
    let before = device.read_buttons()?;
    before.validate()?;
    ensure!(
        before.equivalent(expected),
        "Назначения изменились. Прочитайте кнопки заново."
    );
    if before.equivalent(target) {
        return Ok(ButtonReceipt {
            changed: false,
            backup_path: None,
            snapshot: before,
        });
    }
    let backup_path = storage::backup(directory, "aj159-apex-buttons", &before)?;
    ensure!(
        device.read_buttons()?.equivalent(&before),
        "Назначения изменились во время создания копии"
    );
    let result = (|| -> Result<ButtonSnapshot> {
        device.write_buttons(before.profile, &target.raw_matrix)?;
        let after = device.read_buttons()?;
        after.validate()?;
        ensure!(
            after.equivalent(target),
            "Проверка назначений после записи не прошла"
        );
        Ok(after)
    })();
    match result {
        Ok(snapshot) => Ok(ButtonReceipt {
            changed: true,
            backup_path: Some(backup_path),
            snapshot,
        }),
        Err(error) => {
            let restore = (|| -> Result<()> {
                let current = device.read_buttons()?;
                ensure!(
                    current.mouse_id == before.mouse_id && current.profile == before.profile,
                    "Профиль изменился; восстановление остановлено"
                );
                let write = device.write_buttons(before.profile, &before.raw_matrix);
                ensure!(
                    device.read_buttons()?.equivalent(&before),
                    "Восстановление не подтверждено: {write:?}"
                );
                Ok(())
            })();
            match restore {
                Ok(()) => bail!(
                    "{error:#}. Исходные назначения восстановлены и проверены. Копия: {}",
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
pub fn read() -> Result<ButtonSnapshot> {
    crate::mouse_hid::MouseSession::open()?.read_buttons()
}
pub fn apply(expected: &ButtonSnapshot, edit: &ButtonEdit) -> Result<ButtonReceipt> {
    let target = expected.target(edit)?;
    commit(
        &crate::mouse_hid::MouseSession::open()?,
        expected,
        &target,
        &storage::backup_directory()?,
    )
}
pub fn restore(path: &Path) -> Result<ButtonReceipt> {
    let saved = ButtonSnapshot::load(path)?;
    let device = crate::mouse_hid::MouseSession::open()?;
    let before = device.read_buttons()?;
    ensure!(
        before.mouse_id == saved.mouse_id && before.profile == saved.profile,
        "Другой профиль мыши"
    );
    let mut target = before.clone();
    for &(slot, _) in BUTTONS {
        let start = usize::from(slot) * 4;
        target.raw_matrix[start..start + 4].copy_from_slice(&saved.raw_matrix[start..start + 4]);
    }
    commit(&device, &before, &target, &storage::backup_directory()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn fixture() -> ButtonSnapshot {
        serde_json::from_str(include_str!("../tests/fixtures/aj159-apex-buttons.json")).unwrap()
    }
    #[test]
    fn actual_matrix_decodes_and_modifiers_are_hid_usages() {
        let state = fixture();
        state.validate().unwrap();
        for &(slot, _) in BUTTONS {
            assert_eq!(state.action(slot), Action::default_for(slot));
        }
        let action = Action::Key {
            usage: 6,
            modifier: Modifier::Ctrl,
        };
        assert_eq!(action.encode().unwrap(), [0, 224, 6, 0]);
        assert_eq!(Action::decode(&[0, 224, 6, 0]), Some(action));
        assert_eq!(Action::decode(&[0, 1, 6, 0]), None);
    }
    #[test]
    fn chunking_preserves_every_matrix_byte_including_offset_seven() {
        let mut state = fixture();
        state.raw_matrix[7] = 165;
        let encoded = packets(0, &state.raw_matrix).unwrap();
        assert_eq!(encoded[0][..8], [9, 0, 64, 0, 0, 0, 0, 182]);
        assert_eq!(encoded[1][..8], [9, 0, 64, 0, 1, 0, 0, 181]);
        let decoded: Vec<_> = encoded
            .iter()
            .flat_map(|p| p[8..].iter().copied())
            .take(64)
            .collect();
        assert_eq!(decoded, state.raw_matrix);
        assert!(encoded[1][16..].iter().all(|b| *b == 0));
    }
    struct Mock {
        state: RefCell<ButtonSnapshot>,
        writes: Cell<usize>,
        fail: Cell<bool>,
    }
    impl ButtonsIo for Mock {
        fn read_buttons(&self) -> Result<ButtonSnapshot> {
            Ok(self.state.borrow().clone())
        }
        fn write_buttons(&self, _: u8, matrix: &[u8]) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            if self.fail.replace(false) {
                self.state.borrow_mut().raw_matrix[..56].copy_from_slice(&matrix[..56]);
                bail!("lost second chunk");
            }
            self.state.borrow_mut().raw_matrix.copy_from_slice(matrix);
            Ok(())
        }
    }
    fn mock(fail: bool) -> Mock {
        Mock {
            state: RefCell::new(fixture()),
            writes: Cell::new(0),
            fail: Cell::new(fail),
        }
    }
    fn edit() -> ButtonEdit {
        ButtonEdit {
            slot: 4,
            action: Action::Forward,
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!("ajazz-buttons-test-{}", storage::unique_suffix()))
    }
    #[test]
    fn transaction_writes_verifies_and_restores_partial_failure() {
        let before = fixture();
        let target = before.target(&edit()).unwrap();
        let device = mock(false);
        let receipt = commit(&device, &before, &target, &directory()).unwrap();
        assert!(receipt.changed && receipt.backup_path.unwrap().exists());
        assert!(device.state.borrow().equivalent(&target));
        let device = mock(true);
        let error = commit(&device, &before, &target, &directory())
            .unwrap_err()
            .to_string();
        assert!(error.contains("восстановлены и проверены"));
        assert!(device.state.borrow().equivalent(&before));
        assert_eq!(device.writes.get(), 2);
    }
    #[test]
    fn rejects_stale_reserved_and_loss_of_left_click_before_writing() {
        let before = fixture();
        assert!(
            before
                .target(&ButtonEdit {
                    slot: 0,
                    action: Action::Disabled
                })
                .is_err()
        );
        let mut reserved = before.clone();
        reserved.raw_matrix[48] = 0;
        assert!(validate_target(&before, &reserved).is_err());
        let device = mock(false);
        device.state.borrow_mut().raw_matrix[7] = 1;
        assert!(
            commit(
                &device,
                &before,
                &before.target(&edit()).unwrap(),
                &directory()
            )
            .is_err()
        );
        assert_eq!(device.writes.get(), 0);
        let device = mock(false);
        let path = directory();
        std::fs::write(&path, b"file").unwrap();
        assert!(commit(&device, &before, &before.target(&edit()).unwrap(), &path).is_err());
        assert_eq!(device.writes.get(), 0);
    }
}
