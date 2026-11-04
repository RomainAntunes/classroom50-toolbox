//! Assignment descriptions, which upstream's web app renders as GitHub-flavored Markdown
//! with LaTeX math (`foundation50/classroom50` web 1.59.0), parsed into a typed tree the
//! UI renders with plain markup. A description is written by the teaching staff but read
//! from a public file anyone in the org could have edited, so nothing in it is trusted:
//! raw HTML stays text, links keep only `http`, `https` and `mailto` targets, images become
//! links (nothing is fetched), and the source, the nesting and the node count are capped.
//!
//! Math follows GitHub's rules, which pulldown-cmark's are looser than: `$5 and $10` is
//! prose (a closer followed by a digit, or whitespace inside a delimiter, is no math), and
//! `` $`...`$ `` is the form for expressions that collide with Markdown. Like upstream's
//! `util/markdownMath.ts`, a pre-pass rewrites the source so the parser reads it GitHub's
//! way: openers that can't close are escaped, and the backtick form becomes `$...$`. What
//! is literal (code, raw HTML, autolinks, link definitions) comes from a first parse and is
//! never scanned for dollars.

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::Serialize;

/// Bytes of a description read; the rest is left out.
const MAX_SOURCE: usize = 32 * 1024;
/// Quotes, lists and inline styles nested deeper than this keep their content, unstyled.
pub const MAX_DEPTH: usize = 8;
/// Blocks and inlines in one description; parsing stops there.
pub const MAX_NODES: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Paragraph {
        content: Vec<Inline>,
    },
    /// `level` 1 to 6, as written.
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    List {
        ordered: bool,
        /// The first number of an ordered list.
        start: u32,
        items: Vec<ListItem>,
    },
    /// A fenced or indented block; `lang` is the info string's first word.
    Code {
        lang: Option<String>,
        text: String,
    },
    /// Display math (`$$...$$` alone in its paragraph, or a ```` ```math ```` fence), as
    /// LaTeX source.
    Math {
        source: String,
    },
    Table {
        /// One per column; `None` when the delimiter row sets none.
        align: Vec<Option<Align>>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Quote {
        blocks: Vec<Block>,
    },
    Rule,
    /// The footnotes, last, in the order they are first referenced (as GitHub lists them).
    Footnotes {
        notes: Vec<Footnote>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Inline {
    Text {
        text: String,
    },
    Code {
        text: String,
    },
    Emphasis {
        content: Vec<Inline>,
    },
    Strong {
        content: Vec<Inline>,
    },
    Strike {
        content: Vec<Inline>,
    },
    /// Only `http`, `https` and `mailto` targets; any other link is its text alone.
    Link {
        href: String,
        content: Vec<Inline>,
    },
    /// Inline math, as LaTeX source.
    Math {
        source: String,
    },
    Break,
    FootnoteRef {
        number: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub struct ListItem {
    /// A task list item's checkbox: `Some(true)` when ticked.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub struct Footnote {
    pub number: u32,
    pub blocks: Vec<Block>,
}

/// A table column's alignment, by reading direction (`:---` is the start).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Start,
    Center,
    End,
}

/// The description as a tree, `None` when there is nothing to show.
pub fn document(description: Option<&str>) -> Option<Vec<Block>> {
    let blocks = parse(description.filter(|text| !text.trim().is_empty())?);
    (!blocks.is_empty()).then_some(blocks)
}

pub fn parse(source: &str) -> Vec<Block> {
    let mut end = source.len().min(MAX_SOURCE);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let source = source[..end].replace("\r\n", "\n");
    let text = normalize_math(&source);
    let parser = Parser::new_ext(&text, options() | Options::ENABLE_MATH);
    Builder::default().run(parser)
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

/// The target of a link the UI may open: `http`, `https` or `mailto`, nothing else
/// (`javascript:`, `data:`, `file:` and relative paths are dropped).
fn safe_href(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && url.len() > scheme.len());
    (allowed && !url.chars().any(|c| c.is_control() || c.is_whitespace())).then(|| url.to_owned())
}

// ---------------------------------------------------------------------------------------
// GitHub's dollar rules (a port of upstream's `normalizeGitHubMath`).

/// A stretch of source whose dollars are never math.
struct Literal {
    range: Range<usize>,
    /// A bare URL is text to the parser: its dollars are escaped instead of copied, so
    /// none pairs with a later one.
    url: bool,
}

/// What CommonMark reads literally, in document order: code (fenced, indented, spans, at
/// any nesting), raw HTML, autolinks, link reference definitions, plus GFM's bare URLs. A
/// bracketed link's text is prose, and escapes are decoded in its destination anyway.
fn literal_ranges(source: &str) -> Vec<Literal> {
    let mut found = Vec::new();
    let mut text: Option<Range<usize>> = None;
    let mut inside = 0usize;
    let parser = Parser::new_ext(source, options()).into_offset_iter();
    for (_, def) in parser.reference_definitions().iter() {
        found.push(Literal {
            range: def.span.clone(),
            url: false,
        });
    }
    let flush = |text: &mut Option<Range<usize>>, found: &mut Vec<Literal>| {
        if let Some(run) = text.take() {
            for url in bare_urls(&source[run.clone()]) {
                found.push(Literal {
                    range: run.start + url.start..run.start + url.end,
                    url: true,
                });
            }
        }
    };
    let literal = |range| Literal { range, url: false };
    for (event, range) in parser {
        match event {
            Event::Text(_) if inside == 0 => match &mut text {
                Some(run) if run.end == range.start => run.end = range.end,
                _ => {
                    flush(&mut text, &mut found);
                    text = Some(range);
                }
            },
            Event::Text(_) => {}
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock) => {
                flush(&mut text, &mut found);
                found.push(literal(range));
                inside += 1;
            }
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink | LinkType::Email,
                ..
            }) => {
                flush(&mut text, &mut found);
                found.push(literal(range));
                inside += 1;
            }
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                flush(&mut text, &mut found);
                inside += 1;
            }
            Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock | TagEnd::Link | TagEnd::Image) => {
                inside = inside.saturating_sub(1);
            }
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => {
                flush(&mut text, &mut found);
                found.push(literal(range));
            }
            _ => flush(&mut text, &mut found),
        }
    }
    flush(&mut text, &mut found);
    found.sort_by_key(|literal| (literal.range.start, std::cmp::Reverse(literal.range.end)));
    found
}

