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
