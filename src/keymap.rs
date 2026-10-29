use crate::{hardware::Snapshot, model::keyboard_rows};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MEDIA: &[(u16, &str)] = &[
    (404, "Проводник"),
    (402, "Калькулятор"),
    (394, "Почта"),
    (387, "Медиаплеер"),
    (205, "Воспроизведение / пауза"),
    (183, "Стоп"),
    (182, "Предыдущий трек"),
    (181, "Следующий трек"),
    (233, "Громче"),
    (234, "Тише"),
    (226, "Выключить звук"),
    (547, "Домашняя страница браузера"),
    (551, "Обновить страницу"),
    (549, "Вперёд в браузере"),
    (548, "Назад в браузере"),
    (554, "Избранное"),
    (545, "Поиск"),
];
pub const MODIFIERS: &[(u8, &str)] = &[
    (224, "L-Ctrl"),
    (225, "L-Shift"),
    (226, "L-Alt"),
    (227, "L-Win"),
    (228, "R-Ctrl"),
    (229, "R-Shift"),
    (230, "R-Alt"),
    (231, "R-Win"),
];
pub const MOUSE_ACTIONS: &[(u8, u8, &str)] = &[
    (1, 1, "Левый клик"),
    (1, 2, "Правый клик"),
    (1, 4, "Средний клик"),
    (1, 8, "Назад"),
    (1, 16, "Вперёд"),
    (3, 1, "Колесо вверх"),
    (3, 255, "Колесо вниз"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Assignment {
    Default,
    Key { usage: u8 },
    Media { usage: u16 },
    Toggle { usage: u8 },
    ModTap { hold: u8, tap: u8, hold_ms: u16 },
    Socd { mode: u8, first: u8, second: u8 },
    RappySnappy { first: u8, second: u8 },
    Dks { slot: u8 },
    Macro { slot: u8, mode: u8, repeats: u8 },
    Combo { first: u8, second: u8, key: u8 },
    Mouse { kind: u8, value: u8 },
}
pub const SOCD_MODES: &[(u8, &str)] = &[
    (3, "Последняя нажатая"),
    (1, "Приоритет первой клавиши"),
    (2, "Приоритет второй клавиши"),
    (4, "Обе нажаты — ни одна не активна"),
];
pub fn usage_label(usage: u8) -> String {
    keyboard_rows()
        .into_iter()
        .flatten()
        .find(|k| k.hid == usage && usage != 0)
        .map(|k| k.physical)
        .unwrap_or_else(|| "Неизвестная клавиша".into())
}
fn valid_usage(usage: u8) -> bool {
    usage != 0
        && keyboard_rows()
            .into_iter()
            .flatten()
            .any(|k| k.hid == usage)
}
impl Assignment {
    pub fn pair(&self) -> Option<[u8; 2]> {
        match *self {
            Self::Socd { first, second, .. } | Self::RappySnappy { first, second } => {
                Some([first, second])
            }
            _ => None,
        }
    }
    pub fn encode(&self) -> Result<[u8; 4]> {
        Ok(match *self {
            Self::Combo { first, second, key } => {
                ensure!(
                    (first == 0 || (224..=231).contains(&first))
                        && (second == 0 || (224..=231).contains(&second))
                        && (first != 0 || second != 0)
                        && (first == 0 || first != second),
                    "Выберите один или два разных модификатора"
                );
                ensure!(
                    valid_usage(key) && !(224..=231).contains(&key),
                    "Выберите обычную клавишу сочетания"
                );
                [7, first, second, key]
            }
            Self::Mouse { kind, value } => {
                ensure!(
                    MOUSE_ACTIONS
                        .iter()
                        .any(|(k, v, _)| *k == kind && *v == value),
                    "Неизвестная функция мыши"
                );
                [1, kind, value, 0]
            }
            Self::Default => [0; 4],
            Self::Dks { slot } => {
                ensure!(slot < 64, "Invalid DKS slot");
                [8, slot, 0, 0]
            }
            Self::Macro {
                slot,
                mode,
                repeats,
            } => {
                ensure!(
                    slot < 100 && [0, 1, 2].contains(&mode),
                    "Invalid macro assignment"
                );
                ensure!(
                    if mode == 1 { repeats > 0 } else { repeats == 0 },
                    "Invalid macro repeat count"
                );
                [6, slot, mode, repeats]
            }
            Self::Key { usage } => {
                ensure!(
                    usage != 0
                        && keyboard_rows()
                            .into_iter()
                            .flatten()
                            .any(|k| k.hid == usage),
                    "Unsupported key usage"
                );
                [2, 0, usage, 0]
            }
            Self::Media { usage } => {
                ensure!(
                    MEDIA.iter().any(|(code, _)| *code == usage),
                    "Unsupported media function"
                );
                let [lo, hi] = usage.to_le_bytes();
                [3, lo, hi, 0]
            }
            Self::Toggle { usage } => {
                ensure!(valid_usage(usage), "Unknown toggle key");
                [10, usage, 0, 0]
            }
            Self::ModTap { hold, tap, hold_ms } => {
                ensure!(valid_usage(hold) && valid_usage(tap), "Unknown Mod-Tap key");
                ensure!(
                    hold_ms <= 1000 && hold_ms.is_multiple_of(10),
                    "MT: время 10–1000 мс, шаг 10 мс; 0 — штатные 400 мс"
                );
                [9, hold, tap, (hold_ms / 10) as u8]
            }
            Self::Socd {
                mode,
                first,
                second,
            } => {
                ensure!(
                    SOCD_MODES.iter().any(|(value, _)| *value == mode),
                    "Unknown SOCD mode"
                );
                ensure!(
                    first != second && valid_usage(first) && valid_usage(second),
                    "Выберите две разные клавиши пары"
                );
                [11, mode, first, second]
            }
            Self::RappySnappy { first, second } => {
                ensure!(
                    first != second && valid_usage(first) && valid_usage(second),
                    "Выберите две разные клавиши пары"
                );
                [12, 0, first, second]
            }
        })
    }
    pub fn decode(raw: &[u8]) -> Option<Self> {
        let candidate = match raw {
            [7, first, second, key] => Self::Combo {
                first: *first,
                second: *second,
                key: *key,
            },
            [1, kind, value, 0] => Self::Mouse {
                kind: *kind,
                value: *value,
            },
            [0, 0, 0, 0] => Self::Default,
            [8, slot, 0, 0] => Self::Dks { slot: *slot },
            [6, slot, mode, repeats] => Self::Macro {
                slot: *slot,
                mode: *mode,
                repeats: *repeats,
            },
            [2, 0, usage, 0] => Self::Key { usage: *usage },
            [3, lo, hi, 0] => Self::Media {
                usage: u16::from_le_bytes([*lo, *hi]),
            },
            [10, usage, 0, 0] => Self::Toggle { usage: *usage },
            [9, hold, tap, ticks] => Self::ModTap {
                hold: *hold,
                tap: *tap,
                hold_ms: u16::from(*ticks) * 10,
            },
            [11, mode, first, second] => Self::Socd {
                mode: *mode,
                first: *first,
                second: *second,
            },
            [12, 0, first, second] => Self::RappySnappy {
                first: *first,
                second: *second,
            },
            _ => return None,
        };
        candidate.encode().ok().map(|_| candidate)
    }
    pub fn label(&self) -> String {
        match self {
            Self::Combo { first, second, key } => {
                let mut labels: Vec<String> = [*first, *second]
                    .into_iter()
                    .filter(|v| *v != 0)
                    .filter_map(|v| {
                        MODIFIERS
                            .iter()
                            .find(|(code, _)| *code == v)
                            .map(|(_, label)| (*label).to_owned())
                    })
                    .collect();
                labels.push(usage_label(*key));
                labels.join(" + ")
            }
            Self::Mouse { kind, value } => MOUSE_ACTIONS
                .iter()
                .find(|(k, v, _)| k == kind && v == value)
                .map(|(_, _, label)| (*label).to_owned())
                .unwrap_or_default(),
            Self::Default => "По умолчанию".into(),
            Self::Dks { slot } => format!("DKS · ячейка {}", slot + 1),
            Self::Macro { slot, .. } => format!("Макрос {}", slot + 1),
            Self::Key { usage } => keyboard_rows()
                .into_iter()
                .flatten()
                .find(|k| k.hid == *usage)
                .map(|k| k.physical)
                .unwrap_or_else(|| "Неизвестная клавиша".into()),
            Self::Media { usage } => MEDIA
                .iter()
                .find(|(code, _)| code == usage)
                .map(|(_, label)| label.to_string())
                .unwrap_or_default(),
            Self::Toggle { usage } => format!("TGL · {}", usage_label(*usage)),
            Self::ModTap { hold, tap, hold_ms } => format!(
                "MT · удержание {} / нажатие {}, {} мс",
                usage_label(*hold),
                usage_label(*tap),
                if *hold_ms == 0 { 400 } else { *hold_ms }
            ),
            Self::Socd {
                mode,
                first,
                second,
            } => format!(
                "SOCD · {} + {} · {}",
                usage_label(*first),
                usage_label(*second),
                SOCD_MODES
                    .iter()
                    .find(|(value, _)| value == mode)
                    .map(|(_, label)| *label)
                    .unwrap_or("Неизвестный режим")
            ),
            Self::RappySnappy { first, second } => {
                format!("RS · {} + {}", usage_label(*first), usage_label(*second))
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeymapEdit {
    pub keys: BTreeSet<u8>,
    pub assignment: Assignment,
}

pub fn target(snapshot: &Snapshot, edit: &KeymapEdit) -> Result<Vec<u8>> {
    ensure!(
        !matches!(
            edit.assignment,
            Assignment::Dks { .. } | Assignment::Macro { .. }
        ),
        "Используйте редактор DKS или макросов вместе с таблицей действий"
    );
    let before = snapshot
        .blocks
        .get("keymap")
        .ok_or_else(|| anyhow::anyhow!("No keymap snapshot"))?;
    ensure!(before.len() == 512, "Invalid keymap");
    let ids: BTreeSet<_> = keyboard_rows()
        .into_iter()
        .flatten()
        .filter(|k| k.hid != 0)
        .map(|k| k.id)
        .collect();
    ensure!(
        !edit.keys.is_empty() && edit.keys.is_subset(&ids),
        "Выберите клавиши; Fn пока не переназначается"
    );
    let mut target = before.clone();
    let encoded = edit.assignment.encode()?;
    for &key in &edit.keys {
        let at = usize::from(key) * 4;
        if let Some(pair) =
            Assignment::decode(&before[at..at + 4]).and_then(|assignment| assignment.pair())
        {
            let partners: BTreeSet<_> = keyboard_rows()
                .into_iter()
                .flatten()
                .filter(|key| pair.contains(&key.hid))
                .map(|key| key.id)
                .collect();
            ensure!(
                partners.len() == 2 && partners.is_subset(&edit.keys),
                "Для изменения парного назначения выберите обе его клавиши"
            );
        }
        ensure!(
            Assignment::decode(&before[at..at + 4]).is_some(),
            "На клавише сложное назначение. Его редактор ещё не перенесён."
        );
        target[at..at + 4].copy_from_slice(&encoded);
    }
    if let Some(pair) = edit.assignment.pair() {
        let usages: BTreeSet<_> = keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|key| edit.keys.contains(&key.id))
            .map(|key| key.hid)
            .collect();
        ensure!(
            edit.keys.len() == 2 && usages == BTreeSet::from(pair),
            "Выделите ровно две клавиши, указанные в паре"
        );
    }
    validate_pairs(&target)?;
    Ok(target)
}

pub fn validate_pairs(data: &[u8]) -> Result<()> {
    ensure!(data.len() == 512, "Invalid keymap");
    let keys: Vec<_> = keyboard_rows().into_iter().flatten().collect();
    for key in &keys {
        let offset = usize::from(key.id) * 4;
        if let Some(pair) =
            Assignment::decode(&data[offset..offset + 4]).and_then(|assignment| assignment.pair())
        {
            ensure!(
                pair.contains(&key.hid),
                "Парное назначение записано на постороннюю клавишу"
            );
            let partners: Vec<_> = keys.iter().filter(|key| pair.contains(&key.hid)).collect();
            ensure!(
                partners.len() == 2
                    && partners.iter().all(|key| {
                        let at = usize::from(key.id) * 4;
                        data[at..at + 4] == data[offset..offset + 4]
                    }),
                "Пара RS/SOCD неполна или её параметры различаются"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        let mut info = [0; 48];
        info[4..8].copy_from_slice(&[0x45, 0x0c, 0xa0, 0x80]);
        info[8..10].copy_from_slice(&[0x30, 1]);
        info[12..16].copy_from_slice(&[0x66, 1, 4, 0x10]);
        info[16] = 2;
        let mut data = vec![0; 512];
        // An unknown unused matrix slot must survive every edit unchanged.
        data[508..512].copy_from_slice(&[0xf1, 0x7e, 0xaa, 0x55]);
        Snapshot {
            schema: 1,
            endpoint: crate::hardware::Endpoint {
                path: "test".into(),
                vid: 0x0c45,
                pid: 0xfefc,
                product: "AJAZZ AK820MAX".into(),
                usage_page: 0xff80,
                usage: 1,
                interface: 2,
                role: String::new(),
            },
            info: crate::protocol::KeyboardInfo::parse(&info).unwrap(),
            blocks: [("keymap".into(), data)].into(),
        }
    }
    #[test]
    fn combo_and_mouse_edits_preserve_unselected_and_reject_invalid_actions() {
        let before = snapshot();
        for assignment in [
            Assignment::Combo {
                first: 224,
                second: 225,
                key: 4,
            },
            Assignment::Mouse {
                kind: 3,
                value: 255,
            },
        ] {
            let target = target(
                &before,
                &KeymapEdit {
                    keys: BTreeSet::from([12]),
                    assignment: assignment.clone(),
                },
            )
            .unwrap();
            assert_eq!(Assignment::decode(&target[48..52]), Some(assignment));
            assert_eq!(&target[..48], &before.blocks["keymap"][..48]);
            assert_eq!(&target[52..], &before.blocks["keymap"][52..]);
        }
        for raw in [
            [7, 224, 224, 4],
            [7, 0, 0, 4],
            [7, 224, 0, 225],
            [1, 3, 254, 0],
            [1, 1, 1, 1],
        ] {
            assert!(Assignment::decode(&raw).is_none());
        }
    }
    #[test]
    fn advanced_records_match_vendor_encoder_and_keep_mt_zero_sentinel() {
        for (assignment, bytes) in [
            (Assignment::Toggle { usage: 4 }, [10, 4, 0, 0]),
            (
                Assignment::ModTap {
                    hold: 224,
                    tap: 4,
                    hold_ms: 400,
                },
                [9, 224, 4, 40],
            ),
            (
                Assignment::ModTap {
                    hold: 224,
                    tap: 4,
                    hold_ms: 0,
                },
                [9, 224, 4, 0],
            ),
            (
                Assignment::Socd {
                    mode: 3,
                    first: 4,
                    second: 7,
                },
                [11, 3, 4, 7],
            ),
            (
                Assignment::RappySnappy {
                    first: 4,
                    second: 7,
                },
                [12, 0, 4, 7],
            ),
        ] {
            assert_eq!(assignment.encode().unwrap(), bytes);
            assert_eq!(Assignment::decode(&bytes).unwrap(), assignment);
        }
        assert!(
            Assignment::ModTap {
                hold: 224,
                tap: 4,
                hold_ms: 401
            }
            .encode()
            .is_err()
        );
        assert!(
            Assignment::Socd {
                mode: 5,
                first: 4,
                second: 7
            }
            .encode()
            .is_err()
        );
        assert!(
            Assignment::RappySnappy {
                first: 4,
                second: 4
            }
            .encode()
            .is_err()
        );
    }
    #[test]
    fn paired_assignments_are_atomic_and_unrelated_matrix_records_survive() {
        let before = snapshot();
        let physical: Vec<_> = keyboard_rows()
            .into_iter()
            .flatten()
            .filter(|key| [4, 7].contains(&key.hid))
            .collect();
        let keys: BTreeSet<_> = physical.iter().map(|key| key.id).collect();
        assert_eq!(keys.len(), 2);
        let assignment = Assignment::Socd {
            mode: 3,
            first: 4,
            second: 7,
        };
        let edited = target(
            &before,
            &KeymapEdit {
                keys: keys.clone(),
                assignment: assignment.clone(),
            },
        )
        .unwrap();
        let encoded = assignment.encode().unwrap();
        for id in 0..128u8 {
            let offset = usize::from(id) * 4;
            assert_eq!(
                &edited[offset..offset + 4],
                if keys.contains(&id) {
                    &encoded
                } else {
                    &before.blocks["keymap"][offset..offset + 4]
                }
            );
        }
        let mut paired = before.clone();
        paired.blocks.insert("keymap".into(), edited.clone());
        assert!(
            target(
                &paired,
                &KeymapEdit {
                    keys: BTreeSet::from([physical[0].id]),
                    assignment: Assignment::Default
                }
            )
            .is_err()
        );
        let reset = target(
            &paired,
            &KeymapEdit {
                keys: keys.clone(),
                assignment: Assignment::Default,
            },
        )
        .unwrap();
        assert_eq!(reset, before.blocks["keymap"]);
        let mut broken = edited;
        let offset = usize::from(physical[1].id) * 4;
        broken[offset..offset + 4].fill(0);
        assert!(validate_pairs(&broken).is_err());
        assert!(
            target(
                &before,
                &KeymapEdit {
                    keys,
                    assignment: Assignment::Socd {
                        mode: 3,
                        first: 26,
                        second: 22
                    }
                }
            )
            .is_err()
        );
    }
}
