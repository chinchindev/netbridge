//! Map a present network interface GUID to the physical USB device's instance serial.
use std::collections::HashMap;

#[derive(Default)]
pub struct UsbIdentity(HashMap<String, String>);

impl UsbIdentity {
    pub fn serial(&self, adapter_id: &str) -> Option<&str> {
        self.0
            .get(&adapter_id.to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[cfg(not(windows))]
pub fn read() -> UsbIdentity {
    UsbIdentity::default()
}

#[cfg(any(windows, test))]
fn physical_serial(instance: &str) -> Option<&str> {
    let mut parts = instance.split('\\');
    let (bus, hardware, serial) = (parts.next()?, parts.next()?, parts.next()?);
    let mut ids = hardware.split('&');
    if bus.eq_ignore_ascii_case("USB")
        && ids.next()?.to_ascii_uppercase().starts_with("VID_")
        && ids.next()?.to_ascii_uppercase().starts_with("PID_")
        && ids.next().is_none()
        && parts.next().is_none()
        && !serial.is_empty()
    {
        Some(serial)
    } else {
        None
    }
}

#[cfg(windows)]
pub fn read() -> UsbIdentity {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Devices::DeviceAndDriverInstallation::*,
        Foundation::{ERROR_SUCCESS, INVALID_HANDLE_VALUE},
        System::Registry::{RegCloseKey, RegGetValueW, KEY_QUERY_VALUE, RRF_RT_REG_SZ},
    };
    let mut identities = UsbIdentity::default();
    // All handles are opened read-only. Enumerate only present USB network nodes.
    unsafe {
        let set = SetupDiGetClassDevsW(
            &GUID_DEVCLASS_NET,
            windows_sys::core::w!("USB"),
            ptr::null_mut(),
            DIGCF_PRESENT,
        );
        if set == -1 {
            return identities;
        }
        let mut info: SP_DEVINFO_DATA = mem::zeroed();
        info.cbSize = mem::size_of_val(&info) as u32;
        for index in 0.. {
            if SetupDiEnumDeviceInfo(set, index, &mut info) == 0 {
                break;
            }
            let mut node = info.DevInst;
            let mut instance = [0u16; MAX_DEVICE_ID_LEN as usize];
            // Stop at the first physical device. Never attribute an adapter to an upstream hub.
            let serial = loop {
                if CM_Get_Device_IDW(node, instance.as_mut_ptr(), instance.len() as u32, 0)
                    != CR_SUCCESS
                {
                    break None;
                }
                let id = String::from_utf16_lossy(
                    &instance[..instance
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(instance.len())],
                );
                if let Some(serial) = physical_serial(&id) {
                    break Some(serial.to_owned());
                }
                if !id.to_ascii_uppercase().starts_with("USB\\VID_") {
                    break None;
                }
                let mut parent = 0;
                if CM_Get_Parent(&mut parent, node, 0) != CR_SUCCESS {
                    break None;
                }
                node = parent;
            };
            let Some(serial) = serial else { continue };
            let key =
                SetupDiOpenDevRegKey(set, &info, DICS_FLAG_GLOBAL, 0, DIREG_DRV, KEY_QUERY_VALUE);
            if key == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut guid = [0u16; 64];
            let mut size = mem::size_of_val(&guid) as u32;
            let status = RegGetValueW(
                key,
                ptr::null(),
                windows_sys::core::w!("NetCfgInstanceId"),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                guid.as_mut_ptr().cast(),
                &mut size,
            );
            RegCloseKey(key);
            if status == ERROR_SUCCESS {
                let len = guid.iter().position(|&c| c == 0).unwrap_or(guid.len());
                let id = String::from_utf16_lossy(&guid[..len]).to_ascii_lowercase();
                identities.0.insert(id, serial);
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    identities
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_physical_serial_and_skips_function_nodes() {
        assert_eq!(
            physical_serial(r"USB\VID_04E8&PID_6864\phone42"),
            Some("phone42")
        );
        assert_eq!(physical_serial(r"USB\VID_04E8&PID_6864&RNDIS\port42"), None);
        assert_eq!(physical_serial(r"USB\VID_05C6&PID_90B6&MI_00\port1"), None);
        assert_eq!(physical_serial(r"PCI\VID_1234&PID_1234\port1"), None);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires a connected phone and a separately verified Windows adapter name"]
    fn connected_phone_matches_windows_card() {
        let model = std::env::var("NETBRIDGE_TEST_PHONE").unwrap();
        let card_name = std::env::var("NETBRIDGE_TEST_CARD").unwrap();
        let adb = std::env::var("NETBRIDGE_TEST_ADB").unwrap();
        let phones = crate::devices::list(&adb).unwrap();
        let phone = phones.iter().find(|d| d.usb && d.model == model).unwrap();
        let identities = read();
        let matched: Vec<_> = crate::adapters::enumerate()
            .unwrap()
            .into_iter()
            .filter(|a| {
                a.is_usb_network()
                    && identities
                        .serial(&a.id)
                        .is_some_and(|s| s.eq_ignore_ascii_case(&phone.serial))
            })
            .map(|a| a.name)
            .collect();
        assert_eq!(matched, [card_name]);
    }
}
