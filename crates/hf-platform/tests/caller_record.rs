#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_platform::task_runtime::caller::{CallerRecordLayout, decode_caller_record};

const RECORD_LEN: usize = 648;

fn put_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i64(bytes: &mut [u8], offset: usize, value: i64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn valid_record(pid: i32, parent: i32, uid: u32, status: i32) -> Vec<u8> {
    let mut bytes = vec![0xa5; RECORD_LEN];
    put_i64(&mut bytes, 0, 1234);
    put_i32(&mut bytes, 8, 5678);
    bytes[36] = status as i8 as u8;
    put_i32(&mut bytes, 40, pid);
    put_u32(&mut bytes, 420, uid);
    put_i32(&mut bytes, 560, parent);
    bytes
}

fn decode(
    pid: u32,
    bytes: &[u8],
) -> std::io::Result<hf_platform::task_runtime::caller::CallerProcessRecord> {
    decode_caller_record(CallerRecordLayout::MacOsArm64Sdk26_5, pid, bytes)
}

#[test]
fn accepts_only_exact_layout_length() {
    let bytes = valid_record(42, 7, 501, 2);
    let record = decode(42, &bytes).expect("valid record");
    assert_eq!(record.pid, 42);
    assert_eq!(record.parent, 7);
    assert_eq!(record.uid, 501);
    assert_eq!(record.status, 2);
    assert_eq!(record.start_seconds, 1234);
    assert_eq!(record.start_microseconds, 5678);

    for length in [0, 1, RECORD_LEN - 1] {
        assert!(decode(42, &bytes[..length]).is_err(), "length {length}");
    }
    let mut oversized = bytes;
    oversized.push(0);
    assert!(decode(42, &oversized).is_err());
}

#[test]
fn accepts_supported_pid_parent_time_and_status_boundaries() {
    let init = valid_record(1, 0, 0, 1);
    assert_eq!(decode(1, &init).expect("valid init record").pid, 1);

    let max_pid = valid_record(i32::MAX, 1, 501, 4);
    assert_eq!(
        decode(i32::MAX as u32, &max_pid)
            .expect("max signed pid")
            .pid,
        i32::MAX as u32
    );

    for status in 1..=4 {
        let mut bytes = valid_record(42, 1, 501, status);
        put_i64(&mut bytes, 0, i64::MAX);
        for micros in [0, 999_999] {
            put_i32(&mut bytes, 8, micros);
            let record = decode(42, &bytes).expect("supported boundary values");
            assert_eq!(record.status, status as u32);
            assert_eq!(record.start_seconds, i64::MAX as u64);
            assert_eq!(record.start_microseconds, micros as u64);
        }
    }
}

#[test]
fn refuses_invalid_pid_parent_and_expected_pid_boundaries() {
    let noninit = valid_record(42, 1, 501, 2);
    for expected_pid in [0, 1, u32::MAX] {
        assert!(
            decode(expected_pid, &noninit).is_err(),
            "expected pid {expected_pid}"
        );
    }
    for (pid, parent, expected) in [(0, 1, 0), (1, 2, 1), (42, 0, 42)] {
        assert!(decode(expected, &valid_record(pid, parent, 501, 2)).is_err());
    }
    assert!(decode(0, &valid_record(0, 0, 0, 2)).is_err());
}

#[test]
fn rejects_invalid_signed_fields_and_expected_pid_mismatch() {
    for (offset, value, expected_pid) in [
        (40, -1, 42),
        (560, -1, 42),
        (36, -1, 42),
        (8, 1_000_000, 42),
        (40, 43, 42),
    ] {
        let mut bytes = valid_record(42, 7, 501, 2);
        put_i32(&mut bytes, offset, value);
        assert!(
            decode(expected_pid, &bytes).is_err(),
            "offset {offset}, value {value}"
        );
    }
    for bad_seconds in [0, -1] {
        let mut bytes = valid_record(42, 7, 501, 2);
        put_i64(&mut bytes, 0, bad_seconds);
        assert!(decode(42, &bytes).is_err(), "seconds {bad_seconds}");
    }
    for bad_micros in [-1, 1_000_000] {
        let mut bytes = valid_record(42, 7, 501, 2);
        put_i32(&mut bytes, 8, bad_micros);
        assert!(decode(42, &bytes).is_err(), "microseconds {bad_micros}");
    }
    for status in [0, 5, 127, -128] {
        let mut bytes = valid_record(42, 7, 501, 2);
        bytes[36] = status as i8 as u8;
        assert!(decode(42, &bytes).is_err(), "status {status}");
    }
}

#[test]
fn reads_only_documented_little_endian_fields_and_ignores_padding() {
    let mut bytes = valid_record(42, 560, 420, 2);
    // real UID, effective UID, saved UID, ppid and actual parent intentionally disagree.
    put_u32(&mut bytes, 392, 123);
    put_u32(&mut bytes, 396, 456);
    put_u32(&mut bytes, 416, 392);
    put_u32(&mut bytes, 424, 396);
    put_i32(&mut bytes, 44, 44);
    put_i32(&mut bytes, 556, 44);
    put_i32(&mut bytes, 564, 45);
    bytes[12..16].fill(0x4d);
    bytes[100..116].fill(0x3c);
    bytes[600..RECORD_LEN].fill(0xe7);
    let record = decode(42, &bytes).expect("padding is not interpreted");
    assert_eq!((record.uid, record.parent), (420, 560));
    assert_eq!(
        (record.start_seconds, record.start_microseconds),
        (1234, 5678)
    );

    let mut padding_variant = valid_record(42, 560, 420, 2);
    put_u32(&mut padding_variant, 392, 123);
    put_u32(&mut padding_variant, 396, 456);
    put_u32(&mut padding_variant, 416, 392);
    put_u32(&mut padding_variant, 424, 396);
    put_i32(&mut padding_variant, 44, 44);
    put_i32(&mut padding_variant, 556, 44);
    put_i32(&mut padding_variant, 564, 45);
    padding_variant[12..16].fill(0xb2);
    padding_variant[100..116].fill(0x81);
    padding_variant[600..RECORD_LEN].fill(0x12);
    assert_eq!(
        decode(42, &padding_variant).expect("other padding").uid,
        record.uid
    );
    assert_eq!(
        decode(42, &padding_variant).expect("other padding").parent,
        record.parent
    );
    assert_eq!(
        decode(42, &padding_variant)
            .expect("other padding")
            .start_microseconds,
        record.start_microseconds
    );
}
