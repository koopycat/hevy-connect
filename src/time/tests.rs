use super::*;

#[test]
fn dates_must_exist() {
    assert!(validate_date("2024-02-29").is_ok());
    assert_eq!(
        validate_date("2023-02-29").unwrap_err().message,
        "Date must be a valid calendar date in YYYY-MM-DD format."
    );
    assert_eq!(
        validate_date("2024-1-01").unwrap_err().message,
        "Date must use YYYY-MM-DD format."
    );
    assert!(validate_date("2024-13-01").is_err());
}

#[test]
fn iso_filters_accept_dates_and_offset_timestamps_only() {
    for good in [
        "2024-01-01",
        "2024-01-01T00:00:00Z",
        "2024-01-01T10:30:00.123+02:00",
        "2024-02-29T00:00:00-05:00",
        "2024-01-01T00:00:00.123456789Z",
    ] {
        assert!(validate_iso(good, "--since").is_ok(), "{good}");
    }
    for bad in [
        "1",
        "June 3",
        "2024-01-01T00:00:00",
        "2024-02-30",
        "2023-02-29T00:00:00Z",
        "2024-01-01T25:00:00Z",
        "2024-01-01 00:00:00Z",
        "2024-01-01t00:00:00Z",
        "2024-01-01T00:00:00z",
        "2024-01-01T00:00:00.Z",
        "2024-01-01T00:00:00.1234567890Z",
        "2024-01-01T00:00:00+0500",
        "2024-01-01T00:00:00\u{2212}05:00",
        "",
    ] {
        assert!(validate_iso(bad, "--since").is_err(), "{bad}");
    }
}

#[test]
fn a_bare_date_is_utc_midnight() {
    let date = validate_iso("2024-01-01", "x").unwrap();
    let stamp = validate_iso("2024-01-01T00:00:00Z", "x").unwrap();
    assert_eq!(date, stamp);
    assert!(validate_iso("2024-01-01T00:00:00+01:00", "x").unwrap() < stamp);
}
