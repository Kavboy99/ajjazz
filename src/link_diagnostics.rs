//! Passive monitoring of the vendor control interface; never reads normal keyboard input.
use crate::{
    hardware,
    protocol::{KeyboardInfo, keyboard_link_event},
    storage,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Serialize)]
pub struct LinkEvent {
    elapsed_ms: u64,
    event: String,
    raw: Vec<u8>,
}
#[derive(Serialize)]
pub struct LinkReport {
    schema: u32,
    started_utc: String,
    duration_seconds: u16,
    initial_info: Option<KeyboardInfo>,
    initial_error: Option<String>,
    events: Vec<LinkEvent>,
    end_error: Option<String>,
}
#[cfg(windows)]
pub fn watch(seconds: u16, output: &Path) -> Result<LinkReport> {
    ensure!(
        (1..=600).contains(&seconds),
        "Duration must be 1–600 seconds"
    );
    let device = hardware::Keyboard::open()?;
    let initial = device.read_info();
    let (initial_info, initial_error) = match initial {
        Ok(info) => (Some(info), None),
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    let mut report = LinkReport {
        schema: 1,
        started_utc: chrono::Utc::now().to_rfc3339(),
        duration_seconds: seconds,
        initial_info,
        initial_error,
        events: Vec::new(),
        end_error: None,
    };
    storage::write_json_atomic(output, &report)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(u64::from(seconds)) {
        match device.listen_control_report(200) {
            Ok(raw) if !raw.is_empty() => {
                let event = keyboard_link_event(&raw)
                    .map(|v| format!("{v:?}"))
                    .unwrap_or_else(|| "OtherControlReport".into());
                report.events.push(LinkEvent {
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    event,
                    raw,
                });
                storage::write_json_atomic(output, &report)?;
                if report.events.len() >= 4096 {
                    report.end_error = Some("Control report limit reached".into());
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => {
                report.end_error = Some(format!("{error:#}"));
                break;
            }
        }
    }
    storage::write_json_atomic(output, &report)?;
    Ok(report)
}
#[cfg(windows)]
pub fn reapply_current_general(output: &Path) -> Result<hardware::Snapshot> {
    hardware::Keyboard::open()?.reapply_current_general(output)
}
#[cfg(not(windows))]
pub fn watch(_: u16, _: &Path) -> Result<LinkReport> {
    anyhow::bail!("Windows only")
}
