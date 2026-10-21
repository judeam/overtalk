//! Notes as an HTML fragment for a shared page. Every character of the input is
//! escaped: raw HTML stays visible text, and links and images keep only their label,
//! so a fragment never refers to anything outside the page.
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Escape text for HTML element content and quoted attribute values. Control
/// characters other than line breaks and tabs are dropped.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' | '\t' | '\r' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Render notes Markdown. A source reference `[S12]` becomes `source(12)` when it
/// returns markup (built by the caller from escaped values), otherwise escaped text.
pub fn notes_html(
    markdown: &str,
    source: &dyn Fn(usize) -> Option<String>,
) -> Result<String, String> {
    crate::layout::validate(markdown)?;
    let mut out = String::with_capacity(markdown.len() * 2);
    let mut text = String::new();
    let mut table_head = false;
    let mut images = 0usize;
    let flush = |text: &mut String, out: &mut String| {
        if !text.is_empty() {
            out.push_str(&with_sources(&escape(text), source));
            text.clear();
        }
    };
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
    ) {
        // Brackets reach the renderer as separate text events; join them so a
        // reference split across events is still recognised.
        match event {
            Event::Text(value) => {
                text.push_str(&value);
                continue;
            }
            // Raw HTML is shown as written, never interpreted.
            Event::Html(value) | Event::InlineHtml(value) if images == 0 => {
                text.push_str(&value);
                continue;
            }
            _ => flush(&mut text, &mut out),
        }
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    out.push_str(&format!("<h{}>", heading(level)));
                }
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(_) => out.push_str("<pre><code>"),
                Tag::List(Some(start)) => out.push_str(&format!("<ol start=\"{start}\">")),
                Tag::List(None) => out.push_str("<ul>"),
                Tag::Item => out.push_str("<li>"),
                Tag::Table(_) => out.push_str("<div class=\"table\"><table>"),
                Tag::TableHead => {
                    table_head = true;
                    out.push_str("<thead><tr>");
                }
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str(if table_head { "<th>" } else { "<td>" }),
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                Tag::Strikethrough => out.push_str("<del>"),
                // A link keeps its label; an image keeps its description.
                Tag::Image { .. } => images += 1,
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.push_str("</p>\n"),
                TagEnd::Heading(level) => out.push_str(&format!("</h{}>\n", heading(level))),
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>\n"),
                TagEnd::CodeBlock => out.push_str("</code></pre>\n"),
                TagEnd::List(true) => out.push_str("</ol>\n"),
                TagEnd::List(false) => out.push_str("</ul>\n"),
                TagEnd::Item => out.push_str("</li>\n"),
                TagEnd::Table => out.push_str("</tbody></table></div>\n"),
                TagEnd::TableHead => {
                    table_head = false;
                    out.push_str("</tr></thead><tbody>");
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => out.push_str(if table_head { "</th>" } else { "</td>" }),
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                TagEnd::Strikethrough => out.push_str("</del>"),
                TagEnd::Image => images = images.saturating_sub(1),
                _ => {}
            },
            Event::Code(value) => out.push_str(&format!("<code>{}</code>", escape(&value))),
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str("<hr>\n"),
            Event::TaskListMarker(done) => out.push_str(if done {
                "<span class=\"task\" role=\"img\" aria-label=\"Done\">☑</span> "
            } else {
                "<span class=\"task\" role=\"img\" aria-label=\"Open\">☐</span> "
            }),
            _ => {}
        }
    }
    flush(&mut text, &mut out);
    Ok(out)
}

// The page title is h1 and its sections h2, so notes headings start at h3.
fn heading(level: HeadingLevel) -> usize {
    (level as usize + 1).clamp(3, 6)
}

// `[` and `]` survive escaping unchanged, so references are found in escaped text.
fn with_sources(escaped: &str, source: &dyn Fn(usize) -> Option<String>) -> String {
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped;
    while let Some(start) = rest.find("[S") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        let replaced = (digits > 0 && digits < 7 && tail.as_bytes().get(digits) == Some(&b']'))
            .then(|| tail[..digits].parse().ok().and_then(source))
            .flatten();
        match replaced {
            Some(markup) => {
                out.push_str(&markup);
                rest = &tail[digits + 1..];
            }
            None => {
                out.push_str("[S");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_markup_stays_text_and_links_keep_only_their_label() {
        let html = notes_html(
            "## Decisions <script>alert(1)</script>\n\n- Ship [it](https://evil.example/x) ![logo](https://evil.example/i.png \"t\") <img src=x onerror=alert(1)>\n\n| Owner | Due |\n| --- | --- |\n| \"Ann\" & co | `<b>` |",
            &|_| None,
        )
        .unwrap();
        assert!(!html.contains("<script"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("evil.example"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("<h3>Decisions"));
        assert!(html.contains("Ship it logo"));
        assert!(html.contains("<th>Owner</th>"));
        assert!(html.contains("<td>&quot;Ann&quot; &amp; co</td>"));
        assert!(html.contains("<code>&lt;b&gt;</code>"));
    }

    #[test]
    fn source_references_use_the_callback_outside_code() {
        let html = notes_html(
            "- Decide [S2][S3] and [S9] `[S2]`\n- [S] [Sx] [S12345678]",
            &|n| (n != 9).then(|| format!("<b data-s=\"{n}\"></b>")),
        )
        .unwrap();
        assert!(html.contains("Decide <b data-s=\"2\"></b><b data-s=\"3\"></b> and [S9]"));
        assert!(html.contains("<code>[S2]</code>"));
        assert!(html.contains("[S] [Sx] [S12345678]"));
    }

    #[test]
    fn escape_covers_attribute_quotes_and_controls() {
        assert_eq!(
            escape("a\"b'c<d>&\u{0}e\n"),
            "a&quot;b&#39;c&lt;d&gt;&amp;e\n"
        );
    }
}
