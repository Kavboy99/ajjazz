//! Source-confirmed OLED version and clock commands; no firmware writes.
use anyhow::{Result, ensure};
use serde::Serialize;

pub(crate) fn clock_report(now: chrono::DateTime<chrono::Local>) -> Result<[u8; 65]> {
    use chrono::{Datelike, Timelike};
    ensure!(
        (2000..=2099).contains(&now.year()),
        "Год часов вне диапазона 2000–2099"
    );
    let mut packet = [0; 65];
    packet[1] = 0x28;
    packet[8] = 0xd7; // BIT7: complement of header sum 0x28.
    packet[9..11].copy_from_slice(&(now.year() as u16).to_be_bytes());
    packet[11] = now.month() as u8;
    packet[12] = now.day() as u8;
    packet[13] = now.hour() as u8;
    packet[14] = now.minute() as u8;
    packet[15] = now.second() as u8;
    Ok(packet)
}

#[derive(Clone, Debug, Serialize)]
pub struct ClockReceipt {
    pub local_time: String,
    pub verification: String,
}

pub fn sync_clock() -> Result<ClockReceipt> {
    #[cfg(windows)]
    {
        let session = DockSession::open()?;
        let before = session.info()?;
        ensure!(
            before.oled_version == 0x104,
            "Непроверенная версия дока {}",
            before.label()
        );
        let now = chrono::Local::now();
        session.device.send_feature_report(&clock_report(now)?)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        ensure!(
            session.info()?.oled_version == before.oled_version,
            "Док не ответил после установки часов"
        );
        Ok(ClockReceipt {
            local_time: now.format("%d.%m.%Y %H:%M:%S").to_string(),
            verification: "Время отправлено; обратное чтение часов не предусмотрено. Проверьте штатную страницу часов на доке.".into(),
        })
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DockInfo {
    pub captured_utc: String,
    pub oled_version: u16,
    pub flash_version: u16,
    pub raw_response: Vec<u8>,
}
impl DockInfo {
    pub fn label(&self) -> String {
        format!("OLEDV{:X}", self.oled_version)
    }
    fn parse(response: &[u8]) -> Result<Self> {
        ensure!(
            response.len() == 65 && response[..2] == [0, 0xad],
            "Unexpected dock version response"
        );
        Ok(Self {
            captured_utc: chrono::Utc::now().to_rfc3339(),
            oled_version: u16::from_le_bytes([response[2], response[3]]),
            flash_version: u16::from_le_bytes([response[4], response[5]]),
            raw_response: response.to_vec(),
        })
    }
}
#[cfg(windows)]
pub(crate) struct DockSession {
    pub(crate) device: hidapi::HidDevice,
    _lease: crate::hardware::DeviceLease,
}
#[cfg(windows)]
impl DockSession {
    pub(crate) fn open() -> Result<Self> {
        let lease = crate::hardware::DeviceLease::acquire_timeout("AJ159-APEX", 5000)?
            .ok_or_else(|| anyhow::anyhow!("Док занят другой операцией"))?;
        Self::open_with_lease(lease)
    }
    pub(crate) fn open_background() -> Result<Option<Self>> {
        let Some(lease) = crate::hardware::DeviceLease::acquire_timeout("AJ159-APEX", 0)? else {
            return Ok(None);
        };
        Ok(Some(Self::open_with_lease(lease)?))
    }
    fn open_with_lease(lease: crate::hardware::DeviceLease) -> Result<Self> {
        use anyhow::Context;
        let service = std::net::SocketAddr::from(([127, 0, 0, 1], 3814));
        ensure!(
            std::net::TcpStream::connect_timeout(&service, std::time::Duration::from_millis(100))
                .is_err(),
            "Штатная служба AJAZZ использует приёмник и может подменять ответы. Завершите AJAZZ Driver и его фоновую службу iot_driver, затем повторите операцию."
        );
        let api = hidapi::HidApi::new()?;
        let mut endpoints = api.device_list().filter(|d| {
            d.vendor_id() == 0x3151
                && d.product_id() == 0x5007
                && d.usage_page() == 0xffff
                && d.usage() == 2
        });
        let endpoint = endpoints.next().context("Док APEX 3151:5007 не найден")?;
        ensure!(
            endpoints.next().is_none(),
            "Подключено несколько доков APEX; оставьте один для настройки"
        );
        let device = api.open_path(endpoint.path())?;
        Ok(Self {
            device,
            _lease: lease,
        })
    }
    pub(crate) fn info(&self) -> Result<DockInfo> {
        let mut packet = [0; 65];
        packet[1] = 0xad;
        packet[8] = 0x52;
        self.device.send_feature_report(&packet)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut response = [0; 65];
        let length = self.device.get_feature_report(&mut response)?;
        DockInfo::parse(&response[..length])
    }
}
#[cfg(windows)]
pub fn read_info() -> Result<DockInfo> {
    DockSession::open()?.info()
}
#[cfg(not(windows))]
pub fn read_info() -> Result<DockInfo> {
    anyhow::bail!("Windows only")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_matches_vendor_big_endian_year_and_local_wall_time() {
        use chrono::TimeZone;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 17, 0, 31, 42)
            .unwrap();
        let packet = clock_report(now).unwrap();
        assert_eq!(&packet[..9], &[0, 0x28, 0, 0, 0, 0, 0, 0, 0xd7]);
        assert_eq!(&packet[9..16], &[7, 234, 10, 17, 0, 31, 42]);
        assert!(packet[16..].iter().all(|&byte| byte == 0));
    }
    #[test]
    fn captured_version_is_little_endian_and_wrong_or_short_reply_is_rejected() {
        let mut reply = [0; 65];
        reply[1..4].copy_from_slice(&[0xad, 4, 1]);
        let info = DockInfo::parse(&reply).unwrap();
        assert_eq!(info.oled_version, 0x104);
        assert_eq!(info.label(), "OLEDV104");
        assert_eq!(info.flash_version, 0);
        assert!(DockInfo::parse(&reply[..64]).is_err());
        reply[1] = 0xae;
        assert!(DockInfo::parse(&reply).is_err());
    }
}
