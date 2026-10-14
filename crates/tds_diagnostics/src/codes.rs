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

// ---- type checker ----

/// A name that isn't a variable, function, type or variant in scope.
pub const UNKNOWN_NAME: &str = "TDS0200";
/// A type name that doesn't exist.
pub const UNKNOWN_TYPE: &str = "TDS0201";
/// Two declarations with the same name.
pub const DUPLICATE_DEFINITION: &str = "TDS0202";
/// A value of one type where another was expected.
pub const MISMATCHED_TYPES: &str = "TDS0203";
/// Mixing `Int` and `Float` without an explicit conversion.
pub const NO_IMPLICIT_CONVERSION: &str = "TDS0204";
/// A call with the wrong number of arguments.
pub const WRONG_ARGUMENT_COUNT: &str = "TDS0205";
/// A field that the struct or variant doesn't have.
pub const UNKNOWN_FIELD: &str = "TDS0206";
/// Struct construction that leaves out a field without a default.
pub const MISSING_FIELD: &str = "TDS0207";
/// The same field given twice.
pub const DUPLICATE_FIELD: &str = "TDS0208";
/// A struct constructed with positional instead of named fields.
pub const UNNAMED_STRUCT_FIELD: &str = "TDS0209";
/// A method that the type doesn't have.
pub const UNKNOWN_METHOD: &str = "TDS0210";
/// Calling something that isn't a function.
pub const NOT_CALLABLE: &str = "TDS0211";
/// An `if` / `while` condition that isn't `Bool`.
pub const CONDITION_NOT_BOOL: &str = "TDS0212";
/// A binary operator used on types it doesn't support.
pub const BAD_BINARY_OPERANDS: &str = "TDS0213";
/// A unary operator used on a type it doesn't support.
pub const BAD_UNARY_OPERAND: &str = "TDS0214";
/// Indexing a value that isn't a list, or with a non-`Int` index.
pub const BAD_INDEX: &str = "TDS0215";
/// `for` over a value that can't be iterated.
pub const NOT_ITERABLE: &str = "TDS0216";
/// A `match` that doesn't cover every case.
pub const NON_EXHAUSTIVE_MATCH: &str = "TDS0217";
/// A pattern that can't match the type being matched.
pub const PATTERN_TYPE_MISMATCH: &str = "TDS0218";
/// A variant pattern with the wrong number of fields.
pub const WRONG_PATTERN_ARITY: &str = "TDS0219";
/// An `if` without `else` used as a value.
pub const IF_WITHOUT_ELSE: &str = "TDS0220";
/// An empty list or `none` whose type can't be worked out.
pub const CANNOT_INFER: &str = "TDS0221";
/// A value is needed, but the expression or block doesn't produce one
/// (for example, a function that can end without returning).
pub const MISSING_RETURN: &str = "TDS0222";
/// `break` or `continue` outside a loop.
pub const BREAK_OUTSIDE_LOOP: &str = "TDS0223";
/// `!` or `??` on a value that isn't optional.
pub const NOT_OPTIONAL: &str = "TDS0224";
/// An optional of an optional, like `Int??`.
pub const NESTED_OPTIONAL: &str = "TDS0225";
/// Ordering (`<`) on a type that has no order.
pub const NOT_ORDERED: &str = "TDS0226";
/// The program has no `fn main()`.
pub const NO_MAIN: &str = "TDS0227";
/// `fn main` with parameters or a return value.
pub const BAD_MAIN_SIGNATURE: &str = "TDS0228";
/// Two parameters with the same name.
pub const DUPLICATE_PARAMETER: &str = "TDS0229";
/// A conversion like `Int(x)` from a type that can't be converted.
pub const INVALID_CONVERSION: &str = "TDS0230";
/// A named argument passed to a function.
pub const NAMED_ARGUMENT_TO_FUNCTION: &str = "TDS0231";
/// A struct that contains itself directly and so has infinite size.
pub const RECURSIVE_STRUCT: &str = "TDS0232";
/// A type used with the wrong number of type arguments, like `List`.
pub const WRONG_TYPE_ARGUMENTS: &str = "TDS0233";
/// A type that exists in TDS but not in the Phase 1 prototype.
pub const UNSUPPORTED_TYPE: &str = "TDS0234";
/// An integer literal that doesn't fit in `Int`.
pub const LITERAL_OUT_OF_RANGE: &str = "TDS0235";
/// A variant with fields used without arguments, or vice versa.
pub const VARIANT_ARGUMENTS: &str = "TDS0236";
/// A function or type name used where a value is expected.
pub const NOT_A_VALUE: &str = "TDS0237";
/// `return` with a value in a function that returns `()`, or without one
/// in a function that returns a value.
pub const BAD_RETURN: &str = "TDS0238";
/// A match arm that can never run because an earlier arm covers it.
pub const UNREACHABLE_PATTERN: &str = "TDS0239";

// ---- ownership and parameter modes ----

/// Assigning to a `let` variable.
pub const ASSIGN_TO_IMMUTABLE: &str = "TDS0400";
/// Changing a parameter that was only lent for reading.
pub const ASSIGN_TO_READ_PARAM: &str = "TDS0401";
/// Calling a changing method like `push` on something that can't change.
pub const MUTATING_IMMUTABLE: &str = "TDS0402";
/// A `mut` parameter whose argument isn't marked `mut`.
pub const MISSING_MUT_MARKER: &str = "TDS0403";
/// A `take` parameter whose argument isn't marked `take`.
pub const MISSING_TAKE_MARKER: &str = "TDS0404";
/// An argument marked `mut` or `take` that the parameter doesn't ask for.
pub const WRONG_MODE_MARKER: &str = "TDS0405";
/// A `mut` argument that isn't a variable, field or element.
pub const MUT_ARG_NOT_PLACE: &str = "TDS0406";
/// A `mut` argument that refers to a `let` variable or read parameter.
pub const MUT_ARG_IMMUTABLE: &str = "TDS0407";
/// `take` of a field or list element.
pub const MOVE_OUT_OF_FIELD: &str = "TDS0408";
/// `take` of a parameter that was only borrowed.
pub const MOVE_OUT_OF_BORROW: &str = "TDS0409";
/// A variable lent with `mut` and also used by another argument.
pub const CONFLICTING_BORROW: &str = "TDS0410";
/// A variable moved inside a loop, so the next iteration can't use it.
pub const MOVED_IN_LOOP: &str = "TDS0411";
/// A variable used after its value was moved away with `take`.
pub const USE_AFTER_MOVE: &str = "TDS0412";

// ---- runtime panics ----

/// Integer arithmetic overflowed.
pub const INTEGER_OVERFLOW: &str = "TDS0500";
/// Integer division or remainder by zero.
pub const DIVISION_BY_ZERO: &str = "TDS0501";
/// A list index outside `0..<len`.
pub const INDEX_OUT_OF_BOUNDS: &str = "TDS0502";
/// `x!` where `x` is `none`.
pub const UNWRAP_NONE: &str = "TDS0503";
/// Too many nested calls.
pub const STACK_OVERFLOW: &str = "TDS0504";
/// A numeric conversion that can't be represented, like `Int(1e300)`.
pub const CONVERSION_FAILED: &str = "TDS0505";
/// The program called `panic(...)`.
pub const EXPLICIT_PANIC: &str = "TDS0506";
/// The interpreter hit a state the checker should have ruled out.
pub const INTERNAL_ERROR: &str = "TDS0599";
