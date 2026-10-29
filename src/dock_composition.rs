//! Compact, autonomous screen composition. Four metric nibbles fit an existing
//! page record; no extra flash page, framebuffer or host-rendered image stream.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Metric {
    #[default]
    None = 0,
    Cpu = 1,
    Ram = 2,
    Gpu = 3,
    Vram = 4,
    Fps = 5,
    CpuTemp = 6,
    GpuTemp = 7,
    DiskTemp = 8,
    BoardTemp = 9,
    Power = 10,
    Clock = 11,
}
impl Metric {
    pub const ALL: [Self; 12] = [
        Self::None,
        Self::Cpu,
        Self::Ram,
        Self::Gpu,
        Self::Vram,
        Self::Fps,
        Self::CpuTemp,
        Self::GpuTemp,
        Self::DiskTemp,
        Self::BoardTemp,
        Self::Power,
        Self::Clock,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Убрать",
            Self::Cpu => "Загрузка CPU",
            Self::Ram => "Память RAM",
            Self::Gpu => "Загрузка GPU",
            Self::Vram => "Видеопамять",
            Self::Fps => "FPS игры",
            Self::CpuTemp => "Температура CPU",
            Self::GpuTemp => "Температура GPU",
            Self::DiskTemp => "Температура диска",
            Self::BoardTemp => "Температура платы",
            Self::Power => "Мощность GPU",
            Self::Clock => "Время",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Cpu => "CPU",
            Self::Ram => "RAM",
            Self::Gpu => "GPU",
            Self::Vram => "VRAM",
            Self::Fps => "FPS",
            Self::CpuTemp => "CPU",
            Self::GpuTemp => "GPU",
            Self::DiskTemp => "DISK",
            Self::BoardTemp => "SYS",
            Self::Power => "GPU",
            Self::Clock => "TIME",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Backdrop {
    #[default]
    Still = 0,
    Breathe = 1,
    Orbit = 2,
    Wave = 3,
    Fireflies = 4,
    Scan = 5,
}
impl Backdrop {
    pub const ALL: [Self; 6] = [
        Self::Still,
        Self::Breathe,
        Self::Orbit,
        Self::Wave,
        Self::Fireflies,
        Self::Scan,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Still => "Тишина",
            Self::Breathe => "Дыхание",
            Self::Orbit => "Орбита",
            Self::Wave => "Волна",
            Self::Fireflies => "Светлячки",
            Self::Scan => "Сканер",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Composition {
    pub metrics: [Metric; 4],
    pub backdrop: Backdrop,
    pub bars: bool,
}
impl Default for Composition {
    fn default() -> Self {
        Self {
            metrics: [Metric::Fps, Metric::CpuTemp, Metric::GpuTemp, Metric::Vram],
            backdrop: Backdrop::Still,
            bars: false,
        }
    }
}
impl Composition {
    pub fn wire_kind(&self) -> u8 {
        13 + self.backdrop as u8 + if self.bars { 6 } else { 0 }
    }
    pub fn from_page(packed: u16, kind: u8) -> Result<Self> {
        ensure!((13..=24).contains(&kind), "Неизвестный тип композиции");
        let mut c = Self::decode(packed, (kind - 13) % 6)?;
        c.bars = kind >= 19;
        Ok(c)
    }
    pub fn normalize(&mut self) {
        let mut out = [Metric::None; 4];
        let mut n = 0;
        for m in self.metrics {
            if m != Metric::None && !out.contains(&m) {
                out[n] = m;
                n += 1;
            }
        }
        if n == 0 {
            out[0] = Metric::Fps;
        }
        self.metrics = out;
    }
    pub fn count(&self) -> usize {
        self.metrics.iter().filter(|m| **m != Metric::None).count()
    }
    pub fn validate(&self) -> Result<()> {
        let mut c = self.clone();
        c.normalize();
        ensure!(
            c == *self,
            "Выберите от одного до четырёх разных показателей"
        );
        Ok(())
    }
    pub fn packed(&self) -> Result<u16> {
        self.validate()?;
        Ok(self
            .metrics
            .iter()
            .enumerate()
            .fold(0, |v, (i, m)| v | ((*m as u16) << (i * 4))))
    }
    pub fn decode(packed: u16, effect: u8) -> Result<Self> {
        ensure!(effect < 6, "Неизвестный эффект экрана");
        let mut c = Self {
            backdrop: Backdrop::ALL[usize::from(effect)],
            ..Default::default()
        };
        for (i, m) in c.metrics.iter_mut().enumerate() {
            *m = *Metric::ALL
                .get(usize::from((packed >> (i * 4)) & 15))
                .ok_or_else(|| anyhow::anyhow!("Неизвестный показатель"))?;
        }
        c.validate()?;
        Ok(c)
    }
    pub fn rects(&self) -> Vec<[u32; 4]> {
        match self.count() {
            1 => vec![[10, 18, 108, 92]],
            2 => vec![[10, 8, 108, 52], [10, 70, 108, 52]],
            3 => vec![[10, 7, 108, 55], [10, 76, 50, 46], [70, 76, 50, 46]],
            _ => vec![
                [10, 8, 50, 50],
                [70, 8, 50, 50],
                [10, 72, 50, 50],
                [70, 72, 50, 50],
            ],
        }
    }
    pub fn presets() -> [(&'static str, Self); 4] {
        use Metric::*;
        [
            (
                "Всё важное",
                Self {
                    metrics: [Cpu, Gpu, Ram, Fps],
                    backdrop: Backdrop::Still,
                    bars: false,
                },
            ),
            ("Игра", Self::default()),
            (
                "Тепловой баланс",
                Self {
                    metrics: [CpuTemp, GpuTemp, DiskTemp, None],
                    backdrop: Backdrop::Breathe,
                    bars: false,
                },
            ),
            (
                "На виду",
                Self {
                    metrics: [Clock, Cpu, Gpu, None],
                    backdrop: Backdrop::Orbit,
                    bars: false,
                },
            ),
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock_pages::{Kind, Page, Playlist};
    #[test]
    fn a_clock_composition_is_autonomous_but_data_cells_request_a_feed() {
        let mut p = Playlist {
            pages: vec![Page {
                kind: Kind::Custom,
                composition: Composition {
                    metrics: [Metric::Clock, Metric::None, Metric::None, Metric::None],
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(!crate::dock_telemetry::needs_samples(
            &Default::default(),
            &p
        ));
        p.pages[0].composition.metrics[1] = Metric::Cpu;
        assert!(crate::dock_telemetry::needs_samples(
            &Default::default(),
            &p
        ));
    }
    #[test]
    fn dispatcher_accepts_modern_page_states_and_rejects_unknown_codes() {
        let mut b = [0u8; 64];
        b[..7].copy_from_slice(&[0x2c, b'A', b'J', b'P', b'G', 1, 0]);
        b[7] = 255 - b[..7].iter().fold(0u8, |a, v| a.wrapping_add(*v));
        b[8..56].copy_from_slice(&Playlist::default().encode().unwrap());
        b[63] = 255;
        for kind in [0, 9, 10, 11, 12, 13, 18, 19, 24, 255] {
            b[57] = kind;
            let r = [&[0][..], &b].concat();
            assert!(crate::dock_pages::Status::parse(&r).is_ok(), "kind {kind}");
        }
        b[57] = 25;
        assert!(crate::dock_pages::Status::parse(&[&[0][..], &b].concat()).is_err());
    }
    #[test]
    fn compact_pages_preserve_all_slots_effects_and_duration() {
        for effect in Backdrop::ALL {
            let mut p = Playlist {
                pages: vec![],
                selected: 0,
                ..Default::default()
            };
            for n in 0..8 {
                let mut c = Composition {
                    backdrop: effect,
                    bars: n % 2 == 1,
                    ..Default::default()
                };
                for i in 0..4 {
                    c.metrics[i] = Metric::ALL[(n + i) % 11 + 1];
                }
                p.pages.push(Page {
                    kind: Kind::Custom,
                    seconds: 255,
                    composition: c,
                    ..Default::default()
                });
            }
            let wire = p.encode().unwrap();
            assert_eq!(Playlist::decode(&wire).unwrap(), p);
            assert_eq!(wire.len(), 48);
        }
    }
    #[test]
    fn adaptive_cells_fit_and_bad_or_duplicate_metrics_cannot_be_written() {
        for n in 1..=4 {
            let mut c = Composition {
                metrics: [Metric::Cpu, Metric::Gpu, Metric::Ram, Metric::Fps],
                ..Default::default()
            };
            for i in n..4 {
                c.metrics[i] = Metric::None;
            }
            assert_eq!(c.rects().len(), n);
            for r in c.rects() {
                assert!(r[0] + r[2] <= 128 && r[1] + r[3] <= 128);
            }
        }
        for packed in [0, 0x11, 0x101, 0xf] {
            assert!(Composition::decode(packed, 0).is_err());
        }
        assert!(Composition::decode(1, 6).is_err());
        let mut c = Composition {
            metrics: [Metric::Cpu, Metric::None, Metric::Cpu, Metric::Gpu],
            ..Default::default()
        };
        c.normalize();
        assert_eq!(
            c.metrics,
            [Metric::Cpu, Metric::Gpu, Metric::None, Metric::None]
        );
    }
}
