//! Internal process evidence; these values do not establish tree containment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
    pid: u32,
    start_seconds: u64,
    start_microseconds: u64,
    boot_uuid: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    InvalidPid,
    InvalidStart,
    InvalidBootUuid,
}
impl ProcessIdentity {
    pub fn new(
        pid: u32,
        seconds: u64,
        micros: u64,
        boot_uuid: String,
    ) -> Result<Self, IdentityError> {
        if pid <= 1 || pid > i32::MAX as u32 {
            return Err(IdentityError::InvalidPid);
        }
        if seconds == 0 || micros >= 1_000_000 {
            return Err(IdentityError::InvalidStart);
        }
        if boot_uuid.len() != 36
            || !boot_uuid.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
        {
            return Err(IdentityError::InvalidBootUuid);
        }
        Ok(Self {
            pid,
            start_seconds: seconds,
            start_microseconds: micros,
            boot_uuid: boot_uuid.to_ascii_lowercase(),
        })
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn start_seconds(&self) -> u64 {
        self.start_seconds
    }
    pub fn start_microseconds(&self) -> u64 {
        self.start_microseconds
    }
    pub fn boot_uuid(&self) -> &str {
        &self.boot_uuid
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityObservation {
    Matching,
    Dead,
    Mismatch,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessExit {
    Unobserved,
    Code(i32),
    Signal(i32),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Containment {
    Unknown,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::{Containment, IdentityError, IdentityObservation, ProcessExit, ProcessIdentity};

    #[test]
    fn identity_requires_stable_fields_and_normalizes_uuid_case() {
        let identity = ProcessIdentity::new(
            42,
            1_700_000_000,
            999_999,
            "ABCDEF01-2345-6789-ABCD-EF0123456789".into(),
        )
        .unwrap();
        assert_eq!(identity.pid(), 42);
        assert_eq!(identity.start_seconds(), 1_700_000_000);
        assert_eq!(identity.start_microseconds(), 999_999);
        assert_eq!(identity.boot_uuid(), "abcdef01-2345-6789-abcd-ef0123456789");
    }

    #[test]
    fn identity_rejects_unusable_pid_start_and_boot_evidence() {
        let boot = "abcdef01-2345-6789-abcd-ef0123456789".to_owned();
        assert_eq!(
            ProcessIdentity::new(0, 1, 0, boot.clone()),
            Err(IdentityError::InvalidPid)
        );
        assert_eq!(
            ProcessIdentity::new(1, 1, 0, boot.clone()),
            Err(IdentityError::InvalidPid)
        );
        assert_eq!(
            ProcessIdentity::new(i32::MAX as u32 + 1, 1, 0, boot.clone()),
            Err(IdentityError::InvalidPid)
        );
        assert_eq!(
            ProcessIdentity::new(42, 0, 0, boot.clone()),
            Err(IdentityError::InvalidStart)
        );
        assert_eq!(
            ProcessIdentity::new(42, 1, 1_000_000, boot),
            Err(IdentityError::InvalidStart)
        );
        assert_eq!(
            ProcessIdentity::new(42, 1, 0, "bad".into()),
            Err(IdentityError::InvalidBootUuid)
        );
        assert_eq!(
            ProcessIdentity::new(42, 1, 0, "abcdef012345-6789-abcd-ef0123456789".into()),
            Err(IdentityError::InvalidBootUuid)
        );
        assert!(
            ProcessIdentity::new(42, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).is_ok()
        );
    }

    #[test]
    fn observations_exit_and_containment_remain_separate_evidence() {
        assert_ne!(IdentityObservation::Dead, IdentityObservation::Unknown);
        assert_ne!(IdentityObservation::Mismatch, IdentityObservation::Unknown);
        assert_ne!(ProcessExit::Code(2), ProcessExit::Signal(2));
        assert_ne!(ProcessExit::Unobserved, ProcessExit::Code(0));
        assert_eq!(Containment::Unknown, Containment::Unknown);
    }
}
