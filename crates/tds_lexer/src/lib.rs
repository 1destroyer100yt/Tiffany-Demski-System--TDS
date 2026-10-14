//! The TDS lexer: source text → tokens, including INDENT / DEDENT / NEWLINE.
//!
//! # Layout rules
//!
//! TDS uses indentation for blocks, so the lexer turns leading whitespace
//! into explicit layout tokens and the parser never has to look at columns.
//!
//! * A *logical line* ends with a `NEWLINE` token.
//! * At the start of each logical line the indentation is compared with the
//!   top of an indentation stack:
//!   - deeper → push it and emit one `INDENT` (it must be exactly 4 deeper);
//!   - shallower → pop and emit one `DEDENT` per popped level; the line must
//!     land exactly on an enclosing level;
//!   - equal → nothing.
//! * Blank lines and comment-only lines are ignored completely.
//! * Inside `(` `[` `{` newlines and indentation are ignored.
//! * A line ending in a binary operator or `,` continues on the next line;
//!   the next line's indentation is ignored.
//! * At end of file a final `NEWLINE` is added if needed, then a `DEDENT` for
//!   every open block, then `EOF`.
//! * Tabs are an error anywhere outside strings and comments.
//!
//! The lexer never stops at the first error: it reports a [`Diagnostic`],
//! emits an `Error` token or recovers some other sensible way, and keeps going.

mod token;

pub use token::{StrPart, Token, TokenKind};

use tds_diagnostics::{codes, Diagnostic, Span};

/// Number of spaces per indentation level. Locked by the language design.
pub const INDENT_WIDTH: usize = 4;

/// The result of lexing a file: always a full token stream (ending in
/// `EOF`), plus any errors found along the way.
#[derive(Debug, Clone)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<Diagnostic>,
}

/// Lexes a whole source file.
pub fn lex(src: &str) -> LexOutput {
    let mut lexer = Lexer::new(src, 0, src.len());
    lexer.lex_file();
    LexOutput {
        tokens: lexer.tokens,
        errors: lexer.errors,
    }
}