fn normalize_math(source: &str) -> String {
    let ranges = literal_ranges(source);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n + 16);
    let mut i = 0;
    let mut next = 0;
    let space_after = |at: usize| source[at..].chars().next().is_none_or(char::is_whitespace);
    let space_before = |at: usize| {
        source[..at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
    };
    // The next unescaped single `$` from `from`, before any blank line.
    let inline_closer = |from: usize| -> Option<usize> {
        let mut j = from;
        while j < n {
            match bytes[j] {
                b'\\' => j += 1,
                b'\n' if bytes.get(j + 1) == Some(&b'\n') => return None,
                b'$' if bytes.get(j + 1) == Some(&b'$') => j += 1,
                b'$' => return Some(j),
                _ => {}
            }
            j += 1;
        }
        None
    };

    while i < n {
        while next < ranges.len() && ranges[next].range.end <= i {
            next += 1;
        }
        if let Some(literal) = ranges.get(next).filter(|literal| literal.range.start <= i) {
            let end = literal.range.end;
            if literal.url {
                escape_dollars(&source[i..end], &mut out);
            } else {
                out.push_str(&source[i..end]);
            }
            i = end;
            continue;
        }
        match bytes[i] {
            b'\\' => {
                let end = source[i + 1..]
                    .chars()
                    .next()
                    .map_or(n, |c| i + 1 + c.len_utf8());
                out.push_str(&source[i..end]);
                i = end;
            }
            b'$' if bytes.get(i + 1) == Some(&b'$') => match source[i + 2..].find("$$") {
                Some(at) => {
                    let close = i + 2 + at;
                    out.push_str(&source[i..close + 2]);
                    i = close + 2;
                }
                None => {
                    out.push_str("$$");
                    i += 2;
                }
            },
            b'$' => {
                if bytes.get(i + 1) == Some(&b'`')
                    && let Some(at) = source[i + 2..].find("`$")
                {
                    let close = i + 2 + at;
                    out.push('$');
                    out.push_str(&source[i + 2..close].replace('$', "\\$"));
                    out.push('$');
                    i = close + 2;
                    continue;
                }
                match inline_closer(i + 1) {
                    Some(close)
                        if !space_after(i + 1)
                            && !space_before(close)
                            && !bytes.get(close + 1).is_some_and(u8::is_ascii_digit) =>
                    {
                        out.push_str(&source[i..=close]);
                        i = close + 1;
                    }
                    _ => {
                        out.push_str("\\$");
                        i += 1;
                    }
                }
            }
            _ => {
                let end = source[i..].chars().next().map_or(n, |c| i + c.len_utf8());
                out.push_str(&source[i..end]);
                i = end;
            }
        }
    }
    out
}

/// Copies `text` with each unescaped `$` escaped.
fn escape_dollars(text: &str, out: &mut String) {
    let mut escaped = false;
    for c in text.chars() {
        if c == '$' && !escaped {
            out.push('\\');
        }
        escaped = c == '\\' && !escaped;
        out.push(c);
    }
}

// ---------------------------------------------------------------------------------------
// GFM's extended autolinks: `http://`, `https://` and `www.` URLs in text.

/// Byte ranges of the bare URLs in `text`, trailing punctuation left out as GFM does.
fn bare_urls(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while from < text.len() {
        let rest = &text[from..];
        let Some(at) = ["http://", "https://", "www."]
            .iter()
            .filter_map(|prefix| find_ascii_ci(rest, prefix).map(|at| (at, prefix.len())))
            .min()
        else {
            break;
        };
        let (offset, prefix) = at;
        let start = from + offset;
        let boundary = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || "*_~(".contains(c));
        let mut end = start
            + text[start..]
                .find(|c: char| c.is_whitespace() || c == '<')
                .unwrap_or(text.len() - start);
        // Trailing punctuation, and a `)` the URL didn't open, aren't part of it.
        loop {
            let url = &text[start..end];
            match url.chars().next_back() {
                Some(c) if "?!.,:*_~'\";".contains(c) => end -= 1,
                Some(')') if url.matches(')').count() > url.matches('(').count() => end -= 1,
                _ => break,
            }
        }
        let host = &text[start + prefix..end];
        if boundary && !host.is_empty() && !host.starts_with(['/', '.']) {
            found.push(start..end);
            from = end;
        } else {
            from = start + prefix;
        }
    }
    found
}

fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn href_of_bare(url: &str) -> String {
    if url
        .get(..4)
        .is_some_and(|start| start.eq_ignore_ascii_case("www."))
    {
        format!("https://{url}")
    } else {
        url.to_owned()
    }
}

