//! Init (PID 1) caller evidence from the `kern.proc.pid` record, decoded with the SDK layout.
//!
//! libproc refuses PID 1 for unprivileged callers, so only the init anchor uses this reader;
//! every other process keeps the libproc `BSDInfo` read. The layout is selected only on a
//! host whose architecture, `kern.osrelease` and `kern.osversion` are listed in `QUALIFIED`;
//! qualification runs on every read and any other host refuses without reading the record.
//! The kernel call cannot be cancelled: the caller's post-read deadline check is a
//! cooperative bound, as for the libproc reads.
use super::{CallerProcessRecord, CallerRecordLayout, decode_caller_record, unknown};
use nix::libc::{CTL_KERN, KERN_PROC, KERN_PROC_PID, c_int};
use std::io;
use sysctl::{Ctl, Sysctl};

/// Reviewed `(kern.osrelease, kern.osversion)` pairs on aarch64. Adding one needs its own
/// layout qualification run and review.
const QUALIFIED: [(&str, &str); 1] = [("25.6.0", "25G83")];
const RECORD_LEN: usize = 648;
const BUFFER_LEN: usize = 1024;
const INIT_MIB: [c_int; 4] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, 1];

pub(super) trait InitPlatform {
    fn os_strings(&mut self) -> io::Result<(String, String)>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
}

struct SystemInit;
impl InitPlatform for SystemInit {
    fn os_strings(&mut self) -> io::Result<(String, String)> {
        let string = |name: &str| {
            Ctl::new(name)
                .and_then(|ctl| ctl.value_string())
                .map_err(|_| unknown())
        };
        Ok((string("kern.osrelease")?, string("kern.osversion")?))
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        hf_sys::read_into(&INIT_MIB, buf)
    }
}

fn qualify(arch: &str, release: &str, version: &str) -> Option<CallerRecordLayout> {
    let listed = QUALIFIED.iter().any(|&(r, v)| r == release && v == version);
    (arch == "aarch64" && listed).then_some(CallerRecordLayout::MacOsArm64Sdk26_5)
}

pub(super) fn read_init() -> io::Result<CallerProcessRecord> {
    read_init_with(&mut SystemInit)
}

pub(super) fn read_init_with<P: InitPlatform + ?Sized>(
    platform: &mut P,
) -> io::Result<CallerProcessRecord> {
    let (release, version) = platform.os_strings().map_err(|_| unknown())?;
    let layout = qualify(std::env::consts::ARCH, &release, &version).ok_or_else(unknown)?;
    let mut buf = [0u8; BUFFER_LEN];
    let length = platform.read(&mut buf).map_err(|_| unknown())?;
    if length != RECORD_LEN {
        return Err(unknown());
    }
    decode_caller_record(layout, 1, &buf[..RECORD_LEN])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_runtime::caller::{CallerEvidenceSource, observe_caller_with_source};
    use std::time::{Duration, Instant};

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0xa5; RECORD_LEN];
        bytes[0..8].copy_from_slice(&1234i64.to_le_bytes());
        bytes[8..12].copy_from_slice(&5678i32.to_le_bytes());
        bytes[36] = 2;
        bytes[40..44].copy_from_slice(&1i32.to_le_bytes());
        bytes[420..424].copy_from_slice(&0u32.to_le_bytes());
        bytes[560..564].copy_from_slice(&0i32.to_le_bytes());
        bytes
    }

    struct Fake {
        strings: io::Result<(String, String)>,
        result: Result<usize, i32>,
        record: Vec<u8>,
        reads: usize,
    }
    impl Fake {
        fn qualified(result: Result<usize, i32>) -> Self {
            Self {
                strings: Ok(("25.6.0".into(), "25G83".into())),
                result,
                record: fixture(),
                reads: 0,
            }
        }
    }
    impl InitPlatform for Fake {
        fn os_strings(&mut self) -> io::Result<(String, String)> {
            match &self.strings {
                Ok(pair) => Ok(pair.clone()),
                Err(_) => Err(io::Error::from_raw_os_error(1)),
            }
        }
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            let length = self.result.map_err(io::Error::from_raw_os_error)?;
            let count = self.record.len().min(buf.len());
            buf[..count].copy_from_slice(&self.record[..count]);
            Ok(length)
        }
    }

    #[test]
    fn qualified_host_decodes_valid_init_fixture() {
        let mut platform = Fake::qualified(Ok(RECORD_LEN));
        let record = read_init_with(&mut platform).expect("qualified init record");
        assert_eq!(
            record,
            CallerProcessRecord {
                pid: 1,
                parent: 0,
                uid: 0,
                status: 2,
                start_seconds: 1234,
                start_microseconds: 5678,
            }
        );
        assert_eq!(platform.reads, 1);
    }

    #[test]
    fn system_reader_rejects_short_or_oversized_record() {
        for length in [0, 647, 649, 1024] {
            let mut platform = Fake::qualified(Ok(length));
            assert!(read_init_with(&mut platform).is_err(), "length {length}");
        }
    }

    #[test]
    fn system_reader_refuses_errno() {
        for errno in [nix::libc::ENOMEM, nix::libc::EPERM, nix::libc::ESRCH] {
            let mut platform = Fake::qualified(Err(errno));
            assert!(read_init_with(&mut platform).is_err(), "errno {errno}");
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
                read_init_with(&mut platform).is_err(),
                "{release} {version}"
            );
            assert_eq!(platform.reads, 0);
        }
        let mut platform = Fake::qualified(Ok(RECORD_LEN));
        platform.strings = Err(io::Error::from_raw_os_error(1));
        assert!(read_init_with(&mut platform).is_err());
        assert_eq!(platform.reads, 0);
    }

    #[test]
    fn qualify_refuses_unsupported_architecture() {
        assert_eq!(qualify("x86_64", "25.6.0", "25G83"), None);
        assert_eq!(
            qualify("aarch64", "25.6.0", "25G83"),
            Some(CallerRecordLayout::MacOsArm64Sdk26_5)
        );
    }

    /// Init through the fake platform; one non-init link whose parent is init.
    struct Composed {
        init: Fake,
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
            if pid == 1 {
                return read_init_with(&mut self.init);
            }
            Ok(CallerProcessRecord {
                pid,
                parent: 1,
                uid: 501,
                status: 2,
                start_seconds: 2000,
                start_microseconds: 1,
            })
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
                init: Fake::qualified(Ok(RECORD_LEN)),
                boots,
                calls: 0,
            };
            observe_caller_with_source(42, deadline, &mut source)
        };
        assert!(observe(vec!["00000000-0000-4000-8000-00000000000a"]).is_ok());
        // The first init observation's bookends differ.
        assert!(
            observe(vec![
                "00000000-0000-4000-8000-00000000000a",
                "00000000-0000-4000-8000-00000000000b"
            ])
            .is_err()
        );
    }
}
