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

## Phase 1, steps 3-5: interpreter, data types, parameter modes

**D24. The interpreter walks the AST (the Phase 2 seam).**
Phase 1 executes the checked AST directly. In Phase 2, name resolution and
lowering produce HIR and TIR, and the interpreter is rewritten over TIR. The
interpreter's public API (`load_items`, `run_main`, `run_stmt`) is meant to
survive that change, and so is the driver in `tds_cli`.

**D25. Value semantics via copy-on-write.**
`let b = a` gives `b` its own value. Lists, strings, structs and variants
live behind `Rc`, so a copy is cheap, and a write copies the data only if
it's still shared. Phase 3's ownership checker will decide where an implicit
copy should become an error that asks for `.clone()` or `take`; Phase 1 is
deliberately permissive here.

**D26. How parameter modes run.**
- read: the callee gets a copy (cheap, see D25). It can't change it.
- take: the value moves out of the caller's variable, which becomes unusable
  until it's assigned again. Copy types (numbers, `Bool`, `Char`) are copied
  instead of moved, so they stay usable.
- mut: the value moves out of the caller's place for the call and is written
  back when the call returns. Because `mut` borrows are exclusive, this can't
  be told apart from passing a reference.
- A `take` parameter is an owned local, so the callee may change it.

**D27. Two-phase `mut` arguments.**
Read and take arguments are evaluated first, and `mut` places are taken
just before the call. So `quicksort(mut xs, 0, len(xs) - 1)` works: the read
of `xs` finishes before the exclusive borrow starts (Rust calls this a
two-phase borrow). Passing the same variable itself as another argument,
like `f(mut xs, xs)`, is still a conflict (`TDS0410`).

**D28. Optionals are represented without a wrapper.**
At run time, a present `T?` is just the value, and `none` is a separate
value. That's why optionals of optionals (`Int??`) are not allowed in
Phase 1 (`TDS0225`). `List[T?].pop()` would need one, so `pop` on a list of
optionals is also rejected.

**D29. Integer overflow panics; integer division by zero panics.**
Phase 1 behaves like a Development build: `+ - * / %`, negation, `abs` and
`pow` on `Int` panic on overflow (`TDS0500`). Integer `/` and `%` by zero
panic (`TDS0501`). `Float` follows IEEE 754 (`1.0 / 0.0` is infinity).

**D30. Conversions.**
`Int(Float)` truncates toward zero and panics if the value doesn't fit
(`TDS0505`). `Int(Char)` gives the code point. `Float(Int)` rounds to the
nearest `Float`. `String(x)` formats any value like `print` does.

**D31. `len` of a string counts characters (Unicode scalar values).**
This is the friendlier choice while strings can't be indexed. When string
slicing arrives (Phase 4), a separate byte length may be added.

**D32. `==` compares structurally; `<` only on numbers, `Char` and `String`.**
Phase 1 lets `==` compare any two values of the same type. Phase 3 will
require the `Equal` interface, derivable with `@derive(Equal)`.

**D33. Display format.** `print` and interpolation show values like this:
`Float`s always have a decimal point (`10.0`); strings and chars are quoted
only inside lists and structs; structs print as `Player(name: "Alex",
health: 75)`; unit variants as `Point`; `none` as `none`.

**D34. Built-ins.** Functions: `print`, `len`, `Int`, `Float`, `String`,
`read_file` (returns `String?`), `args`, `panic`. Methods on `List`, `String`,
`Int`, `Float` and `Char` are listed in `docs/spec/`. `read_file` returns
`none` on failure because `Result` isn't in the Phase 1 subset.

**D35. Call depth is limited to 10,000.**
Deeper recursion panics with `TDS0504` instead of crashing the process. The
interpreter runs on a thread with a 1 GiB stack (reserved, not used up
front) so that limit fits.

## Phase 1, step 6: static checker

**D36. Bidirectional type checking.**
Types are inferred inside function bodies. Where the context knows the type
it wants (an annotation, a parameter, a return type, the other side of
`==`), it passes it down. That's how `[]`, `none` and `if`/`match` values
get their types. With no context, `[]` and `none` are errors (`TDS0221`)
that suggest an annotation. Phase 3 (generics) will need real unification;
this design leaves room for it.

**D37. The only implicit conversion is `T` → `T?`.**
An `Int` fits where an `Int?` is expected. Numbers never convert implicitly
(`TDS0204`). Element types of lists must match exactly. A `mut` argument's
type must match the parameter exactly, because the callee may write any
value of the parameter's type into it.

**D38. Copy types in Phase 1:** `Int`, `Float`, `Bool`, `Char`, `()`, ranges
and optionals of these. Structs and enums are never Copy yet. The brief's
"small plain structs" will become Copy through `@derive(Copy)` in Phase 3.

**D39. The move analysis is flow-sensitive.**
Each path through a function tracks which variables have been moved. Where
paths meet (after `if`, `match`, `and`/`or`/`??`, loops), a variable moved
on any path counts as moved. Paths that end in `return`, `break`,
`continue` or `panic` don't count. Assigning a new value to a `var` makes
it usable again. A use after a move is reported once (`TDS0412`).

**D40. Moving inside a loop.**
If a variable declared outside a loop is still moved at the end of an
iteration (or at a `continue`), the next iteration would use it, so it's
an error at the move (`TDS0411`). Moving and then `break`ing, or assigning
a new value before the iteration ends, is fine.

**D41. Borrow conflicts are checked per whole variable.**
In a single call, a variable lent with `mut` can't appear as another
argument (`TDS0410`), even if the two arguments are different fields.
Reading it inside another argument's expression (`len(xs)`) is fine (D27).
Phase 3's real borrow checker can allow disjoint fields.

**D42. Capitalized names in patterns are always variants.**
A capitalized name in a pattern must be a variant of the matched enum. A
lowercase name binds the value. So a misspelled variant (`Circel`) is an
error (`TDS0218`), not a silent catch-all.

**D43. Variant names are global and unique.**
`Circle(1.0)` works without the enum name, as in the brief. That needs
variant names to be unique across all enums (`TDS0202`). `Shape.Circle(1.0)`
and `Shape.Point` also work. Phase 2's modules will scope this properly.

**D44. Other checker rules.**
- Shadowing is allowed: `let x = 1` then `let x = "one"` in the same scope.
- An unreachable match arm is a warning (`TDS0239`), not an error.
- A user function with the same name as a built-in function replaces it.
  Built-in *type* names can't be reused (`TDS0202`).
- In a function returning `()`, a trailing expression's value is discarded.
  `return value` in such a function is an error (`TDS0238`).
- A `()` where a value is needed gets its own message (`TDS0222`, "this has
  no value") instead of a type mismatch.
- `.clone()` exists on every type. In Phase 1 it's the same as a plain copy
  (D25); it's there so code is written the way Phase 3 will require.
- A struct that contains itself directly or through optionals (not through
  `List`) is infinitely large and rejected (`TDS0232`).

**D45. One mistake, one diagnostic.**
The UI test suite requires every test program to produce exactly one
diagnostic. Several recovery rules exist only to keep it that way: an
unterminated string closes the brackets opened earlier on its line; a
stray closing bracket is dropped; a wrong closing bracket is treated as the
right one; an expression whose type is unknown gets the `Error` type, which
fits everywhere; a broken pattern turns off the exhaustiveness check.
