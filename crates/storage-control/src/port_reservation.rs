//! Fixed persistent endpoint reservation. Windows denies unprivileged use of a
//! persistent reservation even if its opaque token is discovered. A worker crash
//! must not allow an ordinary process to impersonate the local block target.
use serde::{Deserialize, Serialize};
pub const PORT: u16 = 33262;
#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn CreatePersistentTcpPortReservation(start: u16, count: u16, token: *mut u64) -> u32;
    fn LookupPersistentTcpPortReservation(start: u16, count: u16, token: *mut u64) -> u32;
    fn DeletePersistentTcpPortReservation(start: u16, count: u16) -> u32;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservation {
    pub port: u16,
    pub count: u16,
    pub token: u64,
}
fn error(operation: &str, code: u32) -> String {
    format!(
        "{operation}: {}",
        std::io::Error::from_raw_os_error(code as i32)
    )
}
impl Reservation {
    pub fn create() -> Result<Self, String> {
        let mut token = 0;
        let code = unsafe { CreatePersistentTcpPortReservation(PORT.to_be(), 1, &mut token) };
        if code != 0 {
            return Err(error("Reserve native target endpoint", code));
        }
        if token == 0 {
            return Err("Invalid native endpoint reservation token".into());
        }
        Ok(Self {
            port: PORT,
            count: 1,
            token,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.port != PORT || self.count != 1 || self.token == 0 {
            return Err("Invalid native endpoint reservation".into());
        }
        let mut token = 0;
        let code = unsafe { LookupPersistentTcpPortReservation(PORT.to_be(), 1, &mut token) };
        if code != 0 {
            return Err(error("Look up native target reservation", code));
        }
        if token != self.token {
            return Err("Native endpoint reservation changed; preserving it".into());
        }
        Ok(())
    }
    pub fn remove(self) -> Result<(), String> {
        self.validate()?;
        let code = unsafe { DeletePersistentTcpPortReservation(PORT.to_be(), 1) };
        if code != 0 {
            return Err(error("Release owned native endpoint reservation", code));
        }
        Ok(())
    }
}
