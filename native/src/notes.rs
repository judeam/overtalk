//! Slint supports inline Markdown and lists. Normalize block headings for its renderer.
pub fn preview(markdown: &str) -> slint::StyledText {
    let normalized = markdown
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let heading = trimmed.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&heading) && trimmed.as_bytes().get(heading) == Some(&b' ') {
                format!("**{}**", trimmed[heading + 1..].trim())
            } else if let Some(content) = trimmed.strip_prefix("- [ ] ") {
                format!("- ☐ {}", content)
            } else if let Some(content) = trimmed.strip_prefix("- [x] ") {
                format!("- ☑ {}", content)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    slint::StyledText::from_markdown(&link_sources(&normalized))
        .unwrap_or_else(|_| slint::StyledText::from_plain_text(markdown))
}

fn link_sources(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut fenced = false;
    let mut fence = "";
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = if trimmed.starts_with("```") {
                "```"
            } else {
                "~~~"
            };
            if !fenced {
                fenced = true;
                fence = marker;
            } else if marker == fence {
                fenced = false;
            }
            output.push_str(line);
            continue;
        }
        if fenced {
            output.push_str(line);
            continue;
        }
        let mut rest = line;
        let mut code_ticks = 0;
        while !rest.is_empty() {
            if rest.starts_with('`') {
                let count = rest.bytes().take_while(|&b| b == b'`').count();
                output.push_str(&rest[..count]);
                rest = &rest[count..];
                if code_ticks == 0 {
                    code_ticks = count;
                } else if code_ticks == count {
                    code_ticks = 0;
                }
            } else if code_ticks == 0 && rest.starts_with("[S") {
                if let Some(end) = rest
                    .find(']')
                    .filter(|&end| rest[2..end].parse::<usize>().is_ok_and(|id| id > 0))
                {
                    // Adjacent source references are separate reading targets.
                    if output.ends_with(')') {
                        output.push(' ');
                    }
                    output.push_str(&rest[..=end]);
                    if !rest[end + 1..].starts_with('(') {
                        output.push_str(&format!("(#source-{})", &rest[2..end]));
                    }
                    rest = &rest[end + 1..];
                    if rest.starts_with("[S") {
                        output.push(' ');
                    }
                } else {
                    output.push('[');
                    rest = &rest[1..];
                }
            } else {
                let ch = rest.chars().next().unwrap();
                output.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    output
}

#[derive(Debug, Clone)]
pub struct Block {
    pub start: usize,
    pub end: usize,
    pub prefix: String,
    pub text: String,
    /// 0 paragraph, 1–6 heading, 7 bullet, 8 numbered list, 9 code.
    pub kind: i32,
    pub suffix: String,
}

/// Preserve byte ranges and Markdown markers so editing one block never rewrites
/// neighbouring sections, citations, or Unicode content.
pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut result = Vec::new();
    let lines: Vec<_> = markdown.split_inclusive('\n').collect();
    let mut offset = 0;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let body = line.trim_end_matches(['\r', '\n']);
        let start = offset;
        offset += line.len();
        index += 1;
        if body.trim().is_empty() {
            continue;
        }
        let trimmed = body.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = &trimmed[..3];
            let content_start = offset;
            let mut content_end = offset;
            let mut closing = "";
            while index < lines.len() {
                let next = lines[index];
                index += 1;
                offset += next.len();
                if next.trim_start().starts_with(marker) {
                    closing = next.trim_end_matches(['\r', '\n']);
                    break;
                }
                content_end = offset;
            }
            result.push(Block {
                start,
                end: offset
                    - if closing.is_empty() {
                        0
                    } else {
                        lines[index - 1].len() - closing.len()
                    },
                prefix: line.into(),
                text: markdown[content_start..content_end]
                    .trim_end_matches('\n')
                    .into(),
                suffix: if closing.is_empty() {
                    String::new()
                } else {
                    format!("\n{closing}")
                },
                kind: 9,
            });
            continue;
        }
        if trimmed.starts_with('|')
            && index < lines.len()
            && lines[index].trim().starts_with('|')
            && lines[index].contains("---")
        {
            while index < lines.len() && lines[index].trim_start().starts_with('|') {
                offset += lines[index].len();
                index += 1;
            }
            let end = offset
                - (lines[index - 1].len() - lines[index - 1].trim_end_matches(['\r', '\n']).len());
            result.push(Block {
                start,
                end,
                prefix: String::new(),
                text: markdown[start..end].into(),
                suffix: String::new(),
                kind: 10,
            });
            continue;
        }
        let leading = body.len() - trimmed.len();
        let level = trimmed.bytes().take_while(|b| *b == b'#').count();
        let (kind, marker) =
            if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
                (level as i32, level + 1)
            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                (7, 2)
            } else if let Some((number, _)) = trimmed
                .split_once(". ")
                .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            {
                (8, number.len() + 2)
            } else {
                (0, 0)
            };
        let marker = leading + marker;
        result.push(Block {
            start,
            end: start + body.len(),
            prefix: body[..marker].into(),
            text: body[marker..].into(),
            kind,
            suffix: String::new(),
        });
    }
    result
}

