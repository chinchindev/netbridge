use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const REPOSITORY: &str = "https://dl.google.com/android/repository/repository2-3.xml";
const DOWNLOAD_ROOT: &str = "https://dl.google.com/android/repository/";
const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
const FILES: [&str; 6] = [
    "adb.exe",
    "AdbWinApi.dll",
    "AdbWinUsbApi.dll",
    "libwinpthread-1.dll",
    "source.properties",
    "NOTICE.txt",
];
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn unique() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Version(pub [u32; 3]);
impl FromStr for Version {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        let parts: Vec<_> = value.split('.').collect();
        if parts.len() != 3
            || parts
                .iter()
                .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(invalid("Expected a stable major.minor.patch version"));
        }
        Ok(Self([
            parts[0].parse().map_err(invalid_parse)?,
            parts[1].parse().map_err(invalid_parse)?,
            parts[2].parse().map_err(invalid_parse)?,
        ]))
    }
}
fn invalid_parse(error: std::num::ParseIntError) -> io::Error {
    invalid(error.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Installation {
    pub version: String,
    pub path: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Pointer {
    directory: String,
}

pub fn bundled_bytes() -> &'static [u8] {
    include_bytes!("../assets/adb/platform-tools-windows.zip")
}
pub fn bundled_release() -> io::Result<Release> {
    serde_json::from_str(include_str!("../assets/adb/bundled.json")).map_err(io::Error::other)
}
pub fn parse_repository(xml: &str) -> io::Result<Release> {
    let doc = roxmltree::Document::parse(xml).map_err(io::Error::other)?;
    let mut releases = Vec::new();
    for package in doc.descendants().filter(|n| {
        n.has_tag_name("remotePackage") && n.attribute("path") == Some("platform-tools")
    }) {
        if !package
            .children()
            .any(|n| n.has_tag_name("channelRef") && n.attribute("ref") == Some("channel-0"))
        {
            continue;
        }
        let revision = package
            .children()
            .find(|n| n.has_tag_name("revision"))
            .ok_or_else(|| invalid("Missing version"))?;
        let part = |name| {
            revision
                .children()
                .find(|n| n.has_tag_name(name))
                .and_then(|n| n.text())
                .unwrap_or("0")
        };
        if revision.children().any(|n| n.has_tag_name("preview")) {
            continue;
        }
        let version = format!("{}.{}.{}", part("major"), part("minor"), part("micro"));
        let numeric = Version::from_str(&version)?;
        for archive in package.descendants().filter(|n| n.has_tag_name("archive")) {
            if !archive
                .children()
                .any(|n| n.has_tag_name("host-os") && n.text() == Some("windows"))
            {
                continue;
            }
            let value = |name| {
                archive
                    .descendants()
                    .find(|n| n.has_tag_name(name))
                    .and_then(|n| n.text())
                    .unwrap_or("")
            };
            let filename = value("url");
            if filename != format!("platform-tools_r{version}-win.zip") {
                return Err(invalid("Unexpected Google archive URL"));
            }
            let checksum = archive
                .descendants()
                .find(|n| n.has_tag_name("checksum"))
                .ok_or_else(|| invalid("Missing checksum"))?;
            if checksum.attribute("type") != Some("sha1") {
                return Err(invalid("Unsupported archive checksum"));
            }
            let release = Release {
                version: version.clone(),
                url: format!("{DOWNLOAD_ROOT}{filename}"),
                sha1: value("checksum").to_ascii_lowercase(),
                size: value("size")
                    .parse()
                    .map_err(|_| invalid("Invalid archive size"))?,
            };
            validate_release(&release)?;
            releases.push((numeric, release));
        }
    }
    releases
        .into_iter()
        .max_by_key(|(version, _)| *version)
        .map(|(_, release)| release)
        .ok_or_else(|| invalid("No stable Windows Platform-Tools package"))
}

fn validate_release(release: &Release) -> io::Result<()> {
    Version::from_str(&release.version)?;
    if release.url != format!("{DOWNLOAD_ROOT}platform-tools_r{}-win.zip", release.version)
        || release.size == 0
        || release.size > MAX_ARCHIVE
        || release.sha1.len() != 40
        || !release.sha1.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid("Invalid release metadata"));
    }
    Ok(())
}

