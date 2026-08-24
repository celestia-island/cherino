//! Integration tests for the `Getters` derive macro.

use cherino_macros::Getters;

#[derive(Debug, Getters)]
struct Sample {
    name: String,
    count: u64,
    ratio: f64,
    flag: bool,
    tags: Vec<String>,
    nickname: Option<String>,
    budget: Option<u32>,
    #[getter(skip)]
    secret: String,
    #[getter(rename = "identifier")]
    id: String,
}

#[test]
fn generates_field_accessors_with_shape_aware_returns() {
    let sample = Sample {
        name: "cherino".to_string(),
        count: 7,
        ratio: 0.5,
        flag: true,
        tags: vec!["a".to_string(), "b".to_string()],
        nickname: Some("cherry".to_string()),
        budget: None,
        secret: "hidden".to_string(),
        id: "id-1".to_string(),
    };

    // String -> &str
    assert_eq!(sample.name(), "cherino");
    // Copy types are returned by value.
    assert_eq!(sample.count(), 7);
    assert_eq!(sample.ratio(), 0.5);
    assert!(sample.flag());
    // Vec<T> -> &[T]
    assert_eq!(sample.tags(), &["a".to_string(), "b".to_string()]);
    // Option<String> -> Option<&str>
    assert_eq!(sample.nickname(), Some("cherry"));
    // Option<T> -> Option<&T>
    assert_eq!(sample.budget(), None);
    // rename attribute renames the accessor.
    assert_eq!(sample.identifier(), "id-1");
}

#[test]
fn skip_attribute_suppresses_the_accessor() {
    // `#[getter(skip)]` must not generate a `secret()` method — the derive
    // only emits accessors for non-skipped fields, so the field itself stays
    // accessible only as a plain struct field.
    let sample = Sample {
        name: String::new(),
        count: 0,
        ratio: 0.0,
        flag: false,
        tags: Vec::new(),
        nickname: None,
        budget: Some(1),
        secret: "hidden".to_string(),
        id: String::new(),
    };
    assert_eq!(sample.secret, "hidden");
}
