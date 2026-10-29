//! Exact common_dangle/OLEDV104 display framing. A5 prepares a transfer; it is NOT readback.
use anyhow::{Result, ensure};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SIDE: u32 = 128;
pub const FRAME_BYTES: usize = 128 * 128 * 2;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayFrame {
    pub slot: u8,
    pub rgb565: Vec<u8>,
}
impl DisplayFrame {
    pub fn load(path: &Path) -> Result<Self> {
        let frame: Self = crate::storage::read_json(path, 512 * 1024)?;
        frame.validate()?;
        Ok(frame)
    }
    pub fn from_image(slot: u8, image: &RgbaImage) -> Result<Self> {
        ensure!(
            image.dimensions() == (SIDE, SIDE),
            "Изображение дока должно иметь размер 128×128"
        );
        let mut rgb565 = Vec::with_capacity(FRAME_BYTES);
        // Vendor kan(): crop -> transpose -> RGB565 in network byte order.
        for x in 0..SIDE {
            for y in 0..SIDE {
                let pixel = image.get_pixel(x, y);
                // Composite alpha over black, matching the screen renderer's opaque result.
                let channel = |index: usize| u16::from(pixel[index]) * u16::from(pixel[3]) / 255;
                let word = ((channel(0) >> 3) << 11) | ((channel(1) >> 2) << 5) | (channel(2) >> 3);
                rgb565.extend_from_slice(&word.to_be_bytes());
            }
        }
        let result = Self { slot, rgb565 };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.slot < 5, "Для дока APEX доступны слоты 1–5");
        ensure!(
            self.rgb565.len() == FRAME_BYTES,
            "Кадр должен содержать 32768 байт RGB565"
        );
        Ok(())
    }
    /// Always uses the full rectangle, including black pixels, to clear previous content.
    pub fn prepare_report(&self) -> Result<[u8; 65]> {
        self.validate()?;
        let mut packet = [0; 65];
        let data = &mut packet[1..];
        data[0] = 0xa5;
        data[1] = self.slot;
        data[2] = 1;
        let length = (FRAME_BYTES as u32).to_le_bytes();
        data[4..6].copy_from_slice(&length[..2]);
        data[10] = SIDE as u8;
        data[11] = SIDE as u8;
        data[16..18].copy_from_slice(&length[2..]);
        // SDK checksum enum BIT7 = 0; vendor uo(..., 0, 0) computes this byte.
        data[7] = 255u8.wrapping_sub(data[..7].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)));
        Ok(packet)
    }
    pub fn data_reports(&self) -> Result<Vec<[u8; 65]>> {
        self.validate()?;
        Ok(self
            .rgb565
            .chunks(56)
            .enumerate()
            .map(|(index, chunk)| {
                let mut packet = [0; 65];
                let data = &mut packet[1..];
                data[0] = 0x25;
                data[1] = self.slot;
                data[2] = 1;
                data[4..6].copy_from_slice(&(index as u16).to_le_bytes());
                data[6] = chunk.len() as u8;
                data[7] =
                    255u8.wrapping_sub(data[..7].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)));
                data[8..8 + chunk.len()].copy_from_slice(chunk);
                packet
            })
            .collect())
    }
    pub fn preview(&self) -> Result<RgbaImage> {
        self.validate()?;
        let mut result = RgbaImage::new(SIDE, SIDE);
        for (index, pixel) in self.rgb565.as_chunks::<2>().0.iter().enumerate() {
            let word = u16::from_be_bytes([pixel[0], pixel[1]]);
            let r = ((word >> 11) & 31) as u8;
            let g = ((word >> 5) & 63) as u8;
            let b = (word & 31) as u8;
            result.put_pixel(
                index as u32 / SIDE,
                index as u32 % SIDE,
                image::Rgba([
                    (r << 3) | (r >> 2),
                    (g << 2) | (g >> 4),
                    (b << 3) | (b >> 2),
                    255,
                ]),
            );
        }
        Ok(result)
    }
}
pub fn prepare_status(raw: &[u8]) -> Result<bool> {
    ensure!(
        raw.len() == 65 && raw[..2] == [0, 0xa5],
        "Ответ дока не соответствует подготовке изображения"
    );
    ensure!(raw[2] <= 1, "Неизвестный статус подготовки изображения");
    Ok(raw[2] == 1)
}

