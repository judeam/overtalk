//! Markdown structure shared by the local document exporters.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(crate) fn validate(markdown: &str) -> Result<(), String> {
    let error =
        "Document exceeds the text, nesting or table complexity limit; the original is unchanged";
    if markdown.len() > 16 * 1024 * 1024 {
        return Err(error.into());
    }
    // Bound table-header allocation inside the parser before requesting events.
    let mut lines = 0;
    for line in markdown.lines() {
        lines += 1;
        if lines > 100_000 || line.bytes().filter(|b| *b == b'|').take(258).count() > 257 {
            return Err(error.into());
        }
    }
    let mut depth = 0usize;
    let mut columns = 0;
    let mut cells = 0usize;
    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(tag) => {
                depth += 1;
                if depth > 64 {
                    return Err(error.into());
                }
                match tag {
                    Tag::Table(alignments) => columns = alignments.len(),
                    Tag::TableHead | Tag::TableRow => {
                        cells = cells.saturating_add(columns);
                        if columns > 256 || cells > 100_000 {
                            return Err(error.into());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

#[derive(Default, Clone)]
pub(crate) struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}
pub(crate) enum Block {
    Paragraph { runs: Vec<Run>, level: u8 },
    Table(Vec<Vec<Vec<Run>>>),
}
impl Block {
    pub(super) fn plain(&self) -> Vec<(String, f32)> {
        match self {
            Self::Paragraph { runs, level } => vec![(
                runs.iter().map(|r| r.text.as_str()).collect(),
                match level {
                    1 => 20.0,
                    2 => 16.0,
                    3..=6 => 13.0,
                    _ => 11.0,
                },
            )],
            Self::Table(rows) => rows
                .iter()
                .map(|row| {
                    (
                        row.iter()
                            .map(|cell| cell.iter().map(|r| r.text.as_str()).collect::<String>())
                            .collect::<Vec<_>>()
                            .join("  |  "),
                        10.0,
                    )
                })
                .collect(),
        }
    }
}
fn flush(blocks: &mut Vec<Block>, runs: &mut Vec<Run>, level: u8) {
    if !runs.is_empty() {
        blocks.push(Block::Paragraph {
            runs: std::mem::take(runs),
            level,
        });
    }
}
pub(crate) fn parse(markdown: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let mut runs = Vec::new();
    let mut level = 0;
    let mut bold = 0;
    let mut italic = 0;
    let mut code = false;
    let mut list = Vec::new();
    let mut table = None::<Vec<Vec<Vec<Run>>>>;
    let mut row = Vec::new();
    let mut links = Vec::new();
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(Tag::Heading { level: heading, .. }) => {
                flush(&mut result, &mut runs, level);
                level = heading as u8;
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut result, &mut runs, level);
                level = 0;
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                links.push(dest_url.into_string())
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some(url) = links.pop() {
                    runs.push(Run {
                        text: format!(" ({url})"),
                        ..Default::default()
                    });
                }
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold -= 1,
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic -= 1,
            Event::Start(Tag::List(number)) => list.push(number),
            Event::End(TagEnd::List(_)) => {
                list.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut result, &mut runs, level);
                let prefix = match list.last_mut() {
                    Some(Some(n)) => {
                        let prefix = format!("{n}. ");
                        *n += 1;
                        prefix
                    }
                    _ => "• ".into(),
                };
                runs.push(Run {
                    text: format!("{}{prefix}", "  ".repeat(list.len().saturating_sub(1))),
                    ..Default::default()
                });
            }
            Event::End(TagEnd::Item) => flush(&mut result, &mut runs, level),
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => {
                flush(&mut result, &mut runs, level);
                code = false;
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut result, &mut runs, level);
                table = Some(Vec::new());
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => row.clear(),
            Event::End(TagEnd::TableCell) => row.push(std::mem::take(&mut runs)),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(table) = &mut table {
                    table.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    result.push(Block::Table(table));
                }
            }
            Event::End(TagEnd::Paragraph) if table.is_none() => {
                flush(&mut result, &mut runs, level)
            }
            Event::Text(text) | Event::Code(text) => runs.push(Run {
                text: text.into_string(),
                bold: bold > 0,
                italic: italic > 0,
                code,
            }),
            Event::SoftBreak | Event::HardBreak => runs.push(Run {
                text: "\n".into(),
                ..Default::default()
            }),
            Event::TaskListMarker(checked) => runs.push(Run {
                text: if checked { "☑ " } else { "☐ " }.into(),
                ..Default::default()
            }),
            Event::Rule => runs.push(Run {
                text: "────────".into(),
                ..Default::default()
            }),
            _ => {}
        }
    }
    flush(&mut result, &mut runs, level);
    result
}
