//! Optional physical sensor readings. Missing sensors stay unavailable.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Readings {
    pub cpu: Option<i16>,
    pub gpu: Option<i16>,
    pub storage: Option<i16>,
    pub board: Option<i16>,
}
impl Readings {
    pub fn valid(&self) -> bool {
        [self.cpu, self.gpu, self.storage, self.board]
            .into_iter()
            .flatten()
            .all(|v| (-100..=1500).contains(&v))
    }
}

fn storage_descriptor(bytes: &[u8]) -> Option<i16> {
    if bytes.len() < 40 {
        return None;
    }
    let size = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let count = u16::from_le_bytes(bytes[12..14].try_into().ok()?) as usize;
    if size < 40 || size > bytes.len() || count == 0 || count > (size - 24) / 16 {
        return None;
    }
    let mut values = Vec::new();
    for i in 0..count {
        let at = 24 + i * 16;
        let temp = i16::from_le_bytes(bytes[at + 2..at + 4].try_into().ok()?);
        if (-10..=150).contains(&temp) {
            values.push(temp * 10);
        }
    }
    values.first().copied()
}
#[cfg(windows)]
fn storage_read() -> Option<i16> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            path: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn DeviceIoControl(
            handle: *mut c_void,
            code: u32,
            input: *const c_void,
            input_size: u32,
            out: *mut c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let mut temperatures = Vec::new();
    for index in 0..16 {
        let path: Vec<u16> = format!("\\\\.\\PhysicalDrive{index}\0")
            .encode_utf16()
            .collect();
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                3,
                std::ptr::null_mut(),
                3,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle as isize == -1 {
            continue;
        }
        let query = [52u32, 0, 0];
        let mut output = [0u8; 512];
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                0x2d1400,
                query.as_ptr().cast(),
                12,
                output.as_mut_ptr().cast(),
                512,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };
        if ok != 0
            && returned <= 512
            && let Some(t) = storage_descriptor(&output[..returned as usize])
        {
            temperatures.push(t);
        }
    }
    temperatures.into_iter().max()
}
#[cfg(windows)]
pub fn sample() -> Readings {
    use std::{
        sync::{Arc, Mutex, OnceLock},
        time::{Duration, Instant},
    };
    type Cached = Arc<Mutex<Option<(Instant, Readings)>>>;
    static NATIVE: OnceLock<Cached> = OnceLock::new();
    let native = NATIVE.get_or_init(|| {
        let cache: Cached = Arc::new(Mutex::new(None));
        let out = cache.clone();
        std::thread::spawn(move || {
            loop {
                let readings = Readings {
                    storage: storage_read(),
                    ..Default::default()
                };
                if let Ok(mut c) = out.lock() {
                    *c = Some((Instant::now(), readings));
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        cache
    });
    let mut readings = native
        .lock()
        .ok()
        .and_then(|v| {
            v.as_ref()
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(15))
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_default();
    let extra = external();
    readings.cpu = extra.cpu;
    readings.board = extra.board;
    if readings.storage.is_none() {
        readings.storage = extra.storage;
    }
    readings
}
#[cfg(windows)]
fn external() -> Readings {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn IsUserAnAdmin() -> i32;
    }
    if unsafe { IsUserAnAdmin() } == 0 {
        return Readings::default();
    }
    use std::{
        io::{BufRead, BufReader, Write},
        os::windows::process::CommandExt,
        sync::{Arc, Mutex, OnceLock},
        time::{Duration, Instant},
    };
    type Cached = Arc<Mutex<Option<(Instant, Readings)>>>;
    static EXTERNAL: OnceLock<Cached> = OnceLock::new();
    let cache = EXTERNAL.get_or_init(|| {
        let cache: Cached = Arc::new(Mutex::new(None));
        let out = cache.clone();
        std::thread::spawn(move || {
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let Some(parent) = exe.parent() else {
                return;
            };
            let directory = parent.join("tools/sensors");
            let manifest: std::collections::BTreeMap<String, String> =
                serde_json::from_str(include_str!("../assets/sensors-manifest.json"))
                    .unwrap_or_default();
            use sha2::{Digest, Sha256};
            if manifest.is_empty()
                || manifest.iter().any(|(name, digest)| {
                    !std::fs::read(directory.join(name))
                        .is_ok_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == *digest)
                })
            {
                return;
            }
            loop {
                let child = std::process::Command::new(directory.join("AjazzSensors.exe"))
                    .current_dir(&directory)
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                let Ok(mut child) = child else {
                    return;
                };
                let mut input = child.stdin.take().unwrap();
                let mut reader = BufReader::new(child.stdout.take().unwrap());
                for _ in 0..12 {
                    if input.write_all(b"sample\n").is_err() || input.flush().is_err() {
                        break;
                    }
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.len() > 4096 {
                        break;
                    }
                    if let Ok(value) = serde_json::from_str::<Readings>(&line)
                        && value.valid()
                        && let Ok(mut c) = out.lock()
                    {
                        *c = Some((Instant::now(), value));
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
                drop(input);
                let success = child.wait().is_ok_and(|s| s.success());
                std::thread::sleep(Duration::from_secs(if success { 2 } else { 30 }));
            }
        });
        cache
    });
    cache
        .lock()
        .ok()
        .and_then(|v| {
            v.as_ref()
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(8))
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_storage_rejects_unreported_and_out_of_bounds_data() {
        let mut b = [0u8; 40];
        b[4..8].copy_from_slice(&40u32.to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[26..28].copy_from_slice(&46i16.to_le_bytes());
        assert_eq!(storage_descriptor(&b), Some(460));
        b[26..28].copy_from_slice(&i16::MIN.to_le_bytes());
        assert_eq!(storage_descriptor(&b), None);
        b[12..14].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(storage_descriptor(&b), None);
        assert_eq!(storage_descriptor(&b[..20]), None);
    }
}