pub fn verify_archive(release: &Release, bytes: &[u8]) -> io::Result<()> {
    validate_release(release)?;
    if bytes.len() as u64 != release.size
        || format!("{:x}", Sha1::digest(bytes)) != release.sha1.to_ascii_lowercase()
    {
        return Err(invalid(
            "Archive size/checksum mismatch; current ADB was preserved",
        ));
    }
    Ok(())
}

pub fn extract_adb(bytes: &[u8], destination: &Path) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(io::Error::other)?;
        if file.enclosed_name().is_none() || file.name().contains('\\') || file.is_symlink() {
            return Err(invalid("Unsafe ZIP entry"));
        }
    }
    // Validate the full required set before writing anything; ignore unrelated SDK executables.
    for name in FILES {
        let full = format!("platform-tools/{name}");
        if archive.file_names().filter(|p| *p == full).count() != 1 {
            return Err(invalid(format!("Missing/duplicate {name}")));
        }
        let file = archive.by_name(&full).map_err(io::Error::other)?;
        if file.size() == 0 || file.size() > 32 * 1024 * 1024 {
            return Err(invalid(format!("Invalid size for {name}")));
        }
    }
    fs::create_dir_all(destination)?;
    for name in FILES {
        let mut file = archive
            .by_name(&format!("platform-tools/{name}"))
            .map_err(io::Error::other)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination.join(name))?;
        io::copy(&mut file, &mut output)?;
        output.sync_all()?;
    }
    Ok(())
}

fn installed_at(path: PathBuf) -> Option<Installation> {
    if !FILES.iter().all(|name| path.join(name).is_file()) {
        return None;
    }
    let props = fs::read_to_string(path.join("source.properties")).ok()?;
    let version = props
        .lines()
        .find_map(|line| line.strip_prefix("Pkg.Revision="))?
        .trim();
    Version::from_str(version).ok()?;
    Some(Installation {
        version: version.into(),
        path: path.join("adb.exe"),
    })
}

fn current_at(root: &Path) -> Option<Installation> {
    let pointer: Pointer =
        serde_json::from_slice(&fs::read(root.join("current.json")).ok()?).ok()?;
    if pointer.directory.is_empty()
        || pointer.directory.starts_with('.')
        || !pointer
            .directory
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return None;
    }
    installed_at(root.join(pointer.directory).join("platform-tools"))
}

pub fn install_release_at(
    release: &Release,
    bytes: &[u8],
    root: &Path,
) -> io::Result<Installation> {
    verify_archive(release, bytes)?;
    fs::create_dir_all(root)?;
    let directory = format!("{}-{}", release.version, unique());
    let target = root.join(&directory).join("platform-tools");
    extract_adb(bytes, &target)?;
    let installed = installed_at(target).ok_or_else(|| invalid("Incomplete ADB installation"))?;
    if installed.version != release.version {
        return Err(invalid("Archive version differs from metadata"));
    }
    let output = crate::devices::command_exact(
        installed
            .path
            .to_str()
            .ok_or_else(|| invalid("Invalid ADB path"))?,
        &["version"],
    )?;
    if !output.contains(&format!("Version {}", release.version)) {
        return Err(invalid("Extracted ADB version could not be verified"));
    }
    let next = root.join(format!("current-{}.json", unique()));
    fs::write(
        &next,
        serde_json::to_vec_pretty(&Pointer { directory }).map_err(io::Error::other)?,
    )?;
    let current = root.join("current.json");
    let backup = root.join(format!("current-{}.bak", unique()));
    if current.exists() {
        fs::rename(&current, &backup)?;
    }
    if let Err(error) = fs::rename(&next, &current) {
        if backup.exists() {
            let _ = fs::rename(&backup, &current);
        }
        return Err(error);
    }
    Ok(installed)
}