struct Lexer<'a> {
    src: &'a str,
    /// Current byte offset.
    pos: usize,
    /// Byte offset where this lexer must stop. For a whole file this is
    /// `src.len()`; for an interpolation it is the closing `}`.
    end: usize,
    tokens: Vec<Token>,
    errors: Vec<Diagnostic>,
    /// Indentation widths of the open blocks. Always starts with 0.
    indents: Vec<usize>,
    /// After a misaligned line, `(column, stack depth)` it was assigned to.
    /// Following lines at the same column join the same block without
    /// another error.
    misaligned: Option<(usize, usize)>,
    /// Open brackets, innermost last, with the span of each.
    brackets: Vec<(char, Span)>,
    /// Set when the previous line ended in an operator or comma, so the
    /// next line's indentation doesn't count.
    continuation: bool,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, start: usize, end: usize) -> Lexer<'a> {
        Lexer {
            src,
            pos: start,
            end,
            tokens: Vec::new(),
            errors: Vec::new(),
            indents: vec![0],
            misaligned: None,
            brackets: Vec::new(),
            continuation: false,
        }
    }

    // ------------------------------------------------------------------
    // Character helpers
    // ------------------------------------------------------------------

    /// The character at byte offset `i`, or `None` past the end.
    fn char_at(&self, i: usize) -> Option<char> {
        if i >= self.end {
            return None;
        }
        self.src[i..self.end].chars().next()
    }

    fn peek(&self) -> Option<char> {
        self.char_at(self.pos)
    }

    /// The character `n` characters after the current one.
    fn peek_nth(&self, n: usize) -> Option<char> {
        self.src[self.pos..self.end].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Consumes `c` if it is next.
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn at_newline(&self) -> bool {
        matches!(self.peek(), Some('\n') | Some('\r'))
    }

    /// Consumes `\n`, `\r\n` or a lone `\r` and returns its span.
    fn consume_newline(&mut self) -> Span {
        let start = self.pos;
        // Either `\r` or `\n` alone, or `\r\n` as a pair.
        self.eat('\r');
        self.eat('\n');
        Span::new(start, self.pos)
    }

    fn push(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(Token { kind, span });
    }

    fn error(&mut self, diag: Diagnostic) {
        self.errors.push(diag);
    }

    fn tab_error(&mut self, span: Span) {
        self.error(
            Diagnostic::error(
                codes::TAB_CHARACTER,
                "tab characters are not allowed in TDS source",
            )
            .with_primary(span, "tab here")
            .with_note("TDS uses spaces only, so code looks the same in every editor")
            .with_help("replace each tab with 4 spaces"),
        );
    }

    // ------------------------------------------------------------------
    // Layout: lines, indentation, newlines
    // ------------------------------------------------------------------

    fn lex_file(&mut self) {
        while self.pos < self.end {
            // We are at the start of a physical line.
            let line_start = self.pos;
            let indent = self.read_indentation();
            let indent_span = Span::new(line_start, self.pos);

            // Blank and comment-only lines don't affect layout at all.
            match self.peek() {
                None => break,
                Some('\n') | Some('\r') => {
                    self.consume_newline();
                    continue;
                }
                Some('/') if self.peek_nth(1) == Some('/') => {
                    self.skip_line_comment();
                    if self.at_newline() {
                        self.consume_newline();
                    }
                    continue;
                }
                _ => {}
            }

            if !self.continuation {
                self.handle_indentation(indent, indent_span);
            }
            self.continuation = false;
            self.lex_line();
        }
        self.finish_file();
    }

    /// Consumes leading spaces (and, with an error, tabs) and returns the
    /// indentation width. A tab counts as 4 spaces for recovery.
    fn read_indentation(&mut self) -> usize {
        let start = self.pos;
        let mut width = 0;
        let mut saw_tab = false;
        loop {
            match self.peek() {
                Some(' ') => width += 1,
                Some('\t') => {
                    width += INDENT_WIDTH;
                    saw_tab = true;
                }
                _ => break,
            }
            self.bump();
        }
        // Tabs on a blank line are harmless; only complain on real lines.
        let blank = matches!(self.peek(), None | Some('\n') | Some('\r'))
            || (self.peek() == Some('/') && self.peek_nth(1) == Some('/'));
        if saw_tab && !blank {
            self.tab_error(Span::new(start, self.pos));
        }
        width
    }

    /// Compares a new line's indentation with the block stack and emits
    /// INDENT / DEDENT tokens.
    fn handle_indentation(&mut self, indent: usize, span: Span) {
        let top = *self.indents.last().expect("indent stack is never empty");

        if indent > top {
            if indent != top + INDENT_WIDTH {
                self.error(
                    Diagnostic::error(
                        codes::BAD_INDENT_WIDTH,
                        format!(
                            "this line is indented {} spaces deeper than its block; expected {}",
                            indent - top,
                            INDENT_WIDTH
                        ),
                    )
                    .with_primary(span, format!("{indent} spaces here"))
                    .with_help(format!(
                        "indent this line to column {} (4 spaces per level)",
                        top + INDENT_WIDTH
                    )),
                );
            }
            // Push the real width either way, so the following lines of the
            // same block line up with it and don't produce more errors.
            self.indents.push(indent);
            self.misaligned = None;
            self.push(TokenKind::Indent, span);
        } else if indent < top {
            if self.misaligned == Some((indent, self.indents.len())) {
                return;
            }
            // Pop every block this line is clearly outside of. A line that
            // falls between two levels stays in the deeper block.
            while self.indents.len() > 1 && indent <= self.indents[self.indents.len() - 2] {
                self.indents.pop();
                self.push(TokenKind::Dedent, Span::empty(self.pos));
            }
            let level = *self.indents.last().unwrap();
            if indent == level {
                self.misaligned = None;
            } else {
                // Recover by treating the line as part of the block at
                // `level`, and remember the column for the lines after it.
                self.misaligned = Some((indent, self.indents.len()));
                let outer = self.indents[self.indents.len() - 2];
                self.error(
                    Diagnostic::error(
                        codes::MISALIGNED_DEDENT,
                        "this line doesn't line up with any enclosing block",
                    )
                    .with_primary(span, format!("{indent} spaces here"))
                    .with_note(format!(
                        "lines in the same block must line up; the nearby blocks are at \
                         {outer} and {level} spaces"
                    ))
                    .with_help(format!("indent this line by {outer} or {level} spaces")),
                );
            }
        }
    }

    /// Lexes the tokens of one logical line, up to and including its end.
    /// Newlines inside brackets are skipped, so one logical line may span
    /// several physical lines.
    fn lex_line(&mut self) {
        loop {
            self.skip_inline_trivia();
            match self.peek() {
                // `finish_file` adds the final NEWLINE.
                None => return,
                Some('\n') | Some('\r') => {
                    let span = self.consume_newline();
                    if !self.brackets.is_empty() {
                        continue;
                    }
                    let continues = self
                        .tokens
                        .last()
                        .is_some_and(|tok| tok.kind.continues_line());
                    if continues {
                        self.continuation = true;
                    } else {
                        self.push(TokenKind::Newline, span);
                    }
                    return;
                }
                Some(_) => self.lex_token(),
            }
        }
    }

    /// Skips spaces and `//` comments within a line (not the newline).
    fn skip_inline_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(' ') => {
                    self.bump();
                }
                Some('\t') => {
                    let start = self.pos;
                    while self.eat('\t') {}
                    self.tab_error(Span::new(start, self.pos));
                }
                Some('/') if self.peek_nth(1) == Some('/') => self.skip_line_comment(),
                _ => return,
            }
        }
    }

    /// Skips from `//` up to (not including) the end of the line.
    /// `///` doc comments are skipped too, for now.
    fn skip_line_comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                break;
            }
            self.bump();
        }
    }

    fn finish_file(&mut self) {
        let eof = Span::empty(self.end);
        self.close_open_brackets();
        let needs_newline = self.tokens.last().is_some_and(|tok| {
            !matches!(
                tok.kind,
                TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent
            )
        });
        if needs_newline {
            self.push(TokenKind::Newline, eof);
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(TokenKind::Dedent, eof);
        }
        self.push(TokenKind::Eof, eof);
    }

    /// Reports every bracket still open at the end of the input and emits
    /// a zero-width closing token for each, innermost first, so the parser
    /// sees balanced brackets and doesn't report the same problem again.
    fn close_open_brackets(&mut self) {
        let open_brackets = std::mem::take(&mut self.brackets);
        for &(open, _) in open_brackets.iter().rev() {
            let kind = match open {
                '(' => TokenKind::RParen,
                '[' => TokenKind::RBracket,
                _ => TokenKind::RBrace,
            };
            self.push(kind, Span::empty(self.end));
        }
        for (open, span) in open_brackets {
            let close = closing_for(open);
            self.error(
                Diagnostic::error(codes::UNCLOSED_BRACKET, format!("`{open}` is never closed"))
                    .with_primary(span, "opened here")
                    .with_help(format!("add a matching `{close}`")),
            );
        }
    }

    // ------------------------------------------------------------------
    // Tokens
    // ------------------------------------------------------------------

    fn lex_token(&mut self) {
        let start = self.pos;
        let c = self.peek().expect("lex_token called at end of input");

        if is_ident_start(c) {
            return self.lex_ident();
        }
        if c.is_ascii_digit() {
            return self.lex_number();
        }
        match c {
            '"' => return self.lex_string(),
            '\'' => return self.lex_quote(),
            '(' | '[' | '{' => return self.lex_open_bracket(c),
            ')' | ']' | '}' => return self.lex_close_bracket(c),
            _ => {}
        }

        self.bump();
        use TokenKind::*;
        let kind = match c {
            ',' => Comma,
            ':' => Colon,
            '@' => At,
            '~' => Tilde,
            '^' => Caret,
            '&' => Amp,
            '|' => Pipe,
            '.' => {
                if self.peek() == Some('.') {
                    self.bump();
                    if self.eat('.') {
                        DotDotDot
                    } else if self.eat('<') {
                        DotDotLess
                    } else {
                        DotDot
                    }
                } else {
                    Dot
                }
            }
            '?' => {
                if self.eat('?') {
                    QuestionQuestion
                } else {
                    Question
                }
            }
            '!' => {
                if self.eat('=') {
                    BangEq
                } else {
                    Bang
                }
            }
            '=' => {
                if self.eat('=') {
                    EqEq
                } else {
                    Eq
                }
            }
            '<' => {
                if self.eat('=') {
                    LtEq
                } else if self.eat('<') {
                    Shl
                } else {
                    Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    GtEq
                } else if self.eat('>') {
                    Shr
                } else {
                    Gt
                }
            }
            '+' => {
                if self.eat('=') {
                    PlusEq
                } else {
                    Plus
                }
            }
            '-' => {
                if self.eat('=') {
                    MinusEq
                } else if self.eat('>') {
                    Arrow
                } else {
                    Minus
                }
            }
            '*' => {
                if self.eat('=') {
                    StarEq
                } else {
                    Star
                }
            }
            '/' => {
                if self.eat('=') {
                    SlashEq
                } else {
                    Slash
                }
            }
            '%' => {
                if self.eat('=') {
                    PercentEq
                } else {
                    Percent
                }
            }
            _ => {
                let span = Span::new(start, self.pos);
                let mut diag = Diagnostic::error(
                    codes::UNEXPECTED_CHARACTER,
                    format!("unexpected character {c:?}"),
                )
                .with_primary(span, "not valid here");
                if c == ';' {
                    diag = diag
                        .with_help("TDS doesn't use semicolons; end the statement with a new line");
                }
                self.error(diag);
                Error
            }
        };
        self.push(kind, Span::new(start, self.pos));
    }

    fn lex_open_bracket(&mut self, c: char) {
        let start = self.pos;
        self.bump();
        let span = Span::new(start, self.pos);
        self.brackets.push((c, span));
        let kind = match c {
            '(' => TokenKind::LParen,
            '[' => TokenKind::LBracket,
            _ => TokenKind::LBrace,
        };
        self.push(kind, span);
    }

    fn lex_close_bracket(&mut self, c: char) {
        let start = self.pos;
        self.bump();
        let span = Span::new(start, self.pos);
        let open = opening_for(c);

        match self.brackets.iter().rposition(|(b, _)| *b == open) {
            // The common case: it closes the innermost bracket.
            Some(i) if i == self.brackets.len() - 1 => {
                self.brackets.pop();
            }
            // It closes an outer bracket; the inner ones were never closed.
            Some(i) => {
                let (inner, inner_span) = *self.brackets.last().unwrap();
                self.error(
                    Diagnostic::error(
                        codes::MISMATCHED_BRACKET,
                        format!("expected `{}` but found `{c}`", closing_for(inner)),
                    )
                    .with_primary(span, "doesn't match")
                    .with_secondary(inner_span, format!("`{inner}` opened here")),
                );
                self.brackets.truncate(i);
            }
            None => match self.brackets.last().copied() {
                None => self.error(
                    Diagnostic::error(
                        codes::UNMATCHED_CLOSE_BRACKET,
                        format!("unmatched closing `{c}`"),
                    )
                    .with_primary(span, "no matching opening bracket")
                    .with_help("remove it, or add the missing opening bracket"),
                ),
                // Most likely a typo for the innermost closer, so treat it
                // as one; otherwise `f(a]` would also report `(` unclosed.
                Some((inner, inner_span)) => {
                    self.error(
                        Diagnostic::error(
                            codes::MISMATCHED_BRACKET,
                            format!("expected `{}` but found `{c}`", closing_for(inner)),
                        )
                        .with_primary(span, "doesn't match")
                        .with_secondary(inner_span, format!("`{inner}` opened here")),
                    );
                    self.brackets.pop();
                }
            },
        }

        let kind = match c {
            ')' => TokenKind::RParen,
            ']' => TokenKind::RBracket,
            _ => TokenKind::RBrace,
        };
        self.push(kind, span);
    }

    fn lex_ident(&mut self) {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        let word = &self.src[start..self.pos];
        let kind = TokenKind::keyword(word).unwrap_or_else(|| TokenKind::Ident(word.to_string()));
        self.push(kind, Span::new(start, self.pos));
    }

    fn lex_number(&mut self) {
        let start = self.pos;

        // Prefixed integers: 0x / 0b / 0o.
        let radix = match (self.peek(), self.peek_nth(1)) {
            (Some('0'), Some('x')) => 16,
            (Some('0'), Some('b')) => 2,
            (Some('0'), Some('o')) => 8,
            _ => 10,
        };
        if radix != 10 {
            self.bump();
            self.bump();
            let digits_start = self.pos;
            while self.peek().is_some_and(|c| c.is_digit(radix) || c == '_') {
                self.bump();
            }
            let digits = self.src[digits_start..self.pos].replace('_', "");
            if digits.is_empty() || self.has_bad_suffix() {
                return self.invalid_number(start);
            }
            let value = self.int_value(&digits, radix, start);
            return self.push(TokenKind::Int(value), Span::new(start, self.pos));
        }

        self.eat_decimal_digits();
        let mut is_float = false;

        // A fraction needs a digit after the dot, so `0..<10` and `x.0.y`
        // style tokens are left alone.
        if self.peek() == Some('.') && self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.bump();
            self.eat_decimal_digits();
        }

        // Exponent: e10, e+10, e-10.
        if matches!(self.peek(), Some('e') | Some('E')) {
            let digit_at = if matches!(self.peek_nth(1), Some('+') | Some('-')) {
                2
            } else {
                1
            };
            if self.peek_nth(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                for _ in 0..digit_at {
                    self.bump();
                }
                self.eat_decimal_digits();
            }
        }

        if self.has_bad_suffix() {
            return self.invalid_number(start);
        }

        let text = self.src[start..self.pos].replace('_', "");
        let span = Span::new(start, self.pos);
        if is_float {
            let value = text.parse::<f64>().expect("validated float literal");
            self.push(TokenKind::Float(value), span);
        } else {
            let value = self.int_value(&text, 10, start);
            self.push(TokenKind::Int(value), span);
        }
    }

    fn eat_decimal_digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.bump();
        }
    }

    /// A number immediately followed by a letter, like `12abc` or `1e`.
    fn has_bad_suffix(&self) -> bool {
        self.peek().is_some_and(is_ident_continue)
    }

    fn invalid_number(&mut self, start: usize) {
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        let span = Span::new(start, self.pos);
        let text = &self.src[start..self.pos];
        self.error(
            Diagnostic::error(
                codes::INVALID_NUMBER,
                format!("invalid number literal `{text}`"),
            )
            .with_primary(span, "not a valid number")
            .with_note(
                "numbers look like `42`, `1_000`, `0xFF`, `0b1010`, `0o17`, `3.14` or `1.5e-3`",
            ),
        );
        self.push(TokenKind::Error, span);
    }

    fn int_value(&mut self, digits: &str, radix: u32, start: usize) -> u64 {
        match u64::from_str_radix(digits, radix) {
            Ok(v) => v,
            Err(_) => {
                self.error(
                    Diagnostic::error(codes::INTEGER_TOO_LARGE, "integer literal is too large")
                        .with_primary(Span::new(start, self.pos), "doesn't fit in 64 bits")
                        .with_note(format!("the largest integer literal is {}", u64::MAX)),
                );
                0
            }
        }
    }

    // ------------------------------------------------------------------
    // Strings, chars and lifetimes
    // ------------------------------------------------------------------

    fn lex_string(&mut self) {
        let start = self.pos;
        self.bump(); // opening quote
        let mut parts = Vec::new();
        let mut text = String::new();

        loop {
            match self.peek() {
                None | Some('\n') | Some('\r') => {
                    self.error(
                        Diagnostic::error(codes::UNTERMINATED_STRING, "string is never closed")
                            .with_primary(Span::new(start, self.pos), "this string")
                            .with_note("strings can't span lines")
                            .with_help("add a closing `\"`"),
                    );
                    break;
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    if let Some(c) = self.lex_escape() {
                        text.push(c);
                    }
                }
                Some('{') => {
                    if !text.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut text)));
                    }
                    if !self.lex_interpolation(&mut parts) {
                        // Already reported; the rest of the line was eaten.
                        break;
                    }
                }
                Some(c) => {
                    text.push(c);
                    self.bump();
                }
            }
        }

        if !text.is_empty() {
            parts.push(StrPart::Text(text));
        }
        self.push(TokenKind::Str(parts), Span::new(start, self.pos));
    }

    /// Lexes a `\` escape. Returns the character, or `None` after reporting
    /// an error (or when the line ends right after the backslash, which the
    /// caller then reports as an unterminated literal).
    fn lex_escape(&mut self) -> Option<char> {
        let start = self.pos;
        self.bump(); // backslash
        let c = match self.peek() {
            None | Some('\n') | Some('\r') => return None,
            Some(c) => c,
        };
        self.bump();
        let value = match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            '{' => '{',
            '}' => '}',
            'u' => return self.lex_unicode_escape(start),
            _ => {
                self.error(
                    Diagnostic::error(codes::INVALID_ESCAPE, format!("unknown escape `\\{c}`"))
                        .with_primary(Span::new(start, self.pos), "unknown escape")
                        .with_note(
                            "valid escapes are \\n \\t \\r \\0 \\\\ \\\" \\' \\{ \\} and \\u{...}",
                        ),
                );
                return None;
            }
        };
        Some(value)
    }

    /// Lexes the `{XXXX}` part of `\u{XXXX}`.
    fn lex_unicode_escape(&mut self, start: usize) -> Option<char> {
        let mut value = None;
        if self.eat('{') {
            let digits_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.bump();
            }
            let digits = &self.src[digits_start..self.pos];
            if self.eat('}') && (1..=6).contains(&digits.len()) {
                value = u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32);
            }
        }
        if value.is_none() {
            self.error(
                Diagnostic::error(codes::INVALID_ESCAPE, "invalid unicode escape")
                    .with_primary(Span::new(start, self.pos), "not a valid unicode escape")
                    .with_note(
                        "write 1 to 6 hex digits of a Unicode scalar value, like `\\u{1F600}`",
                    ),
            );
        }
        value
    }

    /// Lexes `{expr}` inside a string. The expression is lexed by a nested
    /// lexer over the same source, so its token spans are absolute.
    /// Returns `false` if the interpolation was never closed, in which case
    /// the rest of the line has been consumed.
    fn lex_interpolation(&mut self, parts: &mut Vec<StrPart>) -> bool {
        let open = self.pos;
        let Some(close) = self.find_interpolation_end(open) else {
            self.error(
                Diagnostic::error(
                    codes::UNTERMINATED_INTERPOLATION,
                    "`{` in string is never closed",
                )
                .with_primary(Span::new(open, open + 1), "interpolation starts here")
                .with_help("close it with `}`")
                .with_help("or write `\\{` for a literal brace"),
            );
            while !self.at_newline() && self.peek().is_some() {
                self.bump();
            }
            return false;
        };

        let inner = Span::new(open + 1, close);
        if self.src[inner.start..inner.end].trim().is_empty() {
            self.error(
                Diagnostic::error(codes::EMPTY_INTERPOLATION, "empty `{}` in string")
                    .with_primary(Span::new(open, close + 1), "nothing to interpolate")
                    .with_help("put an expression inside, like `{name}`")
                    .with_help("or write `\\{\\}` for literal braces"),
            );
        } else {
            let mut sub = Lexer::new(self.src, inner.start, inner.end);
            sub.lex_fragment();
            self.errors.append(&mut sub.errors);
            parts.push(StrPart::Interp {
                tokens: sub.tokens,
                span: inner,
            });
        }
        self.pos = close + 1;
        true
    }

    /// Finds the `}` matching the `{` at `open`, skipping over nested braces
    /// and nested string literals. Returns `None` if the line ends first.
    fn find_interpolation_end(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut i = open;
        while let Some(c) = self.char_at(i) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                '"' => {
                    i = self.skip_string_at(i)?;
                    continue;
                }
                '\n' | '\r' => return None,
                _ => {}
            }
            i += c.len_utf8();
        }
        None
    }

    /// Given a `"` at `i`, returns the offset just past the closing quote.
    fn skip_string_at(&self, mut i: usize) -> Option<usize> {
        i += 1;
        while let Some(c) = self.char_at(i) {
            match c {
                '"' => return Some(i + 1),
                '\\' => {
                    i += 1;
                    let escaped = self.char_at(i)?;
                    if escaped == '\n' || escaped == '\r' {
                        return None;
                    }
                    i += escaped.len_utf8();
                    continue;
                }
                '{' => {
                    i = self.find_interpolation_end(i)? + 1;
                    continue;
                }
                '\n' | '\r' => return None,
                _ => {}
            }
            i += c.len_utf8();
        }
        None
    }

    /// Lexes an interpolated expression: no layout tokens, no EOF.
    fn lex_fragment(&mut self) {
        loop {
            self.skip_inline_trivia();
            if self.peek().is_none() {
                break;
            }
            self.lex_token();
        }
        self.close_open_brackets();
    }

    /// Lexes either a char literal `'a'` or a lifetime `'src`.
    fn lex_quote(&mut self) {
        let start = self.pos;

        // `'name` not followed by a closing quote is a lifetime.
        let is_lifetime =
            self.peek_nth(1).is_some_and(is_ident_start) && self.peek_nth(2) != Some('\'');
        if is_lifetime {
            self.bump(); // quote
            let name_start = self.pos;
            while self.peek().is_some_and(is_ident_continue) {
                self.bump();
            }
            let name = self.src[name_start..self.pos].to_string();
            return self.push(TokenKind::Lifetime(name), Span::new(start, self.pos));
        }

        self.bump(); // opening quote
        let value = match self.peek() {
            Some('\\') => self.lex_escape(),
            Some('\'') | Some('\n') | Some('\r') | None => None,
            Some(c) => {
                self.bump();
                Some(c)
            }
        };

        if self.eat('\'') {
            if let Some(c) = value {
                return self.push(TokenKind::Char(c), Span::new(start, self.pos));
            }
            if self.pos - start == 2 {
                self.error(
                    Diagnostic::error(codes::INVALID_CHAR_LITERAL, "empty char literal")
                        .with_primary(Span::new(start, self.pos), "no character here")
                        .with_help("for an empty string write `\"\"`"),
                );
            }
            // Otherwise a bad escape was already reported.
        } else {
            // Unterminated or too long, like `'ab'`: eat to the closing
            // quote on this line, if there is one.
            let line_rest = self.src[self.pos..self.end]
                .find(['\n', '\r'])
                .map_or(self.end, |n| self.pos + n);
            if let Some(n) = self.src[self.pos..line_rest].find('\'') {
                self.pos += n + 1;
            }
            self.error(
                Diagnostic::error(codes::INVALID_CHAR_LITERAL, "invalid char literal")
                    .with_primary(
                        Span::new(start, self.pos),
                        "a char literal holds exactly one character",
                    )
                    .with_help("for text, use a string: `\"...\"`"),
            );
        }
        self.push(TokenKind::Error, Span::new(start, self.pos));
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn closing_for(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        _ => '}',
    }
}

fn opening_for(close: char) -> char {
    match close {
        ')' => '(',
        ']' => '[',
        _ => '{',
    }
}
