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

// ---- parser ----

/// An expression was expected but something else was found.
pub const EXPECTED_EXPRESSION: &str = "TDS0100";
/// A specific token was expected but something else was found.
pub const UNEXPECTED_TOKEN: &str = "TDS0101";
/// A `:` that opens a block isn't followed by an indented block.
pub const EXPECTED_BLOCK: &str = "TDS0102";
/// A line is indented but doesn't start a block.
pub const UNEXPECTED_INDENT: &str = "TDS0103";
/// A statement at the top level of a file, outside any function.
pub const STATEMENT_AT_TOP_LEVEL: &str = "TDS0104";
/// The left side of `=` can't be assigned to.
pub const INVALID_ASSIGNMENT_TARGET: &str = "TDS0105";
/// A feature from a later phase that the prototype doesn't support yet.
pub const NOT_SUPPORTED_YET: &str = "TDS0106";
/// Comparisons chained without `and`, like `a < b < c`.
pub const CHAINED_COMPARISON: &str = "TDS0107";
/// A type was expected but something else was found.
pub const EXPECTED_TYPE: &str = "TDS0108";
/// An item such as `fn` or `struct` was expected.
pub const EXPECTED_ITEM: &str = "TDS0109";
/// A positional argument after a named one.
pub const POSITIONAL_AFTER_NAMED: &str = "TDS0110";
/// A parameter without a type.
pub const MISSING_PARAM_TYPE: &str = "TDS0111";
/// `else` with no `if` before it.
pub const ELSE_WITHOUT_IF: &str = "TDS0112";
/// `=` used where `==` was probably meant.
pub const ASSIGN_IN_CONDITION: &str = "TDS0113";
/// `let` or `var` without an initial value.
pub const MISSING_INITIALIZER: &str = "TDS0114";
/// `mut` used outside a parameter type or a call argument.
pub const MISPLACED_MODE: &str = "TDS0115";
/// A pattern was expected in a `match` arm.
pub const EXPECTED_PATTERN: &str = "TDS0116";
