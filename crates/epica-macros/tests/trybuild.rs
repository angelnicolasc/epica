#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/expand/basic.rs");
    t.pass("tests/expand/full_attrs.rs");
}
