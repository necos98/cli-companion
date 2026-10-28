//! Catalogo delle azioni: unica fonte per tool nativi, JSON schema e prompt.

use serde_json::{json, Map, Value};

pub const OPEN: &str = "open";
pub const CLICK: &str = "click";
pub const FILL: &str = "fill";
pub const SELECT: &str = "select";
pub const PRESS: &str = "press";
pub const DONE: &str = "done";

pub const FIELD_PATH: &str = "path";
pub const FIELD_REF: &str = "ref";
pub const FIELD_TEXT: &str = "text";
pub const FIELD_VALUE: &str = "value";
pub const FIELD_KEY: &str = "key";
pub const FIELD_SUCCESS: &str = "success";
pub const FIELD_NOTE: &str = "note";
/// Campo discriminante nella modalita' a schema.
pub const DISCRIMINATOR: &str = "action";

#[derive(Clone, Copy)]
pub enum FieldKind {
    Text,
    Flag,
}

pub struct FieldSpec {
    pub name: &'static str,
    pub kind: FieldKind,
    pub description: &'static str,
}

pub struct ActionSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub fields: &'static [FieldSpec],
}

const fn text(name: &'static str, description: &'static str) -> FieldSpec {
    FieldSpec {
        name,
        kind: FieldKind::Text,
        description,
    }
}

const REF_FIELD: FieldSpec = text(FIELD_REF, "Element ref from the latest snapshot, e.g. e12.");

pub static ACTIONS: [ActionSpec; 6] = [
    ActionSpec {
        name: OPEN,
        description: "Navigate the browser to a site-relative path.",
        fields: &[text(FIELD_PATH, "Site-relative path, e.g. /register.")],
    },
    ActionSpec {
        name: CLICK,
        description: "Click an element.",
        fields: &[REF_FIELD],
    },
    ActionSpec {
        name: FILL,
        description: "Replace the content of a text field.",
        fields: &[REF_FIELD, text(FIELD_TEXT, "Text to type into the field.")],
    },
    ActionSpec {
        name: SELECT,
        description: "Choose an option in a dropdown.",
        fields: &[REF_FIELD, text(FIELD_VALUE, "Option value or label.")],
    },
    ActionSpec {
        name: PRESS,
        description: "Press a keyboard key.",
        fields: &[text(FIELD_KEY, "Key name, e.g. Enter or Tab.")],
    },
    ActionSpec {
        name: DONE,
        description: "Finish the step and report the outcome.",
        fields: &[
            FieldSpec {
                name: FIELD_SUCCESS,
                kind: FieldKind::Flag,
                description: "true if the step was completed, false if it could not be.",
            },
            text(FIELD_NOTE, "Short note of what you observed."),
        ],
    },
];

fn property(field: &FieldSpec, with_description: bool) -> Value {
    let json_type = match field.kind {
        FieldKind::Text => "string",
        FieldKind::Flag => "boolean",
    };
    let mut property = json!({ "type": json_type });
    if with_description {
        property["description"] = json!(field.description);
    }
    property
}

fn properties(spec: &ActionSpec, with_description: bool) -> Map<String, Value> {
    spec.fields
        .iter()
        .map(|field| (field.name.to_string(), property(field, with_description)))
        .collect()
}

fn field_names(spec: &ActionSpec) -> Vec<&'static str> {
    spec.fields.iter().map(|field| field.name).collect()
}

/// Definizioni `tools` per le chiamate native.
pub fn native_tools() -> Vec<Value> {
    ACTIONS
        .iter()
        .map(|spec| {
            json!({
                "type": "function",
                "function": {
                    "name": spec.name,
                    "description": spec.description,
                    "parameters": {
                        "type": "object",
                        "properties": properties(spec, true),
                        "required": field_names(spec),
                        "additionalProperties": false,
                    },
                },
            })
        })
        .collect()
}

/// `response_format` json_schema: `oneOf` discriminato da `action`.
pub fn response_format() -> Value {
    let variants: Vec<Value> = ACTIONS
        .iter()
        .map(|spec| {
            let mut props = properties(spec, false);
            props.insert(DISCRIMINATOR.to_string(), json!({ "const": spec.name }));
            let mut required = vec![DISCRIMINATOR];
            required.extend(field_names(spec));
            json!({
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false,
            })
        })
        .collect();
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "action",
            "strict": true,
            "schema": { "oneOf": variants },
        },
    })
}

/// Elenco testuale delle azioni per il system prompt.
pub fn describe_actions() -> String {
    ACTIONS
        .iter()
        .map(|spec| {
            let args = field_names(spec).join(", ");
            let fields: Vec<String> = spec
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.name, f.description))
                .collect();
            format!(
                "- {}({}): {} {}",
                spec.name,
                args,
                spec.description,
                fields.join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_tools_cover_all_actions() {
        let tools = native_tools();
        assert_eq!(tools.len(), 6);
        assert_eq!(tools[2]["function"]["name"], "fill");
        assert_eq!(
            tools[2]["function"]["parameters"]["required"],
            json!(["ref", "text"])
        );
    }

    #[test]
    fn schema_variants_are_closed_and_discriminated() {
        let format = response_format();
        let variants = format["json_schema"]["schema"]["oneOf"].as_array().unwrap();
        assert_eq!(variants.len(), 6);
        assert_eq!(variants[1]["properties"]["action"]["const"], "click");
        assert_eq!(variants[1]["required"], json!(["action", "ref"]));
        assert_eq!(variants[5]["properties"]["success"]["type"], "boolean");
        assert!(variants.iter().all(|v| v["additionalProperties"] == false));
    }

    #[test]
    fn description_lists_every_action() {
        let text = describe_actions();
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("- done(success, note)"));
    }
}