pub(crate) trait DisplayIo {
    fn info(&self) -> Result<crate::dock::DockInfo>;
    fn prepare(&self, report: &[u8; 65]) -> Result<()>;
    fn send(&self, packet: &[u8; 65]) -> Result<()>;
}
#[cfg(windows)]
impl DisplayIo for crate::dock::DockSession {
    fn info(&self) -> Result<crate::dock::DockInfo> {
        self.info()
    }
    fn prepare(&self, packet: &[u8; 65]) -> Result<()> {
        ensure!(packet[..2] == [0, 0xa5], "Unsupported display preparation");
        for _ in 0..10 {
            self.device.send_feature_report(packet)?;
            std::thread::sleep(std::time::Duration::from_millis(100));
            let mut raw = [0; 65];
            let length = self.device.get_feature_report(&mut raw)?;
            if prepare_status(&raw[..length])? {
                return Ok(());
            }
        }
        anyhow::bail!("Док не подтвердил готовность принять изображение; пиксели не отправлялись")
    }
    fn send(&self, packet: &[u8; 65]) -> Result<()> {
        ensure!(
            packet[0] == 0 && packet[1] == 0x25,
            "Unsupported display packet"
        );
        std::thread::sleep(std::time::Duration::from_millis(3));
        self.device.send_feature_report(packet)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct DisplayReceipt {
    pub slot: u8,
    pub sent_packets: usize,
    pub frame_count: usize,
    /// This stores the NEW image, never claims to back up unknown old device pixels.
    pub upload_record: PathBuf,
    pub preview_png: PathBuf,
    pub verification: String,
}
fn transfer(
    device: &impl DisplayIo,
    frame: &DisplayFrame,
    directory: &Path,
    progress: impl Fn(f32),
) -> Result<DisplayReceipt> {
    frame.validate()?;
    transfer_plan(
        device,
        frame,
        Plan {
            slot: frame.slot,
            frame_count: 1,
            prepare: frame.prepare_report()?,
            reports: frame.data_reports()?,
            preview: frame.preview()?,
        },
        directory,
        progress,
    )
}
struct Plan {
    slot: u8,
    frame_count: usize,
    prepare: [u8; 65],
    reports: Vec<[u8; 65]>,
    preview: RgbaImage,
}
fn transfer_plan(
    device: &impl DisplayIo,
    record_data: &impl Serialize,
    plan: Plan,
    directory: &Path,
    progress: impl Fn(f32),
) -> Result<DisplayReceipt> {
    let info = device.info()?;
    ensure!(
        info.oled_version == 0x0104,
        "Загрузка проверяется только для OLEDV104"
    );
    // A complete reproducible job exists on disk before the slot can be altered.
    let prefix = if plan.frame_count == 1 {
        "dock-screen-upload"
    } else {
        "dock-animation-upload"
    };
    let record = crate::storage::backup(directory, prefix, record_data)?;
    let preview_png = record.with_extension("png");
    let mut png = std::io::Cursor::new(Vec::new());
    plan.preview.write_to(&mut png, image::ImageFormat::Png)?;
    use std::io::Write;
    let mut file = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&preview_png)?;
    file.write_all(png.get_ref())?;
    file.sync_all()?;
    let reports = plan.reports;
    device.prepare(&plan.prepare)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    for (index, packet) in reports.iter().enumerate() {
        if let Err(error) = device.send(packet) {
            anyhow::bail!(
                "Передача слота {} оборвалась после {index}/{} пакетов: {error:#}. Изображение слота может быть неполным. Новые данные для повторной загрузки: {}",
                plan.slot + 1,
                reports.len(),
                record.display()
            );
        }
        progress((index + 1) as f32 / reports.len() as f32);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    ensure!(
        device.info()?.oled_version == info.oled_version,
        "После загрузки док не подтвердил прежнюю версию; изображение уже отправлено"
    );
    Ok(DisplayReceipt {slot:plan.slot,sent_packets:reports.len(),frame_count:plan.frame_count,upload_record:record,preview_png,
        verification:"Данные отправлены. Протокол не предоставляет чтение пикселей; требуется визуальная проверка экрана.".into()})
}
/// Full 128×128 frames: vendor UI uses 8 MiB, 4 KiB pages and reserves five pagesets.
/// GIF data [0x101000, 0x7d1000) fits218 frames before the five photo regions.
pub const MAX_ANIMATION_FRAMES: usize = 218;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayAnimation {
    pub delay_ms: u8,
    pub frames: Vec<Vec<u8>>,
}
impl DisplayAnimation {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = crate::storage::read_json(path, 80 * 1024 * 1024)?;
        value.validate()?;
        Ok(value)
    }
    pub fn from_images(images: &[RgbaImage], delay_ms: u8) -> Result<Self> {
        ensure!(
            (2..=MAX_ANIMATION_FRAMES).contains(&images.len()),
            "Анимация: 2–218 кадра"
        );
        let frames = images
            .iter()
            .map(|image| Ok(DisplayFrame::from_image(0, image)?.rgb565))
            .collect::<Result<Vec<_>>>()?;
        let value = Self { delay_ms, frames };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=MAX_ANIMATION_FRAMES).contains(&self.frames.len()) && self.delay_ms >= 20,
            "Анимация: 2–218 кадра, задержка 20–255 мс"
        );
        ensure!(
            self.frames.iter().all(|f| f.len() == FRAME_BYTES),
            "Некорректный RGB565 кадр анимации"
        );
        Ok(())
    }
    fn plan(&self) -> Result<Plan> {
        self.validate()?;
        let first = DisplayFrame {
            slot: 0,
            rgb565: self.frames[0].clone(),
        };
        let mut prepare = first.prepare_report()?;
        patch_animation_header(&mut prepare, 0, self.frames.len() as u8, self.delay_ms);
        let mut reports = Vec::with_capacity(self.frames.len() * 586);
        for (index, bytes) in self.frames.iter().enumerate() {
            for mut packet in (DisplayFrame {
                slot: 0,
                rgb565: bytes.clone(),
            })
            .data_reports()?
            {
                patch_animation_header(
                    &mut packet,
                    index as u8,
                    self.frames.len() as u8,
                    self.delay_ms,
                );
                reports.push(packet);
            }
        }
        Ok(Plan {
            slot: 0,
            frame_count: self.frames.len(),
            prepare,
            reports,
            preview: first.preview()?,
        })
    }
}
fn patch_animation_header(report: &mut [u8; 65], index: u8, count: u8, delay: u8) {
    report[2] = index;
    report[3] = count;
    report[4] = delay;
    report[8] = 255u8.wrapping_sub(report[1..8].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)));
}
#[cfg(windows)]
pub fn upload_animation(
    animation: &DisplayAnimation,
    progress: impl Fn(f32),
) -> Result<DisplayReceipt> {
    transfer_plan(
        &crate::dock::DockSession::open()?,
        animation,
        animation.plan()?,
        &crate::storage::backup_directory()?,
        progress,
    )
}
#[cfg(not(windows))]
pub fn upload_animation(_: &DisplayAnimation, _: impl Fn(f32)) -> Result<DisplayReceipt> {
    anyhow::bail!("Windows only")
}
#[cfg(windows)]
pub fn upload(frame: &DisplayFrame, progress: impl Fn(f32)) -> Result<DisplayReceipt> {
    transfer(
        &crate::dock::DockSession::open()?,
        frame,
        &crate::storage::backup_directory()?,
        progress,
    )
}
#[cfg(not(windows))]
pub fn upload(_: &DisplayFrame, _: impl Fn(f32)) -> Result<DisplayReceipt> {
    anyhow::bail!("Windows only")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_headers_keep_frame_index_separate_from_chunk_index() {
        let animation = DisplayAnimation {
            delay_ms: 250,
            frames: vec![vec![0; FRAME_BYTES], vec![255; FRAME_BYTES]],
        };
        let plan = animation.plan().unwrap();
        assert_eq!(plan.reports.len(), 1172);
        assert_eq!(plan.prepare[1..5], [0xa5, 0, 2, 250]);
        assert_eq!(plan.reports[0][1..8], [0x25, 0, 2, 250, 0, 0, 56]);
        assert_eq!(plan.reports[586][1..8], [0x25, 1, 2, 250, 0, 0, 56]);
        assert_eq!(plan.reports[1171][5..8], [73, 2, 8]);
        for packet in &plan.reports {
            assert_eq!(
                packet[1..9].iter().fold(0u8, |s, b| s.wrapping_add(*b)),
                255
            );
        }
        let device = Mock {
            version: 0x104,
            prepare_fails: false,
            sent: Cell::new(0),
            fail_at: usize::MAX,
        };
        let receipt = transfer_plan(&device, &animation, plan, &directory(), |_| {}).unwrap();
        assert_eq!(receipt.frame_count, 2);
        assert_eq!(device.sent.get(), 1172);
        assert_eq!(
            DisplayAnimation::load(&receipt.upload_record)
                .unwrap()
                .frames[1][0],
            255
        );
        assert!(
            DisplayAnimation {
                delay_ms: 10,
                frames: animation.frames
            }
            .plan()
            .is_err()
        );
    }
    use std::cell::Cell;
    struct Mock {
        version: u16,
        prepare_fails: bool,
        sent: Cell<usize>,
        fail_at: usize,
    }
    impl DisplayIo for Mock {
        fn info(&self) -> Result<crate::dock::DockInfo> {
            Ok(crate::dock::DockInfo {
                captured_utc: String::new(),
                oled_version: self.version,
                flash_version: 0,
                raw_response: vec![0; 65],
            })
        }
        fn prepare(&self, _: &[u8; 65]) -> Result<()> {
            ensure!(!self.prepare_fails, "prepare rejected");
            Ok(())
        }
        fn send(&self, packet: &[u8; 65]) -> Result<()> {
            let index = self.sent.get();
            self.sent.set(index + 1);
            ensure!(index != self.fail_at, "transfer interrupted");
            ensure!(packet[1] == 0x25, "Wrong command");
            Ok(())
        }
    }
    fn test_frame() -> DisplayFrame {
        DisplayFrame {
            slot: 4,
            rgb565: vec![0; FRAME_BYTES],
        }
    }
    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ajazz-display-test-{}",
            crate::storage::unique_suffix()
        ))
    }
    #[test]
    fn upload_records_new_content_and_never_claims_old_pixels_or_readback() {
        let device = Mock {
            version: 0x104,
            prepare_fails: false,
            sent: Cell::new(0),
            fail_at: usize::MAX,
        };
        let receipt = transfer(&device, &test_frame(), &directory(), |_| {}).unwrap();
        assert_eq!(device.sent.get(), 586);
        assert_eq!(receipt.sent_packets, 586);
        assert!(receipt.preview_png.exists() && receipt.upload_record.exists());
        let saved = DisplayFrame::load(&receipt.upload_record).unwrap();
        assert_eq!(saved.rgb565, test_frame().rgb565);
        assert!(receipt.verification.contains("визуальная проверка"));
    }
    #[test]
    fn unsupported_version_backup_failure_and_prepare_failure_send_no_pixels() {
        for (version, prepare_fails) in [(0x105, false), (0x104, true)] {
            let device = Mock {
                version,
                prepare_fails,
                sent: Cell::new(0),
                fail_at: usize::MAX,
            };
            assert!(transfer(&device, &test_frame(), &directory(), |_| {}).is_err());
            assert_eq!(device.sent.get(), 0);
        }
        let path = directory();
        std::fs::write(&path, b"file").unwrap();
        let device = Mock {
            version: 0x104,
            prepare_fails: false,
            sent: Cell::new(0),
            fail_at: usize::MAX,
        };
        assert!(transfer(&device, &test_frame(), &path, |_| {}).is_err());
        assert_eq!(device.sent.get(), 0);
    }
    #[test]
    fn interrupted_upload_stops_without_retrying_or_erasing_another_slot() {
        let device = Mock {
            version: 0x104,
            prepare_fails: false,
            sent: Cell::new(0),
            fail_at: 3,
        };
        let error = transfer(&device, &test_frame(), &directory(), |_| {})
            .unwrap_err()
            .to_string();
        assert_eq!(device.sent.get(), 4);
        assert!(error.contains("3/586"));
        assert!(error.contains("неполным"));
    }
    #[test]
    fn column_order_big_endian_and_alpha_are_visible_in_decoded_preview() {
        let mut image = RgbaImage::from_pixel(SIDE, SIDE, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.put_pixel(0, 1, image::Rgba([0, 255, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        image.put_pixel(1, 1, image::Rgba([255, 255, 255, 0]));
        let frame = DisplayFrame::from_image(4, &image).unwrap();
        assert_eq!(frame.rgb565[..4], [0xf8, 0, 7, 0xe0]);
        assert_eq!(frame.rgb565[256..260], [0, 31, 0, 0]);
        assert_eq!(frame.preview().unwrap().get_pixel(1, 0).0, [0, 0, 255, 255]);
        let prepare = frame.prepare_report().unwrap();
        assert_eq!(prepare[1..9], [0xa5, 4, 1, 0, 0, 128, 0, 213]);
        assert_eq!(prepare[9..19], [0, 0, 128, 128, 0, 0, 0, 0, 0, 0]);
    }
    #[test]
    fn every_frame_byte_is_sent_with_correct_chunk_length_and_header_checksum() {
        let bytes = (0..FRAME_BYTES).map(|i| i as u8).collect::<Vec<_>>();
        let frame = DisplayFrame {
            slot: 0,
            rgb565: bytes.clone(),
        };
        let reports = frame.data_reports().unwrap();
        assert_eq!(reports.len(), 586);
        let mut decoded = Vec::new();
        for (index, packet) in reports.iter().enumerate() {
            assert_eq!(packet[0], 0);
            assert_eq!(packet[1], 0x25);
            assert_eq!(u16::from_le_bytes([packet[5], packet[6]]) as usize, index);
            assert_eq!(
                packet[1..9].iter().fold(0u8, |s, b| s.wrapping_add(*b)),
                255
            );
            decoded.extend_from_slice(&packet[9..9 + usize::from(packet[7])]);
        }
        assert_eq!(decoded, bytes);
        assert_eq!(reports.last().unwrap()[7], 8);
        let mut reply = [0; 65];
        reply[1] = 0xa5;
        assert!(!prepare_status(&reply).unwrap());
        reply[2] = 1;
        assert!(prepare_status(&reply).unwrap());
        reply[1] = 0xad;
        assert!(prepare_status(&reply).is_err());
        assert!(
            DisplayFrame {
                slot: 5,
                rgb565: bytes
            }
            .data_reports()
            .is_err()
        );
    }
}
