//! Presets for the host-driven saved-key-colors mode. Never auto-start on load.
use crate::model::{Effect, Layer, Scene, keyboard_rows};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    #[default]
    Aurora,
    Plasma,
    Comet,
    DigitalRain,
    Embers,
    Orbit,
}
impl Preset {
    pub const ALL: [Self; 6] = [
        Self::Aurora,
        Self::Plasma,
        Self::Comet,
        Self::DigitalRain,
        Self::Embers,
        Self::Orbit,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Aurora => "Полярные занавесы",
            Self::Plasma => "Неоновая плазма",
            Self::Comet => "Кометный дождь",
            Self::DigitalRain => "Цифровой ливень",
            Self::Embers => "Тлеющие угли",
            Self::Orbit => "Двойная орбита",
        }
    }
    pub fn colors(self) -> [[u8; 3]; 2] {
        match self {
            Self::Aurora => [[15, 240, 135], [125, 35, 255]],
            Self::Plasma => [[255, 15, 115], [20, 145, 255]],
            Self::Comet => [[30, 50, 255], [210, 255, 255]],
            Self::DigitalRain => [[0, 100, 30], [120, 255, 155]],
            Self::Embers => [[255, 12, 0], [255, 175, 15]],
            Self::Orbit => [[255, 90, 20], [30, 180, 255]],
        }
    }
    fn effect(self) -> Effect {
        match self {
            Self::Aurora => Effect::Aurora,
            Self::Plasma => Effect::Plasma,
            Self::Comet => Effect::Comet,
            Self::DigitalRain => Effect::DigitalRain,
            Self::Embers => Effect::Embers,
            Self::Orbit => Effect::Orbit,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub preset: Preset,
    pub colors: [[u8; 3]; 2],
    pub speed: f32,
    pub scale: f32,
    pub brightness: f32,
    pub angle: f32,
    pub reverse: bool,
    pub duration: f32,
    /// Empty selects all physical keys; other keys retain their saved colors.
    pub keys: BTreeSet<u8>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            preset: Preset::Aurora,
            colors: Preset::Aurora.colors(),
            speed: 0.2,
            scale: 1.0,
            brightness: 0.8,
            angle: 0.0,
            reverse: false,
            duration: 30.0,
            keys: BTreeSet::new(),
        }
    }
}
impl Settings {
    pub fn scene(&self) -> Scene {
        Scene {
            schema: 1,
            name: self.preset.label().into(),
            duration: self.duration,
            fps: 30,
            brightness: self.brightness,
            dock_layers: Vec::new(),
            layers: vec![Layer {
                name: self.preset.label().into(),
                effect: self.preset.effect(),
                colors: self.colors,
                speed: self.speed,
                scale: self.scale,
                angle: self.angle + if self.reverse { 180.0 } else { 0.0 },
                keys: self.keys.clone(),
                ..Layer::default()
            }],
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.scene().validate()?;
        let ids: BTreeSet<_> = keyboard_rows()
            .into_iter()
            .flatten()
            .map(|k| k.id)
            .collect();
        ensure!(self.keys.is_subset(&ids), "Неизвестные клавиши в анимации");
        ensure!((0.02..=2.0).contains(&self.speed), "Скорость: 0.02–2");
        ensure!((0.25..=4.0).contains(&self.scale), "Масштаб: 0.25–4");
        ensure!((0.0..=180.0).contains(&self.angle), "Угол: 0–180°");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_move_are_distinct_and_round_trip_with_their_settings() {
        let mut frames = BTreeSet::new();
        for preset in Preset::ALL {
            let settings = Settings {
                preset,
                colors: preset.colors(),
                ..Settings::default()
            };
            settings.validate().unwrap();
            let restored: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            let scene = restored.scene();
            let first = crate::lighting::keyboard_frame(&scene, 0.0, &[]);
            let next = crate::lighting::keyboard_frame(&scene, 1.3, &[]);
            assert_ne!(first, next, "{} must animate", preset.label());
            assert!(frames.insert(first), "duplicate preset");
        }
    }
    #[test]
    fn invalid_settings_and_nonphysical_keys_are_rejected() {
        let mut settings = Settings {
            speed: f32::NAN,
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        settings.speed = 0.2;
        settings.keys.insert(127);
        assert!(settings.validate().is_err());
    }
}
