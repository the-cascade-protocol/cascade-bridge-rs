use crate::error::Result;
use crate::terms::BRIDGE_SCHEMA_RULE_UNNAMED;

pub(crate) mod json_schema;
pub(crate) mod xsd;

#[cfg(test)]
mod cda;

/// Whether a finding's body is a rule of a source schema, which only a source schema's check writes.
pub(crate) fn names_a_schema_rule(body: &str) -> bool {
    body == BRIDGE_SCHEMA_RULE_UNNAMED || xsd::names_a_rule(body) || json_schema::names_a_rule(body)
}

pub(crate) trait Schema: Send {
    fn errors(&self, text: &str) -> Result<Vec<SchemaFinding>>;
}

pub(crate) struct SchemaFinding {
    body: String,
    within: Option<String>,
}

impl SchemaFinding {
    pub(crate) fn body(&self) -> &str {
        &self.body
    }

    /// Relative to what was validated; none where it is that itself.
    pub(crate) fn within(&self) -> Option<&str> {
        self.within.as_deref()
    }
}
