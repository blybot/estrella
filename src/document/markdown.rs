//! Emit logic for the Markdown component.

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::types::Markdown;
use crate::ir::Op;
use crate::protocol::text::{Alignment, Font};

impl Markdown {
    /// Emit IR ops for this markdown component.
    pub fn emit(&self, ops: &mut Vec<Op>) {
        // Skip empty content
        if self.content.trim().is_empty() {
            return;
        }

        // Ensure we start in a known state (left-aligned)
        ops.push(Op::SetAlign(Alignment::Left));

        let parser = Parser::new(&self.content);
        let mut state = ParserState::new(self.show_urls);

        for event in parser {
            match event {
                Event::Start(tag) => state.handle_start_tag(tag, ops),
                Event::End(tag_end) => state.handle_end_tag(tag_end, ops),
                Event::Text(text) => state.handle_text(&text, ops),
                Event::Code(code) => state.handle_inline_code(&code, ops),
                Event::SoftBreak => ops.push(Op::Text(" ".into())),
                Event::HardBreak => ops.push(Op::Newline),
                Event::Rule => {
                    ops.push(Op::Text("\u{2500}".repeat(48)));
                    ops.push(Op::Newline);
                }
                _ => {}
            }
        }
    }
}

/// Internal state for tracking nested formatting during parsing.
struct ParserState {
    show_urls: bool,
    list_depth: usize,
    /// One entry per open list: the next number, or None for bullets.
    list_counters: Vec<Option<usize>>,
    pending_url: Option<String>,
    pending_list_prefix: Option<String>,
    in_heading: bool,
    heading_level: Option<HeadingLevel>,
    just_finished_heading: bool,
}

impl ParserState {
    fn new(show_urls: bool) -> Self {
        Self {
            show_urls,
            list_depth: 0,
            list_counters: Vec::new(),
            pending_url: None,
            pending_list_prefix: None,
            in_heading: false,
            heading_level: None,
            just_finished_heading: false,
        }
    }

    fn handle_start_tag(&mut self, tag: Tag, ops: &mut Vec<Op>) {
        match tag {
            Tag::Paragraph => {
                if self.just_finished_heading {
                    ops.push(Op::Newline);
                    self.just_finished_heading = false;
                }
            }
            Tag::Heading { level, .. } => {
                self.in_heading = true;
                self.heading_level = Some(level);

                match level {
                    HeadingLevel::H1 => {
                        ops.push(Op::SetAlign(Alignment::Center));
                        ops.push(Op::SetBold(true));
                        ops.push(Op::SetSize {
                            height: 3,
                            width: 3,
                        });
                        ops.push(Op::SetSmoothing(true));
                    }
                    HeadingLevel::H2 => {
                        ops.push(Op::SetAlign(Alignment::Center));
                        ops.push(Op::SetBold(true));
                        ops.push(Op::SetSize {
                            height: 2,
                            width: 2,
                        });
                        ops.push(Op::SetSmoothing(true));
                    }
                    HeadingLevel::H3 => {
                        ops.push(Op::SetBold(true));
                        ops.push(Op::SetSize {
                            height: 1,
                            width: 1,
                        });
                        ops.push(Op::SetSmoothing(true));
                    }
                    HeadingLevel::H4 => {
                        ops.push(Op::SetBold(true));
                        ops.push(Op::SetExpandedHeight(1));
                        ops.push(Op::SetSmoothing(true));
                    }
                    HeadingLevel::H5 => {
                        ops.push(Op::SetBold(true));
                    }
                    HeadingLevel::H6 => {
                        ops.push(Op::SetFont(Font::B));
                        ops.push(Op::SetBold(true));
                    }
                }
            }
            Tag::Strong => {
                ops.push(Op::SetBold(true));
            }
            Tag::Emphasis => {
                ops.push(Op::SetUnderline(true));
            }
            Tag::Link { dest_url, .. } => {
                ops.push(Op::SetUnderline(true));
                if self.show_urls {
                    self.pending_url = Some(dest_url.to_string());
                }
            }
            Tag::List(start_num) => {
                // A nested list starts after its parent item's text.
                if self.list_depth > 0 {
                    end_line(ops);
                }
                self.list_counters
                    .push(start_num.map(|start| start as usize));
                self.list_depth += 1;
                if self.list_depth == 1 {
                    ops.push(Op::Newline);
                }
            }
            Tag::Item => {
                let indent_dots = (self.list_depth - 1) * 32;
                if indent_dots > 0 {
                    ops.push(Op::SetAbsolutePosition(indent_dots as u16));
                }

                if let Some(Some(counter)) = self.list_counters.last_mut() {
                    self.pending_list_prefix = Some(format!("{}. ", counter));
                    *counter += 1;
                } else {
                    self.pending_list_prefix = Some("* ".into());
                }
            }
            Tag::CodeBlock(_kind) => {
                ops.push(Op::SetInvert(true));
                ops.push(Op::SetFont(Font::B));
            }
            _ => {}
        }
    }