pub fn table_rows(markdown: &str) -> Vec<Vec<String>> {
    markdown
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, line)| {
            let mut cells = Vec::new();
            let mut cell = String::new();
            let mut escaped = false;
            for ch in line.trim().trim_matches('|').chars() {
                if escaped {
                    cell.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '|' {
                    cells.push(cell.trim().replace("<br>", "\n"));
                    cell.clear();
                } else {
                    cell.push(ch);
                }
            }
            cells.push(cell.trim().replace("<br>", "\n"));
            cells
        })
        .collect()
}

pub fn edit(markdown: &str, index: usize, text: &str) -> Option<String> {
    let block = blocks(markdown).into_iter().nth(index)?;
    let mut edited = markdown.to_owned();
    edited.replace_range(
        block.start..block.end,
        &format!("{}{}{}", block.prefix, text, block.suffix),
    );
    Some(edited)
}

#[cfg(test)]
mod tests {
    #[test]
    fn citations_link_to_source_without_rewriting_existing_links() {
        assert_eq!(
            super::link_sources("Decision [S3] and [S1](#custom), not [Someone]."),
            "Decision [S3](#source-3) and [S1](#custom), not [Someone]."
        );
    }
    #[test]
    fn adjacent_citations_remain_distinct_readable_links() {
        assert_eq!(
            super::link_sources("Agreed [S1][S2][S3]."),
            "Agreed [S1](#source-1) [S2](#source-2) [S3](#source-3)."
        );
    }

    #[test]
    fn citations_in_code_are_not_changed() {
        let input = "`[S1]` and [S2]\n```rust\n[S3]\n```\n";
        assert_eq!(
            super::link_sources(input),
            "`[S1]` and [S2](#source-2)\n```rust\n[S3]\n```\n"
        );
    }
    #[test]
    fn tables_and_code_are_whole_editable_blocks() {
        let input =
            "```rust\nlet x = 1;\n```\n\n| Owner | Task |\n| --- | --- |\n| Léna | Review |\n";
        let blocks = super::blocks(input);
        assert_eq!(blocks.iter().map(|b| b.kind).collect::<Vec<_>>(), [9, 10]);
        assert_eq!(
            super::edit(input, 0, "let y = 2;").unwrap(),
            input.replace("let x = 1;", "let y = 2;")
        );
        assert_eq!(super::table_rows(&blocks[1].text)[1], ["Léna", "Review"]);
    }
    #[test]
    fn editing_preserves_markers_citations_and_unicode() {
        let input = "# Reunião 📝\n\n## Decisions\n\n- Keep **local** data. [S3](#source-3)\n";
        let edited = super::edit(input, 0, "Planejamento 📝").unwrap();
        assert_eq!(
            edited,
            "# Planejamento 📝\n\n## Decisions\n\n- Keep **local** data. [S3](#source-3)\n"
        );
        assert_eq!(
            super::blocks(&edited)
                .iter()
                .map(|b| b.kind)
                .collect::<Vec<_>>(),
            vec![1, 2, 7]
        );
    }
}
