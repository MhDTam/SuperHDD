//! Transactional ordering shared by native service/controllers. Disk ownership
//! and kernel qualification are enforced by the backend, never inferred here.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub source_device: String,
    pub cache_device: String,
    pub source_disk: u32,
    pub cache_disk: u32,
    pub capacity_bytes: u64,
    pub source_volume: String,
    pub cache_volume: String,
    pub container_instance: [u8; 16],
}
#[derive(Debug, Clone, Copy, Default)]
pub struct DriverState {
    pub attached: bool,
    pub paused: bool,
    pub bypass: bool,
    pub source_disk: Option<u32>,
    pub cache_disk: Option<u32>,
}
impl DriverState {
    fn owns(self, target: &Target) -> bool {
        self.attached
            && self.source_disk == Some(target.source_disk)
            && self.cache_disk == Some(target.cache_disk)
    }
}
pub trait Backend {
    fn status(&mut self) -> Result<DriverState, String>;
    fn prepare_backing(&mut self) -> Result<Target, String>;
    fn configure(&mut self, target: &Target) -> Result<(), String>;
    fn pause(&mut self, paused: bool) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
    fn disable(&mut self) -> Result<(), String>;
    fn release_backing(&mut self) -> Result<(), String>;
    /// Retain an attachment whose driver could not be safely disabled. Never
    /// delete, format or force-detach it in a destructor/error recovery path.
    fn preserve_backing(&mut self);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Starting,
    Active,
    Paused,
    Faulted,
    Stopped,
}

