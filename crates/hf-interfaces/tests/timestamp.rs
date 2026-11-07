#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Instant parsing for stored and queried timestamps. Every expected value below was
//! computed with Python's `calendar.timegm`, not with the code under test.
use hf_domain::timestamp::parse_micros;

fn ok(text: &str, micros: i64) {
    assert_eq!(parse_micros(text), Some(micros), "{text:?}");
}

fn refused(text: &str) {
    assert_eq!(parse_micros(text), None, "{text:?}");
}

#[test]
fn the_epoch_and_known_instants_parse_to_exact_microseconds() {
    ok("1970-01-01T00:00:00Z", 0);
    ok("1970-01-01T00:00:00+00:00", 0);
    ok("2026-01-10T10:00:00+00:00", 1_768_039_200_000_000);
    ok("2026-01-14T10:00:00+00:00", 1_768_384_800_000_000);
    ok("2026-11-07T00:01:00+00:00", 1_794_009_660_000_000);
    ok("2038-01-19T03:14:07Z", 2_147_483_647_000_000);
}

#[test]
fn offsets_shift_the_instant_and_equal_instants_compare_equal() {
    ok("2026-01-10T12:00:00+02:00", 1_768_039_200_000_000);
    ok("2026-01-10T10:00:00+02:00", 1_768_032_000_000_000);
    ok("2026-01-10T10:00:00-05:30", 1_768_059_000_000_000);
    ok("2026-01-10T10:00:00+05:30", 1_768_019_400_000_000);
    ok(
        "2026-01-10T12:00:00+02:00",
        parse_micros("2026-01-10T10:00:00Z").unwrap(),
    );
    ok(
        "2026-01-10T10:00:00-05:30",
        parse_micros("2026-01-10T15:30:00Z").unwrap(),
    );
}

#[test]
fn compact_offsets_and_lowercase_z_and_a_space_separator_are_accepted() {
    ok("2026-01-10T10:00:00+0530", 1_768_019_400_000_000);
    ok("2026-01-10T10:00:00-0530", 1_768_059_000_000_000);
    ok("2026-01-10T10:00:00z", 1_768_039_200_000_000);
    ok("2026-01-10 10:00:00+00:00", 1_768_039_200_000_000);
}

#[test]
fn fractions_of_one_to_six_digits_scale_to_microseconds() {
    let base = 1_768_039_200_000_000;
    ok("2026-01-10T10:00:00.5Z", base + 500_000);
    ok("2026-01-10T10:00:00.05Z", base + 50_000);
    ok("2026-01-10T10:00:00.123Z", base + 123_000);
    ok("2026-01-10T10:00:00.123456Z", base + 123_456);
    ok("2026-01-10T10:00:00.000001Z", base + 1);
    ok("2026-01-10T10:00:00.000000Z", base);
    ok("2026-01-10T10:00:00.999999+00:00", base + 999_999);
}

#[test]
fn a_fraction_with_an_offset_applies_both() {
    ok("2026-01-10T12:00:00.25+02:00", 1_768_039_200_250_000);
}

#[test]
fn malformed_fractions_are_refused() {
    refused("2026-01-10T10:00:00.Z");
    refused("2026-01-10T10:00:00.1234567Z");
    refused("2026-01-10T10:00:00,5Z");
    refused("2026-01-10T10:00:00.5");
    refused("2026-01-10T10:00:00.5.5Z");
}

#[test]
fn naive_and_partial_input_is_refused() {
    refused("2026-01-10T10:00:00");
    refused("2026-01-10T10:00:00.123456");
    refused("2026-01-10");
    refused("2026-01-10T10:00Z");
    refused("");
    refused("Z");
    refused("not a timestamp at all, long enough for the length gate");
}

#[test]
fn stray_characters_and_whitespace_are_refused() {
    refused(" 2026-01-10T10:00:00Z");
    refused("2026-01-10T10:00:00Z ");
    refused("2026-01-10T10:00:00Zjunk");
    refused("2026-01-10t10:00:00Z");
    refused("2026/01/10T10:00:00Z");
    refused("2026-01-10T10-00-00Z");
    refused("+026-01-10T10:00:00Z");
    refused("2026-01-1xT10:00:00Z");
}