    fn handle_end_tag(&mut self, tag_end: TagEnd, ops: &mut Vec<Op>) {
        match tag_end {
            TagEnd::Paragraph => {
                ops.push(Op::Newline);
                ops.push(Op::Newline);
            }
            TagEnd::Heading(_level) => {
                self.in_heading = false;

                match self.heading_level {
                    Some(HeadingLevel::H1) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetSize {
                            height: 0,
                            width: 0,
                        });
                        ops.push(Op::SetBold(false));
                        ops.push(Op::SetSmoothing(false));
                        ops.push(Op::SetAlign(Alignment::Left));
                        ops.push(Op::Feed { units: 12 });
                    }
                    Some(HeadingLevel::H2) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetSize {
                            height: 0,
                            width: 0,
                        });
                        ops.push(Op::SetBold(false));
                        ops.push(Op::SetSmoothing(false));
                        ops.push(Op::SetAlign(Alignment::Left));
                        ops.push(Op::Feed { units: 6 });
                    }
                    Some(HeadingLevel::H3) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetSize {
                            height: 0,
                            width: 0,
                        });
                        ops.push(Op::SetBold(false));
                        ops.push(Op::SetSmoothing(false));
                        ops.push(Op::Feed { units: 4 });
                    }
                    Some(HeadingLevel::H4) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetExpandedHeight(0));
                        ops.push(Op::SetBold(false));
                        ops.push(Op::SetSmoothing(false));
                        ops.push(Op::Feed { units: 4 });
                    }
                    Some(HeadingLevel::H5) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetBold(false));
                        ops.push(Op::Feed { units: 2 });
                    }
                    Some(HeadingLevel::H6) => {
                        ops.push(Op::Newline);
                        ops.push(Op::SetBold(false));
                        ops.push(Op::SetFont(Font::A));
                    }
                    None => {
                        ops.push(Op::Newline);
                    }
                }

                self.heading_level = None;
                self.just_finished_heading = true;
            }
            TagEnd::Strong => {
                ops.push(Op::SetBold(false));
            }
            TagEnd::Emphasis => {
                ops.push(Op::SetUnderline(false));
            }
            TagEnd::Link => {
                ops.push(Op::SetUnderline(false));

                if let Some(url) = self.pending_url.take() {
                    ops.push(Op::Text(" (".into()));
                    ops.push(Op::SetFont(Font::C));
                    ops.push(Op::Text(url));
                    ops.push(Op::SetFont(Font::A));
                    ops.push(Op::Text(")".into()));
                }
            }
            TagEnd::List(_is_ordered) => {
                self.list_counters.pop();
                self.list_depth = self.list_depth.saturating_sub(1);
                // Blank line after the outermost list only.
                if self.list_depth == 0 {
                    ops.push(Op::Newline);
                }
            }
            TagEnd::Item => {
                end_line(ops);
                ops.push(Op::SetAbsolutePosition(0));
            }
            TagEnd::CodeBlock => {
                ops.push(Op::SetInvert(false));
                ops.push(Op::SetFont(Font::A));
                ops.push(Op::Newline);
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str, ops: &mut Vec<Op>) {
        if let Some(prefix) = self.pending_list_prefix.take() {
            ops.push(Op::Text(format!("{}{}", prefix, text)));
        } else {
            ops.push(Op::Text(text.to_string()));
        }
    }

    fn handle_inline_code(&mut self, code: &str, ops: &mut Vec<Op>) {
        ops.push(Op::SetInvert(true));
        ops.push(Op::Text(code.to_string()));
        ops.push(Op::SetInvert(false));
    }
}