pub struct Session<B: Backend> {
    backend: B,
    target: Option<Target>,
    phase: Phase,
    has_backing: bool,
}
impl<B: Backend> Session<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            target: None,
            phase: Phase::Idle,
            has_backing: false,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn target(&self) -> Option<&Target> {
        self.target.as_ref()
    }
    pub fn start(&mut self) -> Result<(), String> {
        self.start_with_checkpoint(|_| Ok(()))
    }
    /// The durable owner must commit the exact target before configure can
    /// attach it. A failed checkpoint rolls back the backing without configure.
    pub fn start_with_checkpoint(
        &mut self,
        mut checkpoint: impl FnMut(&Target) -> Result<(), String>,
    ) -> Result<(), String> {
        if !matches!(self.phase, Phase::Idle | Phase::Stopped) {
            return Err("Stop or recover the existing cache session first.".into());
        }
        if self.backend.status()?.attached {
            return Err(
                "A different cache session is already attached; it was not changed.".into(),
            );
        }
        self.phase = Phase::Starting;
        // prepare_backing may fail after attachment, so cleanup must run even if
        // no target could yet be returned.
        self.has_backing = true;
        let target = match self.backend.prepare_backing() {
            Ok(target) => target,
            Err(error) => {
                return self.start_failed(error);
            }
        };
        if target.source_disk == target.cache_disk
            || target.source_device == target.cache_device
            || target.capacity_bytes < 131072
            || target.capacity_bytes > crate::MAX_BUDGET_BYTES
            || !target.capacity_bytes.is_multiple_of(65536)
        {
            return self.start_failed("Invalid or overlapping prepared cache target.".into());
        }
        self.target = Some(target.clone());
        if let Err(error) = checkpoint(&target) {
            return self.start_failed(error);
        }
        if let Err(error) = self.backend.configure(&target) {
            return self.start_failed(error);
        }
        match self.backend.status() {
            Ok(status) if status.owns(&target) && !status.bypass && !status.paused => {
                self.phase = Phase::Active;
                Ok(())
            }
            Ok(_) => self.start_failed(
                "Driver did not confirm the requested active cache identities.".into(),
            ),
            Err(error) => self.start_failed(error),
        }
    }
    fn start_failed(&mut self, error: String) -> Result<(), String> {
        self.phase = Phase::Faulted;
        match self.stop() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}; backing retained: {cleanup}")),
        }
    }
    fn owned_status(&mut self) -> Result<DriverState, String> {
        let target = self.target.as_ref().ok_or("No owned cache session")?;
        let status = match self.backend.status() {
            Ok(status) => status,
            Err(error) => {
                self.phase = Phase::Faulted;
                return Err(error);
            }
        };
        if !status.owns(target) {
            self.phase = Phase::Faulted;
            return Err("Driver ownership changed; no control command was sent.".into());
        }
        Ok(status)
    }
    pub fn pause(&mut self, paused: bool) -> Result<(), String> {
        if !matches!(self.phase, Phase::Active | Phase::Paused) {
            return Err("No running owned cache session.".into());
        }
        let before = self.owned_status()?;
        if before.bypass {
            self.phase = Phase::Faulted;
            return Err("Driver entered bypass; stop and revalidate before starting again.".into());
        }
        self.phase = Phase::Faulted;
        self.backend.pause(paused)?;
        let status = self.owned_status()?;
        if status.paused != paused || status.bypass {
            return Err("Driver did not confirm the requested pause state.".into());
        }
        self.phase = if paused { Phase::Paused } else { Phase::Active };
        Ok(())
    }
    pub fn clear(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::Active | Phase::Paused) {
            return Err("No running owned cache session.".into());
        }
        if self.owned_status()?.bypass {
            self.phase = Phase::Faulted;
            return Err("Cannot clear a bypassed cache without revalidation.".into());
        }
        self.phase = Phase::Faulted;
        self.backend.clear()?;
        let status = self.owned_status()?;
        if status.bypass {
            return Err("Cache entered bypass while clearing.".into());
        }
        self.phase = if status.paused {
            Phase::Paused
        } else {
            Phase::Active
        };
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.has_backing {
            self.phase = Phase::Stopped;
            return Ok(());
        }
        self.phase = Phase::Faulted;
        let status = self.backend.status()?;
        if status.attached {
            if !self
                .target
                .as_ref()
                .is_some_and(|target| status.owns(target))
            {
                return Err(
                    "Unknown driver ownership; refusing to disable or detach its backing.".into(),
                );
            }
            self.backend.disable()?;
            if self.backend.status()?.attached {
                return Err("Driver still holds the cache; backing was not detached.".into());
            }
        }
        self.backend.release_backing()?;
        self.has_backing = false;
        self.target = None;
        self.phase = Phase::Stopped;
        Ok(())
    }
}
impl<B: Backend> Drop for Session<B> {
    fn drop(&mut self) {
        if self.has_backing && self.stop().is_err() {
            self.backend.preserve_backing();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct Trace {
        events: Vec<&'static str>,
        state: DriverState,
        prepare_fails: bool,
        configure_fails: bool,
        disable_fails: bool,
        foreign_after_configure: bool,
    }
    struct Fake(Rc<RefCell<Trace>>);
    impl Backend for Fake {
        fn status(&mut self) -> Result<DriverState, String> {
            Ok(self.0.borrow().state)
        }
        fn prepare_backing(&mut self) -> Result<Target, String> {
            let mut t = self.0.borrow_mut();
            t.events.push("attach");
            if t.prepare_fails {
                return Err("prepare failure".into());
            }
            Ok(Target {
                source_device: "source".into(),
                cache_device: "cache".into(),
                source_disk: 1,
                cache_disk: 2,
                capacity_bytes: 1048576,
                source_volume: "source-volume".into(),
                cache_volume: "cache-volume".into(),
                container_instance: [1; 16],
            })
        }
        fn configure(&mut self, _: &Target) -> Result<(), String> {
            let mut t = self.0.borrow_mut();
            t.events.push("configure");
            t.state = DriverState {
                attached: true,
                source_disk: Some(1),
                cache_disk: Some(if t.foreign_after_configure { 3 } else { 2 }),
                ..Default::default()
            };
            if t.configure_fails {
                Err("configure failure".into())
            } else {
                Ok(())
            }
        }
        fn pause(&mut self, paused: bool) -> Result<(), String> {
            let mut t = self.0.borrow_mut();
            t.events.push(if paused { "pause" } else { "resume" });
            t.state.paused = paused;
            Ok(())
        }
        fn clear(&mut self) -> Result<(), String> {
            self.0.borrow_mut().events.push("clear");
            Ok(())
        }
        fn disable(&mut self) -> Result<(), String> {
            let mut t = self.0.borrow_mut();
            t.events.push("disable");
            if t.disable_fails {
                return Err("disable failure".into());
            }
            t.state.attached = false;
            Ok(())
        }
        fn release_backing(&mut self) -> Result<(), String> {
            self.0.borrow_mut().events.push("detach");
            Ok(())
        }
        fn preserve_backing(&mut self) {
            self.0.borrow_mut().events.push("preserve");
        }
    }
    fn fixture() -> (Session<Fake>, Rc<RefCell<Trace>>) {
        let trace = Rc::new(RefCell::new(Trace::default()));
        (Session::new(Fake(trace.clone())), trace)
    }
    #[test]
    fn durable_checkpoint_failure_prevents_kernel_configure_and_releases_backing() {
        let (mut session, trace) = fixture();
        assert!(session
            .start_with_checkpoint(|target| {
                assert_eq!(target.container_instance, [1; 16]);
                Err("journal disk full".into())
            })
            .is_err());
        assert_eq!(session.phase(), Phase::Stopped);
        assert_eq!(trace.borrow().events, vec!["attach", "detach"]);
    }
    #[test]
    fn backing_is_released_only_after_confirmed_driver_disable() {
        let (mut s, t) = fixture();
        s.start().unwrap();
        s.pause(true).unwrap();
        s.clear().unwrap();
        s.pause(false).unwrap();
        s.stop().unwrap();
        assert_eq!(s.phase(), Phase::Stopped);
        assert_eq!(
            t.borrow().events,
            vec![
                "attach",
                "configure",
                "pause",
                "clear",
                "resume",
                "disable",
                "detach"
            ]
        );
    }
    #[test]
    fn failed_configure_rolls_back_even_when_driver_attached_before_error() {
        let (mut s, t) = fixture();
        t.borrow_mut().configure_fails = true;
        assert!(s.start().is_err());
        assert_eq!(s.phase(), Phase::Stopped);
        assert_eq!(
            t.borrow().events,
            vec!["attach", "configure", "disable", "detach"]
        );
    }
    #[test]
    fn failed_prepare_releases_partial_backing_without_sending_disable() {
        let (mut s, t) = fixture();
        t.borrow_mut().prepare_fails = true;
        assert!(s.start().is_err());
        assert_eq!(t.borrow().events, vec!["attach", "detach"]);
    }
    #[test]
    fn failed_disable_never_detaches_or_deletes_the_backing() {
        let (mut s, t) = fixture();
        s.start().unwrap();
        t.borrow_mut().disable_fails = true;
        assert!(s.stop().is_err());
        drop(s);
        assert!(!t.borrow().events.contains(&"detach"));
        assert_eq!(t.borrow().events.last(), Some(&"preserve"));
    }
    #[test]
    fn foreign_driver_session_is_not_disabled_by_rollback_or_drop() {
        let (mut s, t) = fixture();
        t.borrow_mut().foreign_after_configure = true;
        assert!(s.start().is_err());
        drop(s);
        let events = &t.borrow().events;
        assert!(!events.contains(&"disable"));
        assert!(!events.contains(&"detach"));
        assert_eq!(events.last(), Some(&"preserve"));
    }
    #[test]
    fn preexisting_driver_session_prevents_backing_allocation() {
        let (mut s, t) = fixture();
        t.borrow_mut().state.attached = true;
        assert!(s.start().is_err());
        assert!(t.borrow().events.is_empty());
    }
    #[test]
    fn bypass_cannot_be_resumed_without_stop_and_revalidation() {
        let (mut s, t) = fixture();
        s.start().unwrap();
        t.borrow_mut().state.bypass = true;
        assert!(s.pause(false).is_err());
        assert!(!t.borrow().events.contains(&"resume"));
        s.stop().unwrap();
    }
    #[test]
    fn ownership_change_prevents_control_commands() {
        let (mut s, t) = fixture();
        s.start().unwrap();
        t.borrow_mut().state.source_disk = Some(99);
        assert!(s.clear().is_err());
        drop(s);
        assert!(!t.borrow().events.contains(&"clear"));
        assert!(!t.borrow().events.contains(&"disable"));
    }
}
