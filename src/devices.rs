use serde::Serialize;
use std::{
    io::{self, Read},
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
    #[serde(rename = "adapterIds")]
    pub adapter_ids: Vec<String>,
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
                adapter_ids: Vec::new(),
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
    let output = command_output(adb, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn command_output(adb: &str, args: &[&str]) -> io::Result<std::process::Output> {
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
    // Both dumpsys and a Settings UI hierarchy can exceed the Windows pipe capacity.
    // Drain both streams while polling the child so the timeout also covers large replies.
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let output = thread::scope(|scope| -> io::Result<std::process::Output> {
        let out = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes)?;
            Ok::<_, io::Error>(bytes)
        });
        let err = scope.spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes)?;
            Ok::<_, io::Error>(bytes)
        });
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(12) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "ADB timed out"));
            }
            thread::sleep(Duration::from_millis(80));
        };
        let stdout = out
            .join()
            .map_err(|_| io::Error::other("ADB stdout unavailable"))??;
        let stderr = err
            .join()
            .map_err(|_| io::Error::other("ADB stderr unavailable"))??;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(output)
}
pub fn list(adb: &str) -> io::Result<Vec<Device>> {
    Ok(parse_devices(&command(adb, &["devices", "-l"])?))
}
#[path = "samsung_tether.rs"]
mod samsung_tether;

