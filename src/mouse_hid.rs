//! AJ159 APEX RF mailbox, transcribed from the vendor's YP transport.
//! Transport selection F6/05, status F7, reply consumption FC; no firmware API.
use crate::{
    mouse::{Block, MouseIo, MouseSnapshot},
    mouse_buttons::{ButtonSnapshot, ButtonsIo},
    protocol::{MouseDpi, MouseOptions},
};
use anyhow::{Result, ensure};

#[derive(Debug)]
struct Status {
    can_read: bool,
    can_send: bool,
    online: bool,
    battery: u8,
}
impl Status {
    fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() == 64, "Неполный статус радиоприёмника");
        ensure!(
            [raw[0], raw[3], raw[4], raw[5]].iter().all(|&v| v <= 1) && raw[2] <= 100,
            "Неизвестный формат статуса радиоприёмника"
        );
        Ok(Self {
            can_read: raw[0] == 1,
            can_send: raw[5] == 1,
            online: raw[4] == 0,
            battery: raw[2],
        })
    }
}
fn feature(data: &[u8]) -> Result<[u8; 65]> {
    ensure!(data.len() <= 64, "Oversize RF report");
    let mut report = [0; 65];
    report[1..1 + data.len()].copy_from_slice(data);
    Ok(report)
}
fn checked_payload(data: &[u8]) -> Result<[u8; 64]> {
    ensure!(data.len() <= 64, "Oversize mouse command");
    let mut result = [0; 64];
    result[..data.len()].copy_from_slice(data);
    result[7] = 255u8.wrapping_sub(
        result[..7]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
    );
    Ok(result)
}

