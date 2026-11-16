//! `find_all`: collect every value stored under a key, recursively,
//! in document order. (`serde_json` is built with `preserve_order`,
//! so object iteration follows the document.)

use serde_json::Value;

/// All values stored under `key` anywhere in `root`, in document
/// order. A node that contains `key` contributes its value and is
/// still descended into (a renderer can nest another renderer).
pub fn find_all<'a>(root: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    find_all_into(root, key, &mut out);
    out
}

fn find_all_into<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    out.push(v);
                }
                find_all_into(v, key, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                find_all_into(item, key, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_in_document_order_through_nesting() {
        let root = json!({
            "a": {"target": 1},
            "b": [{"target": 2}, {"c": {"target": 3}}],
            "target": 0
        });
        let found = find_all(&root, "target");
        assert_eq!(found, vec![&json!(1), &json!(2), &json!(3), &json!(0)]);
    }

    #[test]
    fn finds_nothing_when_absent() {
        assert!(find_all(&json!({"a": 1}), "target").is_empty());
    }
}
