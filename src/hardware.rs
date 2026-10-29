use crate::protocol::{self, KeyboardArea, KeyboardInfo};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub usage_page: u16,
    pub usage: u16,
    pub interface: i32,
    pub role: String,
}
fn is_keyboard_control(endpoint: &Endpoint, wired: bool) -> bool {
    endpoint.vid == 0x0c45
        && endpoint.interface == 2
        && if wired {
            endpoint.pid == 0x80a0
                && endpoint.usage_page == 0xff68
                && endpoint.usage == 97
                && endpoint.product == "AK820MAX"
        } else {
            endpoint.pid == 0xfefc
                && endpoint.usage_page == 0xff80
                && endpoint.usage == 1
                && endpoint.product == "AJAZZ AK820MAX"
        }
}
fn select_keyboard_control(
    endpoints: Vec<Endpoint>,
    wired: bool,
    path: Option<&str>,
) -> Result<Endpoint> {
    let mut matches: Vec<_> = endpoints
        .into_iter()
        .filter(|e| is_keyboard_control(e, wired) && path.is_none_or(|p| e.path == p))
        .collect();
    ensure!(
        matches.len() == 1,
        "AK820MAX: для {} найдено интерфейсов: {}. Проверьте выбранное подключение и прочитайте настройки заново.",
        if wired { "USB" } else { "2.4 GHz" },
        matches.len()
    );
    Ok(matches.remove(0))
}

#[cfg(windows)]
pub fn scan() -> Result<Vec<Endpoint>> {
    let api = hidapi::HidApi::new()?;
    Ok(api
        .device_list()
        .filter(|d| matches!(d.vendor_id(), 0x0c45 | 0x3151))
        .map(|d| {
            let role = match (d.vendor_id(), d.product_id(), d.usage_page(), d.usage()) {
                (0x0c45, 0xfefc, 0xff80, 1) => "AK820MAX · управление (32 байта)",
                (0x0c45, 0xfefc, 0xff70, 113) => "AK820MAX · дополнительный интерфейс (64 байта)",
                (0x0c45, 0x80a0, 0xff68, 97) => "AK820MAX · управление USB (64 байта)",
                (0x3151, 0x5007, 0xffff, 2) => "APEX · док и радиоприёмник",
                _ => "Системный HID-интерфейс",
            };
            Endpoint {
                path: d.path().to_string_lossy().into_owned(),
                vid: d.vendor_id(),
                pid: d.product_id(),
                product: d.product_string().unwrap_or("").into(),
                usage_page: d.usage_page(),
                usage: d.usage(),
                interface: d.interface_number(),
                role: role.into(),
            }
        })
        .collect())
}
#[cfg(not(windows))]
pub fn scan() -> Result<Vec<Endpoint>> {
    bail!("This transport currently targets Windows")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub endpoint: Endpoint,
    pub info: KeyboardInfo,
    pub blocks: BTreeMap<String, Vec<u8>>,
}
pub fn usb_snapshot() -> Result<Snapshot> {
    #[cfg(windows)]
    {
        Keyboard::open_usb()?.snapshot()
    }
    #[cfg(not(windows))]
    {
        bail!("Windows only")
    }
}
pub fn keyboard_info(wired: bool) -> Result<KeyboardInfo> {
    #[cfg(windows)]
    {
        let device = if wired {
            Keyboard::open_usb()?
        } else {
            Keyboard::open()?
        };
        device.read_info()
    }
    #[cfg(not(windows))]
    {
        let _ = wired;
        bail!("Windows only")
    }
}
impl Snapshot {
    pub fn is_wired(&self) -> Result<bool> {
        ensure!(
            self.schema == 1 && !self.endpoint.path.is_empty(),
            "Некорректный снимок клавиатуры"
        );
        supported_lighting_revision(&self.info)?;
        let wired = match (
            is_keyboard_control(&self.endpoint, true),
            is_keyboard_control(&self.endpoint, false),
        ) {
            (true, false) => true,
            (false, true) => false,
            _ => bail!("Неизвестный управляющий интерфейс в снимке клавиатуры"),
        };
        ensure!(
            self.info.work_mode == if wired { 15 } else { 2 },
            "Подключение в снимке не совпадает с режимом клавиатуры"
        );
        Ok(wired)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        crate::storage::write_json_atomic(path, self)?;
        Ok(())
    }
}

#[cfg(windows)]
pub struct Keyboard {
    device: hidapi::HidDevice,
    pub endpoint: Endpoint,
    _lease: DeviceLease,
    last_output: std::cell::Cell<Option<std::time::Instant>>,
    report_size: usize,
}
#[cfg(windows)]
pub(crate) struct DeviceLease(*mut std::ffi::c_void);
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        attributes: *const std::ffi::c_void,
        initial_owner: i32,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn ReleaseMutex(handle: *mut std::ffi::c_void) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}
