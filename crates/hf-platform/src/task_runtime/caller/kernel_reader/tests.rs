use super::*;
use crate::task_runtime::caller::{CallerEvidenceSource, observe_caller_with_source};
use std::time::{Duration, Instant};

const BOOT_A: &str = "00000000-0000-4000-8000-00000000000a";
const BOOT_B: &str = "00000000-0000-4000-8000-00000000000b";

fn fixture(pid: i32, parent: i32, uid: u32) -> Vec<u8> {
    let mut bytes = vec![0xa5; RECORD_LEN];
    bytes[0..8].copy_from_slice(&1234i64.to_le_bytes());
    bytes[8..12].copy_from_slice(&5678i32.to_le_bytes());
    bytes[36] = 2;
    bytes[40..44].copy_from_slice(&pid.to_le_bytes());
    bytes[420..424].copy_from_slice(&uid.to_le_bytes());
    bytes[560..564].copy_from_slice(&parent.to_le_bytes());
    bytes
}

struct Fake {
    strings: io::Result<(String, String)>,
    result: Result<usize, i32>,
    claimed_pid: Option<i32>,
    reads: Vec<u32>,
}
impl Fake {
    fn qualified(result: Result<usize, i32>) -> Self {
        Self {
            strings: Ok(("25.6.0".into(), "25G83".into())),
            result,
            claimed_pid: None,
            reads: Vec::new(),
        }
    }
}
impl KernelPlatform for Fake {
    fn os_strings(&mut self) -> io::Result<(String, String)> {
        match &self.strings {
            Ok(pair) => Ok(pair.clone()),
            Err(_) => Err(io::Error::from_raw_os_error(1)),
        }
    }
    fn read(&mut self, pid: u32, buf: &mut [u8]) -> io::Result<usize> {
        self.reads.push(pid);
        let length = self.result.map_err(io::Error::from_raw_os_error)?;
        let native = self.claimed_pid.unwrap_or(pid as i32);
        let record = if pid == 1 {
            fixture(native, 0, 0)
        } else {
            fixture(native, 1, 501)
        };
        buf[..RECORD_LEN].copy_from_slice(&record);
        Ok(length)
    }
}

#[test]
fn qualified_host_decodes_init_and_non_init_fixtures() {
    for (pid, parent, uid) in [(1u32, 0u32, 0u32), (42, 1, 501)] {
        let mut platform = Fake::qualified(Ok(RECORD_LEN));
        let record = read_record_with(&mut platform, pid).expect("qualified record");
        assert_eq!(
            record,
            CallerProcessRecord {
                pid,
                parent,
                uid,
                status: 2,
                start_seconds: 1234,
                start_microseconds: 5678,
            }
        );
        assert_eq!(platform.reads, vec![pid]);
    }
}

#[test]
fn mismatched_pid_record_is_refused() {
    let mut platform = Fake::qualified(Ok(RECORD_LEN));
    platform.claimed_pid = Some(7);
    assert!(read_record_with(&mut platform, 42).is_err());
}

#[test]
fn invalid_pids_are_refused_before_any_read() {
    for pid in [0, u32::try_from(i32::MAX).unwrap() + 1, u32::MAX] {
        let mut platform = Fake::qualified(Ok(RECORD_LEN));
        assert!(read_record_with(&mut platform, pid).is_err(), "pid {pid}");
        assert!(platform.reads.is_empty());
    }
}

#[test]
fn system_reader_rejects_short_or_oversized_record() {
    for length in [0, 647, 649, 1024] {
        let mut platform = Fake::qualified(Ok(length));
        assert!(
            read_record_with(&mut platform, 1).is_err(),
            "length {length}"
        );
    }
}

#[test]
fn system_reader_refuses_errno() {
    for errno in [nix::libc::ENOMEM, nix::libc::EPERM, nix::libc::ESRCH] {
        let mut platform = Fake::qualified(Err(errno));
        assert!(read_record_with(&mut platform, 1).is_err(), "errno {errno}");
    }
}

#[test]
fn system_reader_refuses_unqualified_os_release() {
    let unlisted = [
        ("25.6.1", "25G83"),
        ("25.6.0", "25G84"),
        ("26.0.0", "25G83"),
    ];
    for (release, version) in unlisted {
        let mut platform = Fake::qualified(Ok(RECORD_LEN));
        platform.strings = Ok((release.into(), version.into()));
        assert!(
            read_record_with(&mut platform, 1).is_err(),
            "{release} {version}"
        );
        assert!(platform.reads.is_empty());
    }
    let mut platform = Fake::qualified(Ok(RECORD_LEN));
    platform.strings = Err(io::Error::from_raw_os_error(1));
    assert!(read_record_with(&mut platform, 1).is_err());
    assert!(platform.reads.is_empty());
}

#[test]
fn qualify_refuses_unsupported_architecture() {
    assert_eq!(qualify("x86_64", "25.6.0", "25G83"), None);
    assert_eq!(
        qualify("aarch64", "25.6.0", "25G83"),
        Some(CallerRecordLayout::MacOsArm64Sdk26_5)
    );
}

/// Every record, init and non-init, through the fake platform with scripted boot bookends.
struct Composed {
    platform: Fake,
    boots: Vec<&'static str>,
    calls: usize,
}
impl CallerEvidenceSource for Composed {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn boot_uuid(&mut self) -> io::Result<String> {
        let value = self.boots[self.calls.min(self.boots.len() - 1)];
        self.calls += 1;
        Ok(value.into())
    }
    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord> {
        read_record_with(&mut self.platform, pid)
    }
    fn real_uid(&mut self) -> u32 {
        501
    }
}

#[test]
fn system_reader_boot_bookend_change_refuses() {
    let deadline = Instant::now() + Duration::from_secs(60);
    let observe = |boots: Vec<&'static str>| {
        let mut source = Composed {
            platform: Fake::qualified(Ok(RECORD_LEN)),
            boots,
            calls: 0,
        };
        let result = observe_caller_with_source(42, deadline, &mut source);
        (result, source.platform.reads)
    };
    let (result, reads) = observe(vec![BOOT_A]);
    assert!(result.is_ok());
    assert!(reads.contains(&1) && reads.contains(&42));
    assert!(observe(vec![BOOT_A, BOOT_B]).0.is_err());
}
