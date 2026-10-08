//! Tolerant accessors for Hevy's JSON. The API is early and unstable, so
//! responses are read as plain JSON and checked where they are used, rather
//! than deserialized into rigid types.

use serde_json::{Map, Number, Value};

use crate::error::{Error, Result};

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

pub fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| invalid(what))
}

pub fn array<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a Vec<Value>> {
    value.and_then(Value::as_array).ok_or_else(|| invalid(what))
}

fn invalid(what: &str) -> Error {
    Error::protocol(format!("Hevy returned an invalid {what}."))
}

/// A primitive as-is; anything missing, nested, or absent becomes `null`, so
/// compact rows always have the same shape.
pub fn scalar(value: Option<&Value>) -> Value {
    match value {
        Some(v) if !v.is_object() && !v.is_array() => v.clone(),
        _ => Value::Null,
    }
}

pub fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// A non-negative integer JSON clients can represent exactly.
pub fn safe_integer(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    let number = value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f <= MAX_SAFE_INTEGER as f64)
            .map(|f| f as u64)
    })?;
    (number <= MAX_SAFE_INTEGER).then_some(number)
}

pub fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map_or(0, Vec::len)
}

/// A JSON number that prints like JavaScript would: `60`, not `60.0`.
pub fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER as f64 {
        Value::from(value as i64)
    } else {
        Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn scalar_flattens_everything_that_is_not_a_primitive() {
        assert_eq!(scalar(Some(&json!(3))), json!(3));
        assert_eq!(scalar(Some(&json!("x"))), json!("x"));
        assert_eq!(scalar(Some(&json!({"a": 1}))), Value::Null);
        assert_eq!(scalar(Some(&json!([1]))), Value::Null);
        assert_eq!(scalar(None), Value::Null);
    }

    #[test]
    fn safe_integer_accepts_whole_numbers_only() {
        assert_eq!(safe_integer(Some(&json!(7))), Some(7));
        assert_eq!(safe_integer(Some(&json!(7.0))), Some(7));
        assert_eq!(safe_integer(Some(&json!(7.5))), None);
        assert_eq!(safe_integer(Some(&json!(-1))), None);
        assert_eq!(safe_integer(Some(&json!("7"))), None);
        assert_eq!(safe_integer(Some(&json!(1u64 << 60))), None);
        assert_eq!(safe_integer(None), None);
    }

    #[test]
    fn numbers_print_without_a_spurious_fraction() {
        assert_eq!(serde_json::to_string(&number(60.0)).unwrap(), "60");
        assert_eq!(serde_json::to_string(&number(62.5)).unwrap(), "62.5");
    }
}
