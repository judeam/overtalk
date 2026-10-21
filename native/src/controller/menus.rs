use super::*;

/// Presentation belongs to the native client; command IDs stay unchanged.
pub(super) fn present(action: &str, id: String, title: String) -> DetailRow {
    let (description, icon) = match id.as_str() {
        "hub-history" => (
            "Compare saved versions and restore earlier notes.",
            "history",
        ),
        "hub-learn" => (
            "Suggest project facts from this meeting for your review.",
            "notes",
        ),
        "hub-redact" => (
            "Remove selected names or phrases from an exported copy.",
            "shield",
        ),
        "hub-redact-audio" => (
            "Silence chosen time ranges in an exported recording.",
            "audio",
        ),
        "hub-assign" => (
            "Organize this meeting with its people and project context.",
            "folder",
        ),
        "hub-video-open" => ("Play the video saved with this meeting.", "play"),
        "share-meeting" => (
            "A page anyone can open, or an Overtalk file for a teammate.",
            "share",
        ),
        "pdf" => ("A formatted document ready to print or share.", "download"),
        "docx" => (
            "An editable document for Word and compatible editors.",
            "download",
        ),
        "markdown" => (
            "Portable notes with headings, lists and source links.",
            "notes",
        ),
        "text" => ("A simple text copy for any editor.", "notes"),
        "json" => ("Structured meeting data for other tools.", "download"),
        "rename" => ("Give this meeting a name you can find later.", "edit"),
        "delete" | "trash" => ("Move this item to Trash.", "trash"),
        "hub-reminders" => (
            "Review reminders attached to your action items.",
            "calendar",
        ),
        "hub-backups" => ("Create, restore and schedule encrypted backups.", "shield"),
        "hub-connections" => ("Link calendars, project tools and documents.", "link"),
        "localSpeech" => ("Local speech recognition and speaker separation.", "audio"),
        "builtin-ai" => ("Download and run a notes model on this device.", "notes"),
        "privatemode" => (
            "Use encrypted cloud inference through Privatemode.",
            "shield",
        ),
        _ if action == "hub-assign-choice" => ("", "folder"),
        _ if action.contains("voice") => ("", "person"),
        _ if action.contains("template") => ("", "notes"),
        _ if action.contains("provider") || action.contains("model") => ("", "settings"),
        _ => ("", "arrow"),
    };
    let danger = matches!(
        id.as_str(),
        "delete" | "trash" | "delete-permanently" | "forget-voice"
    );
    DetailRow {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        value: icon.into(),
        checked: danger,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meeting_tools_keep_command_ids_and_explain_their_effects() {
        for id in [
            "hub-history",
            "hub-learn",
            "hub-redact",
            "hub-redact-audio",
            "hub-assign",
            "share-meeting",
        ] {
            let row = present("hub-meeting-tool", id.into(), id.into());
            assert_eq!(row.id.as_str(), id);
            assert!(!row.description.is_empty());
            assert!(!row.value.is_empty());
            assert!(!row.checked);
        }
        assert!(present("row-options", "trash".into(), "Move to trash".into()).checked);
    }
}
