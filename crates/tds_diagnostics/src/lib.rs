//! Shared diagnostic types for every stage of the TDS compiler.
//!
//! Each stage (lexer, parser, checker, ...) produces `Diagnostic` values
//! instead of printing anything. Rendering to the terminal (with source
//! snippets, colors, help lines) happens once, in the driver/CLI, so every
//! stage stays a pure function of its input.

pub mod codes;

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
