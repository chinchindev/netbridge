//! Android 8 on SM-G930U exposes the USB gadget and the tethering service separately.
use super::command;
use std::{io, thread, time::Duration};

fn tethered(dump: &str) -> Option<bool> {
    let mut section = dump.split_once("Tether state:")?.1.lines();
    let mut found = false;
    for line in &mut section {
        if line.trim().is_empty() {
            continue;
        }
        let Some((interface, rest)) = line.trim().split_once(" - ") else {
            break;
        };
        let usb = ["rndis", "usb"].iter().any(|prefix| {
            interface
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|v| v.is_ascii_digit()))
        });
        if !usb {
            continue;
        }
        let (state, err) = rest.split_once(" - ")?;
        let (_, value) = err.split_once('=')?;
        if err.split('=').next()?.trim() != "lastError" || value.trim() != "0" {
            return None;
        }
        match state {
            "TetheredState" => found = true,
            "AvailableState" | "UnavailableState" | "LocalOnlyState" => {}
            _ => return None,
        }
    }
    Some(found)
}
pub(super) fn active(adb: &str, serial: &str) -> io::Result<bool> {
    let output = command(adb, &["-s", serial, "shell", "dumpsys", "connectivity"])?;
    tethered(&output)
        .ok_or_else(|| io::Error::other("Chưa đọc được trạng thái chia sẻ USB của Samsung."))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Switch {
    checked: bool,
    x: u32,
    y: u32,
}
fn center(bounds: &str) -> Option<(u32, u32)> {
    let (start, end) = bounds
        .strip_prefix('[')?
        .strip_suffix(']')?
        .split_once("][")?;
    let point = |s: &str| {
        let (x, y) = s.split_once(',')?;
        Some((x.parse::<u32>().ok()?, y.parse::<u32>().ok()?))
    };
    let ((l, t), (r, b)) = (point(start)?, point(end)?);
    if l >= r || t >= b {
        return None;
    }
    Some((l + (r - l) / 2, t + (b - t) / 2))
}
fn usb_switch(dump: &str) -> Option<Switch> {
    let start = dump.find("<hierarchy")?;
    let end = dump[start..].find("</hierarchy>")? + start + "</hierarchy>".len();
    let doc = roxmltree::Document::parse(&dump[start..end]).ok()?;
    let settings =
        |n: roxmltree::Node<'_, '_>| n.attribute("package") == Some("com.android.settings");
    let mut labels = doc.descendants().filter(|n| {
        settings(*n)
            && n.attribute("text").is_some_and(|t| {
                [
                    "USB tethering",
                    "Chia sẻ kết nối USB",
                    "Chia sẻ kết nối qua USB",
                    "Kết nối USB internet",
                ]
                .iter()
                .any(|name| t.eq_ignore_ascii_case(name))
            })
    });
    let label = labels.next()?;
    if labels.next().is_some() {
        return None;
    }
    // Only the nearest clickable preference row may own the switch.
    let row = label
        .ancestors()
        .find(|n| settings(*n) && n.attribute("clickable") == Some("true"))?;
    let mut checks = row
        .descendants()
        .filter(|n| settings(*n) && n.attribute("checkable") == Some("true"));
    let sw = checks.next()?;
    if checks.next().is_some()
        || sw.attribute("resource-id") != Some("android:id/switch_widget")
        || sw.attribute("class") != Some("android.widget.Switch")
        || sw.attribute("enabled") != Some("true")
    {
        return None;
    }
    let checked = sw.attribute("checked")?.parse::<bool>().ok()?;
    let (x, y) = center(sw.attribute("bounds")?)?;
    Some(Switch { checked, x, y })
}
pub(super) fn set(adb: &str, serial: &str, want: bool) -> io::Result<()> {
    if active(adb, serial)? == want {
        return Ok(());
    }
    command(
        adb,
        &["-s", serial, "shell", "input", "keyevent", "KEYCODE_WAKEUP"],
    )?;
    let policy = command(adb, &["-s", serial, "shell", "dumpsys", "window", "policy"])?;
    if policy.lines().any(|line| line.trim() == "showing=true") {
        return Err(io::Error::other(
            "Mở khóa màn hình S7 rồi bấm kết nối lại để bật hoặc tắt chia sẻ USB.",
        ));
    }
    command(
        adb,
        &[
            "-s",
            serial,
            "shell",
            "am",
            "start",
            "-n",
            "'com.android.settings/.Settings$TetherSettingsActivity'",
        ],
    )
    .map_err(|e| {
        io::Error::other(format!(
            "Không mở được Cài đặt chia sẻ USB của Samsung: {e}"
        ))
    })?;
    let mut control = None;
    for _ in 0..3 {
        let xml = command(
            adb,
            &["-s", serial, "exec-out", "uiautomator", "dump", "/dev/tty"],
        )?;
        control = usb_switch(&xml);
        if control.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(350));
    }
    let control = control.ok_or_else(|| {
        io::Error::other("Mở khóa S7 và kiểm tra công tắc Chia sẻ kết nối USB trong Cài đặt.")
    })?;
    if control.checked != want {
        // Switching USB can disconnect ADB before input returns. Verify the service afterward.
        let tap = command(
            adb,
            &[
                "-s",
                serial,
                "shell",
                "input",
                "tap",
                &control.x.to_string(),
                &control.y.to_string(),
            ],
        );
        if tap.is_err() && active(adb, serial).is_ok_and(|s| s == want) {
            return Ok(());
        }
    }
    for _ in 0..16 {
        thread::sleep(Duration::from_millis(500));
        if active(adb, serial).is_ok_and(|state| state == want) {
            return Ok(());
        }
    }
    Err(io::Error::other("Samsung chưa xác nhận thay đổi chia sẻ mạng. Kiểm tra công tắc USB tethering trên điện thoại."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_usb_tether_service_not_other_interfaces_or_gadget() {
        let state = |s: &str| {
            tethered(&format!(
                "Tethering:\n  Tether state:\n{s}\n  Upstream wanted: false"
            ))
        };
        assert_eq!(state(""), Some(false));
        assert_eq!(state("    wlan0 - TetheredState - lastError = 0\n    rndis0 - AvailableState - lastError = 0"),Some(false));
        assert_eq!(
            state("    rndis0 - TetheredState - lastError = 0"),
            Some(true)
        );
        assert_eq!(state("    rndis0 - TetheredState - lastError = 5"), None);
        assert_eq!(tethered("Permission denied"), None);
    }
    #[test]
    fn only_taps_the_unique_enabled_usb_switch_in_settings() {
        let xml = r#"<hierarchy><node package="com.android.settings" clickable="true"><node text="USB tethering" package="com.android.settings"/><node package="com.android.settings" resource-id="android:id/switch_widget" class="android.widget.Switch" checkable="true" enabled="true" checked="false" bounds="[100,200][300,240]"/></node></hierarchy>dumped"#;
        assert_eq!(
            usb_switch(xml),
            Some(Switch {
                checked: false,
                x: 200,
                y: 220
            })
        );
        for bad in [
            xml.replace("USB tethering", "Bluetooth tethering"),
            xml.replace("com.android.settings", "other"),
            xml.replace("enabled=\"true\"", "enabled=\"false\""),
            xml.replace("100,200", "400,200"),
        ] {
            assert!(usb_switch(&bad).is_none());
        }
    }
    #[test]
    #[ignore = "requires authorized SM-G930U and explicit operator request"]
    fn verify_enable_on_connected_s7() {
        let adb = std::env::var("NETBRIDGE_TEST_ADB").unwrap();
        let phones = super::super::list(&adb).unwrap();
        let s7 = phones
            .iter()
            .find(|p| p.model == "SM G930U" && p.state == "device" && p.usb)
            .unwrap();
        assert!(super::super::is_s7_oreo(&adb, s7).unwrap());
        let before = active(&adb, &s7.serial).unwrap();
        super::super::set_tether(&adb, &s7.serial, true).unwrap();
        assert!(super::super::usb_state(&adb, &s7.serial).unwrap().enabled);
        println!("SM G930U tethered before={before}, after=true");
    }
}
