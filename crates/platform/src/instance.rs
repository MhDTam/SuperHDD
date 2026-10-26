//! One GUI instance per user/session. Ownership ends automatically on process exit.
pub struct InstanceGuard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn claim_named(name: &str) -> Result<Option<InstanceGuard>, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        SetLastError(0);
        let handle = CreateMutexW(std::ptr::null(), 0, wide.as_ptr());
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(InstanceGuard { handle }))
    }
}

pub fn claim_desktop() -> Result<Option<InstanceGuard>, String> {
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        let profile = std::env::var("USERPROFILE")
            .map_err(|_| "User profile unavailable")?
            .to_ascii_lowercase();
        let identity = format!("{:x}", Sha256::digest(profile.as_bytes()));
        let claim = claim_named(&format!("Local\\SuperHDD.Desktop.{}", &identity[..24]))?;
        if claim.is_none() {
            let title: Vec<u16> = "Super HDD".encode_utf16().chain(Some(0)).collect();
            unsafe {
                let window = FindWindowW(std::ptr::null(), title.as_ptr());
                if !window.is_null() {
                    ShowWindow(window, SW_RESTORE);
                    SetForegroundWindow(window);
                }
            }
        }
        Ok(claim)
    }
    #[cfg(not(windows))]
    {
        Ok(Some(InstanceGuard {}))
    }
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;
    #[test]
    fn instance_claim_is_exclusive_and_released_on_drop() {
        let name = format!(
            "Local\\SuperHDD.Test.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let first = claim_named(&name).unwrap().unwrap();
        assert!(claim_named(&name).unwrap().is_none());
        drop(first);
        assert!(claim_named(&name).unwrap().is_some());
    }
}
