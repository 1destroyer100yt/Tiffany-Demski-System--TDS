# Design decisions

A log of the decisions made during implementation where the brief left a
choice open. Newest at the bottom. Each entry says what was chosen, why, and
whether a later phase may revisit it.

## Phase 1, step 1: lexer

**D1. The indentation step must be exactly 4 spaces.**
The brief locks "4 spaces". A block that is indented by any other amount
(2, 8, ...) gives `TDS0003`. The lexer still opens the block at the actual
column, so the rest of that block lines up and doesn't produce more errors.

**D2. Tabs are an error everywhere except inside strings and comments.**
They're also allowed on blank lines, where they can't affect layout. For
recovery, a tab in indentation counts as 4 spaces. A run of tabs gives one
error.

**D3. Recovery for misaligned dedents uses a "phantom" level.**
When a line dedents to a column that matches no open block (`TDS0002`), it is
treated as belonging to the nearest outer block, and the odd column is
remembered as a phantom level that emits no INDENT/DEDENT. Later lines at the
same column don't each get an error.

**D4. Line continuation after a trailing operator.**
A line ending in `,` or one of these continues onto the next line:
`+ - * / % & | ^ << >> == != < <= > >= = += -= *= /= %= . ..< ... -> ?? and or`.
A trailing `:` doesn't continue (it opens a block). The continuation line's
indentation is ignored.

**D5. Newlines inside brackets are ignored entirely.**
This is what the brief asks for. It means a block-bodied lambda inside a call,
like `thread.scope(fn(s): ...)` in the concurrency example, currently lexes
as one logical line. Phase 4 (closures) must revisit this, probably by turning
layout tracking back on after `fn(...):` + newline inside brackets.

**D6. Interpolation syntax: `{expr}`, with `\{` and `\}` as escapes.**
`{` always starts an interpolation. A lone `}` in a string is literal text.
`{}` with nothing inside is an error (`TDS0013`). The embedded expression is
lexed by a nested lexer over the same source, so token spans stay absolute and
errors point at the right place. Nested strings inside an interpolation are
allowed (`"{greet("Bob")}"`). Strings can't span lines.

**D7. String escapes:** `\n \t \r \0 \\ \" \' \{ \} \u{1-6 hex digits}`.

**D8. Integer literals are stored as `u64`.**
A leading `-` is a separate token; the type checker decides the final type and
checks the range. Literals above `u64::MAX` are an error (`TDS0011`).
Underscores are allowed after the first digit; `0x`, `0b`, `0o` prefixes are
supported. A number followed straight away by a letter (`12abc`, `1e`) is an
error (`TDS0012`).

**D9. `1..<10` is a range, not a float.**
A `.` after digits only starts a fraction if a digit follows it.

**D10. `'` starts a char literal or a lifetime.**
`'a'` (quote, char, quote) is a char. `'src` (quote, identifier, no closing
quote) is a lifetime. This is the same rule Rust uses. Lifetimes are lexed now
so Phase 3 doesn't need lexer changes.

**D11. `read` is not a keyword.**
Read is the default parameter mode and is never written, so reserving the
word would only take a common identifier away from users. `mut`, `take` and
`ref` are keywords.

**D12. `///` doc comments are skipped for now.**
`tds doc` (Phase 4) will need them as tokens or trivia; that's the time to add
them. There are no block comments.

**D13. Identifiers may use Unicode letters.**
An identifier starts with `_` or any alphabetic character and continues with
`_` or alphanumerics. `_` on its own is a keyword token (wildcard).

**D14. Diagnostics are data, not output.**
Every stage returns `Diagnostic` values (code, message, labeled spans, notes,
help). Rendering happens once, in the CLI, which keeps stages pure so they
can become incremental queries later. Error codes are grouped by stage in
`tds_diagnostics::codes`: `TDS00xx` lexer, `TDS01xx` parser, `TDS02xx-03xx`
types, `TDS04xx` ownership, `TDS05xx` runtime.
