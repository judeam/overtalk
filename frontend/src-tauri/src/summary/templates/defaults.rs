//! Embedded default templates using compile-time inclusion
//!
//! These templates are bundled into the binary and serve as fallbacks
//! when custom templates are not available.

/// Daily standup template for engineering/product teams
pub(super) const DAILY_STANDUP: &str = include_str!("../../../templates/daily_standup.json");

/// Standard meeting notes template
pub(super) const STANDARD_MEETING: &str = include_str!("../../../templates/standard_meeting.json");
pub(super) const PROJECT_SYNC: &str = include_str!("../../../templates/project_sync.json");
pub(super) const RETROSPECTIVE: &str = include_str!("../../../templates/retrospective.json");
pub(super) const CLIENT_CALL: &str =
    include_str!("../../../templates/sales_marketing_client_call.json");

/// Registry of all built-in templates
///
/// Maps template identifiers to their embedded JSON content
pub(super) fn get_builtin_templates() -> Vec<(&'static str, &'static str)> {
    vec![
        ("daily_standup", DAILY_STANDUP),
        ("standard_meeting", STANDARD_MEETING),
        ("project_sync", PROJECT_SYNC),
        ("retrospective", RETROSPECTIVE),
        ("sales_marketing_client_call", CLIENT_CALL),
    ]
}

/// Get a built-in template by identifier
///
/// # Arguments
/// * `id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// The template JSON content if found, None otherwise
pub(super) fn get_builtin_template(id: &str) -> Option<&'static str> {
    get_builtin_templates()
        .into_iter()
        .find(|(key, _)| *key == id)
        .map(|(_, json)| json)
}

/// List all built-in template identifiers
pub(super) fn list_builtin_template_ids() -> Vec<&'static str> {
    get_builtin_templates()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_valid_json() {
        for (id, content) in get_builtin_templates() {
            let result = serde_json::from_str::<serde_json::Value>(content);
            assert!(
                result.is_ok(),
                "Built-in template '{}' contains invalid JSON: {:?}",
                id,
                result.err()
            );
        }
    }

    #[test]
    fn test_get_builtin_template() {
        assert!(get_builtin_template("daily_standup").is_some());
        assert!(get_builtin_template("standard_meeting").is_some());
        assert!(get_builtin_template("nonexistent").is_none());
    }
}
