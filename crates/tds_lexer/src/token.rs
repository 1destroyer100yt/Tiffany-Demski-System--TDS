//! Token definitions.

use std::fmt;

use tds_diagnostics::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// One piece of a string literal: either plain text or an embedded `{expr}`.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    /// Literal text with escapes already processed.
    Text(String),
    /// The tokens of an interpolated expression. The parser turns these into
    /// an expression. Spans inside are absolute positions in the file.
    /// `span` covers the text between the braces.
    Interp { tokens: Vec<Token>, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ---- literals and names ----
    /// Integer literal. Stored unsigned; a leading `-` is a separate token
    /// and the type checker decides which integer type it becomes.
    Int(u64),
    Float(f64),
    /// String literal, split into text and interpolation parts.
    /// `""` has no parts.
    Str(Vec<StrPart>),
    Char(char),
    Ident(String),
    /// A lifetime name such as `'src` (Level 2; lexed now, used in Phase 3).
    Lifetime(String),

    // ---- keywords ----
    Let,
    Var,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Struct,
    Enum,
    Match,
    Impl,
    Interface,
    True,
    False,
    And,
    Or,
    Not,
    Mut,
    Take,
    Ref,
    Pub,
    Use,
    None,
    /// `self`
    SelfValue,
    Unsafe,
    Const,
    Static,
    Extern,
    /// `_` on its own (wildcard pattern / ignored binding).
    Underscore,

    // ---- brackets ----
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // ---- punctuation ----
    Comma,
    Colon,
    Dot,
    /// `..`
    DotDot,
    /// `..<` exclusive range
    DotDotLess,
    /// `...` inclusive range
    DotDotDot,
    /// `->`
    Arrow,
    At,
    Question,
    /// `??`
    QuestionQuestion,
    Bang,

    // ---- operators ----
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    Eq,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,

    // ---- layout ----
    /// End of a logical line.
    Newline,
    /// Start of a deeper indented block.
    Indent,
    /// End of an indented block.
    Dedent,
    Eof,

    /// Placeholder for text that couldn't be lexed. An error has already
    /// been reported; the parser should skip it quietly.
    Error,
}

impl TokenKind {
    /// Looks up a keyword. Returns `None` for ordinary identifiers.
    pub fn keyword(word: &str) -> Option<TokenKind> {
        use TokenKind::*;
        Some(match word {
            "let" => Let,
            "var" => Var,
            "fn" => Fn,
            "return" => Return,
            "if" => If,
            "else" => Else,
            "while" => While,
            "for" => For,
            "in" => In,
            "break" => Break,
            "continue" => Continue,
            "struct" => Struct,
            "enum" => Enum,
            "match" => Match,
            "impl" => Impl,
            "interface" => Interface,
            "true" => True,
            "false" => False,
            "and" => And,
            "or" => Or,
            "not" => Not,
            "mut" => Mut,
            "take" => Take,
            "ref" => Ref,
            "pub" => Pub,
            "use" => Use,
            "none" => None,
            "self" => SelfValue,
            "unsafe" => Unsafe,
            "const" => Const,
            "static" => Static,
            "extern" => Extern,
            "_" => Underscore,
            _ => return Option::None,
        })
    }

    /// True for tokens that, when they end a line, mean the expression
    /// continues on the next line: binary operators, `=`, `->` and `,`.
    pub fn continues_line(&self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            Comma
                | Dot
                | DotDotLess
                | DotDotDot
                | Arrow
                | QuestionQuestion
                | Plus
                | Minus
                | Star
                | Slash
                | Percent
                | Amp
                | Pipe
                | Caret
                | Shl
                | Shr
                | Eq
                | EqEq
                | BangEq
                | Lt
                | LtEq
                | Gt
                | GtEq
                | PlusEq
                | MinusEq
                | StarEq
                | SlashEq
                | PercentEq
                | And
                | Or
        )
    }

    /// The fixed source text of a keyword or symbol token.
    fn symbol(&self) -> Option<&'static str> {
        use TokenKind::*;
        Some(match self {
            Let => "let",
            Var => "var",
            Fn => "fn",
            Return => "return",
            If => "if",
            Else => "else",
            While => "while",
            For => "for",
            In => "in",
            Break => "break",
            Continue => "continue",
            Struct => "struct",
            Enum => "enum",
            Match => "match",
            Impl => "impl",
            Interface => "interface",
            True => "true",
            False => "false",
            And => "and",
            Or => "or",
            Not => "not",
            Mut => "mut",
            Take => "take",
            Ref => "ref",
            Pub => "pub",
            Use => "use",
            None => "none",
            SelfValue => "self",
            Unsafe => "unsafe",
            Const => "const",
            Static => "static",
            Extern => "extern",
            Underscore => "_",
            LParen => "(",
            RParen => ")",
            LBracket => "[",
            RBracket => "]",
            LBrace => "{",
            RBrace => "}",
            Comma => ",",
            Colon => ":",
            Dot => ".",
            DotDot => "..",
            DotDotLess => "..<",
            DotDotDot => "...",
            Arrow => "->",
            At => "@",
            Question => "?",
            QuestionQuestion => "??",
            Bang => "!",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Tilde => "~",
            Shl => "<<",
            Shr => ">>",
            Eq => "=",
            EqEq => "==",
            BangEq => "!=",
            Lt => "<",
            LtEq => "<=",
            Gt => ">",
            GtEq => ">=",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            Newline => "NEWLINE",
            Indent => "INDENT",
            Dedent => "DEDENT",
            Eof => "EOF",
            Error => "ERROR",
            Int(_) | Float(_) | Str(_) | Char(_) | Ident(_) | Lifetime(_) => return Option::None,
        })
    }
}

/// A compact, unambiguous rendering used by tests and `tds` debug output,
/// e.g. `ident(x)`, `int(5)`, `str("hi " {ident(name)})`, `INDENT`.
impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Int(n) => write!(f, "int({n})"),
            TokenKind::Float(x) => write!(f, "float({x:?})"),
            TokenKind::Char(c) => write!(f, "char({c:?})"),
            TokenKind::Ident(name) => write!(f, "ident({name})"),
            TokenKind::Lifetime(name) => write!(f, "lifetime('{name})"),
            TokenKind::Str(parts) => {
                write!(f, "str(")?;
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    match part {
                        StrPart::Text(text) => write!(f, "{text:?}")?,
                        StrPart::Interp { tokens, .. } => {
                            write!(f, "{{")?;
                            for (j, tok) in tokens.iter().enumerate() {
                                if j > 0 {
                                    write!(f, " ")?;
                                }
                                write!(f, "{}", tok.kind)?;
                            }
                            write!(f, "}}")?;
                        }
                    }
                }
                write!(f, ")")
            }
            other => f.write_str(other.symbol().expect("all other kinds have a symbol")),
        }
    }
}
