//! Local document export. No JavaScript, browser, external process or remote assets.
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};

mod docx;
pub mod html;
mod layout;
mod notes;
mod pdf;
mod word_import;
pub use notes::notes_markdown;
pub use word_import::{append_docx, read_docx};

#[derive(Debug, Serialize, Deserialize)]
pub struct Passage {
    pub time: String,
    pub speaker: String,
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct MeetingDocument {
    pub title: String,
    pub date: String,
    /// How participants were told about the recording; empty when not recorded.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notice: String,
    pub notes: String,
    pub passages: Vec<Passage>,
}

const CONTENT_TYPES_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels""#,
    r#" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
    r#"<Override PartName="/word/document.xml" ContentType=""#,
    r#"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    r#"</Types>"#
);

const PACKAGE_RELATIONSHIPS_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1""#,
    r#" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument""#,
    r#" Target="word/document.xml"/>"#,
    r#"</Relationships>"#
);

impl MeetingDocument {
    pub fn transcript(&self) -> String {
        self.passages
            .iter()
            .map(|p| format!("[{}] {}\n{}", p.time, p.speaker, p.text))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
    pub fn markdown(&self) -> String {
        format!(
            "# {}\n\n{}\n\n{}## Notes\n\n{}\n\n## Transcript\n\n{}\n",
            self.title,
            self.date,
            if self.notice.is_empty() {
                String::new()
            } else {
                format!("{}\n\n", self.notice)
            },
            self.notes,
            self.transcript()
        )
    }
    pub fn render(&self, format: &str) -> Result<Vec<u8>, String> {
        if matches!(format, "docx" | "text") {
            layout::validate(&self.markdown())?;
        } else if format == "pdf" {
            layout::validate(&self.notes)?;
        }
        match format {
            "markdown" => Ok(self.markdown().into_bytes()),
            "text" => Ok(layout::parse(&self.markdown())
                .iter()
                .flat_map(|block| block.plain())
                .map(|(text, _)| text)
                .collect::<Vec<_>>()
                .join("\n\n")
                .into_bytes()),
            "json" => serde_json::to_vec_pretty(self).map_err(|e| e.to_string()),
            "docx" => self.docx(),
            "pdf" => pdf::render(self),
            _ => Err("Unsupported export format".into()),
        }
    }
    fn docx(&self) -> Result<Vec<u8>, String> {
        let document = docx::document(self);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in [
            ("[Content_Types].xml", CONTENT_TYPES_XML),
            ("_rels/.rels", PACKAGE_RELATIONSHIPS_XML),
            ("word/document.xml", document.as_str()),
        ] {
            archive
                .start_file(name, options)
                .map_err(|e| e.to_string())?;
            archive
                .write_all(contents.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        Ok(archive.finish().map_err(|e| e.to_string())?.into_inner())
    }
}
fn xml(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || *c == '\r' || *c >= ' ')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meeting() -> MeetingDocument {
        MeetingDocument {
            title: "Reunião <plan> & review".into(),
            date: "2026-10-14".into(),
            notice: String::new(),
            notes: "## Decisions\nShip safely.".into(),
            passages: vec![Passage {
                time: "01:02".into(),
                speaker: "Léna".into(),
                text: "Ação: improve the interface.".into(),
            }],
        }
    }
    #[test]
    fn exports_keep_unicode_and_escape_xml() {
        let d = meeting();
        assert!(String::from_utf8(d.render("markdown").unwrap())
            .unwrap()
            .contains("Ação"));
        let bytes = d.render("docx").unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("word/document.xml").unwrap(),
            &mut contents,
        )
        .unwrap();
        assert!(contents.contains("&lt;plan&gt; &amp;"));
        assert!(contents.contains("Léna"));
    }
    #[test]
    fn word_exports_real_tables_and_formatted_text() {
        let mut doc = meeting();
        doc.notes =
            "## Decisions\n\n**Approved**\n\n| Owner | Task |\n| --- | --- |\n| Léna | Review |"
                .into();
        let bytes = doc.render("docx").unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("<w:tbl>"));
        assert!(xml.contains("<w:tblHeader/>"));
        assert!(xml.contains("<w:b/>") && !xml.contains("**Approved**"));
        assert!(xml.contains("Léna"));
        let plain = String::from_utf8(doc.render("text").unwrap()).unwrap();
        assert!(plain.contains("Approved") && !plain.contains("**"));
    }
    #[test]
    fn every_format_states_how_participants_were_told() {
        let mut doc = meeting();
        doc.notice = "Participants were notified at 10:02 (notice copied to chat)".into();
        for format in ["markdown", "text", "json"] {
            let output = String::from_utf8(doc.render(format).unwrap()).unwrap();
            assert!(output.contains(&doc.notice), "{format}");
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(doc.render("docx").unwrap())).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains("notice copied to chat"));
        assert!(!doc.render("pdf").unwrap().is_empty());
        // A meeting without a record keeps the previous layout.
        assert!(meeting()
            .markdown()
            .starts_with("# Reunião <plan> & review\n\n2026-10-14\n\n## Notes"));
    }
    #[test]
    fn pdf_is_searchable_and_rejects_unknown_format() {
        let d = meeting();
        let bytes = d.render("pdf").unwrap();
        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert!(bytes.windows(10).any(|w| w == b"/ToUnicode"));
        assert!(d.render("exe").is_err());
    }
}
