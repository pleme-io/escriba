//! `escriba-api` — OpenAPI 3.1 spec generator. Every public type with
//! `schemars::JsonSchema` is emitted into the spec; every Command is a path.

use escriba_command::{CommandRegistry, CommandSpec};
use escriba_config::{CommandDecl, EscribaConfig, KeymapDecl, MajorMode, MinorMode, PluginDecl};
use escriba_core::{Action, Mode, Motion, Operator, Position, Range};
use escriba_mode::ModalState;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSpec(pub Value);

impl OpenApiSpec {
    #[must_use]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.0).unwrap_or_default()
    }

    #[must_use]
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(&self.0).unwrap_or_default()
    }
}

#[must_use]
pub fn build_spec() -> OpenApiSpec {
    let mut schemas = serde_json::Map::new();
    insert_schema(&mut schemas, "Position", schema_for!(Position));
    insert_schema(&mut schemas, "Range", schema_for!(Range));
    insert_schema(&mut schemas, "Mode", schema_for!(Mode));
    insert_schema(&mut schemas, "Motion", schema_for!(Motion));
    insert_schema(&mut schemas, "Operator", schema_for!(Operator));
    insert_schema(&mut schemas, "Action", schema_for!(Action));
    insert_schema(&mut schemas, "ModalState", schema_for!(ModalState));
    insert_schema(&mut schemas, "EscribaConfig", schema_for!(EscribaConfig));
    insert_schema(&mut schemas, "KeymapDecl", schema_for!(KeymapDecl));
    insert_schema(&mut schemas, "CommandDecl", schema_for!(CommandDecl));
    insert_schema(&mut schemas, "PluginDecl", schema_for!(PluginDecl));
    insert_schema(&mut schemas, "MajorMode", schema_for!(MajorMode));
    insert_schema(&mut schemas, "MinorMode", schema_for!(MinorMode));
    insert_schema(&mut schemas, "CommandSpec", schema_for!(CommandSpec));

    let commands = CommandRegistry::default_set().specs();
    let mut paths = serde_json::Map::new();
    for c in &commands {
        paths.insert(
            format!("/commands/{}", c.name),
            json!({
                "post": {
                    "summary": c.description,
                    "operationId": format!("run_{}", c.name.replace('-', "_")),
                    "tags": ["commands"],
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": { "type": "array", "items": { "type": "string" } } } }
                    },
                    "responses": {
                        "200": { "description": "command executed" },
                        "404": { "description": "command not found" },
                        "500": { "description": "command failed" }
                    }
                }
            }),
        );
    }

    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "escriba — the Rust + tatara-lisp editor",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Public API surface for escriba. Every type below is derived from a Rust struct annotated with #[derive(JsonSchema)] or #[derive(TataraDomain)]; this spec is the source of truth for every SDK, the MCP server, and the documentation site.",
            "license": { "name": "MIT" }
        },
        "servers": [ { "url": "unix:///tmp/escriba.sock", "description": "local editor control socket" } ],
        "paths": Value::Object(paths),
        "components": { "schemas": Value::Object(schemas) }
    });
    OpenApiSpec(spec)
}

fn insert_schema(
    map: &mut serde_json::Map<String, Value>,
    name: &str,
    schema: schemars::schema::RootSchema,
) {
    let v = serde_json::to_value(&schema).unwrap_or(Value::Null);
    map.insert(name.to_string(), v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_has_core_schemas() {
        let s = build_spec();
        let schemas = s.0["components"]["schemas"].as_object().unwrap();
        for name in [
            "Position",
            "Range",
            "Mode",
            "Motion",
            "Operator",
            "Action",
            "EscribaConfig",
            "KeymapDecl",
            "CommandDecl",
            "PluginDecl",
            "MajorMode",
            "MinorMode",
        ] {
            assert!(schemas.contains_key(name), "missing schema: {name}");
        }
    }

    #[test]
    fn spec_has_command_paths() {
        let s = build_spec();
        let paths = s.0["paths"].as_object().unwrap();
        assert!(paths.contains_key("/commands/save"));
    }

    #[test]
    fn spec_version_matches_crate() {
        let s = build_spec();
        assert_eq!(
            s.0["info"]["version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn spec_is_valid_openapi_3_1() {
        let s = build_spec();
        assert_eq!(s.0["openapi"].as_str().unwrap(), "3.1.0");
    }
}