pub struct UsbState {
    pub functions: String,
    pub enabled: bool,
    samsung: bool,
}
impl UsbState {
    pub fn service_verified(&self) -> bool {
        self.samsung
    }
}
fn authorized_device(adb: &str, serial: &str) -> io::Result<Device> {
    list(adb)?
        .into_iter()
        .find(|d| d.serial == serial && d.usb && d.state == "device")
        .ok_or_else(|| io::Error::other("Select an authorized physical USB device"))
}
fn is_s7_oreo(adb: &str, phone: &Device) -> io::Result<bool> {
    if !phone
        .model
        .replace(['_', ' '], "-")
        .eq_ignore_ascii_case("SM-G930U")
    {
        return Ok(false);
    }
    Ok(command(
        adb,
        &[
            "-s",
            &phone.serial,
            "shell",
            "getprop",
            "ro.build.version.sdk",
        ],
    )?
    .trim()
        == "26")
}
pub fn usb_state(adb: &str, serial: &str) -> io::Result<UsbState> {
    let phone = authorized_device(adb, serial)?;
    let functions = read_functions(adb, serial)?;
    let samsung = is_s7_oreo(adb, &phone)?;
    let enabled = if samsung {
        samsung_tether::active(adb, serial)?
    } else {
        usb_network_enabled(&functions)
    };
    Ok(UsbState {
        functions,
        enabled,
        samsung,
    })
}
pub fn usb_ethernet_enabled(adb: &str, serial: &str) -> io::Result<bool> {
    Ok(usb_state(adb, serial)?.enabled)
}
pub fn restore_usb_state(adb: &str, serial: &str, previous: &UsbState) -> io::Result<()> {
    if previous.samsung {
        if samsung_tether::active(adb, serial)? != previous.enabled {
            samsung_tether::set(adb, serial, previous.enabled)?;
        }
        if read_functions(adb, serial)? == previous.functions {
            return Ok(());
        }
    }
    restore_usb_functions(adb, serial, &previous.functions)?;
    if previous.samsung && samsung_tether::active(adb, serial)? != previous.enabled {
        return Err(io::Error::other(
            "Chưa phục hồi được trạng thái chia sẻ mạng ban đầu của Samsung.",
        ));
    }
    Ok(())
}
pub fn usb_network_enabled(functions: &str) -> bool {
    functions
        .split(',')
        .any(|part| matches!(part.trim(), "rndis" | "ncm"))
}
pub fn usb_functions(adb: &str, serial: &str) -> io::Result<String> {
    authorized_device(adb, serial)?;
    read_functions(adb, serial)
}
fn read_functions(adb: &str, serial: &str) -> io::Result<String> {
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
fn usb_configuration_args<'a>(
    stdout: &str,
    stderr: &str,
    functions: Option<&'a str>,
    unlock_data: bool,
) -> io::Result<Vec<&'a str>> {
    let supports = |name| {
        stdout
            .split_whitespace()
            .chain(stderr.split_whitespace())
            .any(|word| word == name)
    };
    let command = if supports("setFunctions") {
        "setFunctions"
    } else if supports("setFunction") {
        "setFunction"
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Thiết bị không hỗ trợ lệnh USB cần dùng qua ADB. Bật Chia sẻ kết nối USB trên điện thoại.",
        ));
    };
    let mut args = vec!["shell", "svc", "usb", command];
    if let Some(functions) = functions {
        args.push(functions);
        if command == "setFunction" {
            args.push(if unlock_data { "true" } else { "false" });
        }
    }
    Ok(args)
}
fn set_usb_functions(
    adb: &str,
    serial: &str,
    functions: Option<&str>,
    unlock_data: bool,
) -> io::Result<String> {
    let path = crate::adb_manager::resolve(adb)?;
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("Invalid ADB path"))?;
    // Android 8 and earlier print singular setFunction usage to stderr with exit code 0.
    let help = command_output(path, &["-s", serial, "shell", "svc", "usb"])?;
    let selected = usb_configuration_args(
        &String::from_utf8_lossy(&help.stdout),
        &String::from_utf8_lossy(&help.stderr),
        functions,
        unlock_data,
    )?;
    let mut args = vec!["-s", serial];
    args.extend(selected);
    command_exact(path, &args)
}
pub fn restore_usb_functions(adb: &str, serial: &str, previous: &str) -> io::Result<()> {
    if !can_restore_usb_functions(previous) {
        return Err(io::Error::other(
            "USB mode could not be restored automatically",
        ));
    }
    // Restore the function without granting legacy MTP/PTP data access during rollback.
    let _ = set_usb_functions(adb, serial, Some(previous), false);
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
    let phone = authorized_device(adb, serial)?;
    if is_s7_oreo(adb, &phone)? {
        samsung_tether::set(adb, serial, connect)?;
        return Ok(if connect {
            "Samsung đã bật chia sẻ mạng qua USB."
        } else {
            "Samsung đã ngắt chia sẻ mạng qua USB."
        }
        .into());
    }
    // Re-enumeration can interrupt ADB before the command acknowledges. Verify actual state below.
    let command_result = set_usb_functions(adb, serial, connect.then_some("rndis"), connect);
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
    const LEGACY_HELP: &str = "Control Usb state\nusage: svc usb setFunction [function] [usbDataUnlocked=false]\nsvc usb getFunction\n";
    const MODERN_HELP: &str = "usage: svc usb setFunctions [function]\nsvc usb setScreenUnlockedFunctions [function]\nsvc usb getFunctions\n";
    #[test]
    fn legacy_usb_help_on_stderr_selects_singular_command_and_unlocks_data() {
        assert_eq!(
            usb_configuration_args("", LEGACY_HELP, Some("rndis"), true).unwrap(),
            vec!["shell", "svc", "usb", "setFunction", "rndis", "true"]
        );
    }
    #[test]
    fn modern_usb_help_keeps_plural_command_without_legacy_boolean() {
        assert_eq!(
            usb_configuration_args(MODERN_HELP, "", Some("rndis"), true).unwrap(),
            vec!["shell", "svc", "usb", "setFunctions", "rndis"]
        );
    }
    #[test]
    fn usb_disconnect_and_restore_use_the_selected_command() {
        for (help, command) in [(LEGACY_HELP, "setFunction"), (MODERN_HELP, "setFunctions")] {
            assert_eq!(
                usb_configuration_args("", help, None, false).unwrap(),
                vec!["shell", "svc", "usb", command]
            );
            let mut expected = vec!["shell", "svc", "usb", command, "mtp,adb"];
            if command == "setFunction" {
                expected.push("false");
            }
            assert_eq!(
                usb_configuration_args("", help, Some("mtp,adb"), false).unwrap(),
                expected
            );
        }
    }
    #[test]
    fn unknown_usb_help_does_not_guess_a_mutating_command() {
        for help in [
            "",
            "error: device unauthorized",
            "svc usb getFunctions",
            "svc usb setScreenUnlockedFunctions",
        ] {
            assert!(usb_configuration_args(help, "", Some("rndis"), true).is_err());
        }
    }
    #[test]
    fn distinguishes_usb_network_and_unauthorized_devices() {
        let d=parse_devices("List of devices attached\nDEMO-USB-001 device product:Android_USB model:Android_USB transport_id:1\n192.0.2.117:5555 device model:AndroidDevice\nABC unauthorized\n");
        assert_eq!(d.len(), 3);
        assert!(d[0].usb);
        assert!(!d[1].usb);
        assert_eq!(d[2].state, "unauthorized");
    }
}