/// Splits each text node around the bare URLs it holds.
fn linkify(content: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::with_capacity(content.len());
    for inline in content {
        let Inline::Text { text } = inline else {
            out.push(inline);
            continue;
        };
        let mut last = 0;
        for url in bare_urls(&text) {
            let Some(href) = safe_href(&href_of_bare(&text[url.clone()])) else {
                continue;
            };
            if url.start > last {
                out.push(Inline::Text {
                    text: text[last..url.start].to_owned(),
                });
            }
            out.push(Inline::Link {
                href,
                content: vec![Inline::Text {
                    text: text[url.clone()].to_owned(),
                }],
            });
            last = url.end;
        }
        if last < text.len() {
            out.push(Inline::Text {
                text: text[last..].to_owned(),
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Events to tree.

enum Frame {
    Blocks {
        kind: BlocksKind,
        blocks: Vec<Block>,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<ListItem>,
    },
    Inlines {
        kind: InlinesKind,
        content: Vec<Inline>,
        /// Display math pushed, so a paragraph holding only that becomes a math block.
        display: usize,
    },
    Table {
        align: Vec<Option<Align>>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        row: Vec<Vec<Inline>>,
    },
    Code {
        lang: Option<String>,
        text: String,
    },
    Html {
        text: String,
    },
}

enum BlocksKind {
    Root,
    Quote,
    Item { task: Option<bool> },
    Footnote { label: String },
}

#[derive(PartialEq)]
enum InlinesKind {
    Paragraph,
    /// The loose text of a tight list item.
    Implicit,
    Heading(u8),
    Emphasis,
    Strong,
    Strike,
    /// `None` when the target isn't one the UI opens: the text stays, the link goes.
    Link(Option<String>),
    /// An image: a link to it with its alt text.
    Image(Option<String>),
    Cell,
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    /// One per open tag: whether it pushed a frame (tags nested too deep don't).
    opened: Vec<bool>,
    depth: usize,
    nodes: usize,
    footnotes: HashMap<String, Vec<Block>>,
    /// Footnote labels in the order first referenced; a label's number is its index + 1.
    referenced: Vec<String>,
}

impl Builder {
    fn run<'a>(mut self, parser: impl Iterator<Item = Event<'a>>) -> Vec<Block> {
        self.stack.push(Frame::Blocks {
            kind: BlocksKind::Root,
            blocks: Vec::new(),
        });
        for event in parser {
            if self.nodes >= MAX_NODES {
                break;
            }
            self.event(event);
        }
        while self.stack.len() > 1 {
            self.close();
        }
        let Some(Frame::Blocks { mut blocks, .. }) = self.stack.pop() else {
            return Vec::new();
        };
        let notes: Vec<Footnote> = std::mem::take(&mut self.referenced)
            .into_iter()
            .enumerate()
            .filter_map(|(index, label)| {
                Some(Footnote {
                    number: index as u32 + 1,
                    blocks: self.footnotes.remove(&label)?,
                })
            })
            .collect();
        if !notes.is_empty() {
            blocks.push(Block::Footnotes { notes });
        }
        blocks
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match self.stack.last_mut() {
                Some(Frame::Code { text: code, .. } | Frame::Html { text: code }) => {
                    code.push_str(&text);
                }
                _ => self.inline(Inline::Text {
                    text: text.into_string(),
                }),
            },
            Event::Code(text) => self.inline(Inline::Code {
                text: text.into_string(),
            }),
            Event::InlineMath(source) => self.inline(Inline::Math {
                source: source.trim().to_owned(),
            }),
            Event::DisplayMath(source) => {
                self.inline(Inline::Math {
                    source: source.trim().to_owned(),
                });
                if let Some(Frame::Inlines { display, .. }) = self.stack.last_mut() {
                    *display += 1;
                }
            }
            Event::Html(text) => match self.stack.last_mut() {
                Some(Frame::Html { text: html }) => html.push_str(&text),
                _ => self.inline(Inline::Text {
                    text: text.into_string(),
                }),
            },
            Event::InlineHtml(text) => self.inline(Inline::Text {
                text: text.into_string(),
            }),
            Event::SoftBreak => self.inline(Inline::Text { text: " ".into() }),
            Event::HardBreak => self.inline(Inline::Break),
            Event::Rule => self.block(Block::Rule),
            Event::FootnoteReference(label) => {
                let label = label.into_string();
                let index = match self.referenced.iter().position(|seen| *seen == label) {
                    Some(index) => index,
                    None => {
                        self.referenced.push(label);
                        self.referenced.len() - 1
                    }
                };
                self.inline(Inline::FootnoteRef {
                    number: index as u32 + 1,
                });
            }
            Event::TaskListMarker(checked) => self.task(checked),
        }
    }

    fn start(&mut self, tag: Tag) {
        if !matches!(
            tag,
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
        ) {
            self.close_implicit();
        }
        let frame = match tag {
            Tag::Paragraph => Some(inlines(InlinesKind::Paragraph)),
            Tag::Heading { level, .. } => Some(inlines(InlinesKind::Heading(level as u8))),
            Tag::CodeBlock(kind) => Some(Frame::Code {
                lang: match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .map(|lang| lang.chars().take(32).collect()),
                    CodeBlockKind::Indented => None,
                },
                text: String::new(),
            }),
            Tag::HtmlBlock => Some(Frame::Html {
                text: String::new(),
            }),
            Tag::Table(columns) => Some(Frame::Table {
                align: columns
                    .iter()
                    .map(|column| match column {
                        Alignment::None => None,
                        Alignment::Left => Some(Align::Start),
                        Alignment::Center => Some(Align::Center),
                        Alignment::Right => Some(Align::End),
                    })
                    .collect(),
                head: Vec::new(),
                rows: Vec::new(),
                row: Vec::new(),
            }),
            Tag::TableHead | Tag::TableRow => {
                self.opened.push(false);
                return;
            }
            Tag::TableCell => Some(inlines(InlinesKind::Cell)),
            // An item belongs to the list frame right under it; a list nested too deep has
            // none, and its items' content joins the blocks around it.
            Tag::Item => {
                matches!(self.stack.last(), Some(Frame::List { .. })).then(|| Frame::Blocks {
                    kind: BlocksKind::Item { task: None },
                    blocks: Vec::new(),
                })
            }
            Tag::FootnoteDefinition(label) => Some(Frame::Blocks {
                kind: BlocksKind::Footnote {
                    label: label.into_string(),
                },
                blocks: Vec::new(),
            }),
            Tag::BlockQuote(_) => self.nested(Frame::Blocks {
                kind: BlocksKind::Quote,
                blocks: Vec::new(),
            }),
            Tag::List(start) => self.nested(Frame::List {
                ordered: start.is_some(),
                start: start.map_or(1, |n| n.min(u64::from(u32::MAX)) as u32),
                items: Vec::new(),
            }),
            Tag::Emphasis => self.nested(inlines(InlinesKind::Emphasis)),
            Tag::Strong => self.nested(inlines(InlinesKind::Strong)),
            Tag::Strikethrough => self.nested(inlines(InlinesKind::Strike)),
            Tag::Link { dest_url, .. } => {
                self.nested(inlines(InlinesKind::Link(safe_href(&dest_url))))
            }
            Tag::Image { dest_url, .. } => {
                self.nested(inlines(InlinesKind::Image(safe_href(&dest_url))))
            }
            _ => None,
        };
        let Some(frame) = frame else {
            self.opened.push(false);
            return;
        };
        if styles(&frame) && matches!(self.stack.last(), Some(Frame::Blocks { .. })) {
            self.stack.push(inlines(InlinesKind::Implicit));
        }
        self.opened.push(true);
        self.stack.push(frame);
    }

    /// A frame that nests (quotes, lists, inline styles, links), unless that goes too deep.
    fn nested(&mut self, frame: Frame) -> Option<Frame> {
        (self.depth < MAX_DEPTH).then(|| {
            self.depth += 1;
            frame
        })
    }

    fn end(&mut self, tag: TagEnd) {
        let pushed = self.opened.pop().unwrap_or(false);
        let block_level = !matches!(
            tag,
            TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image
        );
        if block_level {
            self.close_implicit();
        }
        match tag {
            TagEnd::TableHead => {
                if let Some(Frame::Table { head, row, .. }) = self.stack.last_mut() {
                    *head = std::mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Frame::Table { rows, row, .. }) = self.stack.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            _ if pushed => self.close(),
            _ => {}
        }
    }

    fn close_implicit(&mut self) {
        if let Some(Frame::Inlines {
            kind: InlinesKind::Implicit,
            ..
        }) = self.stack.last()
        {
            self.close();
        }
    }

    /// Pops the top frame into the one under it.
    fn close(&mut self) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        let inside_link = self.stack.iter().any(|frame| {
            matches!(
                frame,
                Frame::Inlines {
                    kind: InlinesKind::Link(Some(_)) | InlinesKind::Image(Some(_)),
                    ..
                }
            )
        });
        match frame {
            Frame::Blocks { kind, blocks } => match kind {
                BlocksKind::Root => self.stack.push(Frame::Blocks { kind, blocks }),
                BlocksKind::Quote => {
                    self.depth -= 1;
                    self.block(Block::Quote { blocks });
                }
                BlocksKind::Item { task } => {
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem { task, blocks });
                        self.nodes += 1;
                    }
                }
                BlocksKind::Footnote { label } => {
                    self.footnotes.entry(label).or_insert(blocks);
                }
            },
            Frame::List {
                ordered,
                start,
                items,
            } => {
                self.depth -= 1;
                self.block(Block::List {
                    ordered,
                    start,
                    items,
                });
            }
            Frame::Inlines {
                kind,
                content,
                display,
            } => {
                let linked = matches!(
                    kind,
                    InlinesKind::Link(Some(_)) | InlinesKind::Image(Some(_))
                );
                let content = if inside_link || linked {
                    content
                } else {
                    linkify(content)
                };
                match kind {
                    InlinesKind::Paragraph | InlinesKind::Implicit => {
                        match <[Inline; 1]>::try_from(content) {
                            Ok([Inline::Math { source }]) if display == 1 => {
                                self.block(Block::Math { source })
                            }
                            Ok(single) => self.block(Block::Paragraph {
                                content: single.into(),
                            }),
                            Err(content) if content.is_empty() => {}
                            Err(content) => self.block(Block::Paragraph { content }),
                        }
                    }
                    InlinesKind::Heading(level) => self.block(Block::Heading { level, content }),
                    InlinesKind::Cell => {
                        if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                            row.push(content);
                            self.nodes += 1;
                        }
                    }
                    InlinesKind::Emphasis => self.styled(Inline::Emphasis { content }),
                    InlinesKind::Strong => self.styled(Inline::Strong { content }),
                    InlinesKind::Strike => self.styled(Inline::Strike { content }),
                    InlinesKind::Link(href) => {
                        self.depth -= 1;
                        match href {
                            Some(href) => self.inline(Inline::Link { href, content }),
                            None => content.into_iter().for_each(|inline| self.inline(inline)),
                        }
                    }
                    InlinesKind::Image(href) => {
                        self.depth -= 1;
                        let content = if content.is_empty() {
                            href.iter()
                                .map(|href| Inline::Text { text: href.clone() })
                                .collect()
                        } else {
                            content
                        };
                        match href {
                            Some(href) => self.inline(Inline::Link { href, content }),
                            None => content.into_iter().for_each(|inline| self.inline(inline)),
                        }
                    }
                }
            }
            Frame::Table {
                align, head, rows, ..
            } => self.block(Block::Table { align, head, rows }),
            Frame::Code { lang, text } => {
                if lang.as_deref() == Some("math") {
                    self.block(Block::Math {
                        source: text.trim().to_owned(),
                    });
                } else {
                    self.block(Block::Code { lang, text });
                }
            }
            Frame::Html { text } => {
                let text = text.trim_end().to_owned();
                if !text.is_empty() {
                    self.block(Block::Paragraph {
                        content: vec![Inline::Text { text }],
                    });
                }
            }
        }
    }

    fn styled(&mut self, inline: Inline) {
        self.depth -= 1;
        self.inline(inline);
    }

    fn block(&mut self, block: Block) {
        self.close_implicit();
        if let Some(Frame::Blocks { blocks, .. }) = self.stack.last_mut() {
            blocks.push(block);
            self.nodes += 1;
        }
    }

    fn inline(&mut self, inline: Inline) {
        if matches!(self.stack.last(), Some(Frame::Blocks { .. })) {
            self.stack.push(inlines(InlinesKind::Implicit));
        }
        let Some(Frame::Inlines { content, .. }) = self.stack.last_mut() else {
            return;
        };
        match (content.last_mut(), inline) {
            (Some(Inline::Text { text }), Inline::Text { text: more }) => text.push_str(&more),
            (_, inline) => {
                content.push(inline);
                self.nodes += 1;
            }
        }
    }

    /// A task list item's checkbox, set on the item when it starts it, else kept as text.
    fn task(&mut self, checked: bool) {
        let mut frames = self.stack.iter_mut().rev();
        let item = loop {
            match frames.next() {
                Some(Frame::Inlines {
                    kind: InlinesKind::Paragraph | InlinesKind::Implicit,
                    content,
                    ..
                }) if content.is_empty() => continue,
                Some(Frame::Blocks {
                    kind: BlocksKind::Item { task },
                    blocks,
                }) if blocks.is_empty() && task.is_none() => break Some(task),
                _ => break None,
            }
        };
        match item {
            Some(task) => *task = Some(checked),
            None => self.inline(Inline::Text {
                text: if checked { "[x] " } else { "[ ] " }.into(),
            }),
        }
    }
}

