//! What the generator's structured outputs share: citations, and the
//! changes that suit their JSON schemas to strict structured output.

use schemars::{JsonSchema, Schema, json_schema};
use serde::Deserialize;
use serde_json::Value;
use whetstone_core::WrittenCitation;

/// A citation as the generator writes it: a file's path and a range of its
/// lines.
#[derive(Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(transform = strict)]
pub(crate) struct CitationOutput {
    path: String,
    #[schemars(schema_with = "line_number")]
    first_line: usize,
    #[schemars(schema_with = "line_number")]
    last_line: usize,
}

impl CitationOutput {
    /// The citation as written, with a line it left out as line 0.
    pub(crate) fn into_written(self) -> WrittenCitation {
        WrittenCitation {
            path: self.path,
            first_line: self.first_line,
            last_line: self.last_line,
        }
    }
}

/// A line number: an integer, with no format or bounds, which not every
/// provider's strict structured output takes.
fn line_number(_: &mut schemars::SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer" })
}

/// Takes out what strict structured output doesn't need: the descriptions
/// that doc comments become, which would be prompt text outside the base
/// prompts, and the defaults that `#[serde(default)]` adds.
pub(crate) fn strict(schema: &mut Schema) {
    schema.remove("description");
    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
        for property in properties.values_mut() {
            if let Value::Object(property) = property {
                property.remove("description");
                property.remove("default");
            }
        }
    }
}
