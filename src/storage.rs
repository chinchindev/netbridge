use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub fn stamp() -> String {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}
pub fn read_limited(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::other("Settings file exceeds size limit"));
    }
    Ok(bytes)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = parent.join(format!(".settings-{}.tmp", stamp()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // Rename replaces the destination only after the complete staging file is flushed.
    // Retain failed staging files for recovery; never remove an unrelated file.
    fs::rename(stage, path)
}
pub fn preserve_file(path: &Path) -> io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing settings directory"))?;
    let recovery = parent.join("settings-backups").join(stamp());
    fs::create_dir_all(&recovery)?;
    let copy = recovery.join(
        path.file_name()
            .ok_or_else(|| io::Error::other("Missing filename"))?,
    );
    fs::copy(path, &copy)?;
    Ok(Some(copy))
}