#[test]
fn non_ascii_digits_are_refused_without_panicking() {
    refused("٢٠٢٦-01-10T10:00:00Z");
    refused("2026-01-10T10:00:00+０２:００");
    refused("2026-01-10T10:00:00é+02:00");
}

#[test]
fn the_length_gate_is_twenty_to_thirty_two_bytes() {
    ok("2026-01-10T10:00:00.123456+05:30", 1_768_019_400_123_456);
    assert_eq!("2026-01-10T10:00:00.123456+05:30".len(), 32);
    refused("2026-01-10T10:00:00.123456+05:30 ");
    refused("2026-01-10T10:00:00.1234567+05:30");
    assert_eq!("1970-01-01T00:00:00Z".len(), 20);
    refused("1970-01-01T00:00:0Z");
}

#[test]
fn leap_days_follow_the_gregorian_rules() {
    ok("2024-02-29T00:00:00Z", 1_709_164_800_000_000);
    ok("2000-02-29T00:00:00Z", 951_782_400_000_000);
    refused("2023-02-29T00:00:00Z");
    refused("1900-02-29T00:00:00Z");
    refused("2100-02-29T00:00:00Z");
    ok("2026-03-01T00:00:00Z", 1_772_323_200_000_000);
    refused("2026-02-29T00:00:00Z");
    refused("2026-02-30T00:00:00Z");
}

#[test]
fn month_lengths_and_field_ranges_are_enforced() {
    refused("2026-04-31T00:00:00Z");
    refused("2026-06-31T00:00:00Z");
    refused("2026-09-31T00:00:00Z");
    refused("2026-11-31T00:00:00Z");
    assert!(parse_micros("2026-12-31T00:00:00Z").is_some());
    assert!(parse_micros("2026-01-31T00:00:00Z").is_some());
    refused("2026-00-10T00:00:00Z");
    refused("2026-13-10T00:00:00Z");
    refused("2026-01-00T00:00:00Z");
    refused("2026-01-32T00:00:00Z");
    refused("2026-01-10T24:00:00Z");
    refused("2026-01-10T10:60:00Z");
    refused("2026-01-10T10:00:60Z");
    refused("2016-12-31T23:59:60Z");
    assert!(parse_micros("2026-01-10T23:59:59Z").is_some());
}

#[test]
fn the_year_range_is_one_to_9999() {
    ok("0001-01-01T00:00:00Z", -62_135_596_800_000_000);
    ok("9999-12-31T23:59:59Z", 253_402_300_799_000_000);
    refused("0000-01-01T00:00:00Z");
    refused("10000-01-01T00:00:00Z");
}

#[test]
fn offset_bounds_and_shapes_are_enforced() {
    ok(
        "2026-01-10T10:00:00+23:59",
        1_768_039_200_000_000 - (23 * 3600 + 59 * 60) * 1_000_000,
    );
    ok(
        "2026-01-10T10:00:00-23:59",
        1_768_039_200_000_000 + (23 * 3600 + 59 * 60) * 1_000_000,
    );
    refused("2026-01-10T10:00:00+24:00");
    refused("2026-01-10T10:00:00+05:60");
    refused("2026-01-10T10:00:00+5:30");
    refused("2026-01-10T10:00:00+05:3");
    refused("2026-01-10T10:00:00+05-30");
    refused("2026-01-10T10:00:00+05");
    refused("2026-01-10T10:00:00+");
    refused("2026-01-10T10:00:00+05:300");
    refused("2026-01-10T10:00:00 +05:30");
}

#[test]
fn an_offset_that_crosses_the_epoch_gives_a_negative_instant() {
    ok("1970-01-01T00:00:00+01:00", -3_600_000_000);
    ok("1969-12-31T23:00:00Z", -3_600_000_000);
}

#[test]
fn an_offset_may_carry_the_instant_past_the_year_range_without_overflowing() {
    ok(
        "9999-12-31T23:59:59-23:59",
        253_402_300_799_000_000 + (23 * 3600 + 59 * 60) * 1_000_000,
    );
    ok(
        "0001-01-01T00:00:00+23:59",
        -62_135_596_800_000_000 - (23 * 3600 + 59 * 60) * 1_000_000,
    );
}
