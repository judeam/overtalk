use super::MeetingDocument;
use pdf_writer::{
    types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap},
    Content, Finish, Name, Pdf, Rect, Ref, Str,
};
use std::collections::BTreeMap;

const FONT: &[u8] =
    include_bytes!("../../../frontend/public/fonts/overtalk/InstrumentSans-Variable.ttf");

#[derive(Clone)]
enum Item {
    Text {
        text: String,
        x: f32,
        y: f32,
        size: f32,
        strong: bool,
    },
    Cell {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        header: bool,
    },
}
struct Layout<'a> {
    face: &'a ttf_parser::Face<'a>,
    pages: Vec<Vec<Item>>,
    y: f32,
}
impl<'a> Layout<'a> {
    fn new(face: &'a ttf_parser::Face<'a>) -> Self {
        Self {
            face,
            pages: vec![Vec::new()],
            y: 780.0,
        }
    }
    fn page(&mut self) {
        self.pages.push(Vec::new());
        self.y = 780.0;
    }
    fn item(&mut self, item: Item) {
        self.pages.last_mut().unwrap().push(item);
    }
    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|c| {
                let glyph = self.face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0));
                f32::from(self.face.glyph_hor_advance(glyph).unwrap_or(0)) * size
                    / f32::from(self.face.units_per_em())
            })
            .sum()
    }
    fn wrap(&self, text: &str, size: f32, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let joined = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.width(&joined, size) <= width {
                    line = joined;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                for character in word.chars() {
                    if self.width(&format!("{line}{character}"), size) > width && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push(character);
                }
            }
            lines.push(line);
        }
        lines
    }
    fn paragraph(&mut self, text: &str, size: f32, strong: bool) {
        let lines = self.wrap(text, size, 491.0);
        let leading = size * 1.45;
        if strong {
            self.y -= 9.0;
            if self.y - leading * 3.0 < 55.0 {
                self.page();
            }
        }
        for line in lines {
            if self.y - leading < 55.0 {
                self.page();
            }
            self.item(Item::Text {
                text: line,
                x: 52.0,
                y: self.y,
                size,
                strong,
            });
            self.y -= leading;
        }
        self.y -= 8.0;
    }
    fn table_row(
        &mut self,
        cells: &[Vec<String>],
        widths: &[f32],
        from: usize,
        count: usize,
        header: bool,
    ) {
        let height = count as f32 * 14.0 + 14.0;
        let mut x = 52.0;
        for (column, width) in widths.iter().enumerate() {
            self.item(Item::Cell {
                x,
                y: self.y,
                width: *width,
                height,
                header,
            });
            for (line, text) in cells[column].iter().skip(from).take(count).enumerate() {
                self.item(Item::Text {
                    text: text.clone(),
                    x: x + 7.0,
                    y: self.y - 17.0 - line as f32 * 14.0,
                    size: 10.0,
                    strong: header,
                });
            }
            x += width;
        }
        self.y -= height;
    }
    fn table(&mut self, rows: &[Vec<Vec<super::layout::Run>>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        // Give prose columns more room, while preserving a useful minimum for labels.
        let weights: Vec<f32> = (0..columns)
            .map(|c| {
                rows.iter()
                    .filter_map(|row| row.get(c))
                    .map(|cell| {
                        self.width(
                            &cell.iter().map(|r| r.text.as_str()).collect::<String>(),
                            10.0,
                        )
                    })
                    .fold(40.0_f32, f32::max)
                    .clamp(60.0, 280.0)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        let widths: Vec<f32> = weights.iter().map(|w| 491.0 * w / total).collect();
        let wrapped: Vec<Vec<Vec<String>>> = rows
            .iter()
            .map(|row| {
                (0..columns)
                    .map(|c| {
                        self.wrap(
                            &row.get(c)
                                .map(|cell| {
                                    cell.iter().map(|r| r.text.as_str()).collect::<String>()
                                })
                                .unwrap_or_default(),
                            10.0,
                            (widths[c] - 14.0).max(10.0),
                        )
                    })
                    .collect()
            })
            .collect();
        let header = &wrapped[0];
        let header_lines = header.iter().map(Vec::len).max().unwrap_or(1).min(20);
        let header_height = header_lines as f32 * 14.0 + 14.0;
        if self.y - header_height - 42.0 < 55.0 {
            self.page();
        }
        self.table_row(header, &widths, 0, header_lines, true);
        for cells in wrapped.iter().skip(1) {
            let lines = cells.iter().map(Vec::len).max().unwrap_or(1);
            let mut from = 0;
            while from < lines {
                let mut available = ((self.y - 55.0 - 14.0) / 14.0).floor().max(0.0) as usize;
                // Keep ordinary rows together. Very tall cells can continue across pages.
                if available == 0 || (from == 0 && lines > available && lines <= 40) {
                    self.page();
                    self.table_row(header, &widths, 0, header_lines, true);
                    available = ((self.y - 55.0 - 14.0) / 14.0).floor().max(1.0) as usize;
                }
                let count = (lines - from).min(available);
                self.table_row(cells, &widths, from, count, false);
                from += count;
            }
        }
        self.y -= 16.0;
    }
}

pub(super) fn render(document: &MeetingDocument) -> Result<Vec<u8>, String> {
    let face = ttf_parser::Face::parse(FONT, 0).map_err(|e| e.to_string())?;
    let unit = 1000.0 / f32::from(face.units_per_em());
    let mut pdf = Pdf::new();
    let font = Ref::new(3);
    let descendant = Ref::new(4);
    let descriptor = Ref::new(5);
    let stream = Ref::new(6);
    let unicode = Ref::new(7);
    let info = SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    };
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    pdf.type0_font(font)
        .base_font(Name(b"InstrumentSans"))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(descendant)
        .to_unicode(unicode);
    let mut cid = pdf.cid_font(descendant);
    cid.subtype(CidFontType::Type2)
        .base_font(Name(b"InstrumentSans"))
        .system_info(info)
        .font_descriptor(descriptor)
        .cid_to_gid_map_predefined(Name(b"Identity"));
    cid.widths().consecutive(
        0,
        (0..face.number_of_glyphs()).map(|id| {
            f32::from(face.glyph_hor_advance(ttf_parser::GlyphId(id)).unwrap_or(0)) * unit
        }),
    );
    cid.finish();
    let bbox = face.global_bounding_box();
    pdf.font_descriptor(descriptor)
        .name(Name(b"InstrumentSans"))
        .flags(FontFlags::NON_SYMBOLIC)
        .bbox(Rect::new(
            f32::from(bbox.x_min) * unit,
            f32::from(bbox.y_min) * unit,
            f32::from(bbox.x_max) * unit,
            f32::from(bbox.y_max) * unit,
        ))
        .italic_angle(0.0)
        .ascent(f32::from(face.ascender()) * unit)
        .descent(f32::from(face.descender()) * unit)
        .cap_height(f32::from(face.capital_height().unwrap_or(face.ascender())) * unit)
        .stem_v(80.0)
        .font_file2(stream);
    pdf.stream(stream, FONT)
        .pair(Name(b"Length1"), FONT.len() as i32);
    let mut layout = Layout::new(&face);
    layout.paragraph(&document.title, 22.0, true);
    layout.paragraph(&document.date, 10.0, false);
    if !document.notice.is_empty() {
        layout.paragraph(&document.notice, 10.0, false);
    }
    layout.paragraph("Notes", 17.0, true);
    for block in super::layout::parse(&document.notes) {
        match block {
            super::layout::Block::Paragraph { runs, level } => {
                let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                let size = match level {
                    1 => 20.0,
                    2 => 16.0,
                    3..=6 => 13.0,
                    _ => 11.0,
                };
                layout.paragraph(&text, size, level > 0);
            }
            super::layout::Block::Table(rows) => layout.table(&rows),
        }
    }
    layout.paragraph("Transcript", 17.0, true);
    for passage in &document.passages {
        layout.paragraph(
            &format!("[{}] {}", passage.time, passage.speaker),
            10.0,
            true,
        );
        layout.paragraph(&passage.text, 11.0, false);
    }
    let mut pages = layout.pages;
    let count = pages.len();
    for (index, page) in pages.iter_mut().enumerate() {
        page.push(Item::Text {
            text: format!("{} / {count}", index + 1),
            x: 510.0,
            y: 30.0,
            size: 9.0,
            strong: false,
        });
    }
    let mut characters = BTreeMap::new();
    for page in &pages {
        for item in page {
            if let Item::Text { text, .. } = item {
                for c in text.chars() {
                    characters
                        .entry(face.glyph_index(c).unwrap_or(ttf_parser::GlyphId(0)).0)
                        .or_insert(c);
                }
            }
        }
    }
    let mut cmap = UnicodeCmap::new(Name(b"OvertalkUnicode"), info);
    for (glyph, character) in characters {
        cmap.pair(glyph, character);
    }
    pdf.stream(unicode, &cmap.finish());
    let ids: Vec<_> = (0..pages.len())
        .map(|n| Ref::new(8 + n as i32 * 2))
        .collect();
    pdf.pages(Ref::new(2))
        .kids(ids.iter().copied())
        .count(ids.len() as i32);
    for (index, lines) in pages.iter().enumerate() {
        let id = ids[index];
        let content_id = Ref::new(9 + index as i32 * 2);
        let mut page = pdf.page(id);
        page.parent(Ref::new(2))
            .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
            .contents(content_id);
        page.resources().fonts().pair(Name(b"F1"), font);
        page.finish();
        let mut content = Content::new();
        for item in lines {
            match item {
                Item::Text {
                    text,
                    x,
                    y,
                    size,
                    strong,
                } => {
                    let encoded: Vec<u8> = text
                        .chars()
                        .flat_map(|c| {
                            face.glyph_index(c)
                                .unwrap_or(ttf_parser::GlyphId(0))
                                .0
                                .to_be_bytes()
                        })
                        .collect();
                    content.set_fill_rgb(0.10, 0.13, 0.16);
                    content.begin_text();
                    content.set_font(Name(b"F1"), *size);
                    content.next_line(*x, *y);
                    content.show(Str(&encoded));
                    content.end_text();
                    if *strong {
                        content.begin_text();
                        content.set_font(Name(b"F1"), *size);
                        content.next_line(*x + 0.22, *y);
                        content.show(Str(&encoded));
                        content.end_text();
                    }
                }
                Item::Cell {
                    x,
                    y,
                    width,
                    height,
                    header,
                } => {
                    if *header {
                        content.set_fill_rgb(0.92, 0.95, 0.96);
                        content.rect(*x, y - height, *width, *height);
                        content.fill_nonzero();
                    }
                    content.set_stroke_rgb(0.78, 0.82, 0.85);
                    content.set_line_width(0.5);
                    content.rect(*x, y - height, *width, *height);
                    content.stroke();
                }
            }
        }
        pdf.stream(content_id, &content.finish());
    }
    Ok(pdf.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_tables_repeat_headers_and_keep_text_on_the_page() {
        let face = ttf_parser::Face::parse(FONT, 0).unwrap();
        let mut layout = Layout::new(&face);
        let mut markdown = "| Owner | Task |\n| --- | --- |\n".to_owned();
        for i in 0..100 {
            markdown.push_str(&format!(
                "| Léna {i} | Review a long task description with ordinary wrapping |\n"
            ));
        }
        let super::super::layout::Block::Table(rows) =
            super::super::layout::parse(&markdown).remove(0)
        else {
            panic!("table")
        };
        layout.table(&rows);
        assert!(layout.pages.len() > 2);
        for page in &layout.pages {
            assert!(page
                .iter()
                .any(|item| matches!(item, Item::Text { text, .. } if text == "Owner")));
            for item in page {
                if let Item::Text { y, .. } = item {
                    assert!(*y >= 55.0 && *y <= 780.0);
                }
            }
        }
    }
}