#[cfg(windows)]
impl DeviceLease {
    pub(crate) fn acquire(model: &str) -> Result<Self> {
        Self::acquire_timeout(model, 0)?.ok_or_else(|| {
            anyhow::anyhow!("{model}: устройство уже занято другой операцией AJAZZ Studio")
        })
    }
    pub(crate) fn acquire_timeout(model: &str, milliseconds: u32) -> Result<Option<Self>> {
        let name: Vec<u16> = format!("Local\\AjazzStudio-{model}\0")
            .encode_utf16()
            .collect();
        // The mutex lives for the HID handle's lifetime and is never sent to another thread.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        ensure!(
            !handle.is_null(),
            "Cannot create device mutex: {}",
            std::io::Error::last_os_error()
        );
        let wait = unsafe { WaitForSingleObject(handle, milliseconds) };
        if wait != 0 && wait != 0x80 {
            unsafe {
                CloseHandle(handle);
            }
            if wait == 258 {
                return Ok(None);
            }
            bail!("{model}: не удалось получить доступ к устройству");
        }
        Ok(Some(Self(handle)))
    }
}
#[cfg(windows)]
impl Drop for DeviceLease {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
impl Keyboard {
    pub(crate) fn calibration_control(&self, op: u8) -> Result<()> {
        ensure!(self.report_size == 64, "Калибровка требует USB");
        let packet = crate::keyboard_calibration::command(op)?;
        if op == 0x64 {
            self.arm_input()?;
        }
        let mut out = vec![0];
        out.extend_from_slice(&packet);
        let count = self.device.write(&out)?;
        ensure!(count == 0 || count == 65, "Short calibration command");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while std::time::Instant::now() < deadline {
            let p = self.calibration_read()?;
            if p.len() == 64 && p[..2] == [0x55, op] {
                return Ok(());
            }
        }
        bail!("Нет подтверждения калибровки0x{op:02x}. Команда автоматически не повторяется")
    }
    pub(crate) fn calibration_read(&self) -> Result<Vec<u8>> {
        let mut raw = [0; 65];
        let n = self.device.read_timeout(&mut raw, 50)?;
        Ok(if n == 65 && raw[0] == 0 {
            raw[1..n].to_vec()
        } else {
            raw[..n].to_vec()
        })
    }
    pub(crate) fn memory_exchange(&self, packet: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            self.report_size == 64
                && packet.len() == 64
                && packet[..5] == [0xaa, 0x70, b'A', b'K', 1],
            "AJKS requires wired USB"
        );
        self.arm_input()?;
        if let Some(previous) = self.last_output.get() {
            std::thread::sleep(
                std::time::Duration::from_millis(10).saturating_sub(previous.elapsed()),
            );
        }
        let mut report = vec![0];
        report.extend_from_slice(packet);
        let count = self.device.write(&report)?;
        self.last_output.set(Some(std::time::Instant::now()));
        ensure!(count == 0 || count == 65, "Short AJKS output");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while std::time::Instant::now() < deadline {
            let mut raw = [0; 65];
            let n = self.device.read_timeout(&mut raw, 100)?;
            let p = if n == 65 && raw[0] == 0 {
                &raw[1..n]
            } else {
                &raw[..n]
            };
            if p.len() == 64 && p[..2] == [0x55, 0x70] {
                return crate::keyboard_memory::response(p, packet[5], packet[6]);
            }
        }
        bail!("Нет ответа AJKS; повторите чтение. Команда записи автоматически не повторяется")
    }
    pub fn open() -> Result<Self> {
        Self::open_channel(false)
    }
    pub(crate) fn open_usb() -> Result<Self> {
        Self::open_channel(true)
    }
    pub(crate) fn open_usb_background() -> Result<Option<Self>> {
        let Some(lease) = DeviceLease::acquire_timeout("AK820MAX", 0)? else {
            return Ok(None);
        };
        Ok(Some(Self::open_with_lease(true, None, lease)?))
    }
    pub(crate) fn open_for_snapshot(snapshot: &Snapshot) -> Result<Self> {
        let wired = snapshot.is_wired()?;
        Self::open_control(wired, Some(&snapshot.endpoint.path))
    }
    fn open_channel(usb: bool) -> Result<Self> {
        Self::open_control(usb, None)
    }
    fn open_control(usb: bool, path: Option<&str>) -> Result<Self> {
        // User operations wait for a short background read in their worker;
        // the UI stays interactive and the HID mailbox is never interleaved.
        let lease = DeviceLease::acquire_timeout("AK820MAX", 5000)?
            .ok_or_else(|| anyhow::anyhow!("Клавиатура занята другой операцией"))?;
        Self::open_with_lease(usb, path, lease)
    }
    fn open_with_lease(usb: bool, path: Option<&str>, lease: DeviceLease) -> Result<Self> {
        let endpoint = select_keyboard_control(scan()?, usb, path)?;
        let api = hidapi::HidApi::new()?;
        let path = std::ffi::CString::new(endpoint.path.as_str())?;
        let device = api.open_path(&path)?;
        Ok(Self {
            device,
            endpoint,
            _lease: lease,
            last_output: std::cell::Cell::new(None),
            report_size: if usb { 64 } else { 32 },
        })
    }
    fn transaction(&self, cmd: u8, length: usize, data: Option<&[u8]>) -> Result<Vec<u8>> {
        self.transaction_timed(
            cmd,
            length,
            data,
            std::time::Duration::from_millis(if self.report_size == 64 { 0 } else { 10 }),
            true,
        )
    }
    fn transaction_timed(
        &self,
        cmd: u8,
        length: usize,
        data: Option<&[u8]>,
        gap: std::time::Duration,
        settle: bool,
    ) -> Result<Vec<u8>> {
        self.transaction_region(cmd, length, data, gap, settle, (0, true))
    }
    fn transaction_region(
        &self,
        cmd: u8,
        length: usize,
        data: Option<&[u8]>,
        gap: std::time::Duration,
        settle: bool,
        region: (u16, bool),
    ) -> Result<Vec<u8>> {
        let mut packets =
            protocol::keyboard_packets(cmd, region.0, length, data, self.report_size)?;
        if !region.1 {
            for packet in &mut packets {
                packet[6] = 0;
            }
        }
        let mut result = Vec::with_capacity(length);
        for packet in packets {
            #[cfg(debug_assertions)]
            let trace_start = std::time::Instant::now();
            let mut report = vec![0];
            report.extend_from_slice(&packet);
            let addr = u16::from_le_bytes([packet[3], packet[4]]);
            let mut matched = None;
            // RF may drop an individual GET reply. Repeat that read once; never
            // retry a settings write whose acknowledgement might simply be lost.
            let attempts = if data.is_none() { 2 } else { 1 };
            for _ in 0..attempts {
                // WebHID keeps an input read pending before sending Output.
                // Arm the overlapped read now and handle link notifications
                // before attempting a settings write.
                self.arm_input()?;
                // The native backend can outpace WebHID. Leave the RF MCU a quiet
                // interval between reports, across area boundaries as well.
                if let Some(previous) = self.last_output.get() {
                    let remaining = gap.saturating_sub(previous.elapsed());
                    if !remaining.is_zero() {
                        std::thread::sleep(remaining);
                    }
                }
                #[cfg(debug_assertions)]
                let write_start = std::time::Instant::now();
                let count = self.device.write(&report)?;
                #[cfg(debug_assertions)]
                if std::env::var_os("AJAZZ_TRACE_RGB").is_some() && cmd == 0x24 {
                    eprintln!(
                        "RGB write addr={addr} elapsed_us={}",
                        write_start.elapsed().as_micros()
                    );
                }
                self.last_output.set(Some(std::time::Instant::now()));
                // hidapi's Windows native backend returns 0 when overlapped
                // WriteFile completes synchronously. The matching ACK below,
                // followed by transaction readback, remains the success criterion.
                ensure!(count == 0 || count == report.len(), "Short HID output");
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
                while std::time::Instant::now() < deadline {
                    let mut b = [0; 65];
                    let len = self.device.read_timeout(&mut b, 100)?;
                    match protocol::keyboard_link_event(&b[..len]) {
                        Some(protocol::KeyboardLinkEvent::Disconnected) => bail!(
                            "Приёмник сообщил о потере связи с клавиатурой. Операция остановлена. Проверьте режим 2.4G; при мигающей R переподключите донгл."
                        ),
                        Some(protocol::KeyboardLinkEvent::Sleeping) => bail!(
                            "Клавиатура сообщила о переходе в сон. Нажмите физическую клавишу и повторите чтение."
                        ),
                        _ => {}
                    }
                    if len > 0
                        && let Ok(response) = protocol::parse_keyboard_response(
                            &b[..len],
                            cmd,
                            addr,
                            usize::from(packet[2]),
                        )
                    {
                        matched = Some(response.payload);
                        break;
                    }
                }
                if matched.is_some() {
                    break;
                }
            }
            match matched {
                Some(payload) => result.extend(payload),
                None => {
                    let hint = if self.report_size == 64 {
                        "Проверьте кабель передачи данных и положение переключателя USB."
                    } else {
                        "Нажмите физическую клавишу; если R мигает и ввода нет, переподключите донгл."
                    };
                    bail!(
                        "Нет подтверждённого ответа 0x{cmd:02X}, адрес {addr}. Операция остановлена. {hint}"
                    );
                }
            }
            #[cfg(debug_assertions)]
            if cmd == 0x24 && std::env::var_os("AJAZZ_TRACE_RGB").is_some() {
                eprintln!(
                    "RGB packet addr={addr} len={} final={} elapsed_us={}",
                    packet[2],
                    packet[6],
                    trace_start.elapsed().as_micros()
                );
            }
        }
        // Vendor UI caches a successful SET instead of immediately rereading all
        // areas. Let persistent writes settle before our stronger verification.
        if settle && data.is_some() && matches!(cmd, 0x21..=0x28) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        Ok(result)
    }
    fn arm_input(&self) -> Result<()> {
        let mut latest_link = None;
        for _ in 0..64 {
            let mut raw = [0; 65];
            let count = self.device.read_timeout(&mut raw, 0)?;
            if count == 0 {
                match latest_link {
                    Some(protocol::KeyboardLinkEvent::Disconnected) => {
                        bail!("Приёмник сообщил о потере связи с клавиатурой; запись остановлена")
                    }
                    Some(protocol::KeyboardLinkEvent::Sleeping) => {
                        bail!("Клавиатура уснула; нажмите физическую клавишу и повторите действие")
                    }
                    _ => {}
                }
                return Ok(());
            }
            // An already queued wake supersedes an earlier sleep notification.
            if let Some(event) = protocol::keyboard_link_event(&raw[..count]) {
                latest_link = Some(event);
            }
            // No command is outstanding here. Other queued control reports are
            // stale; they cannot acknowledge the request about to be sent.
        }
        bail!("Поток посторонних ответов HID; закройте другие драйверы клавиатуры")
    }
    pub(crate) fn write_animation_table(&self, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() == 512, "Invalid animation matrix");
        // Leave 20 ms between RF packets after a lost mid-table ACK at 10 ms.
        // Wired RGB does not need the RF quiet period
        // or the configuration readback delay between consecutive frames.
        let gap = std::time::Duration::from_millis(if self.report_size == 64 { 0 } else { 20 });
        self.transaction_timed(0x24, 512, Some(bytes), gap, false)?;
        Ok(())
    }
    pub(crate) fn write_animation_delta(&self, previous: &[u8], bytes: &[u8]) -> Result<()> {
        let regions = protocol::rgb_delta_regions(previous, bytes, self.report_size)?;
        let gap = std::time::Duration::from_millis(if self.report_size == 64 { 0 } else { 20 });
        for (index, region) in regions.iter().enumerate() {
            self.transaction_region(
                0x24,
                region.len(),
                Some(&bytes[region.clone()]),
                gap,
                false,
                (region.start as u16, index + 1 == regions.len()),
            )?;
        }
        Ok(())
    }
    pub(crate) fn read_animation_table(&self) -> Result<Vec<u8>> {
        let gap = std::time::Duration::from_millis(if self.report_size == 64 { 0 } else { 10 });
        self.transaction_timed(0x14, 512, None, gap, false)
    }
    pub fn read_info(&self) -> Result<KeyboardInfo> {
        KeyboardInfo::parse(&self.transaction(0x10, 48, None)?)
    }
    pub(crate) fn listen_control_report(&self, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut bytes = [0; 65];
        let length = self.device.read_timeout(&mut bytes, timeout_ms)?;
        Ok(bytes[..length].to_vec())
    }
    /// Only the source-confirmed temporary RGB/readback commands used by diagnostics.
    pub(crate) fn temporary_exchange(&self, packet: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            packet.len() == self.report_size,
            "Temporary report length mismatch"
        );
        ensure!(
            packet[0] == 0xaa && [0x32, 0x35, 0x37].contains(&packet[1]),
            "Unsupported temporary command"
        );
        if let Some(previous) = self.last_output.get() {
            std::thread::sleep(
                std::time::Duration::from_millis(10).saturating_sub(previous.elapsed()),
            );
        }
        let mut report = vec![0];
        report.extend_from_slice(packet);
        let count = self.device.write(&report)?;
        self.last_output.set(Some(std::time::Instant::now()));
        ensure!(
            count == 0 || count == report.len(),
            "Short temporary report"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1000);
        while std::time::Instant::now() < deadline {
            let mut raw = [0; 65];
            let count = self.device.read_timeout(&mut raw, 100)?;
            let data = if count == self.report_size + 1 && raw[0] == 0 {
                &raw[1..count]
            } else {
                &raw[..count]
            };
            ensure!(
                !matches!(
                    protocol::keyboard_link_event(data),
                    Some(
                        protocol::KeyboardLinkEvent::Disconnected
                            | protocol::KeyboardLinkEvent::Sleeping
                    )
                ),
                "Клавиатура потеряла связь или уснула"
            );
            if data.len() == self.report_size
                && data[..2] == [0x55, packet[1]]
                && (if packet[1] == 0x35 {
                    true
                } else if packet[1] == 0x37 {
                    data[2] == packet[2]
                } else {
                    data[3..5] == packet[3..5]
                })
            {
                return Ok(data.to_vec());
            }
        }
        bail!("Нет ответа временного RGB 0x{:02X}", packet[1])
    }
    pub(crate) fn usb_live_rgb(&self) -> Result<Vec<u8>> {
        ensure!(
            self.report_size == 64,
            "USB live RGB requires the wired interface"
        );
        let ids: Vec<u8> = (0..128).flat_map(|id| [id, 0, 0, 0]).collect();
        self.transaction(0x33, 512, Some(&ids))
    }
    pub fn read_area(&self, area: KeyboardArea) -> Result<Vec<u8>> {
        let (cmd, _, len) = area.spec();
        self.transaction(cmd, len, None)
    }
    pub(crate) fn write_hall(&self, data: &[u8]) -> Result<()> {
        ensure!(data.len() == 1024, "Invalid Hall block");
        self.transaction(0x27, 1024, Some(data))?;
        Ok(())
    }
    pub(crate) fn write_keymap(&self, data: &[u8]) -> Result<()> {
        ensure!(data.len() == 512, "Invalid keymap block");
        self.transaction(0x22, 512, Some(data))?;
        Ok(())
    }
    pub(crate) fn write_dks(&self, data: &[u8]) -> Result<()> {
        ensure!(data.len() == 1024, "Invalid DKS block");
        self.transaction(0x28, 1024, Some(data))?;
        Ok(())
    }
    pub(crate) fn read_macro_bank(&self) -> Result<Vec<u8>> {
        // Exact YU/qG profile: 100 LE32 pointers (400 bytes), 512 bytes of data.
        // This is configuration GET 0x15, not flash/ISP access.
        self.transaction(0x15, 400 + 512, None)
    }
    pub(crate) fn write_macro_bank(&self, data: &[u8]) -> Result<()> {
        ensure!(
            data.len() == crate::keyboard_macros::BANK_BYTES,
            "Invalid macro bank"
        );
        // AZ in the vendor driver: index without final marker, then data at 400
        // with a final marker. Full data length preserves the captured unused tail.
        let gap = std::time::Duration::from_millis(10);
        self.transaction_region(0x25, 400, Some(&data[..400]), gap, false, (0, false))?;
        self.transaction_region(0x25, 512, Some(&data[400..]), gap, true, (400, true))?;
        Ok(())
    }
    pub(crate) fn write_general(&self, data: &[u8]) -> Result<()> {
        ensure!(data.len() == 56, "Invalid game-mode block");
        self.transaction(0x21, 56, Some(data))?;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let info = self.read_info()?;
        ensure!(info.firmware_status == 0, "Device is in boot mode");
        let mut blocks = BTreeMap::new();
        for &(cmd, len, name) in protocol::READ_BLOCKS.iter().skip(1) {
            blocks.insert(name.into(), self.transaction(cmd, len, None)?);
        }
        ensure!(
            self.read_info()?.profile == info.profile,
            "Профиль клавиатуры изменился во время чтения"
        );
        Ok(Snapshot {
            schema: 1,
            endpoint: self.endpoint.clone(),
            info,
            blocks,
        })
    }
    pub(crate) fn reapply_current_general(&self, output: &Path) -> Result<Snapshot> {
        let before = self.snapshot()?;
        supported_lighting_revision(&before.info)?;
        let raw = before
            .blocks
            .get("game_mode")
            .context("Missing game mode")?;
        let settings = crate::keyboard_general::GeneralSettings::parse(raw)?;
        ensure!(
            settings.patch(raw)? == *raw,
            "General block uses unknown values"
        );
        // Diagnostic action can only resend the actual current block, never edits.
        before.save(output)?;
        self.write_general(raw)?;
        let after = self.snapshot()?;
        ensure!(
            after.info.profile == before.info.profile && after.blocks == before.blocks,
            "Повторная запись исходного блока не прошла проверку. Копия: {}",
            output.display()
        );
        Ok(after)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightingState {
    pub lighting: Vec<u8>,
    pub custom_rgb: Vec<u8>,
}
impl LightingState {
    fn validate_effect(&self) -> Result<()> {
        ensure!(self.lighting.len() == 16, "Invalid lighting backup");
        Ok(())
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.validate_effect()?;
        ensure!(self.custom_rgb.len() == 512, "Invalid RGB backup");
        ensure!(
            self.custom_rgb
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .all(|(id, b)| usize::from(b[0]) == id),
            "RGB-таблица не содержит ожидаемые индексы клавиш. Запись собственного кадра остановлена."
        );
        Ok(())
    }
    pub(crate) fn equivalent(&self, other: &Self) -> bool {
        self.custom_rgb == other.custom_rgb
            && protocol::lighting_equivalent(&self.lighting, &other.lighting)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightingBackup {
    pub schema: u32,
    pub created_utc: String,
    pub info: KeyboardInfo,
    pub state: LightingState,
}
impl LightingBackup {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = crate::storage::read_json(path, 64 * 1024)?;
        ensure!(value.schema == 1, "Unsupported backup schema");
        supported_lighting_revision(&value.info)?;
        value.state.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LightingReceipt {
    pub backup_path: PathBuf,
    pub info: KeyboardInfo,
    pub state: LightingState,
    pub rgb_verified: bool,
    pub effect_color_verified: bool,
    pub requested_effect: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EffectBackup {
    schema: u32,
    created_utc: String,
    info: KeyboardInfo,
    lighting: Vec<u8>,
    #[serde(default)]
    color_source: String,
}

pub fn supported_lighting_revision(info: &KeyboardInfo) -> Result<()> {
    supported_keyboard_identity(info)?;
    ensure!(
        matches!(info.work_mode, 2 | 15),
        "Неизвестный режим подключения AK820MAX"
    );
    Ok(())
}
pub(crate) fn supported_keyboard_identity(info: &KeyboardInfo) -> Result<()> {
    ensure!(
        info.vid == 0x0c45
            && info.pid == 0x80a0
            && info.manufacturer == 358
            && info.product == 4100
            && matches!(info.version.as_str(), "1.30" | "1.36")
            && info.firmware_status == 0
            && info.frame_version == 0
            && info.lighting_version == 0,
        "Неизвестная ревизия AK820MAX: нужны ID1004 и версия1.30/1.36"
    );
    Ok(())
}

pub(crate) trait LightingIo {
    fn info(&self) -> Result<KeyboardInfo>;
    fn state(&self) -> Result<LightingState>;
    fn effect(&self) -> Result<Vec<u8>> {
        Ok(self.state()?.lighting)
    }
    fn write_rgb(&self, bytes: &[u8]) -> Result<()>;
    fn write_effect(&self, bytes: &[u8]) -> Result<()>;
}

#[cfg(windows)]
impl LightingIo for Keyboard {
    fn info(&self) -> Result<KeyboardInfo> {
        self.read_info()
    }
    fn state(&self) -> Result<LightingState> {
        let state = LightingState {
            lighting: self.read_area(KeyboardArea::Lighting)?,
            custom_rgb: self.read_area(KeyboardArea::CustomRgb)?,
        };
        state.validate()?;
        Ok(state)
    }
    fn effect(&self) -> Result<Vec<u8>> {
        self.read_area(KeyboardArea::Lighting)
    }
    fn write_rgb(&self, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() == 512, "Invalid RGB block");
        // gZ/kX sends the next report after its ACK, on both USB and 2.4G.
        // No blind streaming or write retries; verification still follows.
        self.transaction_timed(0x24, 512, Some(bytes), std::time::Duration::ZERO, false)?;
        Ok(())
    }
    fn write_effect(&self, bytes: &[u8]) -> Result<()> {
        let bytes = protocol::lighting_command(bytes)?;
        self.transaction_timed(0x23, 16, Some(&bytes), std::time::Duration::ZERO, false)?;
        Ok(())
    }
}

pub(crate) fn backup_directory() -> Result<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?;
    Ok(PathBuf::from(root).join("AjazzStudio/backups"))
}

fn save_lighting_backup(directory: &Path, backup: &LightingBackup) -> Result<PathBuf> {
    crate::storage::backup(directory, "ak820max-lighting", backup)
}

fn commit_lighting(
    device: &impl LightingIo,
    before: &LightingBackup,
    target: &LightingState,
    directory: &Path,
) -> Result<LightingReceipt> {
    commit_lighting_with_rgb(device, before, target, directory, true)
}
fn commit_lighting_with_rgb(
    device: &impl LightingIo,
    before: &LightingBackup,
    target: &LightingState,
    directory: &Path,
    write_rgb: bool,
) -> Result<LightingReceipt> {
    supported_lighting_revision(&before.info)?;
    target.validate()?;
    before.state.validate()?;
    // Durable backup and a fresh profile/state check precede every settings write.
    let backup_path = save_lighting_backup(directory, before)?;
    let fresh_info = device.info()?;
    supported_lighting_revision(&fresh_info)?;
    ensure!(
        fresh_info.profile == before.info.profile && device.state()?.equivalent(&before.state),
        "Настройки изменились в другом приложении. Прочитайте их заново."
    );
    let write_and_verify = |state: &LightingState| -> Result<LightingState> {
        // Match the website: activate custom mode before uploading its table.
        let custom = state.lighting[0] == 20 || state.lighting[0] == 128;
        if custom {
            device.write_effect(&state.lighting)?;
        }
        if write_rgb {
            device.write_rgb(&state.custom_rgb)?;
        }
        if !custom {
            device.write_effect(&state.lighting)?;
        }
        ensure!(
            device.info()?.profile == before.info.profile,
            "Keyboard profile changed"
        );
        let after = device.state()?;
        ensure!(
            after.equivalent(state),
            "Проверка подсветки не прошла: ожидался режим {:?}, прочитан {:?}; различающихся RGB-байт: {}",
            state.lighting,
            after.lighting,
            after
                .custom_rgb
                .iter()
                .zip(&state.custom_rgb)
                .filter(|(a, b)| a != b)
                .count()
        );
        Ok(after)
    };
    let observed = match write_and_verify(target) {
        Ok(state) => state,
        Err(error) => {
            // Never restore another profile if the user switched it during the operation.
            let restored = device.info().and_then(|info| {
                supported_lighting_revision(&info)?;
                ensure!(
                    info.profile == before.info.profile,
                    "Profile changed; automatic restore skipped"
                );
                let rgb_error = if write_rgb {
                    device.write_rgb(&before.state.custom_rgb).err()
                } else {
                    None
                };
                let effect_error = device.write_effect(&before.state.lighting).err();
                ensure!(
                    device.info()?.profile == before.info.profile,
                    "Keyboard profile changed during restore"
                );
                ensure!(
                    device.state()?.equivalent(&before.state),
                    "Исходные цвета не совпали; RGB: {rgb_error:?}, режим: {effect_error:?}"
                );
                Ok(())
            });
            match restored {
                Ok(()) => bail!(
                    "{error:#}. Исходная подсветка восстановлена и проверена. Копия: {}",
                    backup_path.display()
                ),
                Err(restore_error) => bail!(
                    "{error:#}. Восстановление не подтверждено: {restore_error:#}. Резервная копия: {}",
                    backup_path.display()
                ),
            }
        }
    };
    Ok(LightingReceipt {
        backup_path,
        info: fresh_info,
        state: observed,
        rgb_verified: true,
        effect_color_verified: true,
        requested_effect: None,
    })
}

#[cfg(test)]
fn commit_effect(
    device: &impl LightingIo,
    expected: &LightingBackup,
    target: &[u8],
    directory: &Path,
) -> Result<LightingReceipt> {
    commit_effect_with_previous(device, expected, target, directory, None)
}
fn commit_effect_with_previous(
    device: &impl LightingIo,
    expected: &LightingBackup,
    target: &[u8],
    directory: &Path,
    previous_command: Option<&[u8]>,
) -> Result<LightingReceipt> {
    supported_lighting_revision(&expected.info)?;
    // Native effects use only GET13/SET23. GET14 can be an uninitialized
    // all-zero table; its IDs must not block this unrelated operation.
    // Full RGB validation remains mandatory in commit_lighting/frame_target.
    expected.state.validate_effect()?;
    ensure!(target.len() == 16, "Invalid effect");
    let info = device.info()?;
    supported_lighting_revision(&info)?;
    let before = device.effect()?;
    ensure!(
        info.profile == expected.info.profile
            && protocol::lighting_equivalent(&before, &expected.state.lighting),
        "Подсветка или профиль изменились; прочитайте настройки заново"
    );
    let backup = EffectBackup {
        schema: 2,
        created_utc: chrono::Utc::now().to_rfc3339(),
        info: info.clone(),
        lighting: if let Some(previous) = previous_command {
            ensure!(
                protocol::native_effect_readback_matches(&before, previous),
                "Предыдущий режим изменился; прочитайте настройки заново"
            );
            previous.to_vec()
        } else {
            before.clone()
        },
        color_source: if previous_command.is_some() {
            "last_acknowledged_studio_command"
        } else {
            "device_readback_palette_not_available"
        }
        .into(),
    };
    let path = crate::storage::backup(directory, "ak820max-effect", &backup)?;
    ensure!(
        device.info()?.profile == info.profile,
        "Профиль изменился во время создания копии"
    );
    let apply = (|| -> Result<Vec<u8>> {
        // Readback cannot establish the current native palette. Even a target
        // equal to GET may be a requested return from red to white/RGB.
        device.write_effect(target)?;
        let current_info = device.info()?;
        let observed = device.effect()?;
        ensure!(
            current_info.profile == info.profile
                && protocol::native_effect_readback_matches(&observed, target),
            "Режим подсветки не совпал после записи"
        );
        Ok(observed)
    })();
    let observed = match apply {
        Ok(lighting) => lighting,
        Err(error) => {
            let restored = (|| -> Result<()> {
                let current = device.info()?;
                supported_lighting_revision(&current)?;
                ensure!(
                    current.profile == info.profile,
                    "Профиль изменился; возврат остановлен"
                );
                let write_error = device.write_effect(&backup.lighting).err();
                ensure!(
                    device.info()?.profile == info.profile
                        && protocol::native_effect_readback_matches(
                            &device.effect()?,
                            &backup.lighting
                        ),
                    "Исходный режим не совпал: {write_error:?}"
                );
                Ok(())
            })();
            match restored {
                Ok(()) => bail!(
                    "{error:#}. Исходный режим восстановлен и проверен. Копия: {}",
                    path.display()
                ),
                Err(e) => bail!(
                    "{error:#}. Восстановление не подтверждено: {e:#}. Копия: {}",
                    path.display()
                ),
            }
        }
    };
    Ok(LightingReceipt {
        backup_path: path,
        info,
        // A default white/RGB reply is not evidence even when white was requested.
        effect_color_verified: target[0] >= 20 && protocol::lighting_equivalent(&observed, target),
        requested_effect: Some(target.to_vec()),
        state: LightingState {
            lighting: observed,
            custom_rgb: expected.state.custom_rgb.clone(),
        },
        rgb_verified: false,
    })
}

pub fn apply_keyboard_effect(
    expected: &Snapshot,
    settings: &protocol::KeyboardLighting,
) -> Result<LightingReceipt> {
    apply_keyboard_effect_with_previous(expected, settings, None)
}
pub fn apply_keyboard_effect_with_previous(
    expected: &Snapshot,
    settings: &protocol::KeyboardLighting,
    previous_command: Option<&[u8]>,
) -> Result<LightingReceipt> {
    let expected_led = expected
        .blocks
        .get("lighting")
        .context("No lighting in snapshot")?;
    settings.patch(expected_led)?;
    #[cfg(windows)]
    {
        let device = Keyboard::open_for_snapshot(expected)?;
        let before = LightingBackup {
            schema: 1,
            created_utc: chrono::Utc::now().to_rfc3339(),
            info: expected.info.clone(),
            state: LightingState {
                lighting: expected_led.clone(),
                custom_rgb: expected
                    .blocks
                    .get("custom_rgb")
                    .cloned()
                    .unwrap_or_default(),
            },
        };
        commit_effect_with_previous(
            &device,
            &before,
            &settings.patch(expected_led)?,
            &backup_directory()?,
            previous_command,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (expected, settings, previous_command);
        bail!("Windows only")
    }
}

fn capture_lighting(device: &impl LightingIo) -> Result<LightingBackup> {
    let info = device.info()?;
    supported_lighting_revision(&info)?;
    Ok(LightingBackup {
        schema: 1,
        created_utc: chrono::Utc::now().to_rfc3339(),
        info,
        state: device.state()?,
    })
}

fn capture_native_effect(device: &impl LightingIo) -> Result<LightingBackup> {
    let info = device.info()?;
    supported_lighting_revision(&info)?;
    let state = LightingState {
        lighting: device.effect()?,
        custom_rgb: Vec::new(),
    };
    state.validate_effect()?;
    ensure!(
        device.info()?.profile == info.profile,
        "Профиль клавиатуры изменился во время чтения"
    );
    Ok(LightingBackup {
        schema: 1,
        created_utc: chrono::Utc::now().to_rfc3339(),
        info,
        state,
    })
}

pub(crate) fn frame_target(
    before: &LightingState,
    colors: &[(u8, [u8; 3])],
) -> Result<LightingState> {
    before.validate()?;
    ensure!(!colors.is_empty(), "No key colors");
    let ids: std::collections::BTreeSet<_> = crate::model::keyboard_rows()
        .into_iter()
        .flatten()
        .map(|key| key.id)
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut target = before.clone();
    for &(id, rgb) in colors {
        ensure!(
            ids.contains(&id) && seen.insert(id),
            "Unknown or duplicate key ID"
        );
        let offset = usize::from(id) * 4;
        target.custom_rgb[offset + 1..offset + 4].copy_from_slice(&rgb);
    }
    target.lighting = protocol::static_lighting(&before.lighting)?;
    Ok(target)
}

pub fn apply_keyboard_frame(colors: &[(u8, [u8; 3])]) -> Result<LightingReceipt> {
    apply_keyboard_frame_on(colors, false)
}
pub fn apply_keyboard_frame_from_snapshot(
    expected: &Snapshot,
    colors: &[(u8, [u8; 3])],
) -> Result<LightingReceipt> {
    supported_lighting_revision(&expected.info)?;
    expected.is_wired()?;
    let before = LightingBackup {
        schema: 1,
        created_utc: chrono::Utc::now().to_rfc3339(),
        info: expected.info.clone(),
        state: LightingState {
            lighting: expected
                .blocks
                .get("lighting")
                .context("No lighting in snapshot")?
                .clone(),
            custom_rgb: expected
                .blocks
                .get("custom_rgb")
                .context("No RGB in snapshot")?
                .clone(),
        },
    };
    let target = frame_target(&before.state, colors)?;
    #[cfg(windows)]
    {
        let device = Keyboard::open_for_snapshot(expected)?;
        // commit still rereads and compares both areas before any SET. Avoid a
        // second redundant full read used only to reconstruct the same backup.
        commit_lighting(&device, &before, &target, &backup_directory()?)
    }
    #[cfg(not(windows))]
    {
        let _ = (before, target);
        bail!("Windows only")
    }
}
pub fn apply_keyboard_frame_on(colors: &[(u8, [u8; 3])], wired: bool) -> Result<LightingReceipt> {
    #[cfg(windows)]
    {
        let device = if wired {
            Keyboard::open_usb()?
        } else {
            Keyboard::open()?
        };
        let before = capture_lighting(&device)?;
        ensure!(
            before.info.work_mode == if wired { 15 } else { 2 },
            "Клавиатура работает в другом режиме подключения"
        );
        let target = frame_target(&before.state, colors)?;
        commit_lighting(&device, &before, &target, &backup_directory()?)
    }
    #[cfg(not(windows))]
    {
        let _ = (colors, wired);
        bail!("Windows only")
    }
}

pub fn restore_keyboard_lighting(path: &Path) -> Result<LightingReceipt> {
    restore_keyboard_lighting_with_previous(path, None)
}
pub fn restore_keyboard_lighting_with_previous(
    path: &Path,
    previous_command: Option<&[u8]>,
) -> Result<LightingReceipt> {
    let metadata: serde_json::Value = crate::storage::read_json(path, 64 * 1024)?;
    if metadata.get("schema").and_then(|v| v.as_u64()) == Some(3) {
        return crate::keyboard_playback::restore_backup(path);
    }
    if metadata.get("schema").and_then(|v| v.as_u64()) == Some(2) {
        let saved: EffectBackup = serde_json::from_value(metadata)?;
        supported_lighting_revision(&saved.info)?;
        ensure!(saved.lighting.len() == 16, "Invalid effect backup");
        #[cfg(windows)]
        {
            let device = if saved.info.work_mode == 15 {
                Keyboard::open_usb()?
            } else {
                Keyboard::open()?
            };
            let current = capture_native_effect(&device)?;
            ensure!(
                current.info.profile == saved.info.profile,
                "Выбран другой профиль клавиатуры"
            );
            return commit_effect_with_previous(
                &device,
                &current,
                &saved.lighting,
                &backup_directory()?,
                previous_command,
            );
        }
        #[cfg(not(windows))]
        {
            bail!("Windows only");
        }
    }
    let backup = LightingBackup::load(path)?;
    #[cfg(windows)]
    {
        let device = if backup.info.work_mode == 15 {
            Keyboard::open_usb()?
        } else {
            Keyboard::open()?
        };
        let before = capture_lighting(&device)?;
        ensure!(
            before.info.profile == backup.info.profile,
            "Выбран другой профиль клавиатуры"
        );
        commit_lighting(&device, &before, &backup.state, &backup_directory()?)
    }
    #[cfg(not(windows))]
    {
        let _ = (backup, previous_command);
        bail!("Windows only")
    }
}

pub fn keyboard_snapshot() -> Result<Snapshot> {
    keyboard_snapshot_on(false)
}
pub fn keyboard_snapshot_on(wired: bool) -> Result<Snapshot> {
    #[cfg(windows)]
    {
        if wired {
            Keyboard::open_usb()?.snapshot()
        } else {
            Keyboard::open()?.snapshot()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = wired;
        bail!("Windows only")
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn background_lease_yields_to_foreground_without_mailbox_overlap() {
        use super::DeviceLease;
        let name = format!("IO-test-{}", std::process::id());
        let owner_name = name.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let owner = std::thread::spawn(move || {
            let lease = DeviceLease::acquire_timeout(&owner_name, 0)
                .unwrap()
                .unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            drop(lease);
        });
        ready_rx.recv().unwrap();
        assert!(DeviceLease::acquire_timeout(&name, 0).unwrap().is_none());
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            release_tx.send(()).unwrap();
        });
        let foreground = DeviceLease::acquire_timeout(&name, 1000).unwrap().unwrap();
        drop(foreground);
        owner.join().unwrap();
        release.join().unwrap();
        assert!(DeviceLease::acquire_timeout(&name, 0).unwrap().is_some());
    }
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Mock {
        info: KeyboardInfo,
        state: RefCell<LightingState>,
        writes: Cell<usize>,
        commands: RefCell<Vec<u8>>,
        fail_once: Cell<bool>,
    }
    impl LightingIo for Mock {
        fn info(&self) -> Result<KeyboardInfo> {
            Ok(self.info.clone())
        }
        fn state(&self) -> Result<LightingState> {
            Ok(self.state.borrow().clone())
        }
        fn write_rgb(&self, bytes: &[u8]) -> Result<()> {
            self.commands.borrow_mut().push(0x24);
            self.writes.set(self.writes.get() + 1);
            self.state.borrow_mut().custom_rgb = bytes.to_vec();
            ensure!(
                !self.fail_once.replace(false),
                "Simulated interrupted transfer"
            );
            Ok(())
        }
        fn write_effect(&self, bytes: &[u8]) -> Result<()> {
            self.commands.borrow_mut().push(0x23);
            self.writes.set(self.writes.get() + 1);
            self.state.borrow_mut().lighting = bytes.to_vec();
            Ok(())
        }
    }
    fn endpoint(wired: bool, path: &str) -> Endpoint {
        Endpoint {
            path: path.into(),
            vid: 0x0c45,
            pid: if wired { 0x80a0 } else { 0xfefc },
            usage_page: if wired { 0xff68 } else { 0xff80 },
            usage: if wired { 97 } else { 1 },
            interface: 2,
            product: if wired { "AK820MAX" } else { "AJAZZ AK820MAX" }.into(),
            role: String::new(),
        }
    }
    #[test]
    fn both_connections_present_are_selected_by_channel_and_pinned_path() {
        let endpoints = vec![endpoint(false, "radio"), endpoint(true, "usb")];
        assert_eq!(
            select_keyboard_control(endpoints.clone(), true, None)
                .unwrap()
                .path,
            "usb"
        );
        assert_eq!(
            select_keyboard_control(endpoints.clone(), false, None)
                .unwrap()
                .path,
            "radio"
        );
        assert!(select_keyboard_control(endpoints.clone(), true, Some("radio")).is_err());
        let mut duplicates = endpoints;
        duplicates.push(endpoint(true, "other-usb"));
        assert!(select_keyboard_control(duplicates.clone(), true, None).is_err());
        assert_eq!(
            select_keyboard_control(duplicates, true, Some("usb"))
                .unwrap()
                .path,
            "usb"
        );
    }
    #[test]
    fn snapshot_connection_rejects_isp_interface_and_mismatched_mode() {
        let mut snapshot = Snapshot {
            schema: 1,
            endpoint: endpoint(true, "usb"),
            info: fixture().info,
            blocks: BTreeMap::new(),
        };
        snapshot.info.work_mode = 15;
        assert!(snapshot.is_wired().unwrap());
        snapshot.endpoint.usage_page = 0xff67;
        assert!(snapshot.is_wired().is_err());
        snapshot.endpoint = endpoint(false, "radio");
        assert!(snapshot.is_wired().is_err());
        snapshot.info.work_mode = 2;
        assert!(!snapshot.is_wired().unwrap());
        snapshot.endpoint.pid = 0xfefd;
        assert!(snapshot.is_wired().is_err());
    }
    fn fixture() -> Mock {
        Mock {
            info: KeyboardInfo {
                vid: 0x0c45,
                pid: 0x80a0,
                version: "1.30".into(),
                manufacturer: 358,
                product: 4100,
                work_mode: 2,
                battery: 32,
                charging: 0,
                profile: 0,
                rt_precision: 0,
                frame_version: 0,
                lighting_version: 0,
                firmware_status: 0,
                animation_limits: None,
            },
            state: RefCell::new(LightingState {
                lighting: vec![0; 16],
                custom_rgb: protocol::custom_rgb(&[[12, 34, 56]; 128]).unwrap(),
            }),
            writes: Cell::new(0),
            commands: RefCell::new(Vec::new()),
            fail_once: Cell::new(false),
        }
    }
    fn temp_path() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "ajazz-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
    #[test]
    fn interrupted_transfer_restores_original_state() {
        let device = fixture();
        let before = capture_lighting(&device).unwrap();
        let target = frame_target(&before.state, &[(34, [0, 255, 200])]).unwrap();
        device.fail_once.set(true);
        let error = commit_lighting(&device, &before, &target, &temp_path()).unwrap_err();
        assert!(error.to_string().contains("восстановлена и проверена"));
        assert!(device.state().unwrap().equivalent(&before.state));
    }
    #[test]
    fn custom_mode_is_selected_before_table_and_backup_can_restore_it() {
        let device = fixture();
        let before = capture_lighting(&device).unwrap();
        let target = frame_target(&before.state, &[(34, [0, 255, 200])]).unwrap();
        commit_lighting(&device, &before, &target, &temp_path()).unwrap();
        assert_eq!(*device.commands.borrow(), vec![0x23, 0x24]);
        device.commands.borrow_mut().clear();
        let custom_before = capture_lighting(&device).unwrap();
        let mut builtin = target.clone();
        builtin.lighting[0] = 5;
        commit_lighting(&device, &custom_before, &builtin, &temp_path()).unwrap();
        assert_eq!(*device.commands.borrow(), vec![0x24, 0x23]);
        device.commands.borrow_mut().clear();
        let builtin_before = capture_lighting(&device).unwrap();
        commit_lighting(&device, &builtin_before, &target, &temp_path()).unwrap();
        assert_eq!(*device.commands.borrow(), vec![0x23, 0x24]);
        assert!(device.state().unwrap().equivalent(&custom_before.state));
    }
    #[test]
    fn failed_backup_prevents_all_settings_writes() {
        let device = fixture();
        let before = capture_lighting(&device).unwrap();
        let file = temp_path();
        std::fs::write(&file, b"not a directory").unwrap();
        assert!(commit_lighting(&device, &before, &before.state, &file).is_err());
        assert_eq!(device.writes.get(), 0);
        std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn another_app_changing_state_prevents_write() {
        let device = fixture();
        let before = capture_lighting(&device).unwrap();
        device.state.borrow_mut().custom_rgb[1] = 99;
        assert!(commit_lighting(&device, &before, &before.state, &temp_path()).is_err());
        assert_eq!(device.writes.get(), 0);
    }
    #[test]
    fn frame_preserves_unselected_and_unused_slots() {
        let device = fixture();
        let before = device.state().unwrap();
        let target = frame_target(&before, &[(34, [1, 2, 3])]).unwrap();
        assert_eq!(&target.custom_rgb[34 * 4 + 1..34 * 4 + 4], &[1, 2, 3]);
        assert_eq!(&target.custom_rgb[..34 * 4], &before.custom_rgb[..34 * 4]);
        assert_eq!(&target.custom_rgb[35 * 4..], &before.custom_rgb[35 * 4..]);
        assert!(frame_target(&before, &[(127, [0; 3])]).is_err());
        assert!(frame_target(&before, &[(34, [0; 3]), (34, [0; 3])]).is_err());
    }
    #[test]
    fn unknown_firmware_cannot_write() {
        let mut device = fixture();
        device.info.version = "2.00".into();
        assert!(capture_lighting(&device).is_err());
        assert_eq!(device.writes.get(), 0);
    }
    #[test]
    fn support_136_keeps_exact_identity_guards() {
        let mut device = fixture();
        device.info.version = "1.36".into();
        assert!(capture_lighting(&device).is_ok());
        device.info.product = 0;
        assert!(capture_lighting(&device).is_err());
        device.info.product = 4100;
        device.info.frame_version = 1;
        assert!(capture_lighting(&device).is_err());
        assert_eq!(device.writes.get(), 0);
    }
    #[test]
    fn builtin_effect_writes_no_rgb_matrix_and_keeps_reserved_fields() {
        let device = fixture();
        let before = capture_lighting(&device).unwrap();
        let mut target = before.state.clone();
        let settings = protocol::KeyboardLighting {
            mode: 7,
            color: [0, 0, 0],
            rainbow: false,
            brightness: 5,
            speed: 3,
            direction: 0,
        };
        target.lighting = settings.patch(&target.lighting).unwrap();
        let receipt =
            commit_lighting_with_rgb(&device, &before, &target, &temp_path(), false).unwrap();
        assert!(receipt.state.equivalent(&target));
        assert_eq!(device.writes.get(), 1);
        assert_eq!(receipt.state.custom_rgb, before.state.custom_rgb);
    }
    #[test]
    fn native_effect_works_with_uninitialized_rgb_but_frame_writes_stay_guarded() {
        struct NativeOnly(Mock);
        impl LightingIo for NativeOnly {
            fn info(&self) -> Result<KeyboardInfo> {
                self.0.info()
            }
            fn state(&self) -> Result<LightingState> {
                bail!("Native operation must not read RGB")
            }
            fn effect(&self) -> Result<Vec<u8>> {
                Ok(self.0.state.borrow().lighting.clone())
            }
            fn write_rgb(&self, _: &[u8]) -> Result<()> {
                bail!("Native operation must not write RGB")
            }
            fn write_effect(&self, bytes: &[u8]) -> Result<()> {
                self.0.write_effect(bytes)
            }
        }
        for rgb in [vec![0; 512], vec![], vec![7; 512]] {
            let device = fixture();
            device.state.borrow_mut().custom_rgb = rgb.clone();
            let before = capture_lighting(&device).unwrap();
            assert!(before.state.validate().is_err());
            assert!(frame_target(&before.state, &[(0, [255, 0, 0])]).is_err());
            assert!(commit_lighting(&device, &before, &before.state, &temp_path()).is_err());
            assert_eq!(device.writes.get(), 0);
            let native = NativeOnly(device);
            let mut target = before.state.lighting.clone();
            target[0] = 11;
            target[9] = 5;
            let receipt = commit_effect(&native, &before, &target, &temp_path()).unwrap();
            assert!(!receipt.rgb_verified);
            assert_eq!(*native.0.commands.borrow(), [0x23]);
            assert_eq!(native.0.state.borrow().custom_rgb, rgb);
            let saved: EffectBackup =
                crate::storage::read_json(&receipt.backup_path, 16384).unwrap();
            assert_eq!(saved.schema, 2);
            assert_eq!(saved.lighting, before.state.lighting);
            // The schema-2 restore path must also avoid reading the RGB table.
            let current = capture_native_effect(&native).unwrap();
            commit_effect(&native, &current, &saved.lighting, &temp_path()).unwrap();
            assert_eq!(*native.0.commands.borrow(), [0x23, 0x23]);
            assert_eq!(native.0.state.borrow().custom_rgb, rgb);
        }
    }
    #[test]
    fn native_effect_accepts_only_known_palette_fallback_and_never_reads_rgb() {
        struct Native {
            inner: Mock,
            sent: RefCell<Vec<Vec<u8>>>,
            fail_ack: Cell<bool>,
        }
        impl LightingIo for Native {
            fn info(&self) -> Result<KeyboardInfo> {
                self.inner.info()
            }
            fn state(&self) -> Result<LightingState> {
                bail!("RGB must not be read")
            }
            fn effect(&self) -> Result<Vec<u8>> {
                Ok(self.inner.state.borrow().lighting.clone())
            }
            fn write_rgb(&self, _: &[u8]) -> Result<()> {
                bail!("RGB must not be written")
            }
            fn write_effect(&self, bytes: &[u8]) -> Result<()> {
                self.sent.borrow_mut().push(bytes.to_vec());
                let mut observed = bytes.to_vec();
                observed[1..4].fill(255);
                observed[8] = 1;
                observed[4] = 0;
                observed[14..16].fill(0);
                self.inner.state.borrow_mut().lighting = observed;
                ensure!(!self.fail_ack.replace(false), "Lost effect ACK");
                Ok(())
            }
        }
        let inner = fixture();
        {
            let mut state = inner.state.borrow_mut();
            state.lighting[1..4].fill(255);
            state.lighting[8] = 1;
        }
        let before = capture_lighting(&inner).unwrap();
        let device = Native {
            inner,
            sent: RefCell::new(Vec::new()),
            fail_ack: Cell::new(false),
        };
        let mut red = before.state.lighting.clone();
        red[0] = 1;
        red[1..4].copy_from_slice(&[255, 0, 0]);
        red[8] = 0;
        let receipt = commit_effect(&device, &before, &red, &temp_path()).unwrap();
        assert!(!receipt.rgb_verified);
        assert!(!receipt.effect_color_verified);
        assert_eq!(receipt.state.lighting, device.effect().unwrap());
        assert_eq!(receipt.requested_effect, Some(red.clone()));
        assert_eq!(receipt.state.lighting[4], 0);
        let current = LightingBackup {
            state: receipt.state,
            ..before.clone()
        };
        // White is equal to GET, but must still be sent to replace visible red.
        commit_effect(&device, &current, &current.state.lighting, &temp_path()).unwrap();
        assert_eq!(device.sent.borrow().len(), 2);
        device.fail_ack.set(true);
        let error =
            commit_effect(&device, &current, &before.state.lighting, &temp_path()).unwrap_err();
        assert!(error.to_string().contains("восстановлен и проверен"));
        assert_eq!(device.sent.borrow().len(), 4);
        assert!(protocol::lighting_equivalent(
            &device.effect().unwrap(),
            &current.state.lighting
        ));
        let mut wrong = current.state.lighting.clone();
        wrong[9] += 1;
        assert!(!protocol::native_effect_readback_matches(
            &wrong,
            &current.state.lighting
        ));
        let mut green = red.clone();
        green[1..4].copy_from_slice(&[0, 255, 0]);
        let receipt =
            commit_effect_with_previous(&device, &current, &green, &temp_path(), Some(&red))
                .unwrap();
        let saved: EffectBackup = crate::storage::read_json(&receipt.backup_path, 16384).unwrap();
        assert_eq!(saved.lighting, red);
        assert_eq!(saved.color_source, "last_acknowledged_studio_command");
        device.fail_ack.set(true);
        let mut blue = green.clone();
        blue[1..4].copy_from_slice(&[0, 0, 255]);
        assert!(
            commit_effect_with_previous(&device, &current, &blue, &temp_path(), Some(&green))
                .is_err()
        );
        assert_eq!(device.sent.borrow().last(), Some(&green));
        let count = device.sent.borrow().len();
        let mut stale = green.clone();
        stale[0] = 15;
        assert!(
            commit_effect_with_previous(&device, &current, &blue, &temp_path(), Some(&stale))
                .is_err()
        );
        assert_eq!(device.sent.borrow().len(), count);
        wrong = current.state.lighting.clone();
        wrong[1] = 254;
        assert!(!protocol::native_effect_readback_matches(
            &wrong,
            &current.state.lighting
        ));
    }
    #[test]
    fn failed_rgb_restore_still_attempts_effect_restore_and_reports_failure() {
        struct Failing {
            inner: Mock,
            rgb: Cell<u8>,
            effect: Cell<u8>,
        }
        impl LightingIo for Failing {
            fn info(&self) -> Result<KeyboardInfo> {
                self.inner.info()
            }
            fn state(&self) -> Result<LightingState> {
                self.inner.state()
            }
            fn write_rgb(&self, data: &[u8]) -> Result<()> {
                self.rgb.set(self.rgb.get() + 1);
                ensure!(self.rgb.get() != 2, "Failed RGB restore");
                self.inner.write_rgb(data)
            }
            fn write_effect(&self, data: &[u8]) -> Result<()> {
                self.effect.set(self.effect.get() + 1);
                self.inner.write_effect(data)?;
                Ok(())
            }
        }
        let device = Failing {
            inner: fixture(),
            rgb: Cell::new(0),
            effect: Cell::new(0),
        };
        device.inner.fail_once.set(true);
        let before = capture_lighting(&device).unwrap();
        let target = frame_target(&before.state, &[(34, [1, 2, 3])]).unwrap();
        let error = commit_lighting(&device, &before, &target, &temp_path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Восстановление не подтверждено"));
        assert_eq!(device.effect.get(), 2);
        assert!(protocol::lighting_equivalent(
            &device.state().unwrap().lighting,
            &before.state.lighting
        ));
    }
}
