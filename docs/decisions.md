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

**D3. A misaligned line joins the deeper of the two blocks around it.**
When a line dedents to a column that matches no open block (`TDS0002`), it
is treated as part of the deeper block (the one it was most likely meant to
continue). Following lines at the same column join the same block without
another error. Joining the outer block instead would often push the line to
the top level and cause a second, confusing error from the parser.

**D3b. Unclosed brackets are closed at end of file.**
The lexer reports each unclosed bracket (`TDS0010`) and then emits a
matching zero-width closing token, so the parser sees balanced brackets and
doesn't report the same mistake again.

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

## Phase 1, step 2: parser

**D15. A file holds only declarations; code runs from `fn main()`.**
Statements at the top level of a file are an error (`TDS0104`). The REPL is
the exception: there, declarations and statements can be mixed.

**D16. Operator precedence** (loosest first): `or`, `and`, `not`,
comparisons, `??`, ranges (`..<`, `...`), `+ -`, `* / %`, prefix `-` and
`take`, then postfix (calls, indexing, `.field`, `.method()`, `!`).
- `not` binds looser than comparisons, as in Python: `not a == b` is
  `not (a == b)`.
- Comparisons don't chain: `a < b < c` is an error (`TDS0107`) that suggests
  `a < b and b < c`.
- `??` binds tighter than comparisons, so `x > best ?? 0` compares with the
  default. It's right-associative.
- Ranges don't associate: `a..<b..<c` is rejected by the type checker.

**D17. Bodies may be written inline after `:`.**
`if`, `else`, `while`, `for`, `fn` and match arms accept either an indented
block or a single simple statement on the same line (`if done: return`).
For an inline `if`, the `else` may go on the next line.

**D18. A block's value is its last expression.**
When a block is used as a value (function body, `if` branch, match arm), its
value is the last statement if that's an expression. This is what makes the
brief's `fn area(s: Shape) -> Float: match s: ...` work without `return`.

**D19. `take` also works outside call arguments.**
`let b = take a` moves `a` into `b`. `mut` is only valid as a parameter mode
or a call-argument marker (`TDS0115`).

**D20. Call arguments: positional first, then named.**
A positional argument after a named one is an error (`TDS0110`). Named
arguments are used for struct construction; the checker decides where
they're allowed.

**D21. Every variable is initialized when declared.**
`var x: Int` without `= value` is an error (`TDS0114`). This avoids the need
for definite-assignment analysis and matches "no null in safe code".

**D22. Later-phase features get a clear "not supported yet" error.**
`impl`, `interface`, `use`, generics, closures, `?`, tuples, attributes,
references, `unsafe` and friends are recognized and rejected with
`TDS0106`, which names the phase that adds them, instead of a confusing
syntax error.

**D23. The parser doesn't repeat lexer errors.**
It never reports an error at a lexer `Error` token, and the lexer closes
unclosed brackets (D3b), so one typo gives one error.
