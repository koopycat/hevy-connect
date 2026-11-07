use super::*;

#[test]
fn keys_are_cleaned_of_assignments_and_quotes() {
    for (input, want) in [
        ("abc\n", "abc"),
        ("  abc  ", "abc"),
        ("HEVY_API_KEY=abc", "abc"),
        ("HEVY_API_KEY = \"abc\"\n", "abc"),
        ("'abc'", "abc"),
        ("\"abc\"", "abc"),
        ("\"abc'", "\"abc'"),
        ("", ""),
    ] {
        assert_eq!(clean_key(input), want, "{input:?}");
    }
}
