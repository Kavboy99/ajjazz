//! User-approved, read-only elevated collector. No service or group changes.
//! A private loopback connection dies with Studio; the protocol only carries
//! sensor values and bounded capture preferences, never executable commands.
use crate::graphics::{Metrics, Options};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub metrics: Metrics,
    pub notice: Option<String>,
    #[serde(default)]
    pub debug: serde_json::Value,
}
#[derive(Default)]
struct State {
    pending: bool,
    latest: Option<(Instant, Snapshot)>,
    notice: Option<String>,
    options: Options,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}
pub fn snapshot(options: &Options) -> Option<Snapshot> {
    let mut s = state().lock().ok()?;
    s.options = options.clone();
    s.latest
        .as_ref()
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(4))
        .map(|(_, s)| s.clone())
}
pub fn pending() -> bool {
    state().lock().is_ok_and(|s| s.pending)
}
pub fn notice() -> Option<String> {
    state().lock().ok()?.notice.clone()
}
fn read_line(reader: &mut impl BufRead) -> Result<String> {
    let mut line = String::new();
    let n = reader.take(4097).read_line(&mut line)?;
    ensure!(
        n > 0 && n <= 4096 && line.ends_with('\n'),
        "Metrics connection ended or exceeded frame size"
    );
    Ok(line)
}
fn valid_options(o: &Options) -> bool {
    o.process_name.len() <= 260 && !o.process_name.chars().any(char::is_control)
}
fn serve(listener: TcpListener, token: String) -> Result<()> {
    listener.set_nonblocking(true)?;
    let until = Instant::now() + Duration::from_secs(120);
    let stream = loop {
        ensure!(
            Instant::now() < until,
            "Windows не подтвердил подключение сборщика"
        );
        if !pending() {
            return Ok(());
        }
        match listener.accept() {
            Ok((mut stream, addr)) => {
                if !addr.ip().is_loopback() {
                    continue;
                }
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                stream.set_write_timeout(Some(Duration::from_secs(3)))?;
                let mut reader = BufReader::new(stream.try_clone()?);
                if read_line(&mut reader).is_ok_and(|s| s.trim_end() == token) {
                    stream.write_all(b"ready\n")?;
                    break stream;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.into()),
        }
    };
    let mut output = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    loop {
        let line = read_line(&mut reader)?;
        let snapshot: Snapshot = serde_json::from_str(&line)?;
        ensure!(
            snapshot.metrics.temperatures.valid()
                && snapshot.metrics.gpu_permille.is_none_or(|v| v <= 1000),
            "Некорректные показатели сборщика"
        );
        let options = {
            let mut s = state()
                .lock()
                .map_err(|_| anyhow::anyhow!("Metrics state unavailable"))?;
            s.pending = false;
            s.notice = None;
            s.latest = Some((Instant::now(), snapshot));
            s.options.clone()
        };
        serde_json::to_writer(&mut output, &options)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

#[cfg(windows)]
pub fn request() -> Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(a: *mut c_void, b: *mut u8, c: u32, d: u32) -> i32;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            a: *mut c_void,
            b: *const u16,
            c: *const u16,
            d: *const u16,
            e: *const u16,
            f: i32,
        ) -> isize;
    }
    {
        let mut s = state()
            .lock()
            .map_err(|_| anyhow::anyhow!("Metrics state unavailable"))?;
        ensure!(!s.pending, "Ожидается подтверждение Windows");
        ensure!(
            s.latest
                .as_ref()
                .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(4)),
            "Сборщик уже подключён"
        );
        s.pending = true;
        s.notice = None;
    }
    let result = (|| -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let mut random = [0u8; 32];
        ensure!(
            unsafe { BCryptGenRandom(std::ptr::null_mut(), random.as_mut_ptr(), 32, 2) } == 0,
            "Windows RNG недоступен"
        );
        let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let helper = std::env::current_exe()?
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Нет каталога Studio"))?
            .join("ajazz-studio.exe");
        ensure!(
            helper.is_file(),
            "Фоновый сборщик отсутствует в пакете Studio"
        );
        let params: Vec<u16> = format!("--metrics-agent {address} {token}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let path: Vec<u16> = helper.as_os_str().encode_wide().chain(Some(0)).collect();
        std::thread::spawn(move || {
            if let Err(error) = serve(listener, token)
                && let Ok(mut s) = state().lock()
            {
                s.pending = false;
                s.latest = None;
                s.notice = Some(error.to_string());
            }
        });
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                path.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                0,
            )
        };
        ensure!(code > 32, "Запрос Windows отменён (код {code})");
        Ok(())
    })();
    if let Err(error) = &result
        && let Ok(mut s) = state().lock()
    {
        s.pending = false;
        s.notice = Some(error.to_string());
    }
    result
}
#[cfg(not(windows))]
pub fn request() -> Result<()> {
    anyhow::bail!("Windows only")
}

#[cfg(windows)]
pub fn run(address: &str, token: &str) -> Result<()> {
    let address: SocketAddr = address.parse()?;
    ensure!(
        address.ip().is_loopback()
            && address.port() >= 1024
            && token.len() == 64
            && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid collector connection"
    );
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(4)))?;
    writeln!(stream, "{token}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream.try_clone()?);
    ensure!(
        read_line(&mut reader)?.trim() == "ready",
        "Collector handshake failed"
    );
    let mut options = Options::default();
    let mut collector = crate::graphics::Collector::new(options.clone());
    loop {
        let snapshot = Snapshot {
            metrics: collector.sample(),
            notice: collector
                .fps_notice()
                .map(|s| s.chars().take(350).collect()),
            debug: collector.debug(),
        };
        serde_json::to_writer(&mut stream, &snapshot)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        let updated: Options = serde_json::from_str(&read_line(&mut reader)?)?;
        ensure!(valid_options(&updated), "Invalid collector preferences");
        if updated != options {
            options = updated;
            collector = crate::graphics::Collector::new(options.clone());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_local_peer_stays_connected_and_receives_only_capture_preferences() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        *state().lock().unwrap() = State {
            pending: true,
            ..Default::default()
        };
        let server = std::thread::spawn(move || serve(listener, "test-token".into()));
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        std::thread::sleep(Duration::from_millis(150));
        stream.write_all(b"test-token\n").unwrap();
        assert_eq!(read_line(&mut reader).unwrap().trim(), "ready");
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(150));
            serde_json::to_writer(&mut stream, &Snapshot::default()).unwrap();
            stream.write_all(b"\n").unwrap();
            let options: Options = serde_json::from_str(&read_line(&mut reader).unwrap()).unwrap();
            assert!(valid_options(&options));
        }
        assert!(snapshot(&Options::default()).is_some());
        drop(reader);
        drop(stream);
        assert!(server.join().unwrap().is_err());
        *state().lock().unwrap() = State::default();
    }
    #[test]
    fn protocol_is_bounded_and_preferences_cannot_contain_control_characters() {
        assert!(read_line(&mut std::io::Cursor::new(b"{}\n")).is_ok());
        assert!(read_line(&mut std::io::Cursor::new(vec![b'x'; 5000])).is_err());
        assert!(read_line(&mut std::io::Cursor::new(b"unterminated")).is_err());
        assert!(!valid_options(&Options {
            process_name: "game.exe\ncommand".into(),
            ..Default::default()
        }));
    }
}
