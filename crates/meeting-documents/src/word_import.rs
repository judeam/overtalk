//! Bounded Word reading and append-only updates. Existing document parts,
//! media, tables and relationships stay intact; archives are never extracted.
use std::{
    collections::HashSet,
    io::{Cursor, Read, Write},
};
const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAX_PART: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: u64 = 32 * 1024 * 1024;
const MAX_NODES: u32 = 200_000;
fn parse_xml(xml: &str) -> Result<roxmltree::Document<'_>, String> {
    // roxmltree reserves from '<' and '=' counts before enforcing nodes_limit.
    // Bound that preallocation too, including malformed XML and attribute floods.
    let mut tags = 0u32;
    let mut attributes = 0u32;
    for byte in xml.bytes() {
        tags += u32::from(byte == b'<');
        attributes += u32::from(byte == b'=');
        if tags > 2 * MAX_NODES || attributes > 2 * MAX_NODES {
            return Err("Word XML exceeds the structural complexity limit".into());
        }
    }
    roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            nodes_limit: MAX_NODES,
            ..Default::default()
        },
    )
    .map_err(|_| "Word body contains unsupported or excessively complex XML".into())
}
fn document(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_PART as usize {
        return Err("Word document is larger than 16 MiB".into());
    }
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Not a readable Word document")?;
    if zip.len() > 512 {
        return Err("Word document contains too many parts".into());
    }
    let file = zip
        .by_name("word/document.xml")
        .map_err(|_| "Word document body is missing")?;
    if file.size() > MAX_PART {
        return Err("Word document body is too large".into());
    }
    let mut xml = String::new();
    file.take(MAX_PART + 1)
        .read_to_string(&mut xml)
        .map_err(|_| "Word body is not valid UTF-8")?;
    if xml.len() > MAX_PART as usize {
        return Err("Word document body is too large".into());
    }
    Ok(xml)
}
pub fn read_docx(bytes: &[u8]) -> Result<String, String> {
    let xml = document(bytes)?;
    let doc = parse_xml(&xml)?;
    let mut out = String::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(WORD_NS))
    {
        match node.tag_name().name() {
            "t" => out.push_str(node.text().unwrap_or("")),
            "tab" => out.push('\t'),
            "br" => out.push('\n'),
            "p" => {
                if !out.is_empty() {
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    Ok(out)
}
pub fn append_docx(bytes: &[u8], title: &str, body: &str) -> Result<Vec<u8>, String> {
    if title.len() > 300 || body.len() > 64_000 {
        return Err("Use a title and a document under 64 KiB".into());
    }
    let xml = document(bytes)?;
    let doc = parse_xml(&xml)?;
    let root = doc
        .descendants()
        .find(|n| n.has_tag_name((WORD_NS, "body")))
        .ok_or("Word body is missing")?;
    let prefix = root
        .lookup_prefix(WORD_NS)
        .ok_or("Word document uses an unsupported default namespace")?;
    let offset = root
        .children()
        .find(|n| n.has_tag_name((WORD_NS, "sectPr")))
        .map(|n| n.range().start)
        .or_else(|| {
            xml[root.range()]
                .rfind(&format!("</{prefix}:body>"))
                .map(|p| root.range().start + p)
        })
        .ok_or("Word body is not closed")?;
    let paragraphs=std::iter::once(title).chain(body.lines()).map(|line|format!("<{prefix}:p><{prefix}:r><{prefix}:t xml:space=\"preserve\">{}</{prefix}:t></{prefix}:r></{prefix}:p>",super::xml(line))).collect::<String>();
    let updated = format!("{}{}{}", &xml[..offset], paragraphs, &xml[offset..]);
    drop(doc);
    if updated.len() > MAX_PART as usize {
        return Err("Updated Word body exceeds the size limit".into());
    }
    // An update must remain readable under exactly the same parser budget.
    parse_xml(&updated)?;
    let mut source =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Invalid Word archive")?;
    if source.len() > 512 {
        return Err("Word document contains too many parts".into());
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut size = 0;
    let mut names = HashSet::new();
    for i in 0..source.len() {
        let file = source
            .by_index(i)
            .map_err(|_| "Invalid Word archive entry")?;
        let name = file
            .name()
            .map_err(|_| "Invalid Word part name")?
            .into_owned();
        if !names.insert(name.clone()) || file.enclosed_name().is_none() {
            return Err("Word document contains duplicate or unsafe paths".into());
        }
        size += if name == "word/document.xml" {
            updated.len() as u64
        } else {
            file.size()
        };
        if size > MAX_TOTAL || file.size() > MAX_PART {
            return Err("Word document expands beyond the size limit".into());
        }
        if file.is_dir() {
            out.add_directory(&name, options)
                .map_err(|_| "Could not preserve Word directory")?;
            continue;
        }
        out.start_file(&name, options)
            .map_err(|_| "Could not preserve Word part")?;
        if name == "word/document.xml" {
            out.write_all(updated.as_bytes())
                .map_err(|_| "Could not write Word notes")?;
        } else {
            let mut bytes = Vec::new();
            file.take(MAX_PART + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Could not read Word part")?;
            if bytes.len() as u64 > MAX_PART {
                return Err("Word part exceeds the size limit".into());
            }
            out.write_all(&bytes)
                .map_err(|_| "Could not preserve Word part")?;
        }
    }
    Ok(out
        .finish()
        .map_err(|_| "Could not finalize Word document")?
        .into_inner())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::MeetingDocument;
    fn fixture(xml: &str, extra_parts: usize) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        for i in 0..extra_parts {
            zip.start_file(format!("word/part-{i}.xml"), options)
                .unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn word_import_rejects_excessive_nodes_and_parts() {
        let xml = format!(
            "<w:document xmlns:w=\"{WORD_NS}\"><w:body>{}</w:body></w:document>",
            "<w:p/>".repeat(200_000)
        );
        let many_nodes = fixture(&xml, 0);
        assert!(
            read_docx(&many_nodes).is_err(),
            "XML node count must be bounded even when the compressed body is small"
        );
        assert!(append_docx(&many_nodes, "Notes", "Review").is_err());
        let many_parts = fixture(
            &format!("<w:document xmlns:w=\"{WORD_NS}\"><w:body/></w:document>"),
            512,
        );
        assert!(read_docx(&many_parts).is_err());
    }
    #[test]
    fn appending_notes_keeps_original_content_and_package_parts() {
        let original = MeetingDocument {
            title: "Original & Ω".into(),
            date: "".into(),
            notice: String::new(),
            notes: "| A | B |\n|---|---|\n|1|2|".into(),
            passages: vec![],
        }
        .render("docx")
        .unwrap();
        let updated = append_docx(&original, "Follow-up <today>", "Ana owns this.\nनमस्ते").unwrap();
        let text = read_docx(&updated).unwrap();
        assert!(text.contains("Original & Ω"));
        assert!(text.contains("Follow-up <today>"));
        assert!(text.contains("नमस्ते"));
        let mut old = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut new = zip::ZipArchive::new(Cursor::new(&updated)).unwrap();
        assert_eq!(old.len(), new.len());
        for name in ["[Content_Types].xml", "_rels/.rels"] {
            let mut a = Vec::new();
            let mut b = Vec::new();
            old.by_name(name).unwrap().read_to_end(&mut a).unwrap();
            new.by_name(name).unwrap().read_to_end(&mut b).unwrap();
            assert_eq!(a, b);
        }
    }
    #[test]
    fn malformed_or_external_entity_documents_are_rejected() {
        assert!(read_docx(b"not zip").is_err());
        let xml = r#"<!DOCTYPE x [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>&xxe;</w:t></w:r></w:p></w:body></w:document>"#;
        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        assert!(read_docx(&zip.finish().unwrap().into_inner()).is_err());
    }
}
