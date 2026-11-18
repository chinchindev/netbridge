use serde::Serialize;
use std::{
    io,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub serial: String,
    pub model: String,
    pub state: String,
    pub usb: bool,
}

pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            }
            Some(Device {
                serial: parts[0].into(),
                state: parts[1].into(),
                model: parts
                    .iter()
                    .find_map(|s| s.strip_prefix("model:"))
                    .unwrap_or("Android")
                    .replace('_', " "),
                usb: !parts[0].contains(':') && !parts[0].starts_with("emulator-"),
            })
        })
        .collect()
}

pub fn command(adb: &str, args: &[&str]) -> io::Result<String> {
    let path = crate::adb_manager::resolve(adb)?;
    command_exact(
        path.to_str()
            .ok_or_else(|| io::Error::other("Invalid ADB path"))?,
        args,
    )
}
pub fn command_exact(adb: &str, args: &[&str]) -> io::Result<String> {
    let mut command = Command::new(adb);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() > Duration::from_secs(12) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "ADB timed out"));
        }
        thread::sleep(Duration::from_millis(80));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
pub fn list(adb: &str) -> io::Result<Vec<Device>> {
    Ok(parse_devices(&command(adb, &["devices", "-l"])?))
}
pub fn usb_ethernet_enabled(adb: &str, serial: &str) -> io::Result<bool> {
    Ok(usb_network_enabled(&usb_functions(adb, serial)?))
}
pub fn usb_network_enabled(functions: &str) -> bool {
    functions
        .split(',')
        .any(|part| matches!(part.trim(), "rndis" | "ncm"))
}
pub fn usb_functions(adb: &str, serial: &str) -> io::Result<String> {
    if !list(adb)?
        .iter()
        .any(|device| device.serial == serial && device.usb && device.state == "device")
    {
        return Err(io::Error::other("Select an authorized physical USB device"));
    }
    Ok(
        command(adb, &["-s", serial, "shell", "getprop", "sys.usb.state"])?
            .trim()
            .to_owned(),
    )
}
pub fn can_restore_usb_functions(previous: &str) -> bool {
    !previous.is_empty()
        && previous.split(',').all(|value| {
            matches!(
                value,
                "adb"
                    | "mtp"
                    | "ptp"
                    | "midi"
                    | "rndis"
                    | "ncm"
                    | "accessory"
                    | "audio_source"
                    | "none"
            )
        })
}
pub fn restore_usb_functions(adb: &str, serial: &str, previous: &str) -> io::Result<()> {
    if !can_restore_usb_functions(previous) {
        return Err(io::Error::other(
            "USB mode could not be restored automatically",
        ));
    }
    let _ = command(
        adb,
        &[
            "-s",
            serial,
            "shell",
            "svc",
            "usb",
            "setFunctions",
            previous,
        ],
    );
    let mut expected: Vec<_> = previous.split(',').collect();
    expected.sort_unstable();
    for _ in 0..8 {
        if let Ok(current) = command(adb, &["-s", serial, "shell", "getprop", "sys.usb.state"]) {
            let mut actual: Vec<_> = current.trim().split(',').collect();
            actual.sort_unstable();
            if actual == expected {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(400));
    }
    Err(io::Error::other("Cannot confirm the previous USB mode"))
}
pub fn set_tether(adb: &str, serial: &str, connect: bool) -> io::Result<String> {
    let devices = list(adb)?;
    if !devices
        .iter()
        .any(|d| d.serial == serial && d.usb && d.state == "device")
    {
        return Err(io::Error::other(
            "Select an authorized USB device, not a TCP/emulator connection",
        ));
    }
    let mut args = vec!["-s", serial, "shell", "svc", "usb", "setFunctions"];
    if connect {
        args.push("rndis");
    }
    // Re-enumeration can interrupt ADB before the command acknowledges. Verify actual state below.
    let command_result = command(adb, &args);
    for _ in 0..8 {
        thread::sleep(Duration::from_millis(500));
        if let Ok(state) = command(adb, &["-s", serial, "shell", "getprop", "sys.usb.state"]) {
            let expected_network = if connect {
                state.split(',').any(|part| part.trim() == "rndis")
            } else {
                !usb_network_enabled(&state)
            };
            if expected_network && state.contains("adb") {
                let state = state.trim();
                return Ok(format!(
                    "{}: {state}. Bấm Làm mới để kiểm tra card mạng.",
                    if connect {
                        "Đã bật USB Ethernet"
                    } else {
                        "Đã ngắt USB Ethernet"
                    }
                ));
            }
        }
    }
    Err(io::Error::other(format!(
        "Chưa xác minh được chế độ USB. Kiểm tra cáp/ADB. {}",
        command_result
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_usb_network_and_unauthorized_devices() {
        let d=parse_devices("List of devices attached\nDEMO-USB-001 device product:Android_USB model:Android_USB transport_id:1\n192.0.2.117:5555 device model:AndroidDevice\nABC unauthorized\n");
        assert_eq!(d.len(), 3);
        assert!(d[0].usb);
        assert!(!d[1].usb);
        assert_eq!(d[2].state, "unauthorized");
    }
}
