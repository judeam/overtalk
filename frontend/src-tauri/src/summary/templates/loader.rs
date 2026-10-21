use super::defaults;
use super::types::Template;
use once_cell::sync::Lazy;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::{debug, info, warn};

pub(super) const MAX_TEMPLATE_BYTES: usize = 256 * 1024;

pub(super) fn validate_template_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(
            "Template ID must contain only letters, numbers, underscores or hyphens".into(),
        );
    }
    Ok(())
}

pub fn is_builtin_template(id: &str) -> bool {
    defaults::get_builtin_template(id).is_some()
}

fn read_template_file(path: &Path) -> Result<String, String> {
    let bytes = crate::security::storage::read_limited(path, MAX_TEMPLATE_BYTES)
        .map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "Template is not valid UTF-8".into())
}

pub(super) fn save_template_in(dir: &Path, id: &str, template: &Template) -> Result<(), String> {
    validate_template_id(id)?;
    if is_builtin_template(id) {
        return Err("Built-in templates are read-only; create a copy first".into());
    }
    template.validate()?;
    let json = serde_json::to_vec_pretty(template).map_err(|e| e.to_string())?;
    if json.len() > MAX_TEMPLATE_BYTES {
        return Err("Template exceeds 256 KiB".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    temp.write_all(&json).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(dir.join(format!("{id}.json")))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_custom_template(id: &str, template: &Template) -> Result<(), String> {
    let dir = get_custom_templates_dir().ok_or("Application data directory is unavailable")?;
    save_template_in(&dir, id, template)
}

pub fn delete_custom_template(id: &str) -> Result<(), String> {
    validate_template_id(id)?;
    if is_builtin_template(id) {
        return Err("Built-in templates cannot be deleted".into());
    }
    let dir = get_custom_templates_dir().ok_or("Application data directory is unavailable")?;
    std::fs::remove_file(dir.join(format!("{id}.json"))).map_err(|e| e.to_string())
}

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Sonefold/templates/
/// - Windows: %APPDATA%\Sonefold\templates\
/// - Linux: ~/.local/share/Sonefold/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("Sonefold");
    path.push("templates");
    Some(path)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match read_template_file(&template_path) {
        Ok(content) => {
            info!(
                "Loaded bundled template '{}' from {:?}",
                template_id, template_path
            );
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_custom_template(template_id: &str) -> Option<String> {
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match read_template_file(&template_path) {
        Ok(content) => {
            info!(
                "Loaded custom template '{}' from {:?}",
                template_id, template_path
            );
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load and parse a template by identifier
///
/// Built-ins are immutable embedded resources. Other IDs resolve from the
/// user's custom directory, then bundled resources, or return an error.
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    validate_template_id(template_id)?;
    info!("Loading template: {}", template_id);

    // Resolve immutable built-ins before user-authored files.
    let json_content = if let Some(builtin_content) = defaults::get_builtin_template(template_id) {
        builtin_content.to_string()
    } else if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(template_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    if json_content.len() > MAX_TEMPLATE_BYTES {
        return Err("Template exceeds 256 KiB".into());
    }
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            if bundled_dir.exists() {
                match std::fs::read_dir(bundled_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = filename.trim_end_matches(".json").to_string();
                                    if validate_template_id(&id).is_ok() && !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read bundled templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        if custom_dir.exists() {
            match std::fs::read_dir(&custom_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".json") {
                                let id = filename.trim_end_matches(".json").to_string();
                                if validate_template_id(&id).is_ok() && !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read custom templates directory: {}", e);
                }
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_reserved_mutations() {
        for id in ["../secret", "a/b", "a\\b", ".", "", "/tmp/file"] {
            assert!(validate_template_id(id).is_err());
            assert!(get_template(id).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let template = get_template("standard_meeting").unwrap();
        assert!(save_template_in(dir.path(), "standard_meeting", &template).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn writes_and_replaces_custom_templates_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut template = get_template("standard_meeting").unwrap();
        save_template_in(dir.path(), "custom_test", &template).unwrap();
        template.name = "Renamed template".into();
        save_template_in(dir.path(), "custom_test", &template).unwrap();
        let stored = read_template_file(&dir.path().join("custom_test.json")).unwrap();
        assert_eq!(
            validate_and_parse_template(&stored).unwrap().name,
            "Renamed template"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        template.sections.clear();
        assert!(save_template_in(dir.path(), "custom_test", &template).is_err());
        assert_eq!(
            read_template_file(&dir.path().join("custom_test.json")).unwrap(),
            stored
        );
    }

    #[test]
    fn embeds_all_six_templates_and_bounds_content() {
        assert_eq!(defaults::list_builtin_template_ids().len(), 5);
        for id in defaults::list_builtin_template_ids() {
            assert!(get_template(id).is_ok());
        }
        assert!(validate_and_parse_template(&" ".repeat(MAX_TEMPLATE_BYTES + 1)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_reads() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.json");
        std::fs::write(&source, "{}").unwrap();
        let link = dir.path().join("linked.json");
        std::os::unix::fs::symlink(source, &link).unwrap();
        assert!(read_template_file(&link).is_err());
    }

    #[test]
    fn test_get_builtin_template() {
        let template = get_template("daily_standup");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Daily Standup");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"daily_standup".to_string()));
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