pub(crate) struct MouseSession {
    #[cfg(windows)]
    dock: crate::dock::DockSession,
}
impl MouseSession {
    pub(crate) fn open() -> Result<Self> {
        #[cfg(windows)]
        {
            let session = Self {
                dock: crate::dock::DockSession::open()?,
            };
            session.send(&[0xf1])?;
            let identity = session.raw_read()?;
            ensure!(
                identity[0] == 0xf1 && u16::from_le_bytes([identity[1], identity[2]]) == 1759,
                "Приёмник не является доком AJ159 APEX ID1759"
            );
            let mouse = session.get(0x8f, 0)?;
            ensure!(
                u32::from_le_bytes(mouse[1..5].try_into().unwrap()) == 1755,
                "Подключена другая модель мыши"
            );
            Ok(session)
        }
        #[cfg(not(windows))]
        {
            anyhow::bail!("Windows only")
        }
    }
    fn send(&self, data: &[u8]) -> Result<()> {
        let report = feature(data)?;
        #[cfg(windows)]
        {
            self.dock.device.send_feature_report(&report)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = report;
            anyhow::bail!("Windows only")
        }
    }
    fn raw_read(&self) -> Result<Vec<u8>> {
        #[cfg(windows)]
        {
            let mut raw = [0; 65];
            let len = self.dock.device.get_feature_report(&mut raw)?;
            ensure!(len == 65 && raw[0] == 0, "Неполный ответ дока");
            Ok(raw[1..].to_vec())
        }
        #[cfg(not(windows))]
        {
            anyhow::bail!("Windows only")
        }
    }
    fn status(&self) -> Result<Status> {
        self.send(&[0xf7])?;
        Status::parse(&self.raw_read()?)
    }
    fn wait(&self, reply: bool) -> Result<Status> {
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let status = self.status()?;
            ensure!(
                status.online,
                "Мышь спит или отключена. Нажмите её физическую кнопку и повторите чтение."
            );
            if if reply {
                status.can_read
            } else {
                status.can_send
            } {
                return Ok(status);
            }
        }
        anyhow::bail!(
            "Радиоприёмник не подтвердил готовность {}",
            if reply {
                "ответа"
            } else {
                "к отправке"
            }
        )
    }
    fn send_mouse(&self, data: &[u8]) -> Result<()> {
        ensure!(
            matches!(
                data.first(),
                Some(0x8f | 0xd0 | 0xd3 | 0xd4 | 0x09 | 0x53 | 0x54)
            ),
            "Unsupported mouse command"
        );
        self.send(&[0xf6, 5])?;
        self.wait(false)?;
        self.send(&checked_payload(data)?)
    }
    fn get(&self, command: u8, profile: u8) -> Result<Vec<u8>> {
        ensure!(
            [0x8f, 0xd0, 0xd3, 0xd4].contains(&command),
            "Unsupported mouse GET"
        );
        self.send_mouse(&[command, profile])?;
        self.wait(true)?;
        self.send(&[0xfc])?;
        let data = self.raw_read()?;
        if command != 0xd0 {
            ensure!(
                data[0] == command,
                "Ожидался ответ мыши 0x{command:02X}, получен 0x{:02X}; операция остановлена",
                data[0]
            );
        }
        Ok(data)
    }
    pub(crate) fn read_buttons(&self) -> Result<ButtonSnapshot> {
        let before = MouseOptions::parse(&self.get(0xd3, 0)?)?;
        let matrix = self.get(0xd0, before.profile)?;
        let after = MouseOptions::parse(&self.get(0xd3, 0)?)?;
        ensure!(
            before.profile == after.profile,
            "Профиль мыши изменился во время чтения кнопок"
        );
        let snapshot = ButtonSnapshot {
            schema: 1,
            captured_utc: chrono::Utc::now().to_rfc3339(),
            mouse_id: 0x800006db,
            profile: before.profile,
            raw_matrix: matrix,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}
impl MouseIo for MouseSession {
    fn read(&self) -> Result<MouseSnapshot> {
        let raw_options = self.get(0xd3, 0)?;
        let options = MouseOptions::parse(&raw_options)?;
        let raw_dpi = self.get(0xd4, options.profile)?;
        let dpi = MouseDpi::parse(&raw_dpi)?;
        ensure!(
            MouseOptions::parse(&self.get(0xd3, 0)?)? == options,
            "Настройки мыши изменились во время чтения"
        );
        let snapshot = MouseSnapshot {
            captured_utc: chrono::Utc::now().to_rfc3339(),
            source: "Прямой HID · AJ159 APEX RF mailbox".into(),
            mouse_id: 0x800006db,
            last_battery_percent: Some(self.status()?.battery),
            polling_hz: options.polling_hz()?,
            options,
            dpi,
            raw_options,
            raw_dpi,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn write(&self, block: Block, data: &[u8]) -> Result<()> {
        let (read, write) = match block {
            Block::Options => (0xd3, 0x53),
            Block::Dpi => (0xd4, 0x54),
        };
        ensure!(
            data.len() == 64 && data[0] == read,
            "Invalid mouse configuration block"
        );
        let mut packet = data.to_vec();
        packet[0] = write;
        self.send_mouse(&packet)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
        Ok(())
    }
}
impl ButtonsIo for MouseSession {
    fn read_buttons(&self) -> Result<ButtonSnapshot> {
        self.read_buttons()
    }
    fn write_buttons(&self, profile: u8, matrix: &[u8]) -> Result<()> {
        for packet in crate::mouse_buttons::packets(profile, matrix)? {
            self.send_mouse(&packet)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        Ok(())
    }
}
pub fn read_snapshot() -> Result<MouseSnapshot> {
    MouseSession::open()?.read()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_mailbox_status_and_payload_checksum() {
        let mut raw = [0; 64];
        raw[..6].copy_from_slice(&[0, 0, 80, 1, 0, 1]);
        let status = Status::parse(&raw).unwrap();
        assert!(status.online && status.can_send && !status.can_read);
        assert_eq!(status.battery, 80);
        let packet = checked_payload(&[0xd3, 0]).unwrap();
        assert_eq!(packet[7], 0x2c);
        assert_eq!(&feature(&packet).unwrap()[1..], &packet);
        raw[4] = 2;
        assert!(Status::parse(&raw).is_err());
        assert!(Status::parse(&raw[..63]).is_err());
        assert!(checked_payload(&[0; 65]).is_err());
    }
}
