//! The registry of TDS error codes.
//!
//! Codes are grouped by compiler stage:
//!
//! | Range         | Stage          |
//! |---------------|----------------|
//! | TDS0001-0099  | lexer          |
//! | TDS0100-0199  | parser         |
//! | TDS0200-0399  | type checker   |
//! | TDS0400-0499  | ownership      |
//! | TDS0500-0599  | runtime/interp |
//!
//! A code is never reused once published, even if the error is removed.
//! `tds explain CODE` will print the long-form text kept next to each code.

/// A tab character appears in source code outside a string or comment.
pub const TAB_CHARACTER: &str = "TDS0001";
/// A line dedents to a column that doesn't match any enclosing block.
pub const MISALIGNED_DEDENT: &str = "TDS0002";
/// A block is indented by something other than exactly 4 more spaces.
pub const BAD_INDENT_WIDTH: &str = "TDS0003";
/// A character that can't start any token.
pub const UNEXPECTED_CHARACTER: &str = "TDS0004";
/// A string literal with no closing quote on the same line.
pub const UNTERMINATED_STRING: &str = "TDS0005";
/// An unknown or malformed `\` escape in a string or char literal.
pub const INVALID_ESCAPE: &str = "TDS0006";
/// A `{` in a string that is never closed by `}`.
pub const UNTERMINATED_INTERPOLATION: &str = "TDS0007";
/// A `)`, `]` or `}` with no opening bracket.
pub const UNMATCHED_CLOSE_BRACKET: &str = "TDS0008";
/// A closing bracket of the wrong kind, e.g. `(a]`.
pub const MISMATCHED_BRACKET: &str = "TDS0009";
/// An opening bracket still open at the end of the file.
pub const UNCLOSED_BRACKET: &str = "TDS0010";
/// An integer literal that doesn't fit in 64 bits.
pub const INTEGER_TOO_LARGE: &str = "TDS0011";
/// A malformed number such as `0x`, `1e` or `12abc`.
pub const INVALID_NUMBER: &str = "TDS0012";
/// `{}` with nothing inside it in a string.
pub const EMPTY_INTERPOLATION: &str = "TDS0013";
/// A char literal that is empty, unterminated or holds more than one char.
pub const INVALID_CHAR_LITERAL: &str = "TDS0014";
