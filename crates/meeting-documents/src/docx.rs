use crate::{
    layout::{self, Block, Run},
    xml, MeetingDocument,
};

fn paragraph(runs: &[Run], level: u8) -> String {
    let size = match level {
        1 => 36,
        2 => 30,
        3..=6 => 26,
        _ => 22,
    };
    let mut output = format!(
        "<w:p><w:pPr><w:spacing w:after=\"160\"/>{}</w:pPr>",
        if level > 0 { "<w:keepNext/>" } else { "" }
    );
    for run in runs {
        output.push_str(&format!(
            "<w:r><w:rPr><w:sz w:val=\"{size}\"/>{}{}{}</w:rPr>",
            if run.bold || level > 0 { "<w:b/>" } else { "" },
            if run.italic { "<w:i/>" } else { "" },
            if run.code {
                "<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/>"
            } else {
                ""
            }
        ));
        for (index, line) in run.text.split('\n').enumerate() {
            if index > 0 {
                output.push_str("<w:br/>");
            }
            output.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", xml(line)));
        }
        output.push_str("</w:r>");
    }
    output.push_str("</w:p>");
    output
}
fn heading(text: &str, level: u8) -> String {
    paragraph(
        &[Run {
            text: text.into(),
            ..Default::default()
        }],
        level,
    )
}
pub(crate) fn document(meeting: &MeetingDocument) -> String {
    let mut output = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:body>"#
    ));
    output.push_str(&heading(&meeting.title, 1));
    output.push_str(&heading(&meeting.date, 0));
    if !meeting.notice.is_empty() {
        output.push_str(&heading(&meeting.notice, 0));
    }
    output.push_str(&heading("Notes", 2));
    for block in layout::parse(&meeting.notes) {
        match block {
            Block::Paragraph { runs, level } => output.push_str(&paragraph(&runs, level)),
            Block::Table(rows) => {
                let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let column_width = 9638 / columns;
                output.push_str(concat!(
                    r#"<w:tbl><w:tblPr><w:tblW w:w="9638" w:type="dxa"/>"#,
                    r#"<w:tblLayout w:type="fixed"/><w:tblBorders>"#,
                    r#"<w:top w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"<w:left w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"<w:bottom w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"<w:right w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"<w:insideH w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"<w:insideV w:val="single" w:sz="4" w:color="CCD4D9"/>"#,
                    r#"</w:tblBorders></w:tblPr>"#
                ));
                output.push_str("<w:tblGrid>");
                for _ in 0..columns {
                    output.push_str(&format!("<w:gridCol w:w=\"{column_width}\"/>"));
                }
                output.push_str("</w:tblGrid>");
                for (index, row) in rows.iter().enumerate() {
                    output.push_str(if index == 0 {
                        "<w:tr><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr>"
                    } else {
                        "<w:tr><w:trPr><w:cantSplit/></w:trPr>"
                    });
                    for cell in row {
                        output.push_str(&format!(
                            "<w:tc><w:tcPr><w:tcW w:w=\"{column_width}\" w:type=\"dxa\"/>\
                             {}</w:tcPr>",
                            if index == 0 {
                                "<w:shd w:fill=\"EBF2F5\"/>"
                            } else {
                                ""
                            }
                        ));
                        output.push_str(&paragraph(cell, if index == 0 { 3 } else { 0 }));
                        output.push_str("</w:tc>");
                    }
                    output.push_str("</w:tr>");
                }
                output.push_str("</w:tbl><w:p/>");
            }
        }
    }
    output.push_str(&heading("Transcript", 2));
    for passage in &meeting.passages {
        output.push_str(&heading(
            &format!("[{}] {}", passage.time, passage.speaker),
            3,
        ));
        output.push_str(&heading(&passage.text, 0));
    }
    output.push_str(concat!(
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134"/>"#,
        r#"</w:sectPr></w:body></w:document>"#
    ));
    output
}