fn inlines(kind: InlinesKind) -> Frame {
    Frame::Inlines {
        kind,
        content: Vec::new(),
        display: 0,
    }
}

/// An inline frame inside a paragraph (a style, a link or an image).
fn styles(frame: &Frame) -> bool {
    matches!(
        frame,
        Frame::Inlines {
            kind: InlinesKind::Emphasis
                | InlinesKind::Strong
                | InlinesKind::Strike
                | InlinesKind::Link(_)
                | InlinesKind::Image(_),
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Inline {
        Inline::Text { text: text.into() }
    }

    fn math(source: &str) -> Inline {
        Inline::Math {
            source: source.into(),
        }
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph { content }
    }

    fn link(href: &str, label: &str) -> Inline {
        Inline::Link {
            href: href.into(),
            content: vec![text(label)],
        }
    }

    /// The one paragraph `source` makes.
    fn inline(source: &str) -> Vec<Inline> {
        match parse(source).as_slice() {
            [Block::Paragraph { content }] => content.clone(),
            other => panic!("{source:?} gave {other:?}"),
        }
    }

    // Upstream's `markdownMath.test.ts`, case for case, on the rewritten source.

    #[test]
    fn keeps_valid_inline_and_display_math() {
        let src = "Inline $c=\\frac{a+b}{2}$ then\n\n$$\nS(a,b)=\\frac{b-a}{6}\\left[f(a)+4f(c)+f(b)\\right].\n$$";
        assert_eq!(normalize_math(src), src);
        assert_eq!(
            parse(src),
            [
                para(vec![
                    text("Inline "),
                    math("c=\\frac{a+b}{2}"),
                    text(" then")
                ]),
                Block::Math {
                    source: "S(a,b)=\\frac{b-a}{6}\\left[f(a)+4f(c)+f(b)\\right].".into()
                },
            ]
        );
    }

    #[test]
    fn dollar_amounts_stay_prose() {
        assert_eq!(
            normalize_math("Costs $5 and $10 each"),
            r"Costs \$5 and \$10 each"
        );
        assert_eq!(
            inline("Costs $5 and $10 each"),
            [text("Costs $5 and $10 each")]
        );
    }

    #[test]
    fn a_later_pair_closes_after_an_invalid_opener() {
        assert_eq!(
            normalize_math("Budget $5, then $x^2$"),
            r"Budget \$5, then $x^2$"
        );
        assert_eq!(
            inline("Budget $5, then $x^2$"),
            [text("Budget $5, then "), math("x^2")]
        );
    }

    #[test]
    fn whitespace_inside_a_delimiter_is_no_math() {
        assert_eq!(normalize_math("a $ x$ b"), r"a \$ x\$ b");
        assert_eq!(normalize_math("a $x $ b"), r"a \$x \$ b");
    }

    #[test]
    fn a_closer_followed_by_a_digit_is_no_math() {
        assert_eq!(
            normalize_math("Price range $5-$10"),
            r"Price range \$5-\$10"
        );
        assert_eq!(normalize_math("a $x$5 b"), r"a \$x\$5 b");
        assert_eq!(inline("a $x$5 b"), [text("a $x$5 b")]);
    }

    #[test]
    fn inline_math_never_spans_a_blank_line() {
        assert_eq!(normalize_math("one $a\n\nb$ two"), "one \\$a\n\nb\\$ two");
    }

    #[test]
    fn reads_the_backtick_form() {
        assert_eq!(
            normalize_math("see $`\\sqrt{3x-1}+(1+x)^2`$ here"),
            "see $\\sqrt{3x-1}+(1+x)^2$ here"
        );
        assert_eq!(
            inline("see $`\\sqrt{3x-1}+(1+x)^2`$ here"),
            [text("see "), math("\\sqrt{3x-1}+(1+x)^2"), text(" here")]
        );
    }

    /// Unlike remark-math, pulldown-cmark never closes on an escaped dollar, so a body
    /// holding one keeps its single delimiters.
    #[test]
    fn a_literal_dollar_inside_math_stays_inside() {
        assert_eq!(normalize_math("$`\\text{$}`$"), "$\\text{\\$}$");
        assert_eq!(inline("$`\\text{$}`$ x"), [math("\\text{\\$}"), text(" x")]);
        assert_eq!(
            normalize_math(r"pay $\sqrt{\$4}$ now"),
            r"pay $\sqrt{\$4}$ now"
        );
        assert_eq!(
            inline(r"pay $\sqrt{\$4}$ now"),
            [text("pay "), math(r"\sqrt{\$4}"), text(" now")]
        );
    }

    #[test]
    fn dollars_in_code_never_count() {
        let src = "run `echo $HOME` then\n\n```sh\necho $5 and $10\n```\n\nand $5 more";
        assert_eq!(
            normalize_math(src),
            "run `echo $HOME` then\n\n```sh\necho $5 and $10\n```\n\nand \\$5 more"
        );
        assert_eq!(
            parse(src),
            [
                para(vec![
                    text("run "),
                    Inline::Code {
                        text: "echo $HOME".into()
                    },
                    text(" then")
                ]),
                Block::Code {
                    lang: Some("sh".into()),
                    text: "echo $5 and $10\n".into()
                },
                para(vec![text("and $5 more")]),
            ]
        );
    }

    #[test]
    fn a_code_span_closes_on_an_equal_backtick_run() {
        assert_eq!(normalize_math("``a ` $b`` $5"), "``a ` $b`` \\$5");
    }

    #[test]
    fn indented_code_is_literal() {
        let src = "Run this:\n\n    echo $5 and $10\n    ls $HOME\n\nthen $5";
        assert_eq!(
            normalize_math(src),
            "Run this:\n\n    echo $5 and $10\n    ls $HOME\n\nthen \\$5"
        );
    }

    #[test]
    fn nested_fences_are_literal() {
        let quoted = "> ~~~\n> echo $5 and $10\n> ~~~";
        assert_eq!(normalize_math(quoted), quoted);
        let listed = "1. x\n\n   ```sh\n   echo $5 and $10\n   ```";
        assert_eq!(normalize_math(listed), listed);
        let doc = parse(listed);
        let [Block::List { items, .. }] = doc.as_slice() else {
            panic!();
        };
        assert_eq!(
            items[0].blocks[1],
            Block::Code {
                lang: Some("sh".into()),
                text: "echo $5 and $10\n".into()
            }
        );
    }

    /// A bare URL's dollars are escaped (it is text to the parser), which reads the same.
    #[test]
    fn autolinks_raw_html_and_definitions_are_literal() {
        let src = "see <https://x.com/?a=$5> or https://y.com/$6 and <a href='?q=$7'>x</a>\n\n[ref]: https://r.com/$8";
        assert_eq!(normalize_math(src), src.replace("y.com/$6", "y.com/\\$6"));
        assert_eq!(
            inline(src),
            [
                text("see "),
                link("https://x.com/?a=$5", "https://x.com/?a=$5"),
                text(" or "),
                link("https://y.com/$6", "https://y.com/$6"),
                text(" and <a href='?q=$7'>x</a>"),
            ]
        );
    }

    #[test]
    fn dollars_in_a_link_text_are_prose() {
        assert_eq!(
            normalize_math("[costs $5](https://x.com)"),
            r"[costs \$5](https://x.com)"
        );
        assert_eq!(
            inline("[costs $5](https://x.com)"),
            [link("https://x.com", "costs $5")]
        );
    }

    #[test]
    fn keeps_escapes() {
        assert_eq!(normalize_math(r"pay \$5 for $x$"), r"pay \$5 for $x$");
        assert_eq!(inline(r"pay \$5 for $x$"), [text("pay $5 for "), math("x")]);
    }

    #[test]
    fn math_fences_and_inline_double_dollars() {
        assert_eq!(
            parse("```math\n\\sqrt{2}\n```"),
            [Block::Math {
                source: "\\sqrt{2}".into()
            }]
        );
        assert_eq!(inline("x $$y$$ z"), [text("x "), math("y"), text(" z")]);
    }

    // The tree.

    #[test]
    fn reads_headings_lists_and_styles() {
        let doc = parse("# Lab *one*\n\n3. **first**\n4. ~~second~~\n\n---\n\n> quoted\\\nline");
        assert_eq!(
            doc,
            [
                Block::Heading {
                    level: 1,
                    content: vec![
                        text("Lab "),
                        Inline::Emphasis {
                            content: vec![text("one")]
                        }
                    ]
                },
                Block::List {
                    ordered: true,
                    start: 3,
                    items: vec![
                        ListItem {
                            task: None,
                            blocks: vec![para(vec![Inline::Strong {
                                content: vec![text("first")]
                            }])]
                        },
                        ListItem {
                            task: None,
                            blocks: vec![para(vec![Inline::Strike {
                                content: vec![text("second")]
                            }])]
                        },
                    ]
                },
                Block::Rule,
                Block::Quote {
                    blocks: vec![para(vec![text("quoted"), Inline::Break, text("line")])]
                },
            ]
        );
    }

    #[test]
    fn reads_task_lists() {
        let doc =
            parse("- [x] read the spec\n- [ ] write tests\n- plain\n\n* [ ] loose\n\n* [x] two");
        let [Block::List { items, .. }, Block::List { items: loose, .. }] = doc.as_slice() else {
            panic!("{doc:?}");
        };
        let tasks: Vec<_> = items.iter().map(|item| item.task).collect();
        assert_eq!(tasks, [Some(true), Some(false), None]);
        assert_eq!(items[0].blocks, [para(vec![text("read the spec")])]);
        let tasks: Vec<_> = loose.iter().map(|item| item.task).collect();
        assert_eq!(tasks, [Some(false), Some(true)]);
        assert_eq!(loose[1].blocks, [para(vec![text("two")])]);
    }

    #[test]
    fn reads_tables() {
        let doc = parse("| Test | Points |\n| :--- | ---: |\n| `compiles` | 2 |\n| runs | **3** |");
        assert_eq!(
            doc,
            [Block::Table {
                align: vec![Some(Align::Start), Some(Align::End)],
                head: vec![vec![text("Test")], vec![text("Points")]],
                rows: vec![
                    vec![
                        vec![Inline::Code {
                            text: "compiles".into()
                        }],
                        vec![text("2")]
                    ],
                    vec![
                        vec![text("runs")],
                        vec![Inline::Strong {
                            content: vec![text("3")]
                        }]
                    ],
                ]
            }]
        );
    }

    #[test]
    fn numbers_footnotes_by_first_reference() {
        let doc = parse(
            "A[^b] and B[^a] and A again[^b].\n\n[^a]: Second.\n[^b]: First.\n[^unused]: Never.",
        );
        assert_eq!(
            doc,
            [
                para(vec![
                    text("A"),
                    Inline::FootnoteRef { number: 1 },
                    text(" and B"),
                    Inline::FootnoteRef { number: 2 },
                    text(" and A again"),
                    Inline::FootnoteRef { number: 1 },
                    text("."),
                ]),
                Block::Footnotes {
                    notes: vec![
                        Footnote {
                            number: 1,
                            blocks: vec![para(vec![text("First.")])]
                        },
                        Footnote {
                            number: 2,
                            blocks: vec![para(vec![text("Second.")])]
                        },
                    ]
                },
            ]
        );
    }

    #[test]
    fn opens_only_web_and_mail_links() {
        assert_eq!(inline("[a](javascript:alert(1))"), [text("a")]);
        assert_eq!(inline("[a](JavaScript:alert(1))"), [text("a")]);
        assert_eq!(inline("[a](data:text/html,<b>x</b>)"), [text("a")]);
        assert_eq!(inline("[a](file:///etc/passwd)"), [text("a")]);
        assert_eq!(inline("[a](spec.md)"), [text("a")]);
        assert_eq!(
            inline("<javascript:alert(1)>"),
            [text("javascript:alert(1)")]
        );
        assert_eq!(inline("[a](HTTPS://x.org)"), [link("HTTPS://x.org", "a")]);
        assert_eq!(
            inline("[mail](mailto:staff@school.example)"),
            [link("mailto:staff@school.example", "mail")]
        );
        assert_eq!(inline("[a](http://)"), [text("a")]);
    }

    #[test]
    fn links_bare_urls() {
        assert_eq!(
            inline(
                "See https://docs.example.org/a_b_c. Or www.example.org/x), or (https://e.org/w_(x))!"
            ),
            [
                text("See "),
                link(
                    "https://docs.example.org/a_b_c",
                    "https://docs.example.org/a_b_c"
                ),
                text(". Or "),
                link("https://www.example.org/x", "www.example.org/x"),
                text("), or ("),
                link("https://e.org/w_(x)", "https://e.org/w_(x)"),
                text(")!"),
            ]
        );
        // Not inside a word, a link or code.
        assert_eq!(inline("xhttps://a.org"), [text("xhttps://a.org")]);
        assert_eq!(
            inline("[site https://a.org](https://b.org) `https://c.org`"),
            [
                link("https://b.org", "site https://a.org"),
                text(" "),
                Inline::Code {
                    text: "https://c.org".into()
                },
            ]
        );
    }

    #[test]
    fn raw_html_is_inert_text() {
        assert_eq!(
            inline("<b onclick=\"x()\">bold</b> <script>alert(1)</script>"),
            [text(
                "<b onclick=\"x()\">bold</b> <script>alert(1)</script>"
            )]
        );
        assert_eq!(
            parse("<details>\n<summary>Hint</summary>\n\nUse a loop.\n\n</details>"),
            [
                para(vec![text("<details>\n<summary>Hint</summary>")]),
                para(vec![text("Use a loop.")]),
                para(vec![text("</details>")]),
            ]
        );
    }

    #[test]
    fn images_become_links_to_them() {
        assert_eq!(
            inline("![the *maze*](https://x.org/maze.png)"),
            [Inline::Link {
                href: "https://x.org/maze.png".into(),
                content: vec![
                    text("the "),
                    Inline::Emphasis {
                        content: vec![text("maze")]
                    }
                ]
            }]
        );
        assert_eq!(
            inline("![](https://x.org/a.png)"),
            [link("https://x.org/a.png", "https://x.org/a.png")]
        );
        assert_eq!(inline("![alt](data:image/png;base64,AAAA)"), [text("alt")]);
    }

    fn depth(blocks: &[Block]) -> usize {
        fn inlines(content: &[Inline]) -> usize {
            content
                .iter()
                .map(|inline| match inline {
                    Inline::Emphasis { content }
                    | Inline::Strong { content }
                    | Inline::Strike { content }
                    | Inline::Link { content, .. } => 1 + inlines(content),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        }
        blocks
            .iter()
            .map(|block| match block {
                Block::Quote { blocks } => 1 + depth(blocks),
                Block::List { items, .. } => {
                    1 + items
                        .iter()
                        .map(|item| depth(&item.blocks))
                        .max()
                        .unwrap_or(0)
                }
                Block::Paragraph { content } | Block::Heading { content, .. } => inlines(content),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn caps_the_nesting() {
        let quotes = format!("{} deep", ">".repeat(500));
        let doc = parse(&quotes);
        assert_eq!(depth(&doc), MAX_DEPTH);
        assert!(format!("{doc:?}").contains("deep"));

        let lists: String = (0..100)
            .map(|n| format!("{}- level {n}\n", "  ".repeat(n)))
            .collect();
        let doc = parse(&lists);
        assert!(depth(&doc) <= MAX_DEPTH);
        assert!(format!("{doc:?}").contains("level 99"));

        let styles = format!("{}x{}", "*_".repeat(100), "_*".repeat(100));
        assert!(depth(&parse(&styles)) <= MAX_DEPTH);
        let links = format!("{}x{}", "[*".repeat(100), "*](https://a.org)".repeat(100));
        assert!(depth(&parse(&links)) <= MAX_DEPTH);
    }

    fn count(blocks: &[Block]) -> usize {
        fn inlines(content: &[Inline]) -> usize {
            content
                .iter()
                .map(|inline| match inline {
                    Inline::Emphasis { content }
                    | Inline::Strong { content }
                    | Inline::Strike { content }
                    | Inline::Link { content, .. } => 1 + inlines(content),
                    _ => 1,
                })
                .sum()
        }
        blocks
            .iter()
            .map(|block| {
                1 + match block {
                    Block::Paragraph { content } | Block::Heading { content, .. } => {
                        inlines(content)
                    }
                    Block::Quote { blocks } => count(blocks),
                    Block::List { items, .. } => {
                        items.iter().map(|item| 1 + count(&item.blocks)).sum()
                    }
                    Block::Table { head, rows, .. } => head
                        .iter()
                        .chain(rows.iter().flatten())
                        .map(|cell| 1 + inlines(cell))
                        .sum(),
                    Block::Footnotes { notes } => {
                        notes.iter().map(|note| count(&note.blocks)).sum()
                    }
                    _ => 0,
                }
            })
            .sum()
    }

    #[test]
    fn caps_the_size() {
        let long = "word ".repeat(20_000);
        let doc = parse(&long);
        let [Block::Paragraph { content }] = doc.as_slice() else {
            panic!();
        };
        let [Inline::Text { text }] = content.as_slice() else {
            panic!();
        };
        assert!(text.len() <= MAX_SOURCE);

        // Cut on a character, not inside one.
        let wide = "é".repeat(MAX_SOURCE);
        assert!(!parse(&wide).is_empty());

        let many = "*a* `b` ".repeat(5_000);
        assert!(count(&parse(&many)) <= MAX_NODES + MAX_DEPTH + 2);
        let cells = format!("|a|b|\n|-|-|\n{}", "|`x`|*y*|\n".repeat(3_000));
        assert!(count(&parse(&cells)) <= MAX_NODES + MAX_DEPTH + 4);
    }

    #[test]
    fn nothing_to_show_is_none() {
        assert_eq!(document(None), None);
        assert_eq!(document(Some("  \n ")), None);
        assert_eq!(
            document(Some("<!-- -->")),
            Some(vec![para(vec![text("<!-- -->")])])
        );
        assert_eq!(document(Some("Hi")), Some(vec![para(vec![text("Hi")])]));
    }

    #[test]
    fn windows_line_ends_read_like_unix_ones() {
        assert_eq!(parse("a $5\r\n\r\nb$ c"), parse("a $5\n\nb$ c"));
        assert_eq!(parse("a $5\r\n\r\nb$ c").len(), 2);
    }

    /// The description the UI flows show (`tests/e2e/description.json`) is what this parser
    /// makes of its source; `UPDATE_GOLDEN=1` rewrites the tree.
    #[test]
    fn the_ui_fixture_matches_the_parser() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/e2e/description.json");
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let source = saved["source"].as_str().unwrap();
        let fresh = serde_json::json!({ "source": source, "doc": parse(source) });
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            let json = serde_json::to_string_pretty(&fresh).unwrap();
            std::fs::write(path, json + "\n").unwrap();
        } else {
            assert_eq!(saved, fresh, "stale: UPDATE_GOLDEN=1 cargo test markdown");
        }
    }
}
