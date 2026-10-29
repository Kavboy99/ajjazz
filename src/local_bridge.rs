//! Allowlisted AJ159 APEX configuration through the installed AJAZZ RF service.
pub use crate::mouse::MouseSnapshot;
use crate::mouse::{Block, MouseIo};
use crate::protocol::{MouseDpi, MouseOptions};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{io::Read, time::Duration};
#[derive(Clone, Debug, Serialize)]
pub struct RadioStatus {
    pub source: String,
    pub mouse_id: u32,
    pub last_battery_percent: Option<u8>,
    pub service_online_flag: bool,
    pub device_path: String,
}

#[derive(Debug)]
enum Value<'a> {
    Number(u64),
    Bytes(&'a [u8]),
}
fn varint(data: &[u8], pos: &mut usize) -> Result<u64> {
    let mut n = 0;
    for shift in (0..70).step_by(7) {
        let b = *data.get(*pos).context("Truncated varint")?;
        *pos += 1;
        ensure!(shift < 63 || b <= 1, "Varint overflow");
        n |= u64::from(b & 127) << shift;
        if b & 128 == 0 {
            return Ok(n);
        }
    }
    bail!("Invalid varint")
}
fn fields(data: &[u8]) -> Result<Vec<(u64, Value<'_>)>> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let tag = varint(data, &mut pos)?;
        ensure!(tag >> 3 > 0, "Invalid protobuf field");
        let value = match tag & 7 {
            0 => Value::Number(varint(data, &mut pos)?),
            2 => {
                let len = usize::try_from(varint(data, &mut pos)?)?;
                let end = pos.checked_add(len).context("Length overflow")?;
                let bytes = data.get(pos..end).context("Truncated message")?;
                pos = end;
                Value::Bytes(bytes)
            }
            1 | 5 => {
                let len = if tag & 7 == 1 { 8 } else { 4 };
                ensure!(pos + len <= data.len(), "Truncated fixed field");
                pos += len;
                continue;
            }
            _ => bail!("Unsupported protobuf wire type"),
        };
        result.push((tag >> 3, value));
    }
    Ok(result)
}
fn number(fields: &[(u64, Value<'_>)], key: u64) -> u64 {
    fields
        .iter()
        .find_map(|(k, v)| {
            if *k == key {
                if let Value::Number(n) = v {
                    Some(*n)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or(0)
}
fn bytes<'a>(fields: &[(u64, Value<'a>)], key: u64) -> Option<&'a [u8]> {
    fields.iter().find_map(|(k, v)| {
        if *k == key {
            if let Value::Bytes(b) = v {
                Some(*b)
            } else {
                None
            }
        } else {
            None
        }
    })
}
pub fn decode_status(data: &[u8]) -> Result<Vec<RadioStatus>> {
    let list = fields(data)?;
    let mut result = Vec::new();
    for (tag, entry) in list {
        if tag != 1 {
            continue;
        }
        let Value::Bytes(entry) = entry else { continue };
        let dj = fields(entry)?;
        let Some(common) = bytes(&dj, 2) else {
            continue;
        };
        let common = fields(common)?;
        if (number(&common, 7), number(&common, 8)) != (0x3151, 0x5007) {
            continue;
        }
        let mouse = bytes(&common, 2)
            .map(fields)
            .transpose()?
            .unwrap_or_default();
        let status = bytes(&mouse, 2).map(fields).transpose()?;
        let battery = status.as_ref().map(|s| number(s, 1));
        ensure!(battery.is_none_or(|v| v <= 100), "Invalid service battery");
        result.push(RadioStatus {
            source: "AJAZZ local service / watchDevList".into(),
            mouse_id: u32::try_from(number(&common, 6))?,
            last_battery_percent: battery.map(|v| v as u8),
            service_online_flag: status.as_ref().is_some_and(|s| number(s, 2) != 0),
            device_path: bytes(&common, 3)
                .map(std::str::from_utf8)
                .transpose()?
                .unwrap_or("")
                .into(),
        });
    }
    Ok(result)
}
fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(500))
        .timeout(Duration::from_secs(3))
        .no_proxy()
        .build()?)
}
fn rpc(client: &reqwest::blocking::Client, method: &str, message: &[u8]) -> Result<Vec<u8>> {
    let mut body = vec![0];
    body.extend_from_slice(&(message.len() as u32).to_be_bytes());
    body.extend_from_slice(message);
    let mut response = client
        .post(format!("http://127.0.0.1:3814/driver.DriverGrpc/{method}"))
        .header("Content-Type", "application/grpc-web+proto")
        .header("X-Grpc-Web", "1")
        .body(body)
        .send()
        .context("Служба AJAZZ недоступна")?
        .error_for_status()?;
    let mut header = [0; 5];
    response.read_exact(&mut header)?;
    ensure!(
        header[0] == 0,
        "Compressed/trailer gRPC message unsupported"
    );
    let len = u32::from_be_bytes(header[1..].try_into()?) as usize;
    ensure!(len <= 65536, "Service message too large");
    let mut message = vec![0; len];
    response.read_exact(&mut message)?;
    Ok(message)
}
pub fn read_radio_status() -> Result<Vec<RadioStatus>> {
    decode_status(&rpc(&client()?, "watchDevList", &[])?)
}
/// Three whitelisted GETs through the running service; no DFU or flash operations.
pub(crate) fn read_firmware_identity() -> Result<crate::mouse_firmware::Identity> {
    use crate::mouse_firmware::{Identity, RADIO_MOUSE_ID, verify_mouse, version_word};
    #[cfg(windows)]
    let _lease = crate::hardware::DeviceLease::acquire("AJ159-APEX")?;
    let client = client()?;
    let candidates = decode_status(&rpc(&client, "watchDevList", &[])?)?;
    ensure!(
        candidates.len() == 1,
        "Нужен ровно один док APEX в службе AJAZZ"
    );
    let device = &candidates[0];
    ensure!(
        device.mouse_id == RADIO_MOUSE_ID,
        "Служба сообщает другую модель мыши"
    );
    let dock = firmware_get(&client, &device.device_path, 0xad)?;
    let mut report = Identity {
        captured_utc: chrono::Utc::now().to_rfc3339(),
        source: "Служба AJAZZ · только GET версии/ID".into(),
        dock_oled_version: version_word(0xad, &dock)?,
        dock_flash_version: u16::from_le_bytes([dock[3], dock[4]]),
        service_mouse_id: device.mouse_id,
        mouse_id_verified: false,
        mouse_version: None,
        mouse_read_error: None,
        raw_dock: dock,
        raw_mouse_identity: None,
        raw_mouse_version: None,
        firmware_programming_available: false,
    };
    let read_mouse = (|| -> Result<()> {
        ensure!(
            device.service_online_flag,
            "Мышь спит по данным службы. Нажмите её физическую кнопку и повторите чтение."
        );
        let identity = firmware_get(&client, &device.device_path, 0x8f)?;
        verify_mouse(&identity)?;
        report.raw_mouse_identity = Some(identity);
        report.mouse_id_verified = true;
        let version = firmware_get(&client, &device.device_path, 0x80)?;
        let word = version_word(0x80, &version)?;
        let identity = firmware_get(&client, &device.device_path, 0x8f)?;
        verify_mouse(&identity)?;
        report.raw_mouse_version = Some(version);
        report.mouse_version = Some(word);
        Ok(())
    })();
    if let Err(error) = read_mouse {
        report.mouse_read_error = Some(format!("{error:#}"));
    }
    let after = decode_status(&rpc(&client, "watchDevList", &[])?)?;
    ensure!(
        after.len() == 1
            && after[0].device_path == device.device_path
            && after[0].mouse_id == device.mouse_id,
        "Приёмник изменился во время чтения"
    );
    Ok(report)
}
fn firmware_get(client: &reqwest::blocking::Client, path: &str, command: u8) -> Result<Vec<u8>> {
    ensure!(
        [0xad, 0x8f, 0x80].contains(&command),
        "Не разрешён запрос прошивки"
    );
    let raw = command == 0xad;
    let mut packet = vec![0; if raw { 64 } else { 8 }];
    packet[0] = command;
    let mut message = Vec::new();
    append_bytes(&mut message, 1, path.as_bytes());
    append_bytes(&mut message, 2, &packet);
    if !raw {
        message.extend_from_slice(&[32, 2]);
    }
    check_error(&rpc(
        client,
        if raw { "sendRawFeature" } else { "sendMsg" },
        &message,
    )?)?;
    std::thread::sleep(Duration::from_millis(20));
    let mut message = Vec::new();
    append_bytes(&mut message, 1, path.as_bytes());
    let response = rpc(
        client,
        if raw { "readRawFeature" } else { "readMsg" },
        &message,
    )?;
    check_error(&response)?;
    let values = fields(&response)?;
    let data = bytes(&values, 2).context("Пустой ответ версии")?;
    ensure!(
        data.len() == 64 && data[0] == command,
        "Ответ версии не соответствует запросу"
    );
    Ok(data.to_vec())
}
fn append_varint(out: &mut Vec<u8>, mut n: usize) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn append_bytes(out: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
    out.push((tag << 3) | 2);
    append_varint(out, bytes.len());
    out.extend_from_slice(bytes);
}
fn check_error(message: &[u8]) -> Result<()> {
    let values = fields(message)?;
    if let Some(error) = bytes(&values, 1) {
        ensure!(
            error.is_empty(),
            "AJAZZ service: {}",
            String::from_utf8_lossy(error)
        );
    }
    Ok(())
}
fn mouse_get(
    client: &reqwest::blocking::Client,
    path: &str,
    command: u8,
    profile: u8,
) -> Result<Vec<u8>> {
    ensure!(
        [0xd3, 0xd4].contains(&command),
        "Only mouse GET queries are allowed"
    );
    let mut packet = [0; 8];
    packet[0] = command;
    if command == 0xd4 {
        packet[1] = profile;
    }
    mouse_send(client, path, &packet)?;
    std::thread::sleep(Duration::from_millis(5));
    let mut message = Vec::new();
    append_bytes(&mut message, 1, path.as_bytes());
    let response = rpc(client, "readMsg", &message)?;
    check_error(&response)?;
    let values = fields(&response)?;
    let data = bytes(&values, 2).context("Empty mouse reply")?;
    ensure!(
        data.len() == 64 && data[0] == command,
        "Ответ мыши не соответствует запросу"
    );
    Ok(data.to_vec())
}
fn mouse_send(client: &reqwest::blocking::Client, path: &str, packet: &[u8]) -> Result<()> {
    ensure!(
        matches!(
            (packet.first(), packet.len()),
            (Some(0xd3 | 0xd4), 8) | (Some(0x53 | 0x54 | 0xd0 | 0x09), 64)
        ),
        "Unsupported mouse command"
    );
    let mut message = Vec::new();
    append_bytes(&mut message, 1, path.as_bytes());
    append_bytes(&mut message, 2, packet);
    // BIT7=0 is protobuf's omitted default. DangleDevType.MOUSE=2.
    message.extend_from_slice(&[32, 2]);
    check_error(&rpc(client, "sendMsg", &message)?)?;
    Ok(())
}
pub(crate) struct MouseSession {
    client: reqwest::blocking::Client,
    device: RadioStatus,
    #[cfg(windows)]
    _lease: crate::hardware::DeviceLease,
}
impl MouseSession {
    pub(crate) fn open() -> Result<Self> {
        #[cfg(windows)]
        let lease = crate::hardware::DeviceLease::acquire("AJ159-APEX")?;
        let client = client()?;
        let device = find_mouse(&client)?;
        Ok(Self {
            client,
            device,
            #[cfg(windows)]
            _lease: lease,
        })
    }
    pub(crate) fn read_buttons(&self) -> Result<crate::mouse_buttons::ButtonSnapshot> {
        let device = find_mouse(&self.client)?;
        ensure!(
            device.device_path == self.device.device_path,
            "Приёмник переподключён. Повторите чтение."
        );
        let options = MouseOptions::parse(&mouse_get(&self.client, &device.device_path, 0xd3, 0)?)?;
        let mut packet = [0; 64];
        packet[0] = 0xd0;
        packet[1] = options.profile;
        mouse_send(&self.client, &device.device_path, &packet)?;
        std::thread::sleep(Duration::from_millis(5));
        let mut message = Vec::new();
        append_bytes(&mut message, 1, device.device_path.as_bytes());
        let response = rpc(&self.client, "readMsg", &message)?;
        check_error(&response)?;
        let values = fields(&response)?;
        let matrix = bytes(&values, 2)
            .context("Empty mouse button reply")?
            .to_vec();
        let check = MouseOptions::parse(&mouse_get(&self.client, &device.device_path, 0xd3, 0)?)?;
        ensure!(
            options.profile == check.profile,
            "Профиль мыши изменился во время чтения кнопок"
        );
        let snapshot = crate::mouse_buttons::ButtonSnapshot {
            schema: 1,
            captured_utc: chrono::Utc::now().to_rfc3339(),
            mouse_id: device.mouse_id,
            profile: options.profile,
            raw_matrix: matrix,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}
impl crate::mouse_buttons::ButtonsIo for MouseSession {
    fn read_buttons(&self) -> Result<crate::mouse_buttons::ButtonSnapshot> {
        MouseSession::read_buttons(self)
    }
    fn write_buttons(&self, profile: u8, matrix: &[u8]) -> Result<()> {
        let mut failures = Vec::new();
        for packet in crate::mouse_buttons::packets(profile, matrix)? {
            if let Err(error) = mouse_send(&self.client, &self.device.device_path, &packet) {
                failures.push(format!("{error:#}"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(500));
        ensure!(failures.is_empty(), "{}", failures.join("; "));
        Ok(())
    }
}
fn find_mouse(client: &reqwest::blocking::Client) -> Result<RadioStatus> {
    let devices = decode_status(&rpc(client, "watchDevList", &[])?)?;
    let device = devices
        .into_iter()
        .find(|dev| dev.mouse_id == 0x800006db)
        .context("AJ159 APEX ID1755 не найден в службе AJAZZ")?;
    ensure!(
        device.service_online_flag,
        "Мышь спит по данным службы. Нажмите её физическую кнопку и повторите чтение."
    );
    ensure!(
        device
            .device_path
            .to_ascii_lowercase()
            .starts_with("\\\\?\\hid#vid_3151&pid_5007&mi_02#"),
        "Unexpected dock path"
    );
    Ok(device)
}
impl MouseIo for MouseSession {
    fn read(&self) -> Result<MouseSnapshot> {
        let client = &self.client;
        let device = find_mouse(client)?;
        ensure!(
            device.device_path == self.device.device_path,
            "Приёмник мыши переподключён. Повторите операцию."
        );
        let raw_options = mouse_get(client, &device.device_path, 0xd3, 0)?;
        let options = MouseOptions::parse(&raw_options)?;
        let raw_dpi = mouse_get(client, &device.device_path, 0xd4, options.profile)?;
        let dpi = MouseDpi::parse(&raw_dpi)?;
        let check = mouse_get(client, &device.device_path, 0xd3, 0)?;
        ensure!(
            MouseOptions::parse(&check)? == options,
            "Параметры мыши изменились во время чтения"
        );
        let snapshot = MouseSnapshot {
            captured_utc: chrono::Utc::now().to_rfc3339(),
            source: "AJAZZ service · D3/D4 · route MOUSE".into(),
            mouse_id: device.mouse_id,
            last_battery_percent: device.last_battery_percent,
            polling_hz: options.polling_hz()?,
            options,
            dpi,
            raw_options,
            raw_dpi,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    fn write(&self, block: Block, state: &[u8]) -> Result<()> {
        ensure!(state.len() == 64, "Invalid mouse block");
        let (read, write) = match block {
            Block::Options => (0xd3, 0x53),
            Block::Dpi => (0xd4, 0x54),
        };
        ensure!(state[0] == read, "Invalid mouse block header");
        let mut packet = state.to_vec();
        packet[0] = write;
        packet[7] = 0;
        mouse_send(&self.client, &self.device.device_path, &packet)?;
        std::thread::sleep(Duration::from_millis(50));
        Ok(())
    }
}
pub fn read_mouse_snapshot() -> Result<MouseSnapshot> {
    MouseSession::open()?.read()
}
#[cfg(test)]
mod tests {
    use super::*;
    // Reduced protobuf fixture using the installed driver's field numbers.
    #[test]
    fn decodes_battery_and_separate_online_flag() {
        let data = [
            0x0a, 0x13, 0x12, 0x11, 0x12, 0x06, 0x12, 0x04, 0x08, 80, 0x10, 1, 0x30, 1, 0x38, 0xd1,
            0x62, 0x40, 0x87, 0xa0, 1,
        ];
        let s = decode_status(&data).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].last_battery_percent, Some(80));
        assert!(s[0].service_online_flag);
        assert_eq!(s[0].mouse_id, 1);
    }
    #[test]
    fn rejects_malformed_varint() {
        let mut p = 0;
        assert!(varint(&[255; 11], &mut p).is_err());
    }
    #[test]
    fn refuses_truncated_message() {
        assert!(decode_status(&[0x0a, 10, 1]).is_err());
    }
}
