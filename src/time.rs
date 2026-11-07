//! Date and timestamp validation for identifiers and time filters.

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::{Error, Result};

/// `text` with every ASCII digit replaced by `0`, so its layout can be
/// compared with a template.
fn shape(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '0' } else { c })
        .collect()
}

/// A `YYYY-MM-DD` date, or `None` if it has another layout or does not exist.
fn calendar_date(text: &str) -> Option<NaiveDate> {
    if shape(text) != "0000-00-00" {
        return None;
    }
    let part = |range| text.get(range).and_then(|digits: &str| digits.parse().ok());
    NaiveDate::from_ymd_opt(part(0..4)? as i32, part(5..7)?, part(8..10)?)
}

/// Whether `text` is laid out as `YYYY-MM-DDTHH:MM:SS`, then at most nine
/// fractional digits, then `Z` or a `±HH:MM` offset.
fn is_timestamp(text: &str) -> bool {
    let shape = shape(text);
    let Some(rest) = shape.strip_prefix("0000-00-00T00:00:00") else {
        return false;
    };
    let offset = match rest.strip_prefix('.') {
        Some(fraction) => {
            let offset = fraction.trim_start_matches('0');
            if !(1..=9).contains(&(fraction.len() - offset.len())) {
                return false;
            }
            offset
        }
        None => rest,
    };
    matches!(offset, "Z" | "+00:00" | "-00:00")
}

/// A `YYYY-MM-DD` date that exists on the calendar.
pub fn validate_date(text: &str) -> Result<&str> {
    if shape(text) != "0000-00-00" {
        return Err(Error::validation("Date must use YYYY-MM-DD format."));
    }
    match calendar_date(text) {
        Some(_) => Ok(text),
        None => Err(Error::validation(
            "Date must be a valid calendar date in YYYY-MM-DD format.",
        )),
    }
}

/// Parse a Hevy-supplied timestamp, or `None` if it is not RFC 3339.
pub fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// A `YYYY-MM-DD` date or an ISO-8601 timestamp with an offset. Free text and
/// impossible days are rejected rather than rolled into another date; a bare
/// date is UTC midnight.
pub fn validate_iso(text: &str, name: &str) -> Result<DateTime<Utc>> {
    let instant = match calendar_date(text) {
        Some(date) => date.and_hms_opt(0, 0, 0).map(|t| t.and_utc()),
        None => is_timestamp(text).then(|| parse_timestamp(text)).flatten(),
    };
    instant.ok_or_else(|| {
        Error::validation(format!(
            "{name} must be a YYYY-MM-DD date or an ISO-8601 timestamp with an offset, such as 2024-01-01T00:00:00Z."
        ))
    })
}

#[cfg(test)]
mod tests;
