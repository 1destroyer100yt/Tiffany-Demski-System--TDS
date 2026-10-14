# Errors and panics

## How errors are reported

Every problem has a code, a one-line message, the source line with the
problem marked, and usually notes and a suggested fix:

```text
error[TDS0412]: `player` was moved and can't be used here
   ┌─ main.tds:12:11
   │
10 │     save(take player)
   │          ----------- ownership moved into `save` here
11 │
12 │     print(player.health)
   │           ^^^^^^ used after the move
   │
   = help: pass a read borrow instead: `save(player)`
   = help: or move a copy and keep the original: `take player.clone()`
```

`^^^` marks the problem and `---` marks related places. Run
`tds explain CODE` for a longer explanation with examples.

The compiler keeps going after an error, so one run reports every problem
it can find, and it tries to report each mistake exactly once. Warnings
(like `TDS0239`, an unreachable match arm) don't stop a program from
running.

## Panics

A panic is an error found while the program runs: integer overflow, an
index out of bounds, unwrapping `none`, and so on. A panic stops the
program with exit status 1, using the same report format. Output printed
before the panic is kept. Panics are for bugs; Phase 3 adds `Result` for
errors a program is expected to handle.

## Error codes

Codes are grouped by the stage that reports them. A code is never reused
for a different error.

### Lexer

