//! Crate wiring smoke test: proves the library target links and core helpers behave.

#[test]
fn lib_version_is_set() {
    assert!(!opencodetest::VERSION.is_empty());
}

#[test]
fn money_formatting_is_stable() {
    assert_eq!(opencodetest::util::format_cents(123_456), "1,234.56");
}
