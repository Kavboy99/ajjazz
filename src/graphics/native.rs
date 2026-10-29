//! Native Windows ETW, no injection, service, settings changes or capture executable.
//! DXGI/D3D9 manifest facts: GameTechDev/PresentMon v2.6.0 PresentData/ETW headers.
//! Present calls remain separate per process/swapchain, and TEST/failed calls are excluded.
use super::Frames;
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Etw::*,
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    },
    core::GUID,
};

const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
const D3D9: GUID = GUID::from_u128(0x783aca0a_790e_4d7f_8451_aa850511c6b9);
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
    fn QueryPerformanceCounter(counter: *mut i64) -> i32;
}
fn same_guid(a: GUID, b: GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}
#[derive(Default)]
struct Presents {
    starts: HashMap<(u32, u32), (u64, u64)>,
    previous: HashMap<(u32, u64), u64>,
}
impl Presents {
    fn start(&mut self, pid: u32, tid: u32, chain: u64, tick: u64, test: bool) {
        if self.starts.len() > 4096 {
            self.starts.clear();
        }
        self.starts.remove(&(pid, tid));
        if !test {
            self.starts.insert((pid, tid), (chain, tick));
        }
    }
    fn stop(&mut self, pid: u32, tid: u32, result: u32, frequency: u64) -> Option<(u64, f64)> {
        let (chain, tick) = self.starts.remove(&(pid, tid))?;
        if result != 0 || frequency == 0 {
            return None;
        }
        if self.previous.len() > 512 {
            self.previous
                .retain(|_, old| tick.saturating_sub(*old) <= frequency.saturating_mul(3));
        }
        let old = self.previous.insert((pid, chain), tick)?;
        let ms = tick.checked_sub(old)? as f64 * 1000.0 / frequency as f64;
        (0.1..=1000.0).contains(&ms).then_some((chain, ms))
    }
}
struct Context {
    frames: Arc<Mutex<Frames>>,
    presents: Presents,
    names: HashMap<u32, String>,
    frequency: u64,
}
fn process_name(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return "<unknown>".into();
        }
        let mut name = [0u16; 1024];
        let mut len = name.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, name.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return "<unknown>".into();
        }
        String::from_utf16_lossy(&name[..len as usize])
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("<unknown>")
            .to_owned()
    }
}
unsafe fn property(r: *const EVENT_RECORD, name: *const u16) -> Option<u64> {
    let d = PROPERTY_DATA_DESCRIPTOR {
        PropertyName: name as u64,
        ArrayIndex: u32::MAX,
        Reserved: 0,
    };
    let mut value = 0u64;
    (unsafe {
        TdhGetProperty(
            r,
            0,
            std::ptr::null(),
            1,
            &d,
            8,
            (&mut value as *mut u64).cast(),
        )
    } == 0)
        .then_some(value)
}
unsafe extern "system" fn event(r: *mut EVENT_RECORD) {
    if r.is_null() {
        return;
    }
    let record = unsafe { &*r };
    if record.UserContext.is_null() {
        return;
    }
    let c = unsafe { &mut *record.UserContext.cast::<Context>() };
    let h = &record.EventHeader;
    let dxgi = same_guid(h.ProviderId, DXGI);
    if !dxgi && !same_guid(h.ProviderId, D3D9) {
        return;
    }
    if let Ok(mut s) = c.frames.lock() {
        s.native_events = s.native_events.saturating_add(1);
    }
    let id = h.EventDescriptor.Id;
    let start = if dxgi {
        [42, 55].contains(&id)
    } else {
        id == 1
    };
    let stop = if dxgi {
        [43, 56].contains(&id)
    } else {
        id == 2
    };
    if start {
        let chain = unsafe {
            property(
                r,
                if dxgi {
                    windows_sys::core::w!("pIDXGISwapChain")
                } else {
                    windows_sys::core::w!("pSwapchain")
                },
            )
        };
        let flags = unsafe { property(r, windows_sys::core::w!("Flags")) };
        if let (Some(chain), Some(flags)) = (chain, flags) {
            if let Ok(mut s) = c.frames.lock() {
                s.native_decoded = s.native_decoded.saturating_add(1);
            }
            c.presents.start(
                h.ProcessId,
                h.ThreadId,
                chain,
                h.TimeStamp as u64,
                dxgi && (flags & 1 != 0),
            );
        }
    } else if stop
        && let Some(result) = unsafe { property(r, windows_sys::core::w!("Result")) }
        && let Some((chain, ms)) =
            c.presents
                .stop(h.ProcessId, h.ThreadId, result as u32, c.frequency)
    {
        if c.names.len() > 256 {
            c.names.clear();
        }
        let name = c
            .names
            .entry(h.ProcessId)
            .or_insert_with(|| process_name(h.ProcessId))
            .clone();
        // Use event time, not batch-delivery time, for the averaging window.
        let now = Instant::now();
        let mut counter = 0;
        let age = if unsafe { QueryPerformanceCounter(&mut counter) } != 0 {
            Duration::from_secs_f64(
                (counter as u64).saturating_sub(h.TimeStamp as u64) as f64 / c.frequency as f64,
            )
        } else {
            Duration::ZERO
        };
        if let Ok(mut s) = c.frames.lock() {
            s.native_lag_ms = Some(age.as_millis().min(u128::from(u32::MAX)) as u32);
            s.observe(
                h.ProcessId,
                format!("native:{chain:016x}"),
                name,
                ms,
                now.checked_sub(age).unwrap_or(now),
            );
        }
    }
}
fn properties(name: &[u16]) -> Vec<u64> {
    let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + name.len() * 2;
    let mut memory = vec![0u64; size.div_ceil(8)];
    unsafe {
        let p = &mut *memory.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>();
        p.Wnode.BufferSize = (memory.len() * 8) as u32;
        p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        p.Wnode.ClientContext = 1;
        p.BufferSize = 64;
        let cpus = std::thread::available_parallelism().map_or(16, |v| v.get()) as u32;
        p.MinimumBuffers = (cpus * 2).max(8);
        p.MaximumBuffers = p.MinimumBuffers * 2;
        p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        p.FlushTimer = 1;
        p.LoggerNameOffset = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            memory
                .as_mut_ptr()
                .cast::<u8>()
                .add(std::mem::size_of::<EVENT_TRACE_PROPERTIES>())
                .cast(),
            name.len(),
        );
    }
    memory
}
struct Session {
    name: Vec<u16>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let mut p = properties(&self.name);
        unsafe {
            ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                self.name.as_ptr(),
                p.as_mut_ptr().cast(),
                EVENT_TRACE_CONTROL_STOP,
            )
        };
    }
}
pub(super) fn run(frames: Arc<Mutex<Frames>>, stop: Arc<AtomicBool>) -> Result<()> {
    let mut frequency = 0i64;
    ensure!(
        unsafe { QueryPerformanceFrequency(&mut frequency) } != 0 && frequency > 0,
        "Windows QPC недоступен"
    );
    let name: Vec<u16> = format!("AJAZZ-NativeFPS-{}\0", crate::storage::unique_suffix())
        .encode_utf16()
        .collect();
    let mut p = properties(&name);
    let mut trace = CONTROLTRACE_HANDLE { Value: 0 };
    let code = unsafe { StartTraceW(&mut trace, name.as_ptr(), p.as_mut_ptr().cast()) };
    ensure!(code == 0, "Native FPS StartTrace: Windows {code}");
    let session = Session { name: name.clone() };
    for provider in [DXGI, D3D9] {
        let code = unsafe {
            EnableTraceEx2(
                trace,
                &provider,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                5,
                0x8000000000000002,
                0,
                0,
                std::ptr::null(),
            )
        };
        ensure!(code == 0, "Native FPS EnableTrace: Windows {code}");
    }
    let mut context = Box::new(Context {
        frames: frames.clone(),
        presents: Presents::default(),
        names: HashMap::new(),
        frequency: frequency as u64,
    });
    let mut name = name;
    let mut log = EVENT_TRACE_LOGFILEW {
        LoggerName: name.as_mut_ptr(),
        Context: (&mut *context as *mut Context).cast(),
        ..Default::default()
    };
    log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME
        | PROCESS_TRACE_MODE_EVENT_RECORD
        | PROCESS_TRACE_MODE_RAW_TIMESTAMP;
    log.Anonymous2.EventRecordCallback = Some(event);
    let process = unsafe { OpenTraceW(&mut log) };
    ensure!(
        process.Value != u64::MAX,
        "Native FPS OpenTrace: {}",
        std::io::Error::last_os_error()
    );
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let stop_name = session.name.clone();
    let watcher = std::thread::spawn(move || {
        let mut flush_ok = true;
        while !done.load(Ordering::Relaxed) {
            if stop.load(Ordering::Relaxed) {
                let mut p = properties(&stop_name);
                unsafe {
                    ControlTraceW(
                        CONTROLTRACE_HANDLE { Value: 0 },
                        stop_name.as_ptr(),
                        p.as_mut_ptr().cast(),
                        EVENT_TRACE_CONTROL_STOP,
                    )
                };
                break;
            }
            // Flush only our own trace. The default ETW timer is at least one
            // second, which otherwise delays even a fast local FPS consumer.
            if flush_ok {
                let mut p = properties(&stop_name);
                let code = unsafe {
                    ControlTraceW(
                        CONTROLTRACE_HANDLE { Value: 0 },
                        stop_name.as_ptr(),
                        p.as_mut_ptr().cast(),
                        EVENT_TRACE_CONTROL_FLUSH,
                    )
                };
                if code != 0 && !done.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
                    flush_ok = false;
                    if let Ok(mut s) = frames.lock() {
                        s.native_error = Some(format!("Native FPS flush: Windows {code}"));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let result = unsafe { ProcessTrace(&process, 1, std::ptr::null(), std::ptr::null()) };
    finished.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    unsafe { CloseTrace(process) };
    ensure!(
        result == 0 || result == 1223,
        "Native FPS ProcessTrace: Windows {result}"
    );
    drop(session);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_successful_non_test_presents_count_per_swapchain() {
        let mut p = Presents::default();
        p.start(1, 2, 10, 1000, false);
        assert!(p.stop(1, 2, 0, 1000).is_none());
        p.start(1, 2, 10, 1010, true);
        assert!(p.stop(1, 2, 0, 1000).is_none());
        p.start(1, 2, 10, 1020, false);
        assert!(p.stop(1, 2, 0x887a0001, 1000).is_none());
        p.start(1, 2, 10, 1030, false);
        assert_eq!(p.stop(1, 2, 0, 1000), Some((10, 30.0)));
        p.start(1, 2, 20, 1040, false);
        assert!(p.stop(1, 2, 0, 1000).is_none());
        p.start(1, 2, 10, 1040, false);
        assert_eq!(p.stop(1, 2, 0, 1000), Some((10, 10.0)));
    }
}
