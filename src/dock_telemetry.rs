//! AJSC v1 CPU/RAM screen extension. Stock firmware is detected before any SET.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub clock: bool,
    pub percentages: bool,
    pub bars: bool,
    pub memory_size: bool,
    pub show_title: bool,
    pub border: bool,
    pub clock_scale: u8,
    pub clock_position: [u8; 2],
    pub cpu_position: [u8; 2],
    pub ram_position: [u8; 2],
    pub bar_width: u8,
    pub bar_height: u8,
    pub label_scale: u8,
    pub brightness: u8,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub accent: [u8; 3],
    pub title_position: [u8; 2],
    pub title: String,
    pub style: u8,
    pub focus_minutes: u8,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            clock: true,
            percentages: true,
            bars: true,
            memory_size: true,
            show_title: true,
            border: false,
            clock_scale: 1,
            clock_position: [50, 112],
            cpu_position: [6, 32],
            ram_position: [6, 67],
            bar_width: 116,
            bar_height: 7,
            label_scale: 1,
            brightness: 40,
            foreground: [237, 244, 248],
            background: [13, 20, 31],
            accent: [48, 220, 190],
            title_position: [6, 8],
            title: "PC / LIVE".into(),
            style: 0,
            focus_minutes: 25,
        }
    }
}
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = (crc << 1) ^ if crc & 0x8000 != 0 { 0x1021 } else { 0 };
        }
    }
    crc
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.style <= 3 && (1..=180).contains(&self.focus_minutes),
            "Стиль: 0–3, таймер: 1–180 минут"
        );
        ensure!(
            (1..=3).contains(&self.clock_scale) && (1..=2).contains(&self.label_scale),
            "Масштаб текста вне диапазона"
        );
        ensure!(
            (16..=124).contains(&self.bar_width) && (2..=12).contains(&self.bar_height),
            "Размер полос вне диапазона"
        );
        ensure!((1..=100).contains(&self.brightness), "Яркость: 1–100%");
        ensure!(
            self.clock_position
                .iter()
                .chain(&self.cpu_position)
                .chain(&self.ram_position)
                .chain(&self.title_position)
                .all(|&n| n < 128),
            "Координаты экрана: 0–127"
        );
        ensure!(
            self.title.len() <= 20 && self.title.bytes().all(|c| (32..=126).contains(&c)),
            "Заголовок: до 20 латинских букв, цифр и знаков ASCII"
        );
        Ok(())
    }
    pub fn encode(&self) -> Result<[u8; 48]> {
        self.validate()?;
        let flags = [
            self.enabled,
            self.clock,
            self.percentages,
            self.bars,
            self.memory_size,
            self.show_title,
            self.border,
        ]
        .iter()
        .enumerate()
        .fold(0u8, |v, (i, &b)| v | (u8::from(b) << i));
        let mut c = [0; 48];
        c[..12].copy_from_slice(&[
            flags,
            self.clock_scale,
            self.clock_position[0],
            self.clock_position[1],
            self.cpu_position[0],
            self.cpu_position[1],
            self.ram_position[0],
            self.ram_position[1],
            self.bar_width,
            self.bar_height,
            self.label_scale,
            self.brightness,
        ]);
        c[12..15].copy_from_slice(&self.foreground);
        c[15..18].copy_from_slice(&self.background);
        c[18..21].copy_from_slice(&self.accent);
        c[21..23].copy_from_slice(&self.title_position);
        c[23] = self.title.len() as u8;
        c[24..24 + self.title.len()].copy_from_slice(self.title.as_bytes());
        c[44] = self.style;
        c[45] = if self.focus_minutes == 25 {
            0
        } else {
            self.focus_minutes
        };
        let crc = crc16(&c[..46]);
        c[46..].copy_from_slice(&crc.to_le_bytes());
        Ok(c)
    }
    pub fn decode(c: &[u8]) -> Result<Self> {
        ensure!(
            c.len() == 48 && crc16(&c[..46]) == u16::from_le_bytes([c[46], c[47]]),
            "Ошибка CRC оформления экрана"
        );
        ensure!(
            c[0] < 128 && c[23] <= 20 && c[44] <= 3 && c[45] <= 180,
            "Неизвестный формат оформления"
        );
        ensure!(
            c[24 + usize::from(c[23])..44].iter().all(|&n| n == 0),
            "Некорректный хвост заголовка"
        );
        let flag = |bit: u32| c[0] & (1u8 << bit) != 0;
        let value = Self {
            enabled: flag(0),
            clock: flag(1),
            percentages: flag(2),
            bars: flag(3),
            memory_size: flag(4),
            show_title: flag(5),
            border: flag(6),
            clock_scale: c[1],
            clock_position: [c[2], c[3]],
            cpu_position: [c[4], c[5]],
            ram_position: [c[6], c[7]],
            bar_width: c[8],
            bar_height: c[9],
            label_scale: c[10],
            brightness: c[11],
            foreground: c[12..15].try_into()?,
            background: c[15..18].try_into()?,
            accent: c[18..21].try_into()?,
            title_position: [c[21], c[22]],
            title: std::str::from_utf8(&c[24..24 + usize::from(c[23])])?.into(),
            style: c[44],
            focus_minutes: if c[45] == 0 { 25 } else { c[45] },
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sample {
    pub cpu_permille: u16,
    pub ram_permille: u16,
    pub used_mib: u32,
    pub total_mib: u32,
    pub sequence: u16,
    #[serde(default)]
    pub graphics: crate::graphics::Metrics,
}
impl Sample {
    pub fn encode_extended(&self) -> Result<[u8; 48]> {
        let mut p = [0; 48];
        p[..32].copy_from_slice(&self.encode_graphics()?);
        let t = &self.graphics.temperatures;
        ensure!(t.valid(), "Некорректная температура");
        p[32] = 1;
        let values = [t.cpu, t.gpu, t.storage, t.board];
        for (i, v) in values.iter().enumerate() {
            if let Some(v) = v {
                p[33] |= 1 << i;
                p[34 + i * 2..36 + i * 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        if let Some(v) = self.graphics.power_deciwatts {
            p[33] |= 16;
            p[42..44].copy_from_slice(&v.to_le_bytes());
        }
        let crc = crc16(&p[..46]);
        p[46..].copy_from_slice(&crc.to_le_bytes());
        Ok(p)
    }
    pub fn encode_graphics(&self) -> Result<[u8; 32]> {
        let mut p = [0; 32];
        p[..16].copy_from_slice(&self.encode()?);
        let g = &self.graphics;
        ensure!(
            g.gpu_permille.is_none_or(|v| v <= 1000) && g.fps_tenths.is_none_or(|v| v > 0),
            "Некорректные GPU/FPS"
        );
        ensure!(
            match (g.vram_used_mib, g.vram_total_mib) {
                (None, None) => true,
                (Some(a), Some(b)) => b > 0 && b <= 1_048_576 && a <= b,
                _ => false,
            },
            "Некорректная VRAM"
        );
        p[16] = 1;
        p[17] = u8::from(g.gpu_permille.is_some())
            | u8::from(g.vram_total_mib.is_some()) << 1
            | u8::from(g.fps_tenths.is_some()) << 2;
        p[18..20].copy_from_slice(&g.gpu_permille.unwrap_or(0).to_le_bytes());
        p[20..24].copy_from_slice(&g.vram_used_mib.unwrap_or(0).to_le_bytes());
        p[24..28].copy_from_slice(&g.vram_total_mib.unwrap_or(0).to_le_bytes());
        p[28..30].copy_from_slice(&g.fps_tenths.unwrap_or(0).to_le_bytes());
        let crc = crc16(&p[..30]);
        p[30..].copy_from_slice(&crc.to_le_bytes());
        Ok(p)
    }
    pub fn encode(&self) -> Result<[u8; 16]> {
        ensure!(
            self.cpu_permille <= 1000
                && self.ram_permille <= 1000
                && self.total_mib > 0
                && self.total_mib <= 8_388_608
                && self.used_mib <= self.total_mib,
            "Некорректные показатели компьютера"
        );
        let mut p = [0; 16];
        p[..2].copy_from_slice(&self.cpu_permille.to_le_bytes());
        p[2..4].copy_from_slice(&self.ram_permille.to_le_bytes());
        p[4..8].copy_from_slice(&self.used_mib.to_le_bytes());
        p[8..12].copy_from_slice(&self.total_mib.to_le_bytes());
        p[12..14].copy_from_slice(&self.sequence.to_le_bytes());
        let crc = crc16(&p[..14]);
        p[14..].copy_from_slice(&crc.to_le_bytes());
        Ok(p)
    }
}
pub fn packet(action: u8, payload: &[u8]) -> Result<[u8; 65]> {
    ensure!(
        matches!(
            (action, payload.len()),
            (0, 0)
                | (1 | 3, 48)
                | (2, 16)
                | (14, 32)
                | (15, 48)
                | (4 | 9 | 10 | 12 | 13, 0)
                | (5 | 6, 48)
                | (7 | 8 | 11, 1)
        ),
        "Неизвестная экранная команда"
    );
    let mut r = [0; 65];
    r[1] = 0x2c;
    r[8] = 0xd3;
    r[9..13].copy_from_slice(b"AJSC");
    r[13] = 1;
    r[14] = action;
    r[15] = payload.len() as u8;
    r[17..17 + payload.len()].copy_from_slice(payload);
    Ok(r)
}
#[derive(Clone, Debug, Serialize)]
pub struct Reply {
    pub graphics_supported: bool,
    pub design_version: u8,
    pub graphics: crate::graphics::Metrics,
    pub capabilities: u8,
    pub config: Config,
    pub cpu_permille: u16,
    pub ram_permille: u16,
    pub sequence: u16,
    pub has_sample: bool,
    pub status: u8,
}
impl Reply {
    fn read_graphics_capability(&mut self, b: &[u8]) -> bool {
        if b.len() != 65
            || b[..7] != [0, 0x2c, b'A', b'J', b'G', b'M', 1]
            || b[1..9].iter().fold(0u8, |a, v| a.wrapping_add(*v)) != 255
            || b[7] != 0
            || b[9] != 1
        {
            return false;
        }
        self.graphics_supported = true;
        self.graphics = Default::default();
        self.design_version = 0;
        if b[10..12] == *b"UI" && b[10..14].iter().fold(0u8, |a, v| a.wrapping_add(*v)) == 255 {
            self.design_version = b[12];
        }
        let t = &b[17..65];
        if self.has_sample && t[16] == 1 && t[17] <= 7 {
            let u16_at = |i| u16::from_le_bytes([t[i], t[i + 1]]);
            let u32_at = |i| u32::from_le_bytes(t[i..i + 4].try_into().unwrap());
            let mut metrics = crate::graphics::Metrics {
                gpu_permille: (t[17] & 1 != 0).then(|| u16_at(18)),
                vram_used_mib: (t[17] & 2 != 0).then(|| u32_at(20)),
                vram_total_mib: (t[17] & 2 != 0).then(|| u32_at(24)),
                fps_tenths: (t[17] & 4 != 0).then(|| u16_at(28)),
                ..Default::default()
            };
            if self.design_version >= 2
                && t[32] == 1
                && t[33] <= 31
                && t[44..46] == [0, 0]
                && crc16(&t[..46]) == u16_at(46)
            {
                let temp = |i: usize| (t[33] & (1u8 << i) != 0).then(|| u16_at(34 + i * 2) as i16);
                let temperatures = crate::temperatures::Readings {
                    cpu: temp(0),
                    gpu: temp(1),
                    storage: temp(2),
                    board: temp(3),
                };
                if temperatures.valid() {
                    metrics.temperatures = temperatures;
                    metrics.power_deciwatts = (t[33] & 16 != 0).then(|| u16_at(42));
                }
            }
            let s = Sample {
                cpu_permille: u16_at(0),
                ram_permille: u16_at(2),
                used_mib: u32_at(4),
                total_mib: u32_at(8),
                sequence: u16_at(12),
                graphics: metrics.clone(),
            };
            if s.encode_graphics().is_ok_and(|encoded| encoded == t[..32]) {
                self.graphics = metrics;
            }
        }
        if b[10..12] == *b"UI" && b[10..14].iter().fold(0u8, |a, v| a.wrapping_add(*v)) == 255 {
            self.design_version = b[12];
        }
        true
    }
    pub fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(
            raw.len() == 65 && raw[..7] == [0, 0x2c, b'A', b'J', b'S', b'C', 1],
            "Экран не подтвердил AJSC v1"
        );
        let b = &raw[1..];
        ensure!(
            b[..8].iter().fold(0u8, |v, &n| v.wrapping_add(n)) == 255
                && b[63] & 15 == 15
                && b[62] <= 1
                && b[6] <= 3,
            "Повреждён ответ экранной прошивки"
        );
        let read = |at| u16::from_le_bytes([b[at], b[at + 1]]);
        ensure!(
            read(56) <= 1000 && read(58) <= 1000,
            "Некорректный ответ телеметрии"
        );
        Ok(Self {
            graphics_supported: false,
            design_version: 0,
            graphics: Default::default(),
            capabilities: b[63],
            config: Config::decode(&b[8..56])?,
            cpu_permille: read(56),
            ram_permille: read(58),
            sequence: read(60),
            has_sample: b[62] == 1,
            status: b[6],
        })
    }
}

#[cfg(windows)]
struct Control(crate::dock::DockSession);
#[cfg(windows)]
impl Control {
    fn open() -> Result<Self> {
        let session = crate::dock::DockSession::open()?;
        Self::checked(session)
    }
    fn checked(session: crate::dock::DockSession) -> Result<Self> {
        ensure!(
            session.info()?.oled_version == 0x104,
            "Требуется экран OLEDV104"
        );
        let result = Self(session);
        let mut query = [0; 65];
        query[1] = 0xf1;
        query[8] = 0x0e;
        result.0.device.send_feature_report(&query)?;
        std::thread::sleep(Duration::from_millis(30));
        let mut reply = [0; 65];
        let count = result.0.device.get_feature_report(&mut reply)?;
        ensure!(
            count == 65 && reply[..4] == [0, 0xf1, 0xdf, 6],
            "Требуется док ID1759"
        );
        Ok(result)
    }
    fn exchange(&self, r: &[u8; 65], accept: impl Fn(&Reply) -> bool) -> Result<Reply> {
        // The dock bridges one HID mailbox to UART. A command or its reply can
        // be missed while another native operation completes. GET, RAM config
        // and a sequenced sample are idempotent; a flash-save request is not
        // automatically repeated because an absent reply does not undo a save.
        let attempts = if matches!(r[14], 3 | 6 | 8) { 1 } else { 3 };
        for _ in 0..attempts {
            self.0.device.send_feature_report(r)?;
            for _ in 0..15 {
                std::thread::sleep(Duration::from_millis(20));
                let mut b = [0; 65];
                let n = self.0.device.get_feature_report(&mut b)?;
                if let Ok(reply) = Reply::parse(&b[..n]) {
                    ensure!(
                        reply.status == 0,
                        "Экран отклонил команду, статус {}",
                        reply.status
                    );
                    if accept(&reply) {
                        return Ok(reply);
                    }
                }
            }
        }
        if r[14] == 0 {
            anyhow::bail!(
                "Экран не подтвердил AJSC v1. Для этой функции требуется совместимая экранная прошивка."
            )
        }
        anyhow::bail!(
            "Нет подтверждения приёма команды экраном. Изменения могли быть применены; повторно прочитайте оформление."
        )
    }
    fn probe(&self) -> Result<Reply> {
        let mut reply = self.exchange(&packet(0, &[])?, |_| true)?;
        self.0.device.send_feature_report(&packet(13, &[])?)?;
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            let mut b = [0; 65];
            let n = self.0.device.get_feature_report(&mut b)?;
            if reply.read_graphics_capability(&b[..n]) {
                break;
            }
            if Reply::parse(&b[..n]).is_ok_and(|r| r.status != 0) {
                break;
            }
        }
        Ok(reply)
    }
    fn apply(&self, config: &Config, save: bool) -> Result<Reply> {
        if config.style == 3 {
            ensure!(
                self.probe()?.graphics_supported,
                "Игровая сводка требует обновления OLED"
            );
        }
        if config.style != 0 || config.focus_minutes != 25 {
            ensure!(
                self.probe()?.capabilities & 128 != 0,
                "Оформление требует новой экранной прошивки"
            );
        }
        let bytes = config.encode()?;
        self.exchange(&packet(if save { 3 } else { 1 }, &bytes)?, |r| {
            r.config == *config
        })
    }
    fn pages_exchange(
        &self,
        r: &[u8; 65],
        accept: impl Fn(&crate::dock_pages::Status) -> bool,
    ) -> Result<crate::dock_pages::Status> {
        let attempts = if matches!(r[14], 6 | 8) { 1 } else { 3 };
        for _ in 0..attempts {
            self.0.device.send_feature_report(r)?;
            for _ in 0..15 {
                std::thread::sleep(Duration::from_millis(20));
                let mut b = [0; 65];
                let n = self.0.device.get_feature_report(&mut b)?;
                if let Ok(reply) = crate::dock_pages::Status::parse(&b[..n]) {
                    ensure!(
                        reply.status == 0,
                        "Диспетчер отклонил команду: {}",
                        reply.status
                    );
                    if accept(&reply) {
                        return Ok(reply);
                    }
                }
            }
        }
        anyhow::bail!("Нет подтверждения диспетчера страниц; перечитайте состояние дока")
    }
    fn pages_info(&self) -> Result<crate::dock_pages::Status> {
        ensure!(
            self.probe()?.capabilities & 32 != 0,
            "Нужна новая экранная прошивка с диспетчером страниц"
        );
        self.pages_status()
    }
    // Only the running session uses this after confirming capabilities. Every
    // acquired HID session still verifies the physical dock's identity.
    fn pages_status(&self) -> Result<crate::dock_pages::Status> {
        self.pages_exchange(&packet(4, &[])?, |_| true)
    }
    fn pages_apply(
        &self,
        p: &crate::dock_pages::Playlist,
        save: bool,
    ) -> Result<crate::dock_pages::Status> {
        let before = self.pages_info()?;
        ensure!(
            p.pages
                .iter()
                .all(|e| e.kind != crate::dock_pages::Kind::Custom)
                || self.probe()?.design_version >= 3,
            "Свой экран требует обновления OLED до UI 3"
        );
        ensure!(
            p.pages.iter().all(|e| e.kind as u8 <= 8) || self.probe()?.design_version >= 2,
            "Эти экраны требуют обновления OLED до UI 2"
        );
        ensure!(
            before.capabilities & 128 != 0 || p.pages.iter().all(|e| (e.kind as u8) <= 4),
            "Новые экраны требуют обновления OLED"
        );
        ensure!(
            before.erasing.is_none(),
            "Дождитесь окончания удаления ресурса"
        );
        backup_pages(&before)?;
        self.pages_exchange(&packet(if save { 6 } else { 5 }, &p.encode()?)?, |s| {
            s.playlist == *p
        })
    }
    fn editor(
        &self,
        config: &Config,
        pages: &crate::dock_pages::Playlist,
        save: bool,
    ) -> Result<crate::dock_pages::Status> {
        config.validate()?;
        pages.validate()?;
        let before = self.pages_info()?;
        ensure!(
            pages
                .pages
                .iter()
                .all(|e| e.kind != crate::dock_pages::Kind::Custom)
                || self.probe()?.design_version >= 3,
            "Свой экран требует обновления OLED до UI 3"
        );
        ensure!(
            pages.pages.iter().all(|e| e.kind as u8 <= 8) || self.probe()?.design_version >= 2,
            "Эти экраны требуют обновления OLED до UI 2"
        );
        ensure!(before.erasing.is_none(), "Дождитесь удаления ресурса");
        ensure!(
            before.capabilities & 128 != 0
                || (config.style == 0
                    && config.focus_minutes == 25
                    && pages.pages.iter().all(|p| p.kind as u8 <= 4)),
            "Новые экраны и оформление требуют обновления OLED"
        );
        self.apply(config, save)?;
        self.pages_apply(pages, save)
    }
    fn select_page(&self, index: u8) -> Result<crate::dock_pages::Status> {
        let before = self.pages_info()?;
        ensure!(
            usize::from(index) < before.playlist.pages.len()
                && before.playlist.pages[usize::from(index)].enabled,
            "Страница недоступна"
        );
        self.pages_exchange(&packet(7, &[index])?, |s| s.active == index)
    }
    fn delete_asset(&self, index: u8) -> Result<crate::dock_pages::Status> {
        let before = self.pages_info()?;
        ensure!(before.has_asset(index), "В этом слоте нет ресурса");
        ensure!(before.erasing.is_none(), "Дождитесь окончания удаления");
        backup_pages(&before)?;
        self.pages_exchange(&packet(8, &[index])?, |s| {
            s.erasing == Some(index) || !s.has_asset(index)
        })
    }
    fn finish_stream(&self, config: &Config) -> Result<()> {
        if self.probe()?.capabilities & 32 != 0 {
            self.pages_exchange(&packet(9, &[])?, |_| true)?;
        } else {
            let mut c = config.clone();
            c.enabled = false;
            self.apply(&c, false)?;
        }
        Ok(())
    }
    fn native(&self) -> Result<crate::dock_pages::Status> {
        self.pages_info()?;
        self.pages_exchange(&packet(10, &[])?, |s| !s.playlist.enabled)
    }
    fn sample(&self, s: &Sample, graphics: bool, design: u8) -> Result<()> {
        let request = if design >= 2 {
            packet(15, &s.encode_extended()?)?
        } else if graphics {
            packet(14, &s.encode_graphics()?)?
        } else {
            packet(2, &s.encode()?)?
        };
        self.exchange(&request, |r| {
            r.has_sample
                && r.sequence == s.sequence
                && r.cpu_permille == s.cpu_permille
                && r.ram_permille == s.ram_permille
        })?;
        Ok(())
    }
    fn timer(&self, command: Option<u8>) -> Result<crate::dock_timer::Status> {
        ensure!(
            self.probe()?.capabilities & 128 != 0,
            "Таймеры требуют новой экранной прошивки"
        );
        self.timer_exchange(command)
    }
    fn timer_exchange(&self, command: Option<u8>) -> Result<crate::dock_timer::Status> {
        let packet = if let Some(cmd) = command {
            ensure!(cmd <= 5, "Неизвестная команда таймера");
            packet(11, &[cmd])?
        } else {
            packet(12, &[])?
        };
        for _ in 0..if command.is_some() { 1 } else { 3 } {
            self.0.device.send_feature_report(&packet)?;
            for _ in 0..15 {
                std::thread::sleep(Duration::from_millis(20));
                let mut r = [0; 65];
                let n = self.0.device.get_feature_report(&mut r)?;
                if let Ok(status) = crate::dock_timer::Status::parse(&r[..n]) {
                    let accepted = match command {
                        Some(0) => status.mode == 1 && status.running && status.elapsed <= 1,
                        Some(1) => status.mode == 2 && status.running && status.elapsed <= 1,
                        Some(2) => !status.running,
                        Some(3) => status.running,
                        Some(4) => !status.running && status.elapsed == 0,
                        Some(5) => status.mode == 0,
                        _ => true,
                    };
                    if accepted {
                        return Ok(status);
                    }
                }
            }
        }
        anyhow::bail!("Нет подтверждения таймера; повторите чтение состояния")
    }
}
pub fn probe() -> Result<Reply> {
    #[cfg(windows)]
    {
        Control::open()?.probe()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
pub fn apply(config: &Config, save: bool) -> Result<Reply> {
    config.validate()?;
    #[cfg(windows)]
    {
        let c = Control::open()?;
        c.probe()?;
        c.apply(config, save)
    }
    #[cfg(not(windows))]
    {
        let _ = save;
        anyhow::bail!("Windows only")
    }
}
fn backup_pages(before: &crate::dock_pages::Status) -> Result<()> {
    let directory = crate::storage::backup_directory()?;
    let name = format!(
        "dock-pages-before-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%f")
    );
    crate::storage::write_json_atomic(&directory.join(name), before)
}
pub fn stop_stream() -> Result<Reply> {
    #[cfg(windows)]
    {
        let c = Control::open()?;
        let info = c.probe()?;
        c.finish_stream(&info.config)?;
        c.probe()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
pub fn apply_with_pages(
    config: &Config,
    pages: &crate::dock_pages::Playlist,
    save: bool,
) -> Result<Reply> {
    config.validate()?;
    pages.validate()?;
    #[cfg(windows)]
    {
        let c = Control::open()?;
        if c.probe()?.capabilities & 32 != 0 {
            let mut p = pages.clone();
            p.enabled = true;
            c.pages_apply(&p, false)?;
        }
        c.apply(config, save)
    }
    #[cfg(not(windows))]
    {
        let _ = save;
        anyhow::bail!("Windows only")
    }
}
pub fn pages_info() -> Result<crate::dock_pages::Status> {
    #[cfg(windows)]
    {
        Control::open()?.pages_info()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
pub type BackgroundStatus = (crate::dock_pages::Status, Option<crate::dock_timer::Status>);
/// One optional mailbox observation. This never applies configuration, occupies
/// UI state or waits for a foreground operation to release the dock.
pub fn background_status(timer: bool) -> Result<Option<BackgroundStatus>> {
    #[cfg(windows)]
    {
        let Some(session) = crate::dock::DockSession::open_background()? else {
            return Ok(None);
        };
        let control = Control(session);
        let pages = control.pages_exchange(&packet(4, &[])?, |_| true)?;
        let timer = if timer {
            Some(control.timer(None)?)
        } else {
            None
        };
        Ok(Some((pages, timer)))
    }
    #[cfg(not(windows))]
    {
        let _ = timer;
        Ok(None)
    }
}
pub fn read_editor() -> Result<(Config, crate::dock_pages::Status)> {
    #[cfg(windows)]
    {
        let c = Control::open()?;
        Ok((c.probe()?.config, c.pages_info()?))
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
/// Apply the validated editor draft while holding a single dock mailbox lease.
pub fn apply_editor(
    config: &Config,
    pages: &crate::dock_pages::Playlist,
    save: bool,
) -> Result<crate::dock_pages::Status> {
    config.validate()?;
    pages.validate()?;
    #[cfg(windows)]
    {
        let c = Control::open()?;
        c.editor(config, pages, save)
    }
    #[cfg(not(windows))]
    {
        let _ = save;
        anyhow::bail!("Windows only")
    }
}
/// Show the complete editor draft, including its palette and layout.
pub fn show_editor(
    config: &Config,
    pages: &crate::dock_pages::Playlist,
    index: u8,
) -> Result<crate::dock_pages::Status> {
    config.validate()?;
    pages.validate()?;
    ensure!(
        pages
            .pages
            .get(usize::from(index))
            .is_some_and(|p| p.enabled),
        "Страница недоступна"
    );
    #[cfg(windows)]
    {
        let c = Control::open()?;
        c.editor(config, pages, false)?;
        c.select_page(index)
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
pub fn pages_apply(
    p: &crate::dock_pages::Playlist,
    save: bool,
) -> Result<crate::dock_pages::Status> {
    p.validate()?;
    #[cfg(windows)]
    {
        Control::open()?.pages_apply(p, save)
    }
    #[cfg(not(windows))]
    {
        let _ = save;
        anyhow::bail!("Windows only")
    }
}
pub fn select_page(index: u8) -> Result<crate::dock_pages::Status> {
    #[cfg(windows)]
    {
        Control::open()?.select_page(index)
    }
    #[cfg(not(windows))]
    {
        let _ = index;
        anyhow::bail!("Windows only")
    }
}
pub fn delete_asset(index: u8) -> Result<crate::dock_pages::Status> {
    #[cfg(windows)]
    {
        Control::open()?.delete_asset(index)
    }
    #[cfg(not(windows))]
    {
        let _ = index;
        anyhow::bail!("Windows only")
    }
}
pub fn native() -> Result<crate::dock_pages::Status> {
    #[cfg(windows)]
    {
        Control::open()?.native()
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("Windows only")
    }
}
pub enum Command {
    Graphics(crate::graphics::Options),
    Editor(Config, crate::dock_pages::Playlist, bool, Option<u8>),
    Timer(Option<u8>),
    Config(Config, bool),
    Pages(crate::dock_pages::Playlist, bool),
    Select(u8),
    Delete(u8),
    ReadPages(bool),
    Native,
    Stop,
}
pub fn timer(
    command: Option<u8>,
    editor: Option<(&Config, &crate::dock_pages::Playlist)>,
) -> Result<crate::dock_timer::Status> {
    if let Some((c, p)) = editor {
        c.validate()?;
        p.validate()?;
    }
    #[cfg(windows)]
    {
        let control = Control::open()?;
        ensure!(
            control.probe()?.capabilities & 128 != 0,
            "Таймеры требуют обновления OLED"
        );
        if let Some((c, p)) = editor {
            control.apply(c, false)?;
            control.pages_apply(p, false)?;
        }
        control.timer(command)
    }
    #[cfg(not(windows))]
    {
        let _ = command;
        anyhow::bail!("Windows only")
    }
}
pub enum Update {
    Design(u8),
    Connection(bool),
    Editor(Config, crate::dock_pages::Status),
    Timer(crate::dock_timer::Status),
    Sample(Sample),
    Settings(Config, bool),
    Pages(crate::dock_pages::Status, bool),
    Notice(String),
}
#[cfg(any(windows, test))]
fn sample_interval(
    config: &Config,
    pages: &crate::dock_pages::Playlist,
    active: u8,
    fps: bool,
) -> Duration {
    use crate::dock_pages::Kind;
    let fast = fps
        && if pages.enabled {
            pages.pages.get(usize::from(active)).is_some_and(|p| {
                p.enabled
                    && (p.kind == Kind::Gaming
                        || (p.kind == Kind::Custom
                            && p.composition
                                .metrics
                                .contains(&crate::dock_composition::Metric::Fps))
                        || (p.kind == Kind::Summary && config.style == 3))
            })
        } else {
            config.enabled && config.style == 3
        };
    if fast {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(1)
    }
}
pub fn run(
    mut config: Config,
    mut playlist: crate::dock_pages::Playlist,
    graphics_options: &mut crate::graphics::Options,
    commands: &Receiver<Command>,
    automatic: bool,
    mut update: impl FnMut(Update),
) -> Result<()> {
    #[cfg(windows)]
    {
        let mut counters = windows::Counters::new()?;
        let control = Control::open()?;
        let initial = control.probe()?;
        let supports_pages = initial.capabilities & 32 != 0;
        let supports_graphics = initial.graphics_supported;
        update(Update::Design(initial.design_version));
        let mut graphics = None;
        let mut notices = std::collections::BTreeSet::new();
        let mut active_page = playlist.selected;
        if automatic {
            config = initial.config;
            let status = control.pages_info()?;
            active_page = status.active;
            playlist = status.playlist.clone();
            update(Update::Editor(config.clone(), status));
        } else {
            config.enabled = true;
        }
        control
            .0
            .device
            .send_feature_report(&crate::dock::clock_report(chrono::Local::now())?)?;
        // Native SET_TIME has no reply. Give its UART transfer time to finish
        // before submitting another report through the same mailbox, as the
        // standalone clock operation does. Then confirm AJSC is responsive.
        std::thread::sleep(Duration::from_millis(50));
        control.probe()?;
        let mut sequence = 0u16;
        let mut history = Vec::new();
        let mut native_selected = false;
        let mut sent_sample = false;
        let result = (|| -> Result<()> {
            if !automatic {
                control.apply(&config, false)?;
            }
            if supports_pages && !automatic {
                let status = control.pages_apply(&playlist, false)?;
                active_page = status.active;
                update(Update::Pages(status, false));
            }
            drop(control);
            update(Update::Connection(true));
            let mut next = Instant::now();
            let mut next_status = Instant::now();
            let mut pc_sample: Option<(Instant, Sample)> = None;
            loop {
                let command = commands.recv_timeout(next.saturating_duration_since(Instant::now()));
                if matches!(
                    command,
                    Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected)
                ) {
                    break;
                }
                let cycle_started = Instant::now();
                // Release the mailbox while waiting, so media uploads and other
                // device operations can run without a manual Stop button.
                let control = if matches!(command, Err(RecvTimeoutError::Timeout)) {
                    match crate::dock::DockSession::open_background()? {
                        Some(s) => Control::checked(s)?,
                        None => {
                            next = Instant::now() + Duration::from_millis(250);
                            continue;
                        }
                    }
                } else {
                    Control::open()?
                };
                match command {
                    Ok(Command::Graphics(options)) => {
                        *graphics_options = options;
                        graphics = None;
                        notices.clear();
                    }
                    Ok(Command::Editor(new, pages, save, selected)) => {
                        let result = control.editor(&new, &pages, save).and_then(|s| {
                            if let Some(index) = selected {
                                control.select_page(index)
                            } else {
                                Ok(s)
                            }
                        });
                        match result {
                            Ok(s) => {
                                config = new;
                                playlist = pages;
                                active_page = s.active;
                                update(Update::Settings(config.clone(), save));
                                update(Update::Pages(s, false));
                            }
                            Err(e) => update(Update::Notice(format!(
                                "Настройки дока: {e:#}. Перечитайте состояние дока."
                            ))),
                        }
                    }
                    Ok(Command::Timer(command)) => match control.timer(command) {
                        Ok(s) => update(Update::Timer(s)),
                        Err(e) => update(Update::Notice(format!("Таймер: {e:#}"))),
                    },
                    Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => unreachable!(),
                    Ok(Command::Pages(p, save)) => match control.pages_apply(&p, save) {
                        Ok(s) => {
                            playlist = p;
                            active_page = s.active;
                            update(Update::Pages(s, false));
                        }
                        Err(e) => update(Update::Notice(format!("Страницы: {e:#}"))),
                    },
                    Ok(Command::Select(index)) => match control.select_page(index) {
                        Ok(s) => {
                            active_page = s.active;
                            next = Instant::now();
                            update(Update::Pages(s, false));
                        }
                        Err(e) => update(Update::Notice(format!("Страницы: {e:#}"))),
                    },
                    Ok(Command::Delete(index)) => match control.delete_asset(index) {
                        Ok(s) => update(Update::Pages(s, false)),
                        Err(e) => update(Update::Notice(format!("Страницы: {e:#}"))),
                    },
                    Ok(Command::ReadPages(replace)) => match control.pages_info() {
                        Ok(s) => {
                            active_page = s.active;
                            if replace {
                                config = control.probe()?.config;
                                playlist = s.playlist.clone();
                                update(Update::Editor(config.clone(), s));
                            } else {
                                update(Update::Pages(s, false));
                            }
                        }
                        Err(e) => update(Update::Notice(format!("Страницы: {e:#}"))),
                    },
                    Ok(Command::Native) => {
                        update(Update::Pages(control.native()?, false));
                        native_selected = true;
                        break;
                    }
                    Ok(Command::Config(mut new, save)) => {
                        new.enabled = true;
                        control.apply(&new, save)?;
                        config = new;
                        update(Update::Settings(config.clone(), save));
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let poll_status = Instant::now() >= next_status;
                        let status = if supports_pages && poll_status {
                            Some(control.pages_status()?)
                        } else {
                            None
                        };
                        if let Some(s) = &status {
                            playlist = s.playlist.clone();
                            active_page = s.active;
                        }
                        if poll_status {
                            next_status = Instant::now() + Duration::from_secs(1);
                        }
                        if !needs_samples(&config, &playlist) {
                            graphics = None;
                            if let Some(s) = status {
                                update(Update::Pages(s, false));
                            }
                            next = Instant::now() + Duration::from_secs(1);
                            continue;
                        }
                        sequence = sequence.wrapping_add(1);
                        // Only FPS needs a short update interval. CPU and memory
                        // retain a stable one-second measurement window.
                        if pc_sample
                            .as_ref()
                            .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(1))
                        {
                            pc_sample = Some((Instant::now(), counters.sample(sequence)?));
                        }
                        let mut sample = pc_sample.as_ref().unwrap().1.clone();
                        sample.sequence = sequence;
                        if supports_graphics {
                            let collector = graphics.get_or_insert_with(|| {
                                crate::graphics::Collector::new(graphics_options.clone())
                            });
                            sample.graphics = collector.sample();
                            for message in [collector.gpu_notice.clone(), collector.fps_notice()]
                                .into_iter()
                                .flatten()
                            {
                                if notices.insert(message.clone()) {
                                    update(Update::Notice(message));
                                }
                            }
                        } else {
                            graphics = None;
                        }
                        let gpu = sample
                            .graphics
                            .gpu_permille
                            .map_or(15, |v| u32::from(v) * 14 / 1000);
                        history.insert(
                            0,
                            ((u32::from(sample.cpu_permille) * 14 / 1000) | (gpu << 4)) as u8,
                        );
                        history.truncate(32);
                        sample.graphics.load_history = history.clone();
                        control.sample(&sample, supports_graphics, initial.design_version)?;
                        sent_sample = true;
                        update(Update::Sample(sample));
                        if let Some(s) = status {
                            update(Update::Pages(s, false));
                        }
                        if poll_status
                            && initial.capabilities & 128 != 0
                            && playlist
                                .pages
                                .get(usize::from(active_page))
                                .is_some_and(|p| {
                                    matches!(
                                        p.kind,
                                        crate::dock_pages::Kind::Focus
                                            | crate::dock_pages::Kind::Stopwatch
                                    )
                                })
                        {
                            update(Update::Timer(control.timer_exchange(None)?));
                        }
                        let interval = sample_interval(
                            &config,
                            &playlist,
                            active_page,
                            supports_graphics && graphics_options.fps_enabled,
                        );
                        // Do not add USB transaction time to every interval, or
                        // issue a burst of catch-up samples after a slow reply.
                        next = (cycle_started + interval)
                            .max(Instant::now() + Duration::from_millis(30));
                    }
                }
            }
            Ok(())
        })();
        let stopped = if native_selected || !sent_sample {
            Ok(())
        } else {
            Control::open().and_then(|c| c.finish_stream(&config))
        };
        match (result, stopped) {
            (Ok(()), Ok(_)) => Ok(()),
            (Err(error), Ok(_)) => Err(error),
            (Ok(()), Err(error)) => {
                Err(error.context("Не подтверждена остановка потока показателей"))
            }
            (Err(error), Err(stop)) => {
                anyhow::bail!("{error:#}. Остановка потока также не подтверждена: {stop:#}")
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            &mut config,
            playlist,
            graphics_options,
            commands,
            automatic,
            &mut update,
        );
        anyhow::bail!("Windows only")
    }
}

/// Keep fresh samples ready for rear-button navigation, even while an
/// autonomous page (e.g. calendar) is currently visible.
pub fn needs_samples(config: &Config, pages: &crate::dock_pages::Playlist) -> bool {
    if !pages.enabled {
        return config.enabled;
    }
    pages.pages.iter().any(|p| {
        p.enabled
            && matches!(
                p.kind,
                crate::dock_pages::Kind::Summary
                    | crate::dock_pages::Kind::Cpu
                    | crate::dock_pages::Kind::Memory
                    | crate::dock_pages::Kind::Gaming
                    | crate::dock_pages::Kind::Thermals
                    | crate::dock_pages::Kind::Gpu
                    | crate::dock_pages::Kind::Pulse
                    | crate::dock_pages::Kind::Custom
            )
            && (p.kind != crate::dock_pages::Kind::Custom
                || p.composition.metrics.iter().any(|m| {
                    !matches!(
                        m,
                        crate::dock_composition::Metric::Clock
                            | crate::dock_composition::Metric::None
                    )
                }))
    })
}

fn cpu_load(previous: [u64; 3], now: [u64; 3]) -> Result<u16> {
    let idle = now[0].checked_sub(previous[0]);
    let kernel = now[1].checked_sub(previous[1]);
    let user = now[2].checked_sub(previous[2]);
    let (Some(idle), Some(kernel), Some(user)) = (idle, kernel, user) else {
        anyhow::bail!("Счётчики CPU сброшены")
    };
    let total = u128::from(kernel) + u128::from(user);
    ensure!(
        total > 0 && u128::from(idle) <= total,
        "Нет нового замера CPU"
    );
    Ok((((total - u128::from(idle)) * 1000) / total) as u16)
}
#[cfg(windows)]
pub mod windows {
    use super::*;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn value(&self) -> u64 {
            u64::from(self.low) | (u64::from(self.high) << 32)
        }
    }
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        total_page: u64,
        available_page: u64,
        total_virtual: u64,
        available_virtual: u64,
        extended: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn GlobalMemoryStatusEx(memory: *mut Memory) -> i32;
    }
    fn times() -> Result<[u64; 3]> {
        let (mut idle, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // SAFETY: writable, correctly laid out FILETIME outputs live through the call.
        ensure!(
            unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0,
            "GetSystemTimes: {}",
            std::io::Error::last_os_error()
        );
        Ok([idle.value(), kernel.value(), user.value()])
    }
    pub struct Counters {
        previous: [u64; 3],
    }
    impl Counters {
        pub fn new() -> Result<Self> {
            Ok(Self { previous: times()? })
        }
        pub fn sample(&mut self, sequence: u16) -> Result<Sample> {
            let now = times()?;
            let cpu = cpu_load(self.previous, now)?;
            self.previous = now;
            let mut memory = Memory {
                length: std::mem::size_of::<Memory>() as u32,
                ..Default::default()
            };
            // SAFETY: MEMORYSTATUSEX has its required length and a valid writable buffer.
            ensure!(
                unsafe { GlobalMemoryStatusEx(&mut memory) } != 0,
                "GlobalMemoryStatusEx: {}",
                std::io::Error::last_os_error()
            );
            ensure!(
                memory.total > 0 && memory.available <= memory.total,
                "Некорректный объём физической памяти"
            );
            let used = memory.total - memory.available;
            let sample = Sample {
                cpu_permille: cpu,
                ram_permille: ((u128::from(used) * 1000) / u128::from(memory.total)) as u16,
                used_mib: (used / (1024 * 1024)).try_into()?,
                total_mib: (memory.total / (1024 * 1024)).try_into()?,
                sequence,
                graphics: Default::default(),
            };
            sample.encode()?;
            Ok(sample)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fast_samples_follow_only_the_visible_fps_page() {
        use crate::dock_pages::{Kind, Page, Playlist};
        let mut config = super::Config {
            style: 3,
            ..Default::default()
        };
        let pages = Playlist {
            pages: [
                Kind::Summary,
                Kind::Gaming,
                Kind::Thermals,
                Kind::Pulse,
                Kind::Calendar,
            ]
            .into_iter()
            .map(|kind| Page {
                kind,
                ..Default::default()
            })
            .collect(),
            ..Default::default()
        };
        for active in [0, 1] {
            assert_eq!(
                super::sample_interval(&config, &pages, active, true),
                std::time::Duration::from_millis(250)
            );
        }
        for active in [2, 3, 4, 255] {
            assert_eq!(
                super::sample_interval(&config, &pages, active, true),
                std::time::Duration::from_secs(1)
            );
        }
        assert_eq!(
            super::sample_interval(&config, &pages, 1, false),
            std::time::Duration::from_secs(1)
        );
        config.style = 1;
        assert_eq!(
            super::sample_interval(&config, &pages, 0, true),
            std::time::Duration::from_secs(1)
        );
    }
    #[test]
    fn extended_temperature_packet_preserves_legacy_and_checks_the_tail() {
        let mut s = super::Sample {
            cpu_permille: 234,
            ram_permille: 625,
            used_mib: 10000,
            total_mib: 16000,
            sequence: 42,
            graphics: Default::default(),
        };
        s.graphics.temperatures = crate::temperatures::Readings {
            cpu: Some(623),
            gpu: Some(-100),
            storage: None,
            board: Some(1500),
        };
        s.graphics.power_deciwatts = Some(2270);
        let p = s.encode_extended().unwrap();
        assert_eq!(p[..32], s.encode_graphics().unwrap());
        assert_eq!(p[33], 27);
        assert_eq!(i16::from_le_bytes(p[36..38].try_into().unwrap()), -100);
        assert_eq!(
            super::crc16(&p[..46]),
            u16::from_le_bytes(p[46..].try_into().unwrap())
        );
        assert_eq!(super::packet(15, &p).unwrap()[17..], p);
        s.graphics.temperatures.cpu = Some(1501);
        assert!(s.encode_extended().is_err());
    }
    use super::*;
    #[test]
    fn automatic_samples_follow_enabled_pages_and_legacy_ownership() {
        use crate::dock_pages::{Kind, Page, Playlist};
        let mut c = Config::default();
        let mut p = Playlist {
            enabled: true,
            autoplay: false,
            selected: 0,
            pages: vec![
                Page {
                    kind: Kind::Calendar,
                    ..Default::default()
                },
                Page {
                    kind: Kind::Summary,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        c.enabled = false;
        assert!(needs_samples(&c, &p)); // Calendar visible; Summary ready for rear button.
        p.pages[1].enabled = false;
        assert!(!needs_samples(&c, &p));
        p.pages[1].kind = Kind::Cpu;
        p.pages[1].enabled = true;
        assert!(needs_samples(&c, &p));
        p.pages[1].kind = Kind::Memory;
        assert!(needs_samples(&c, &p));
        p.enabled = false;
        assert!(!needs_samples(&c, &p));
        c.enabled = true;
        assert!(needs_samples(&c, &p)); // Legacy manual summary.
    }
    #[test]
    fn design_marker_is_optional_and_checksum_protected() {
        let mut raw = [0; 65];
        raw[1..7].copy_from_slice(b"\x2cAJSC\x01");
        raw[8] = 255 - raw[1..8].iter().fold(0u8, |a, v| a.wrapping_add(*v));
        raw[9..57].copy_from_slice(&Config::default().encode().unwrap());
        raw[64] = 15;
        let mut r = Reply::parse(&raw).unwrap();
        let mut g = [0; 65];
        g[1..7].copy_from_slice(b"\x2cAJGM\x01");
        g[8] = 255 - g[1..8].iter().fold(0u8, |a, v| a.wrapping_add(*v));
        g[9] = 1;
        assert!(r.read_graphics_capability(&g));
        assert_eq!(r.design_version, 0);
        g[10..13].copy_from_slice(b"UI\x01");
        g[13] = 255 - g[10] - g[11] - g[12];
        assert!(r.read_graphics_capability(&g));
        assert_eq!(r.design_version, 1);
        let mut legacy = Reply::parse(&raw).unwrap();
        g[13] ^= 1;
        assert!(legacy.read_graphics_capability(&g));
        assert_eq!(legacy.design_version, 0);
        assert!(!legacy.read_graphics_capability(&g[..12]));
        g[10..13].copy_from_slice(b"UI\x02");
        g[13] = 255 - g[10] - g[11] - g[12];
        let s = Sample {
            cpu_permille: 200,
            ram_permille: 300,
            used_mib: 2,
            total_mib: 4,
            sequence: 1,
            graphics: crate::graphics::Metrics {
                temperatures: crate::temperatures::Readings {
                    cpu: Some(623),
                    ..Default::default()
                },
                power_deciwatts: Some(2270),
                ..Default::default()
            },
        };
        g[17..65].copy_from_slice(&s.encode_extended().unwrap());
        legacy.has_sample = true;
        assert!(legacy.read_graphics_capability(&g));
        assert_eq!(legacy.design_version, 2);
        assert_eq!(legacy.graphics.temperatures.cpu, Some(623));
        assert_eq!(legacy.graphics.power_deciwatts, Some(2270));
        g[63] ^= 1;
        legacy.read_graphics_capability(&g);
        assert_eq!(legacy.graphics.temperatures.cpu, None);
        g[1] ^= 1;
        assert!(!legacy.read_graphics_capability(&g));
    }
    #[test]
    fn graphics_readback_requires_a_fresh_crc_checked_sample() {
        let mut raw = [0; 65];
        raw[1..7].copy_from_slice(b"\x2cAJSC\x01");
        raw[8] = 255 - raw[1..8].iter().fold(0u8, |a, v| a.wrapping_add(*v));
        raw[9..57].copy_from_slice(&Config::default().encode().unwrap());
        raw[63] = 1;
        raw[64] = 15;
        let sample = Sample {
            cpu_permille: 10,
            ram_permille: 500,
            used_mib: 8,
            total_mib: 16,
            sequence: 1,
            graphics: crate::graphics::Metrics {
                gpu_permille: Some(400),
                vram_used_mib: Some(1000),
                vram_total_mib: Some(8192),
                fps_tenths: Some(1440),
                ..Default::default()
            },
        };
        let mut g = [0; 65];
        g[1..7].copy_from_slice(b"\x2cAJGM\x01");
        g[8] = 255 - g[1..8].iter().fold(0u8, |a, v| a.wrapping_add(*v));
        g[9] = 1;
        g[17..49].copy_from_slice(&sample.encode_graphics().unwrap());
        let mut r = Reply::parse(&raw).unwrap();
        assert!(r.read_graphics_capability(&g));
        assert_eq!(r.graphics, sample.graphics);
        for bit in 0..256 {
            let mut bad = g;
            bad[17 + bit / 8] ^= 1 << (bit % 8);
            let mut r = Reply::parse(&raw).unwrap();
            assert!(r.read_graphics_capability(&bad));
            assert_eq!(r.graphics, Default::default());
        }
        raw[63] = 0;
        let mut stale = Reply::parse(&raw).unwrap();
        stale.read_graphics_capability(&g);
        assert_eq!(stale.graphics, Default::default());
    }
    #[test]
    fn graphics_sample_preserves_legacy_crc_and_missing_values() {
        let mut sample = Sample {
            cpu_permille: 100,
            ram_permille: 500,
            used_mib: 8000,
            total_mib: 16000,
            sequence: 1,
            graphics: Default::default(),
        };
        let p = sample.encode_graphics().unwrap();
        assert_eq!(&p[..16], &sample.encode().unwrap());
        assert_eq!(p[16..18], [1, 0]);
        assert_eq!(
            crc16(&p[..30]),
            u16::from_le_bytes(p[30..].try_into().unwrap())
        );
        sample.graphics = crate::graphics::Metrics {
            gpu_permille: Some(999),
            vram_used_mib: Some(6144),
            vram_total_mib: Some(8192),
            fps_tenths: Some(1440),
            ..Default::default()
        };
        let p = sample.encode_graphics().unwrap();
        assert_eq!(p[17], 7);
        assert_eq!(u16::from_le_bytes(p[28..30].try_into().unwrap()), 1440);
        sample.graphics.vram_used_mib = Some(8193);
        assert!(sample.encode_graphics().is_err());
        sample.graphics.vram_used_mib = None;
        assert!(sample.encode_graphics().is_err());
        sample.graphics.vram_total_mib = None;
        sample.graphics.fps_tenths = Some(0);
        assert!(sample.encode_graphics().is_err());
        assert!(packet(14, &[0; 31]).is_err());
        assert!(packet(13, &[0]).is_err());
    }
    #[test]
    fn styles_and_focus_duration_roundtrip_with_legacy_default() {
        let legacy = Config::default().encode().unwrap();
        assert_eq!(&legacy[44..46], &[0, 0]);
        assert_eq!(Config::decode(&legacy).unwrap().focus_minutes, 25);
        let mut c = Config {
            style: 2,
            focus_minutes: 180,
            ..Default::default()
        };
        assert_eq!(Config::decode(&c.encode().unwrap()).unwrap(), c);
        c.style = 4;
        assert!(c.encode().is_err());
        c.style = 0;
        c.focus_minutes = 0;
        assert!(c.encode().is_err());
        assert!(packet(11, &[0]).is_ok());
        assert!(packet(12, &[]).is_ok());
        assert!(packet(12, &[0]).is_err());
    }
    #[test]
    fn config_roundtrip_and_corruption() {
        let c = Config::default();
        let bytes = c.encode().unwrap();
        assert_eq!(Config::decode(&bytes).unwrap(), c);
        for bit in 0..384 {
            let mut b = bytes;
            b[bit / 8] ^= 1 << (bit % 8);
            assert!(Config::decode(&b).is_err());
        }
        let mut bad = c;
        bad.title = "Заголовок".into();
        assert!(bad.encode().is_err());
    }
    #[test]
    fn packets_are_bounded_and_stock_or_cached_ack_is_rejected() {
        let p = packet(
            2,
            &Sample {
                cpu_permille: 234,
                ram_permille: 625,
                used_mib: 10000,
                total_mib: 16000,
                sequence: 42,
                graphics: Default::default(),
            }
            .encode()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(&p[1..9], &[0x2c, 0, 0, 0, 0, 0, 0, 0xd3]);
        assert_eq!(&p[9..17], b"AJSC\x01\x02\x10\0");
        assert!(packet(3, &[0; 49]).is_err());
        assert!(Reply::parse(&[0; 65]).is_err());
        let mut reply = [0; 65];
        reply[1..7].copy_from_slice(b"\x2cAJSC\x01");
        reply[8] = 255 - reply[1..8].iter().fold(0u8, |v, &b| v.wrapping_add(b));
        reply[9..57].copy_from_slice(&Config::default().encode().unwrap());
        reply[64] = 15;
        // The wrapping checksum is tested below; a response requires all capabilities.
        assert!(Reply::parse(&reply).is_ok());
        reply[64] = 0;
        assert!(Reply::parse(&reply).is_err());
    }
    #[test]
    fn cpu_counts_kernel_idle_once_and_handles_large_counters() {
        assert_eq!(cpu_load([100, 200, 50], [130, 260, 90]).unwrap(), 700);
        assert_eq!(
            cpu_load([0, 0, 0], [u64::MAX, u64::MAX, u64::MAX]).unwrap(),
            500
        );
        assert!(cpu_load([100, 0, 0], [90, 10, 10]).is_err());
        assert!(cpu_load([0; 3], [0; 3]).is_err());
    }
}