/// End the current line, unless nothing has been printed on it yet.
fn end_line(ops: &mut Vec<Op>) {
    let at_line_start = ops
        .iter()
        .rev()
        .find(|op| matches!(op, Op::Text(_) | Op::Newline))
        .is_none_or(|op| matches!(op, Op::Newline));
    if !at_line_start {
        ops.push(Op::Newline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_markdown(text: &str) -> Vec<Op> {
        let md = Markdown::new(text);
        let mut ops = Vec::new();
        md.emit(&mut ops);
        ops
    }

    #[test]
    fn test_basic_bold() {
        let ops = compile_markdown("**bold text**");
        assert!(ops.contains(&Op::SetBold(true)));
        assert!(ops.contains(&Op::Text("bold text".into())));
        assert!(ops.contains(&Op::SetBold(false)));
    }

    #[test]
    fn test_basic_italic() {
        let ops = compile_markdown("*italic text*");
        assert!(ops.contains(&Op::SetUnderline(true)));
        assert!(ops.contains(&Op::Text("italic text".into())));
        assert!(ops.contains(&Op::SetUnderline(false)));
    }

    #[test]
    fn test_heading_h1() {
        let ops = compile_markdown("# Title");
        assert!(ops.contains(&Op::SetAlign(Alignment::Center)));
        assert!(ops.contains(&Op::SetSize {
            height: 3,
            width: 3
        }));
        assert!(ops.contains(&Op::Text("Title".into())));
    }

    #[test]
    fn test_unordered_list() {
        let ops = compile_markdown("- Item 1\n- Item 2");
        let text_ops: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                Op::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(text_ops.iter().any(|s: &&str| s.contains("* ")));
    }

    /// The printed lines: (indent in dots, text). Style ops are ignored.
    fn lines(ops: &[Op]) -> Vec<(u16, String)> {
        let mut lines = vec![(0, String::new())];
        for op in ops {
            let current = lines.last_mut().unwrap();
            match op {
                Op::Text(s) => current.1.push_str(s),
                Op::SetAbsolutePosition(dots) if current.1.is_empty() => current.0 = *dots,
                Op::Newline => lines.push((0, String::new())),
                _ => {}
            }
        }
        lines
    }

    fn line(indent: u16, text: &str) -> (u16, String) {
        (indent, text.to_string())
    }

    #[test]
    fn nested_list_items_start_on_their_own_line() {
        let printed = lines(&compile_markdown("- one\n  - two\n    - three\n- four"));

        assert_eq!(
            printed,
            [
                line(0, ""),
                line(0, "* one"),
                line(32, "* two"),
                line(64, "* three"),
                line(0, "* four"),
                line(0, ""),
                line(0, ""),
            ]
        );
    }

    #[test]
    fn nested_ordered_lists_number_independently() {
        let printed = lines(&compile_markdown(
            "1. step one\n   1. sub a\n   2. sub b\n2. step two",
        ));

        let text: Vec<_> = printed
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .cloned()
            .collect();
        assert_eq!(
            text,
            [
                line(0, "1. step one"),
                line(32, "1. sub a"),
                line(32, "2. sub b"),
                line(0, "2. step two"),
            ]
        );
    }

    #[test]
    fn bullets_inside_a_numbered_list_stay_bullets() {
        let printed = lines(&compile_markdown("1. step\n   - note\n2. next"));

        let text: Vec<_> = printed
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .cloned()
            .collect();
        assert_eq!(
            text,
            [line(0, "1. step"), line(32, "* note"), line(0, "2. next")]
        );
    }

    #[test]
    fn a_list_is_followed_by_one_blank_line() {
        let printed = lines(&compile_markdown("- a\n  - b\n\nafter"));

        let after = printed.iter().position(|(_, t)| t == "after").unwrap();
        assert_eq!(printed[after - 1], line(0, ""));
        assert_ne!(printed[after - 2], line(0, ""), "{printed:?}");
    }

    #[test]
    fn test_empty_markdown() {
        let ops = compile_markdown("");
        assert!(ops.is_empty());
    }

    #[test]
    fn test_horizontal_rule() {
        let ops = compile_markdown("---");
        let text_ops: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                Op::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(text_ops.iter().any(|s: &&str| s.contains("\u{2500}")));
    }
}
