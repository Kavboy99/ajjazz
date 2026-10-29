//! Autonomous AJSC page schedule and bounded media library protocol.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Kind {
    #[default]
    Summary = 0,
    Cpu = 1,
    Memory = 2,
    Clock = 3,
    Media = 4,
    Analog = 5,
    Calendar = 6,
    Focus = 7,
    Stopwatch = 8,
    Gaming = 9,
    Thermals = 10,
    Gpu = 11,
    Pulse = 12,
    Custom = 13,
}
impl Kind {
    pub const ALL: [Self; 14] = [
        Self::Summary,
        Self::Cpu,
        Self::Memory,
        Self::Clock,
        Self::Media,
        Self::Analog,
        Self::Calendar,
        Self::Focus,
        Self::Stopwatch,
        Self::Gaming,
        Self::Thermals,
        Self::Gpu,
        Self::Pulse,
        Self::Custom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Summary => "Сводка компьютера",
            Self::Cpu => "Загрузка CPU",
            Self::Memory => "Память",
            Self::Clock => "Часы и дата",
            Self::Media => "Фото / GIF",
            Self::Analog => "Аналоговые часы",
            Self::Calendar => "Календарь",
            Self::Focus => "Таймер фокуса",
            Self::Stopwatch => "Секундомер",
            Self::Gaming => "В игре",
            Self::Thermals => "Температуры",
            Self::Gpu => "Видеокарта",
            Self::Pulse => "Пульс системы",
            Self::Custom => "Свой экран",
        }
    }
    fn decode(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Summary,
            1 => Self::Cpu,
            2 => Self::Memory,
            3 => Self::Clock,
            4 => Self::Media,
            5 => Self::Analog,
            6 => Self::Calendar,
            7 => Self::Focus,
            8 => Self::Stopwatch,
            9 => Self::Gaming,
            10 => Self::Thermals,
            11 => Self::Gpu,
            12 => Self::Pulse,
            13..=24 => Self::Custom,
            _ => anyhow::bail!("Неизвестный тип страницы"),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub enabled: bool,
    pub kind: Kind,
    pub asset: u8,
    pub seconds: u16,
    #[serde(default)]
    pub composition: crate::dock_composition::Composition,
}
impl Default for Page {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: Kind::Summary,
            asset: 0,
            seconds: 10,
            composition: Default::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Playlist {
    pub enabled: bool,
    pub autoplay: bool,
    pub skip_offline: bool,
    pub selected: u8,
    pub clock_scale: u8,
    pub clock_position: [u8; 2],
    pub show_date: bool,
    pub show_seconds: bool,
    pub detail_scale: u8,
    pub cpu_position: [u8; 2],
    pub memory_position: [u8; 2],
    pub pages: Vec<Page>,
}
impl Default for Playlist {
    fn default() -> Self {
        Self {
            enabled: true,
            autoplay: true,
            skip_offline: true,
            selected: 0,
            clock_scale: 3,
            clock_position: [19, 45],
            show_date: true,
            show_seconds: true,
            detail_scale: 3,
            cpu_position: [6, 42],
            memory_position: [6, 42],
            pages: Kind::ALL[..4]
                .iter()
                .map(|&kind| Page {
                    kind,
                    ..Default::default()
                })
                .collect(),
        }
    }
}
pub(crate) fn crc(data: &[u8]) -> u16 {
    let mut c = 0xffffu16;
    for &v in data {
        c ^= u16::from(v) << 8;
        for _ in 0..8 {
            c = (c << 1) ^ if c & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    c
}
impl Playlist {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=8).contains(&self.pages.len()) && usize::from(self.selected) < self.pages.len(),
            "Список: 1–8 страниц, выбранная страница должна существовать"
        );
        ensure!(
            self.pages.iter().any(|p| p.enabled),
            "Оставьте хотя бы одну включённую страницу"
        );
        ensure!(
            (1..=3).contains(&self.clock_scale) && (1..=3).contains(&self.detail_scale),
            "Масштаб: 1–3"
        );
        ensure!(
            self.clock_position
                .iter()
                .chain(&self.cpu_position)
                .chain(&self.memory_position)
                .all(|&v| v < 128),
            "Координаты: 0–127"
        );
        for p in &self.pages {
            ensure!(
                p.kind == Kind::Custom || p.composition == Default::default(),
                "Параметры композиции допустимы только для своего экрана"
            );
            if p.kind == Kind::Custom {
                p.composition.validate()?;
                ensure!(p.seconds <= 255, "Время своего экрана: 1–255 секунд");
            }
            ensure!(
                (1..=3600).contains(&p.seconds),
                "Время страницы: 1–3600 секунд"
            );
            ensure!(
                if p.kind == Kind::Media {
                    p.asset < 6
                } else {
                    p.asset == 0
                },
                "Неверный ресурс страницы"
            );
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<[u8; 48]> {
        self.validate()?;
        let mut p = [0; 48];
        p[..8].copy_from_slice(&[
            u8::from(self.enabled)
                | (u8::from(self.autoplay) << 1)
                | (u8::from(self.skip_offline) << 2),
            self.pages.len() as u8,
            self.selected,
            self.clock_scale,
            self.clock_position[0],
            self.clock_position[1],
            u8::from(self.show_date),
            u8::from(self.show_seconds),
        ]);
        for (i, e) in self.pages.iter().enumerate() {
            let a = 8 + i * 4;
            if e.kind == Kind::Custom {
                p[a] = e.composition.wire_kind() | if e.enabled { 0 } else { 128 };
                p[a + 1] = e.seconds as u8;
                p[a + 2..a + 4].copy_from_slice(&e.composition.packed()?.to_le_bytes());
                continue;
            }
            p[a] = e.kind as u8 | if e.enabled { 0 } else { 128 };
            p[a + 1] = e.asset;
            p[a + 2..a + 4].copy_from_slice(&e.seconds.to_le_bytes());
        }
        p[40..45].copy_from_slice(&[
            self.detail_scale,
            self.cpu_position[0],
            self.cpu_position[1],
            self.memory_position[0],
            self.memory_position[1],
        ]);
        let c = crc(&p[..46]);
        p[46..].copy_from_slice(&c.to_le_bytes());
        Ok(p)
    }
    pub fn decode(p: &[u8]) -> Result<Self> {
        ensure!(
            p.len() == 48 && crc(&p[..46]) == u16::from_le_bytes([p[46], p[47]]),
            "Ошибка CRC списка экранов"
        );
        ensure!(
            p[0] <= 7 && (1..=8).contains(&p[1]) && p[6] <= 1 && p[7] <= 1 && p[45] == 0,
            "Неизвестный формат списка экранов"
        );
        ensure!(
            p[8 + usize::from(p[1]) * 4..40].iter().all(|&v| v == 0),
            "Повреждён хвост списка экранов"
        );
        let mut pages = Vec::new();
        for e in p[8..8 + usize::from(p[1]) * 4].as_chunks::<4>().0 {
            pages.push(Page {
                enabled: e[0] & 128 == 0,
                kind: Kind::decode(e[0] & 127)?,
                asset: e[1],
                seconds: if (13..=24).contains(&(e[0] & 127)) {
                    u16::from(e[1])
                } else {
                    u16::from_le_bytes([e[2], e[3]])
                },
                composition: if (13..=24).contains(&(e[0] & 127)) {
                    crate::dock_composition::Composition::from_page(
                        u16::from_le_bytes([e[2], e[3]]),
                        e[0] & 127,
                    )?
                } else {
                    Default::default()
                },
            });
            if let Some(last) = pages.last_mut()
                && last.kind == Kind::Custom
            {
                last.asset = 0;
            }
        }
        let v = Self {
            enabled: p[0] & 1 != 0,
            autoplay: p[0] & 2 != 0,
            skip_offline: p[0] & 4 != 0,
            selected: p[2],
            clock_scale: p[3],
            clock_position: [p[4], p[5]],
            show_date: p[6] == 1,
            show_seconds: p[7] == 1,
            detail_scale: p[40],
            cpu_position: [p[41], p[42]],
            memory_position: [p[43], p[44]],
            pages,
        };
        v.validate()?;
        Ok(v)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub capabilities: u8,
    pub playlist: Playlist,
    pub active: u8,
    pub kind: u8,
    pub assets: u8,
    pub gif_frames: u8,
    pub erasing: Option<u8>,
    pub progress: u8,
    pub save_pending: bool,
    pub status: u8,
}
impl Status {
    pub fn parse(r: &[u8]) -> Result<Self> {
        ensure!(
            r.len() == 65 && r[..7] == [0, 0x2c, b'A', b'J', b'P', b'G', 1],
            "Нужна экранная прошивка AJSC с диспетчером страниц"
        );
        let b = &r[1..];
        ensure!(
            b[..8].iter().fold(0u8, |a, &v| a.wrapping_add(v)) == 255
                && b[63] & 32 != 0
                && b[6] <= 3
                && b[56] < 8
                && (b[57] <= 24 || b[57] == 255)
                && b[58] < 64
                && b[59] <= 218
                && b[60] <= 6
                && b[61] <= 100
                && b[62] <= 1,
            "Повреждён ответ диспетчера страниц"
        );
        Ok(Self {
            capabilities: b[63],
            playlist: Playlist::decode(&b[8..56])?,
            active: b[56],
            kind: b[57],
            assets: b[58],
            gif_frames: b[59],
            erasing: b[60].checked_sub(1),
            progress: b[61],
            save_pending: b[62] == 1,
            status: b[6],
        })
    }
    pub fn has_asset(&self, index: u8) -> bool {
        index < 6 && self.assets & (1 << index) != 0
    }
}
pub fn asset_label(index: u8) -> String {
    if index == 5 {
        "GIF-анимация".into()
    } else {
        format!("Фото {}", index + 1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playlist_roundtrip_and_rejects_corruption() {
        let p = Playlist::default();
        let encoded = p.encode().unwrap();
        assert_eq!(Playlist::decode(&encoded).unwrap(), p);
        for bit in 0..384 {
            let mut b = encoded;
            b[bit / 8] ^= 1 << (bit % 8);
            assert!(Playlist::decode(&b).is_err());
        }
    }
    #[test]
    fn pages_preserve_order_disabled_entries_and_boundary_assets() {
        let mut p = Playlist {
            pages: vec![
                Page {
                    kind: Kind::Media,
                    asset: 5,
                    seconds: 3600,
                    enabled: true,
                    composition: Default::default(),
                },
                Page {
                    kind: Kind::Clock,
                    enabled: false,
                    ..Default::default()
                },
            ],
            selected: 1,
            ..Default::default()
        };
        assert_eq!(Playlist::decode(&p.encode().unwrap()).unwrap(), p);
        p.pages[0].asset = 6;
        assert!(p.encode().is_err());
        p.pages[0].asset = 5;
        p.pages[0].enabled = false;
        assert!(p.encode().is_err());
        p.pages[0].enabled = true;
        p.pages[0].seconds = 0;
        assert!(p.encode().is_err());
    }
}
