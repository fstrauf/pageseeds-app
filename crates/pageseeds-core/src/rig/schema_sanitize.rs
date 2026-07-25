//! Sanitize JSON Schemas for OpenAI-shaped tool/function parameters.
//!
//! Providers used via the Kimi bridge (and other OpenAI-compatible APIs) reject
//! several constructs that schemars emits by default for Rust `Option<T>`:
//!
//! - `anyOf: [{$ref}, {type: null}]` for optional nested objects
//! - top-level `$schema` draft markers
//! - unsupported `format` values (e.g. `int64`) on some gateways
//!
//! ContentFixPatch extracts successfully because its Option fields are all
//! scalars/arrays (`type: [string|array, null]`). CtrFixPatch failed because
//! `Option<CtrSnippetPatch>` becomes `anyOf` + `$ref` — a known
//! `invalid_function_parameters` trigger.
//!
//! Call [`sanitize_tool_parameters`] on every schemars schema before attaching
//! it as `tools[].function.parameters`.

use schemars::JsonSchema;
use serde_json::{json, Map, Value};

/// Build a provider-safe tool/function `parameters` object for type `T`.
///
/// Combines `schemars::schema_for!(T)` with [`sanitize_tool_parameters`] so every
/// structured-extract path (OpenAI / Claude / Ollama / Kimi / CLI JSON mode)
/// shares one sanitize boundary. Prefer this over raw `schema_for!` whenever the
/// schema will be attached as tool parameters or injected into a JSON-mode prompt.
pub fn schemars_tool_parameters<T: JsonSchema>() -> Result<Value, String> {
    let schema = schemars::schema_for!(T);
    let value = serde_json::to_value(&schema)
        .map_err(|e| format!("Failed to serialize JSON schema: {}", e))?;
    Ok(sanitize_tool_parameters(value))
}

/// Sanitize a full schemars schema for use as tool/function parameters.
pub fn sanitize_tool_parameters(schema: Value) -> Value {
    let mut root = schema;
    // Drop draft marker — not part of the OpenAI function-parameters subset.
    if let Some(obj) = root.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
        obj.remove("description");
    }

    // Collect $defs for inlining, then remove them after refs are resolved.
    let defs = root
        .as_object()
        .and_then(|o| o.get("$defs"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    root = inline_refs(root, &defs, 0);
    if let Some(obj) = root.as_object_mut() {
        obj.remove("$defs");
        obj.remove("definitions");
    }

    rewrite_optional_forms(root)
}

/// Recursively inline `#/$defs/Name` references (depth-capped).
fn inline_refs(value: Value, defs: &Value, depth: usize) -> Value {
    if depth > 32 {
        return value;
    }
    match value {
        Value::Object(mut map) => {
            if let Some(Value::String(r)) = map.get("$ref").cloned() {
                if let Some(name) = r.strip_prefix("#/$defs/") {
                    if let Some(def) = defs.get(name) {
                        // Prefer the resolved def; merge sibling keys (rare) under it.
                        let mut resolved = inline_refs(def.clone(), defs, depth + 1);
                        map.remove("$ref");
                        if let Some(resolved_obj) = resolved.as_object_mut() {
                            for (k, v) in map {
                                if k != "$ref" {
                                    resolved_obj
                                        .entry(k)
                                        .or_insert_with(|| inline_refs(v, defs, depth + 1));
                                }
                            }
                            return Value::Object(resolved_obj.clone());
                        }
                        return resolved;
                    }
                }
            }
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k, inline_refs(v, defs, depth + 1));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| inline_refs(v, defs, depth + 1))
                .collect(),
        ),
        other => other,
    }
}

/// Rewrite provider-hostile optional forms after refs are inlined.
///
/// - `anyOf: [T, {type:null}]` / `oneOf` same → use T alone when T is an object
///   (field optionality comes from not being in `required`), or merge null into
///   a type union for scalars.
/// - Drop unsupported `format` values (`int64`, `int32`, `float`, `double`).
fn rewrite_optional_forms(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut map = map;

            // Collapse anyOf/oneOf optional patterns first.
            for key in ["anyOf", "oneOf"] {
                if let Some(variants) = map.get(key).and_then(|v| v.as_array()).cloned() {
                    if let Some(collapsed) = collapse_optional_union(&variants) {
                        map.remove(key);
                        // Merge collapsed schema into this object (may already have other keys).
                        if let Value::Object(collapsed_map) = collapsed {
                            for (k, v) in collapsed_map {
                                map.entry(k).or_insert(v);
                            }
                        } else {
                            return rewrite_optional_forms(collapsed);
                        }
                        break;
                    }
                }
            }

            // Drop integer formats OpenAI-shaped gateways often reject.
            if let Some(Value::String(fmt)) = map.get("format").cloned() {
                if matches!(
                    fmt.as_str(),
                    "int64" | "int32" | "float" | "double" | "uint64" | "uint32"
                ) {
                    map.remove("format");
                }
            }

            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k, rewrite_optional_forms(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(rewrite_optional_forms).collect()),
        other => other,
    }
}

