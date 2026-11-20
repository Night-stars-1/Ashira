//! Incremental syntax highlighting for fenced code blocks.
//!
//! Inputs are single lines without line endings, already sanitized by the renderer.

use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Color as SyntaxColor, FontStyle, StyleModifier, Theme, ThemeItem},
    parsing::SyntaxSet,
};

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn theme() -> Theme {
    let mut theme = Theme::default();
    // A transparent foreground is our sentinel for inheriting the terminal foreground.
    theme.settings.foreground = Some(SyntaxColor { r: 0, g: 0, b: 0, a: 0 });
    for (scope, (r, g, b), font_style) in [
        ("keyword, storage", (125, 207, 255), None),
        ("entity.name.function", (105, 220, 158), None),
        ("string", (240, 223, 120), None),
        ("constant.numeric", (255, 179, 102), None),
        ("comment", (145, 153, 166), Some(FontStyle::ITALIC)),
    ] {
        theme.scopes.push(ThemeItem {
            scope: scope.parse().expect("valid built-in scope selector"),
            style: StyleModifier {
                foreground: Some(SyntaxColor { r, g, b, a: 255 }),
                font_style,
                ..StyleModifier::default()
            },
        });
    }
    theme
}

/// Each fenced block owns its parser state; shared grammar and theme load lazily.
pub struct CodeHighlighter {
    highlighter: Option<HighlightLines<'static>>,
}

impl CodeHighlighter {
    pub fn new(info: &str) -> Self {
        let token = info.split_whitespace().next().unwrap_or("");
        if token.is_empty() {
            return Self { highlighter: None };
        }
        let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_nonewlines);
        let highlighter = syntaxes.find_syntax_by_token(token).map(|syntax| {
            HighlightLines::new(syntax, THEME.get_or_init(theme))
        });
        Self { highlighter }
    }

    /// Advance the block state exactly once for a completed line.
    pub fn highlight_line(&mut self, line: &str) -> Vec<Span<'static>> {
        let Some(highlighter) = &mut self.highlighter else {
            return vec![Span::raw(line.to_owned())];
        };
        match highlighter.highlight_line(line, SYNTAXES.get().expect("initialized with highlighter")) {
            Ok(regions) => regions.into_iter().map(|(syntax_style, text)| {
                let mut style = Style::new();
                let color = syntax_style.foreground;
                if color.a != 0 {
                    style = style.fg(Color::Rgb(color.r, color.g, color.b));
                }
                if syntax_style.font_style.contains(FontStyle::ITALIC) {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if syntax_style.font_style.contains(FontStyle::BOLD) {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if syntax_style.font_style.contains(FontStyle::UNDERLINE) {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                Span::styled(text.to_owned(), style)
            }).collect(),
            Err(_) => {
                // A failed parse may leave parser state inconsistent: preserve all text
                // and render the rest of this block without syntax coloring.
                self.highlighter = None;
                vec![Span::raw(line.to_owned())]
            }
        }
    }

    /// An unfinished line is displayed without mutating the committed parser state.
    pub fn preview_line(&self, line: &str) -> Vec<Span<'static>> {
        vec![Span::raw(line.to_owned())]
    }
}