| Code | Meaning |
|---|---|
| TDS0001 | A tab character appears in source code outside a string or comment. |
| TDS0002 | A line dedents to a column that doesn't match any enclosing block. |
| TDS0003 | A block is indented by something other than exactly 4 more spaces. |
| TDS0004 | A character that can't start any token. |
| TDS0005 | A string literal with no closing quote on the same line. |
| TDS0006 | An unknown or malformed `\` escape in a string or char literal. |
| TDS0007 | A `{` in a string that is never closed by `}`. |
| TDS0008 | A `)`, `]` or `}` with no opening bracket. |
| TDS0009 | A closing bracket of the wrong kind, e.g. `(a]`. |
| TDS0010 | An opening bracket still open at the end of the file. |
| TDS0011 | An integer literal that doesn't fit in 64 bits. |
| TDS0012 | A malformed number such as `0x`, `1e` or `12abc`. |
| TDS0013 | `{}` with nothing inside it in a string. |
| TDS0014 | A char literal that is empty, unterminated or holds more than one char. |

### Parser

| Code | Meaning |
|---|---|
| TDS0100 | An expression was expected but something else was found. |
| TDS0101 | A specific token was expected but something else was found. |
| TDS0102 | A `:` that opens a block isn't followed by an indented block. |
| TDS0103 | A line is indented but doesn't start a block. |
| TDS0104 | A statement at the top level of a file, outside any function. |
| TDS0105 | The left side of `=` can't be assigned to. |
| TDS0106 | A feature from a later phase that the prototype doesn't support yet. |
| TDS0107 | Comparisons chained without `and`, like `a < b < c`. |
| TDS0108 | A type was expected but something else was found. |
| TDS0109 | An item such as `fn` or `struct` was expected. |
| TDS0110 | A positional argument after a named one. |
| TDS0111 | A parameter without a type. |
| TDS0112 | `else` with no `if` before it. |
| TDS0113 | `=` used where `==` was probably meant. |
| TDS0114 | `let` or `var` without an initial value. |
| TDS0115 | `mut` used outside a parameter type or a call argument. |
| TDS0116 | A pattern was expected in a `match` arm. |

### Type checker

| Code | Meaning |
|---|---|
| TDS0200 | A name that isn't a variable, function, type or variant in scope. |
| TDS0201 | A type name that doesn't exist. |
| TDS0202 | Two declarations with the same name. |
| TDS0203 | A value of one type where another was expected. |
| TDS0204 | Mixing `Int` and `Float` without an explicit conversion. |
| TDS0205 | A call with the wrong number of arguments. |
| TDS0206 | A field that the struct or variant doesn't have. |
| TDS0207 | Struct construction that leaves out a field without a default. |
| TDS0208 | The same field given twice. |
| TDS0209 | A struct constructed with positional instead of named fields. |
| TDS0210 | A method that the type doesn't have. |
| TDS0211 | Calling something that isn't a function. |
| TDS0212 | An `if` / `while` condition that isn't `Bool`. |
| TDS0213 | A binary operator used on types it doesn't support. |
| TDS0214 | A unary operator used on a type it doesn't support. |
| TDS0215 | Indexing a value that isn't a list, or with a non-`Int` index. |
| TDS0216 | `for` over a value that can't be iterated. |
| TDS0217 | A `match` that doesn't cover every case. |
| TDS0218 | A pattern that can't match the type being matched. |
| TDS0219 | A variant pattern with the wrong number of fields. |
| TDS0220 | An `if` without `else` used as a value. |
| TDS0221 | An empty list or `none` whose type can't be worked out. |
| TDS0222 | A value is needed, but the expression or block doesn't produce one (for example, a function that can end without returning). |
| TDS0223 | `break` or `continue` outside a loop. |
| TDS0224 | `!` or `??` on a value that isn't optional. |
| TDS0225 | An optional of an optional, like `Int??`. |
| TDS0226 | Ordering (`<`) on a type that has no order. |
| TDS0227 | The program has no `fn main()`. |
| TDS0228 | `fn main` with parameters or a return value. |
| TDS0229 | Two parameters with the same name. |
| TDS0230 | A conversion like `Int(x)` from a type that can't be converted. |
| TDS0231 | A named argument passed to a function. |
| TDS0232 | A struct that contains itself directly and so has infinite size. |
| TDS0233 | A type used with the wrong number of type arguments, like `List`. |
| TDS0234 | A type that exists in TDS but not in the Phase 1 prototype. |
| TDS0235 | An integer literal that doesn't fit in `Int`. |
| TDS0236 | A variant with fields used without arguments, or vice versa. |
| TDS0237 | A function or type name used where a value is expected. |
| TDS0238 | `return` with a value in a function that returns `()`, or without one in a function that returns a value. |
| TDS0239 | A match arm that can never run because an earlier arm covers it. |

### Modules

| Code | Meaning |
|---|---|
| TDS0240 | `use` names a module whose file doesn't exist. |
| TDS0241 | An item from another module that isn't declared `pub`. |
| TDS0242 | `use` names an item that the module doesn't declare. |
| TDS0243 | Two imports, or an import and a declaration, with the same name. |
| TDS0244 | A module name used as a value. |

### Ownership and parameter modes

| Code | Meaning |
|---|---|
| TDS0400 | Assigning to a `let` variable. |
| TDS0401 | Changing a parameter that was only lent for reading. |
| TDS0402 | Calling a changing method like `push` on something that can't change. |
| TDS0403 | A `mut` parameter whose argument isn't marked `mut`. |
| TDS0404 | A `take` parameter whose argument isn't marked `take`. |
| TDS0405 | An argument marked `mut` or `take` that the parameter doesn't ask for. |
| TDS0406 | A `mut` argument that isn't a variable, field or element. |
| TDS0407 | A `mut` argument that refers to a `let` variable or read parameter. |
| TDS0408 | `take` of a field or list element. |
| TDS0409 | `take` of a parameter that was only borrowed. |
| TDS0410 | A variable lent with `mut` and also used by another argument. |
| TDS0411 | A variable moved inside a loop, so the next iteration can't use it. |
| TDS0412 | A variable used after its value was moved away with `take`. |

### Runtime panics

| Code | Meaning |
|---|---|
| TDS0500 | Integer arithmetic overflowed. |
| TDS0501 | Integer division or remainder by zero. |
| TDS0502 | A list index outside `0..<len`. |
| TDS0503 | `x!` where `x` is `none`. |
| TDS0504 | Too many nested calls. |
| TDS0505 | A numeric conversion that can't be represented, like `Int(1e300)`. |
| TDS0506 | The program called `panic(...)`. |
| TDS0599 | The interpreter hit a state the checker should have ruled out. |
