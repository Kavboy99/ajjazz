use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn unique_suffix() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

pub fn read_json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Файл превышает допустимый размер");
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn backup_directory() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?)
            .join("AjazzStudio/backups"),
    )
}

/// A failed durable backup prevents the caller from starting any device writes.
pub fn backup<T: Serialize>(directory: &Path, prefix: &str, data: &T) -> Result<PathBuf> {
    let bytes = serde_json::to_vec_pretty(data)?;
    std::fs::create_dir_all(directory)?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%f");
    let path = directory.join(format!("{prefix}-{stamp}-{}.json", unique_suffix()));
    let mut file = File::options().write(true).create_new(true).open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(path)
}

/// Serialize and sync a complete replacement before touching the previous file.
pub fn write_json_atomic<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(data)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("Missing file name")?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}.{}.tmp", unique_suffix()));
    let mut file = File::options().write(true).create_new(true).open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    // Antivirus/indexers and readers can briefly hold a Windows file without
    // delete sharing. Keep the old complete file until replacement succeeds.
    let mut attempt = 0;
    loop {
        match std::fs::rename(&temp, path) {
            Ok(()) => break,
            Err(e)
                if cfg!(windows)
                    && matches!(e.raw_os_error(), Some(5 | 32 | 33))
                    && attempt < 6 =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 * attempt));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(e.into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn temporary_windows_reader_does_not_lose_an_autosave() {
        use std::os::windows::fs::OpenOptionsExt;
        let p = std::env::temp_dir().join(format!("ajazz-locked-{}.json", unique_suffix()));
        write_json_atomic(&p, &1).unwrap();
        let reader = File::options().read(true).share_mode(1).open(&p).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(110));
            drop(reader);
        });
        write_json_atomic(&p, &2).unwrap();
        release.join().unwrap();
        assert_eq!(read_json::<u8>(&p, 16).unwrap(), 2);
        std::fs::remove_file(p).unwrap();
    }
    #[test]
    fn atomic_replacement_reloads_and_read_size_is_bounded() {
        let path = std::env::temp_dir().join(format!(
            "ajazz-storage-test-{}.json",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        write_json_atomic(&path, &vec![1, 2, 3]).unwrap();
        write_json_atomic(&path, &vec![4, 5]).unwrap();
        assert_eq!(read_json::<Vec<u8>>(&path, 64).unwrap(), vec![4, 5]);
        assert!(read_json::<Vec<u8>>(&path, 3).is_err());
    }
}