fn roots() -> io::Result<[PathBuf; 2]> {
    let exe = std::env::current_exe()?;
    Ok([
        exe.parent()
            .ok_or_else(|| invalid("Missing executable directory"))?
            .join("adb"),
        crate::config::data_dir().join("adb"),
    ])
}

pub fn installed() -> io::Result<Option<Installation>> {
    Ok(roots()?
        .iter()
        .filter_map(|root| current_at(root))
        .max_by_key(|i| Version::from_str(&i.version).ok()))
}

fn writable_root() -> io::Result<PathBuf> {
    let mut last = io::Error::other("No writable ADB directory");
    for root in roots()? {
        let attempt = (|| {
            fs::create_dir_all(&root)?;
            // A unique directory doubles as a harmless write probe, retained for diagnostics.
            fs::create_dir(root.join(format!("probe-{}", unique())))?;
            Ok::<_, io::Error>(root.clone())
        })();
        match attempt {
            Ok(path) => return Ok(path),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(windows)]
struct InstallLock(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl InstallLock {
    fn acquire() -> io::Result<Self> {
        use windows_sys::Win32::{Foundation::*, System::Threading::*};
        let name: Vec<u16> = "Local\\NetBridge.AdbInstaller"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let status = WaitForSingleObject(handle, 120_000);
            if status != WAIT_OBJECT_0 && status != WAIT_ABANDONED {
                CloseHandle(handle);
                return Err(io::Error::other("Another ADB operation is still running"));
            }
            Ok(Self(handle))
        }
    }
}
#[cfg(windows)]
impl Drop for InstallLock {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.0);
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
struct InstallLock;
#[cfg(not(windows))]
impl InstallLock {
    fn acquire() -> io::Result<Self> {
        Ok(Self)
    }
}

pub fn ensure_bundled() -> io::Result<Installation> {
    let release = bundled_release()?;
    let suitable = || {
        installed().ok().flatten().filter(|i| {
            Version::from_str(&i.version).ok() >= Version::from_str(&release.version).ok()
        })
    };
    if let Some(current) = suitable() {
        return Ok(current);
    }
    let _lock = InstallLock::acquire()?;
    if let Some(current) = suitable() {
        return Ok(current);
    }
    install_release_at(&release, bundled_bytes(), &writable_root()?)
}

pub fn resolve(configured: &str) -> io::Result<PathBuf> {
    let value = configured.trim();
    if !value.is_empty()
        && !value.eq_ignore_ascii_case("auto")
        && value != "adb.exe"
        && Path::new(value).is_file()
    {
        return Ok(PathBuf::from(value));
    }
    Ok(ensure_bundled()?.path)
}

pub fn check_update() -> io::Result<Release> {
    let bytes = crate::http_client::download(REPOSITORY, 8 * 1024 * 1024, &|_| {})?;
    parse_repository(std::str::from_utf8(&bytes).map_err(io::Error::other)?)
}

pub fn update(progress: &dyn Fn(String)) -> io::Result<Installation> {
    progress("Đang kiểm tra phiên bản ổn định trên Google…".into());
    let release = check_update()?;
    let _lock = InstallLock::acquire()?;
    if let Some(current) = installed()? {
        if Version::from_str(&current.version)? >= Version::from_str(&release.version)? {
            progress(format!("ADB {} đã là bản mới nhất.", current.version));
            return Ok(current);
        }
    }
    progress(format!(
        "Đang tải ADB {} ({:.1} MB)…",
        release.version,
        release.size as f64 / 1_000_000.0
    ));
    let mut response = crate::http_client::builder(std::time::Duration::from_secs(90))
        .https_only(true)
        .build()
        .map_err(io::Error::other)?
        .get(&release.url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(release.size + 1)
        .read_to_end(&mut bytes)?;
    progress("Đang xác minh checksum và cài vào thư mục phiên bản mới…".into());
    install_release_at(&release, &bytes, &writable_root()?)
}
