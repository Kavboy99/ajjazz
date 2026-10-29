use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryIndicator {
    pub enabled: bool,
    pub whole_keyboard: bool,
    pub background: bool,
    pub hold_seconds: u8,
    pub display_seconds: u8,
    pub filled: [u8; 3],
    pub empty: [u8; 3],
    pub brightness: u8,
}
impl Default for BatteryIndicator {
    fn default() -> Self {
        Self {
            enabled: false,
            whole_keyboard: false,
            background: false,
            hold_seconds: 2,
            display_seconds: 3,
            filled: [34, 211, 134],
            empty: [8, 12, 20],
            brightness: 255,
        }
    }
}
impl BatteryIndicator {
    pub(super) fn parse(p: &[u8]) -> Result<Self> {
        ensure!(p.len() == 512, "Неполные настройки индикатора");
        if p[28] == 0 {
            return Ok(Self::default());
        }
        ensure!(
            p[28] == 1
                && p[29] <= 1
                && p[30] <= 1
                && p[31] <= 1
                && (1..=5).contains(&p[32])
                && (1..=10).contains(&p[33]),
            "Неверные настройки индикатора заряда"
        );
        Ok(Self {
            enabled: p[29] != 0,
            whole_keyboard: p[30] != 0,
            background: p[31] != 0,
            hold_seconds: p[32],
            display_seconds: p[33],
            filled: p[34..37].try_into()?,
            empty: p[37..40].try_into()?,
            brightness: p[40],
        })
    }
    pub(super) fn write(&self, p: &mut [u8]) -> Result<()> {
        ensure!(p.len() == 512, "Неполные настройки индикатора");
        p[28..34].copy_from_slice(&[
            1,
            self.enabled.into(),
            self.whole_keyboard.into(),
            self.background.into(),
            self.hold_seconds,
            self.display_seconds,
        ]);
        p[34..37].copy_from_slice(&self.filled);
        p[37..40].copy_from_slice(&self.empty);
        p[40] = self.brightness;
        Self::parse(p)?;
        Ok(())
    }
}
/// Preview in the same physical order as the autonomous PWM overlay. Unfilled
/// keys keep their original scene color unless a background was requested.
pub fn battery_preview(base: &mut [[u8; 3]], percent: u8, config: &BatteryIndicator) {
    let leds = crate::lighting::keyboard_leds();
    if base.len() != leds.len() || percent > 100 {
        return;
    }
    let mut order: Vec<_> = (0..leds.len())
        .filter(|&i| config.whole_keyboard || leds[i].y == 0.0)
        .collect();
    if config.whole_keyboard {
        order.sort_by(|&a, &b| {
            leds[a]
                .x
                .total_cmp(&leds[b].x)
                .then(leds[a].y.total_cmp(&leds[b].y))
        });
    }
    let filled = (usize::from(percent) * order.len()).div_ceil(100);
    for (rank, i) in order.into_iter().enumerate() {
        if rank < filled || config.background {
            base[i] = (if rank < filled {
                config.filled
            } else {
                config.empty
            })
            .map(|v| {
                super::preview::display_pwm(
                    (u32::from(v) * u32::from(config.brightness) / 255) as u8,
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn battery_settings_legacy_roundtrip_bounds_and_overlay() {
        let mut page = vec![0; 512];
        assert!(!BatteryIndicator::parse(&page).unwrap().enabled);
        let mut c = BatteryIndicator {
            enabled: true,
            filled: [255, 0, 0],
            empty: [0, 0, 255],
            ..Default::default()
        };
        c.write(&mut page).unwrap();
        assert_eq!(BatteryIndicator::parse(&page).unwrap(), c);
        c.hold_seconds = 0;
        assert!(c.write(&mut page).is_err());
        c.hold_seconds = 2;
        let mut colors = vec![[1, 2, 3]; 82];
        battery_preview(&mut colors, 50, &c);
        assert_eq!(colors.iter().filter(|&&v| v == [255, 0, 0]).count(), 7);
        assert_eq!(colors.iter().filter(|&&v| v == [1, 2, 3]).count(), 75);
        c.background = true;
        battery_preview(&mut colors, 0, &c);
        assert_eq!(colors.iter().filter(|&&v| v == [0, 0, 255]).count(), 14);
        c.whole_keyboard = true;
        battery_preview(&mut colors, 100, &c);
        assert!(colors.iter().all(|&v| v == [255, 0, 0]));
    }
}
