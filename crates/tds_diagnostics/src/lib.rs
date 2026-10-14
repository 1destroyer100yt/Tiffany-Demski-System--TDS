//! Shared diagnostic types for every stage of the TDS compiler.
//!
//! Each stage (lexer, parser, checker, ...) produces `Diagnostic` values
//! instead of printing anything. Rendering to the terminal (with source
//! snippets, colors, help lines) happens once, in the driver/CLI, so every
//! stage stays a pure function of its input.

pub mod codes;
mod source_map;

pub use source_map::{LineCol, SourceFile, SourceMap};

/// A half-open byte range `start..end` into a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        debug_assert!(start <= end, "span start {start} is after end {end}");
        Span { start, end }
    }

    /// A zero-width span at `at`. Used for tokens that have no text of their
    /// own, such as DEDENT or a NEWLINE synthesized at end of file.
    pub fn empty(at: usize) -> Span {
        Span { start: at, end: at }
    }

    /// The smallest span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A span with a message attached. The primary label marks *where* the
/// problem is; secondary labels point at related places ("opened here").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

/// One error or warning, with everything needed to render it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable error code such as `TDS0001`. See [`codes`].
    pub code: &'static str,
    /// One-line summary shown in the header.
    pub message: String,
    pub labels: Vec<Label>,
    /// `= note:` lines: plain-language explanations.
    pub notes: Vec<String>,
    /// `= help:` lines: suggested fixes.
    pub help: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(code, message)
        }
    }

    pub fn with_primary(mut self, span: Span, message: impl Into<String>) -> Diagnostic {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: true,
        });
        self
    }

    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Diagnostic {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Diagnostic {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help.push(help.into());
        self
    }

    /// The span of the first primary label, if any.
    pub fn primary_span(&self) -> Option<Span> {
        self.labels.iter().find(|l| l.primary).map(|l| l.span)
    }
}

/// The long-form explanation of an error code, for `tds explain`.
pub fn explain(code: &str) -> Option<&'static str> {
    const TEXT: &str = include_str!("explanations.md");
    let code = code.trim().to_uppercase();
    let header = format!("## {code}\n");
    let start = TEXT.find(&header)? + header.len();
    let rest = &TEXT[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code in the registry has an explanation, and every
    /// explanation belongs to a real code.
    #[test]
    fn every_code_is_explained() {
        let registry = include_str!("codes.rs");
        let codes: Vec<&str> = registry
            .split('"')
            .filter(|s| s.len() == 7 && s.starts_with("TDS"))
            .collect();
        assert!(codes.len() > 80);
        for code in &codes {
            assert!(
                explain(code).is_some_and(|t| !t.is_empty()),
                "{code} has no explanation"
            );
        }
        for section in include_str!("explanations.md").split("\n## ").skip(1) {
            let code = &section[..7];
            assert!(codes.contains(&code), "explanation for unknown code {code}");
        }
    }

    #[test]
    fn explain_is_case_insensitive() {
        assert_eq!(explain("tds0412"), explain("TDS0412"));
        assert!(explain("TDS9999").is_none());
    }
}

#[cfg(test)]
mod spec_tests {
    /// The error table in the language reference lists every code.
    #[test]
    fn spec_lists_every_code() {
        let spec = include_str!("../../../docs/spec/errors.md");
        for code in include_str!("codes.rs")
            .split('"')
            .filter(|s| s.len() == 7 && s.starts_with("TDS"))
        {
            assert!(
                spec.contains(&format!("| {code} |")),
                "docs/spec/errors.md doesn't list {code}"
            );
        }
    }
}
