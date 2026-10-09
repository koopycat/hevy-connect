//! Test-only access to the committed OpenAPI capture, so the hand-written
//! tables that mirror it (`resource`, `compact`, `mutate`) fail `just check`
//! when `just api-sync` brings in a contract change that needs review.

use std::sync::LazyLock;

use serde_json::{Map, Value};

static DOC: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../docs/hevy-openapi.json")).expect("the capture is JSON")
});

pub fn doc() -> &'static Value {
    &DOC
}

/// Follow a local `$ref`; any other schema is returned as given.
pub fn resolve(schema: &'static Value) -> &'static Value {
    match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .unwrap_or_else(|| panic!("unsupported reference {reference}"));
            &DOC["components"]["schemas"][name]
        }
        None => schema,
    }
}

pub fn schema(name: &str) -> &'static Value {
    let found = &DOC["components"]["schemas"][name];
    assert!(!found.is_null(), "schema {name} is gone from the capture");
    found
}

/// The properties of an object schema.
pub fn properties(schema: &'static Value) -> &'static Map<String, Value> {
    resolve(schema)["properties"]
        .as_object()
        .expect("an object schema with properties")
}

/// One property of an object schema, with `$ref` followed.
pub fn property(schema: &'static Value, name: &str) -> Option<&'static Value> {
    properties(schema).get(name).map(resolve)
}

/// The element schema of an array schema.
pub fn items(schema: &'static Value) -> &'static Value {
    resolve(&resolve(schema)["items"])
}
