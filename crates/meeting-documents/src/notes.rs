//! Read existing web-editor documents without dropping nested lists, links or tables.
use serde_json::Value;

pub fn notes_markdown(value: &Value) -> Result<String, String> {
    const MAX_TEXT: usize = 8 * 1024 * 1024;
    if let Some(markdown) = value.get("markdown").and_then(Value::as_str) {
        return if markdown.len() <= MAX_TEXT {
            crate::layout::validate(markdown)?;
            Ok(markdown.into())
        } else {
            Err("Saved notes exceed the 8 MiB reading limit; the original is unchanged".into())
        };
    }
    let mut budget = RenderBudget { nodes: 0, bytes: 0 };
    budget.check(value, 0)?;
    let rendered = render_notes(value);
    if rendered.len() > MAX_TEXT {
        return Err("Saved notes exceed the reading limit; the original is unchanged".into());
    }
    crate::layout::validate(&rendered)?;
    Ok(rendered)
}

// Reject amplification before building padded tables or recursively indenting
// children. This deliberately overestimates escapes, separators and indentation.
struct RenderBudget {
    nodes: usize,
    bytes: usize,
}
impl RenderBudget {
    fn add(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > 8 * 1024 * 1024 {
            return Err(
                "Saved notes are too complex to display safely; the original is unchanged".into(),
            );
        }
        Ok(())
    }
    fn check(&mut self, value: &Value, depth: usize) -> Result<(), String> {
        self.nodes += 1;
        if depth > 48 || self.nodes > 100_000 {
            return Err(
                "Saved notes exceed the nesting or structure limit; the original is unchanged"
                    .into(),
            );
        }
        self.add(128 + depth * 4)?;
        match value {
            Value::String(text) => self.add(
                text.len()
                    .saturating_mul(4)
                    .saturating_add(text.bytes().filter(|b| *b == b'\n').count() * depth * 4),
            )?,
            Value::Array(items) => {
                for item in items {
                    self.check(item, depth + 1)?;
                }
            }
            Value::Object(items) => {
                if value["type"] == "table" {
                    if let Some(rows) = value["content"]["rows"].as_array() {
                        let columns = rows
                            .iter()
                            .filter_map(|row| row["cells"].as_array().map(Vec::len))
                            .max()
                            .unwrap_or(0);
                        let cells = columns.saturating_mul(rows.len());
                        if columns > 256 || cells > 100_000 {
                            return Err(
                                "Saved notes contain an oversized table; the original is unchanged"
                                    .into(),
                            );
                        }
                        self.add(cells * 16 + rows.len() * depth * 4)?;
                    }
                }
                for (key, item) in items {
                    self.add(key.len().saturating_mul(4))?;
                    self.check(item, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn render_notes(value: &Value) -> String {
    if let Some(markdown) = value.get("markdown").and_then(Value::as_str) {
        return markdown.into();
    }
    if let Some(blocks) = value.get("summary_json").and_then(Value::as_array) {
        return blocks.iter().map(block).collect::<Vec<_>>().join("\n\n");
    }
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|text| format!("## {key}\n\n{text}")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn inline(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    if let Some(items) = value.as_array() {
        return items.iter().map(inline).collect();
    }
    if value["type"] == "link" {
        let label = inline(&value["content"]);
        let href = value["href"]
            .as_str()
            .unwrap_or_default()
            .replace(')', "%29");
        return format!("[{label}]({href})");
    }
    let mut text = value["text"].as_str().unwrap_or_default().to_owned();
    let styles = &value["styles"];
    if styles["code"] == true {
        return format!("`{}`", text.replace('`', "\\`"));
    }
    if styles["bold"] == true {
        text = format!("**{text}**");
    }
    if styles["italic"] == true {
        text = format!("*{text}*");
    }
    if styles["strike"] == true {
        text = format!("~~{text}~~");
    }
    text
}

fn block(value: &Value) -> String {
    let text = inline(&value["content"]);
    let props = &value["props"];
    let mut rendered = match value["type"].as_str().unwrap_or("paragraph") {
        "heading" => format!(
            "{} {text}",
            "#".repeat(props["level"].as_u64().unwrap_or(2).clamp(1, 6) as usize)
        ),
        "bulletListItem" => format!("- {text}"),
        "numberedListItem" => format!("1. {text}"),
        "checkListItem" => format!(
            "- [{}] {text}",
            if props["checked"] == true { "x" } else { " " }
        ),
        "quote" => format!("> {text}"),
        "codeBlock" => format!(
            "```{}\n{text}\n```",
            props["language"].as_str().unwrap_or_default()
        ),
        "table" => {
            let rows = value["content"]["rows"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let columns = rows
                .iter()
                .filter_map(|row| row["cells"].as_array().map(Vec::len))
                .max()
                .unwrap_or(0);
            let mut result = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                let cells = row["cells"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let cells = (0..columns)
                    .map(|i| {
                        cells
                            .get(i)
                            .map(|cell| {
                                inline(cell.get("content").unwrap_or(cell))
                                    .replace('|', "\\|")
                                    .replace('\n', "<br>")
                            })
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                result.push(format!("| {} |", cells.join(" | ")));
                if index == 0 {
                    result.push(format!("| {} |", vec!["---"; columns].join(" | ")));
                }
            }
            result.join("\n")
        }
        "image" | "video" | "audio" | "file" => format!(
            "[{}]({})",
            props["caption"]
                .as_str()
                .or_else(|| props["name"].as_str())
                .unwrap_or("Attachment"),
            props["url"].as_str().unwrap_or_default()
        ),
        _ => text,
    };
    if let Some(children) = value["children"].as_array() {
        for child in children {
            rendered.push('\n');
            rendered.push_str(
                &block(child)
                    .lines()
                    .map(|line| format!("    {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_web_editor_structure_and_inline_content() {
        let notes = json!({"summary_json":[
            {"type":"heading","props":{"level":2},"content":[{"text":"Decisions"}]},
            {"type":"bulletListItem","content":[{"text":"Ship","styles":{"bold":true}},{"type":"link","href":"https://example.org","content":[{"text":" source"}]}],"children":[{"type":"checkListItem","props":{"checked":true},"content":[{"text":"Reviewed"}]}]},
            {"type":"table","content":{"rows":[{"cells":[[{"text":"Owner"}],[{"text":"Task"}]]},{"cells":[[{"text":"Léna"}],[{"text":"Review"}]]}]}}
        ]});
        let markdown = notes_markdown(&notes).unwrap();
        assert!(markdown.contains("## Decisions"));
        assert!(markdown.contains("- **Ship**[ source](https://example.org)\n    - [x] Reviewed"));
        assert!(markdown.contains("| Owner | Task |\n| --- | --- |\n| Léna | Review |"));
    }
    #[test]
    fn sparse_tables_cannot_amplify_a_small_document() {
        let mut rows = vec![json!({"cells": vec![json!(""); 512]})];
        rows.extend((0..512).map(|_| json!({"cells":[]})));
        let notes = json!({"summary_json":[{"type":"table","content":{"rows":rows}}]});
        assert!(notes_markdown(&notes).is_err());
    }
    #[test]
    fn deeply_nested_notes_are_rejected_without_rendering() {
        let mut node = json!({"type":"paragraph","content":[{"text":"Text"}]});
        for _ in 0..60 {
            node = json!({"type":"paragraph","children":[node]});
        }
        assert!(notes_markdown(&json!({"summary_json":[node]})).is_err());
    }
    #[test]
    fn raw_markdown_tables_have_the_same_complexity_boundary() {
        let header = format!("|{}\n|{}\n", "field|".repeat(200), "---|".repeat(200));
        let markdown = header + &"|value|\n".repeat(501);
        assert!(notes_markdown(&json!({"markdown":markdown})).is_err());
    }
}