/// If `variants` is exactly one real schema + one pure-null schema, collapse it.
fn collapse_optional_union(variants: &[Value]) -> Option<Value> {
    if variants.len() != 2 {
        return None;
    }
    let mut non_null: Option<&Value> = None;
    let mut saw_null = false;
    for v in variants {
        if is_pure_null_schema(v) {
            saw_null = true;
        } else if non_null.is_none() {
            non_null = Some(v);
        } else {
            return None; // two non-null variants — leave alone
        }
    }
    if !saw_null {
        return None;
    }
    let schema = non_null?.clone();

    // Nested objects: keep the object schema as-is. Optionality is expressed by
    // the parent property not being required — `type: [object, null]` / anyOf
    // is what providers reject.
    if schema
        .get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t == "object")
        || schema.get("properties").is_some()
    {
        return Some(schema);
    }

    // Scalars / arrays: merge null into a type union when type is a single string.
    if let Some(Value::String(t)) = schema.get("type") {
        let mut merged = schema.clone();
        if let Some(obj) = merged.as_object_mut() {
            obj.insert("type".to_string(), json!([t, "null"]));
        }
        return Some(merged);
    }

    // type already an array — append null if missing.
    if let Some(Value::Array(types)) = schema.get("type") {
        if !types.iter().any(|t| t.as_str() == Some("null")) {
            let mut merged = schema.clone();
            if let Some(obj) = merged.as_object_mut() {
                let mut new_types = types.clone();
                new_types.push(json!("null"));
                obj.insert("type".to_string(), Value::Array(new_types));
            }
            return Some(merged);
        }
    }

    Some(schema)
}

fn is_pure_null_schema(v: &Value) -> bool {
    match v {
        Value::Object(map) => {
            map.get("type").and_then(|t| t.as_str()) == Some("null")
                && map
                    .keys()
                    .all(|k| k == "type" || k == "description" || k == "title")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ctr::CtrFixPatch;
    use crate::models::content_review::ContentFixPatch;

    #[test]
    fn schemars_tool_parameters_ctr_fix_patch_has_no_anyof_or_defs() {
        let sanitized = schemars_tool_parameters::<CtrFixPatch>().unwrap();
        let s = sanitized.to_string();
        assert!(!s.contains("\"anyOf\""), "sanitized still has anyOf: {}", s);
        assert!(!s.contains("\"oneOf\""), "sanitized still has oneOf: {}", s);
        assert!(!s.contains("$ref"), "sanitized still has $ref: {}", s);
        assert!(!s.contains("$defs"), "sanitized still has $defs: {}", s);
        assert_eq!(
            sanitized.get("type").and_then(|t| t.as_str()),
            Some("object")
        );
    }

    #[test]
    fn ctr_fix_patch_sanitized_has_no_anyof_or_defs() {
        let raw = schemars::schema_for!(CtrFixPatch);
        let raw_val = serde_json::to_value(&raw).unwrap();
        // Precondition: unsanitized schema has the hostile anyOf form.
        let raw_str = raw_val.to_string();
        assert!(
            raw_str.contains("anyOf") || raw_str.contains("$ref"),
            "expected raw CtrFixPatch schema to use anyOf/$ref (got: {})",
            raw_str
        );

        let sanitized = sanitize_tool_parameters(raw_val);
        let s = sanitized.to_string();
        assert!(!s.contains("\"anyOf\""), "sanitized still has anyOf: {}", s);
        assert!(!s.contains("\"oneOf\""), "sanitized still has oneOf: {}", s);
        assert!(!s.contains("$ref"), "sanitized still has $ref: {}", s);
        assert!(!s.contains("$defs"), "sanitized still has $defs: {}", s);
        assert!(!s.contains("$schema"), "sanitized still has $schema: {}", s);
        assert!(!s.contains("int64"), "sanitized still has int64 format: {}", s);

        // snippet_patch must be a plain object (optional via not-required).
        let snippet = sanitized
            .pointer("/properties/changes/properties/snippet_patch")
            .expect("snippet_patch path");
        assert_eq!(
            snippet.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "snippet_patch should be type object after sanitize: {}",
            snippet
        );
        assert!(
            snippet.get("properties").is_some(),
            "snippet_patch should have inlined properties: {}",
            snippet
        );
    }

    #[test]
    fn content_fix_patch_still_sane_after_sanitize() {
        let raw = schemars::schema_for!(ContentFixPatch);
        let sanitized = sanitize_tool_parameters(serde_json::to_value(&raw).unwrap());
        let s = sanitized.to_string();
        assert!(!s.contains("$ref"));
        assert!(!s.contains("$defs"));
        // Scalar option fields keep type unions.
        let title = sanitized
            .pointer("/properties/changes/properties/title")
            .expect("title");
        let title_type = title.get("type").unwrap();
        assert!(
            title_type.as_str() == Some("string")
                || title_type
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("string"))),
            "title type unexpected: {}",
            title
        );
    }

    #[test]
    fn collapses_anyof_object_null() {
        let input = json!({
            "type": "object",
            "properties": {
                "nested": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": { "a": { "type": "string" } },
                            "required": ["a"]
                        },
                        { "type": "null" }
                    ]
                }
            }
        });
        let out = sanitize_tool_parameters(input);
        let nested = out.pointer("/properties/nested").unwrap();
        assert_eq!(nested.get("type").and_then(|t| t.as_str()), Some("object"));
        assert!(nested.get("anyOf").is_none());
    }
}
