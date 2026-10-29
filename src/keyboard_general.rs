//! AK820MAX game-mode fields established by the matching web driver.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralSettings {
    pub sleep_minutes: u8,
    pub key_delay: u8,
    /// Hundredths of a millimetre, as stored by firmware.
    pub top_dead_zone: u8,
    pub bottom_dead_zone: u8,
    pub stability: bool,
    pub auto_calibration: bool,
}
impl GeneralSettings {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() == 56, "Invalid game-mode block");
        ensure!(
            raw[11] <= 1 && raw[14] <= 1,
            "Unknown stability/calibration encoding"
        );
        Ok(Self {
            sleep_minutes: raw[3],
            key_delay: raw[4],
            top_dead_zone: raw[8],
            bottom_dead_zone: raw[9],
            stability: raw[11] != 0,
            auto_calibration: raw[14] != 0,
        })
    }
    pub fn patch(&self, raw: &[u8]) -> Result<Vec<u8>> {
        Self::parse(raw)?;
        ensure!(self.sleep_minutes <= 30, "Сон: 0–30 минут; 0 отключает сон");
        ensure!(self.key_delay <= 5, "Недопустимый уровень задержки клавиш");
        ensure!(
            self.top_dead_zone <= 50 && self.bottom_dead_zone <= 50,
            "Мёртвые зоны: 0–0,50 мм"
        );
        let mut out = raw.to_vec();
        out[3] = self.sleep_minutes;
        out[4] = self.key_delay;
        out[8] = self.top_dead_zone;
        out[9] = self.bottom_dead_zone;
        out[11] = u8::from(self.stability);
        out[14] = u8::from(self.auto_calibration);
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_unknown_fields_and_check_real_boundaries() {
        let mut raw: Vec<u8> = (0..56).collect();
        raw[11] = 1;
        raw[14] = 1;
        let settings = GeneralSettings {
            sleep_minutes: 6,
            key_delay: 0,
            top_dead_zone: 1,
            bottom_dead_zone: 50,
            stability: false,
            auto_calibration: true,
        };
        let edited = settings.patch(&raw).unwrap();
        for i in (0..56).filter(|i| ![3, 4, 8, 9, 11, 14].contains(i)) {
            assert_eq!(raw[i], edited[i]);
        }
        assert_eq!(GeneralSettings::parse(&edited).unwrap(), settings);
        let mut invalid = settings.clone();
        invalid.sleep_minutes = 31;
        assert!(invalid.patch(&raw).is_err());
        invalid = settings.clone();
        invalid.bottom_dead_zone = 51;
        assert!(invalid.patch(&raw).is_err());
        invalid = settings;
        invalid.key_delay = 6;
        assert!(invalid.patch(&raw).is_err());
    }
}
