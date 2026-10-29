use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

pub type Rgb = [u8; 3];

#[derive(Clone, Debug, Deserialize)]
pub struct Key {
    pub id: u8,
    pub label: String,
    pub hid: u8,
    pub physical: String,
    pub width: f32,
}

pub fn keyboard_rows() -> Vec<Vec<Key>> {
    serde_json::from_str(include_str!("../assets/ak820max-layout.json")).expect("embedded layout")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Solid,
    Gradient,
    Wave,
    Breathing,
    Ripple,
    Sparkle,
    Reactive,
    Aurora,
    Plasma,
    Comet,
    DigitalRain,
    Embers,
    Orbit,
    Visor,
    TwinWave,
    Checkerboard,
    Fireflies,
    Raindrops,
    Lightning,
    Heartbeat,
    Nebula,
    Mosaic,
}
impl Effect {
    pub const ALL: [Self; 22] = [
        Self::Solid,
        Self::Gradient,
        Self::Wave,
        Self::Breathing,
        Self::Ripple,
        Self::Sparkle,
        Self::Reactive,
        Self::Aurora,
        Self::Plasma,
        Self::Comet,
        Self::DigitalRain,
        Self::Embers,
        Self::Orbit,
        Self::Visor,
        Self::TwinWave,
        Self::Checkerboard,
        Self::Fireflies,
        Self::Raindrops,
        Self::Lightning,
        Self::Heartbeat,
        Self::Nebula,
        Self::Mosaic,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Цвет",
            Self::Gradient => "Градиент",
            Self::Wave => "Волна",
            Self::Breathing => "Дыхание",
            Self::Ripple => "Круги",
            Self::Sparkle => "Звёзды",
            Self::Reactive => "Отклик на нажатие",
            Self::Aurora => "Полярные занавесы",
            Self::Plasma => "Плазма",
            Self::Comet => "Кометы",
            Self::DigitalRain => "Цифровой ливень",
            Self::Embers => "Угли",
            Self::Orbit => "Орбиты",
            Self::Visor => "Сканер",
            Self::TwinWave => "Встречные волны",
            Self::Checkerboard => "Шахматный ритм",
            Self::Fireflies => "Светлячки",
            Self::Raindrops => "Капли на воде",
            Self::Lightning => "Гроза",
            Self::Heartbeat => "Сердцебиение",
            Self::Nebula => "Туманность",
            Self::Mosaic => "Живой витраж",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    Replace,
    Add,
    Multiply,
}
impl Blend {
    pub const ALL: [Self; 3] = [Self::Replace, Self::Add, Self::Multiply];
    pub fn label(self) -> &'static str {
        match self {
            Self::Replace => "Обычный",
            Self::Add => "Сложение",
            Self::Multiply => "Умножение",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub name: String,
    pub enabled: bool,
    pub effect: Effect,
    pub blend: Blend,
    /// Empty means the whole surface; IDs refer to the vendor's physical matrix.
    pub keys: BTreeSet<u8>,
    pub colors: [Rgb; 2],
    pub opacity: f32,
    pub speed: f32,
    pub scale: f32,
    pub angle: f32,
    pub phase: f32,
}
impl Default for Layer {
    fn default() -> Self {
        Self {
            name: "Новый слой".into(),
            enabled: true,
            effect: Effect::Wave,
            blend: Blend::Replace,
            keys: BTreeSet::new(),
            colors: [[38, 225, 190], [111, 76, 240]],
            opacity: 1.0,
            speed: 0.25,
            scale: 1.0,
            angle: 0.0,
            phase: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub schema: u32,
    pub name: String,
    pub duration: f32,
    pub fps: u32,
    pub brightness: f32,
    pub layers: Vec<Layer>,
    pub dock_layers: Vec<Layer>,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            schema: 1,
            name: "Северное сияние".into(),
            duration: 8.0,
            fps: 30,
            brightness: 0.8,
            layers: vec![
                Layer {
                    name: "Сияние".into(),
                    ..Default::default()
                },
                Layer {
                    name: "Акцент WASD".into(),
                    effect: Effect::Breathing,
                    keys: [34, 49, 50, 51].into_iter().collect(),
                    colors: [[255, 172, 74], [255, 88, 120]],
                    opacity: 0.75,
                    speed: 0.5,
                    ..Default::default()
                },
            ],
            dock_layers: vec![Layer {
                name: "Ореол дока · эскиз".into(),
                ..Default::default()
            }],
        }
    }
}
impl Scene {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1,
            "Неизвестная версия проекта: {}",
            self.schema
        );
        ensure!(
            self.duration.is_finite() && (0.1..=300.0).contains(&self.duration),
            "Длительность: 0.1–300 с"
        );
        ensure!((1..=60).contains(&self.fps), "Частота: 1–60 кадров/с");
        ensure!(
            self.brightness.is_finite() && (0.0..=1.0).contains(&self.brightness),
            "Некорректная яркость"
        );
        ensure!(
            self.layers.len() <= 32 && self.dock_layers.len() <= 32,
            "Не более 32 слоёв"
        );
        for l in self.layers.iter().chain(&self.dock_layers) {
            ensure!(l.keys.iter().all(|&k| k < 128), "Некорректный ID клавиши");
            ensure!(
                [l.opacity, l.speed, l.scale, l.angle, l.phase]
                    .iter()
                    .all(|v| v.is_finite()),
                "Параметры слоя должны быть конечными"
            );
            ensure!(
                (0.0..=1.0).contains(&l.opacity)
                    && (0.0..=10.0).contains(&l.speed)
                    && (0.05..=20.0).contains(&l.scale)
                    && l.angle.abs() <= 360.0
                    && l.phase.abs() <= 1000.0,
                "Параметр слоя вне диапазона"
            );
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let scene: Self = crate::storage::read_json(path, 2_000_000)?;
        scene.validate()?;
        Ok(scene)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        crate::storage::write_json_atomic(path, self)?;
        Ok(())
    }
}
