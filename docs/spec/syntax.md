# Syntax

The grammar below uses `NEWLINE`, `INDENT` and `DEDENT` for the layout
tokens described in [Lexical structure](lexical.md). `x?` means optional,
`x*` means zero or more, and `x,*` means a comma-separated list that may
end with a comma.

## Files

```
file        = item*
item        = "pub"? ( fn_decl | struct_decl | enum_decl )
```

A file contains only declarations. The program starts at `fn main()`,
which takes no parameters and returns nothing. Statements at the top level
of a file are an error (`TDS0104`); the REPL allows them.

`pub` is accepted and ignored: Phase 1 programs are a single module.

## Functions

```
fn_decl     = "fn" NAME "(" param,* ")" ( "->" type )? fn_body
param       = NAME ":" mode? type
mode        = "mut" | "take"
fn_body     = ":" body
            | "=" expr NEWLINE
```

```tds
fn greet(name: String) -> String:
    return "Hello, {name}!"

fn square(x: Int) -> Int = x * x

fn damage(player: mut Player, amount: Int):
    player.health -= amount
```

A function without `-> Type` returns `()`. A function's value is the value
of its body block (see [Blocks](#blocks)), or what `return` gives.

## Structs and enums

```
struct_decl = "struct" NAME ":" NEWLINE INDENT field+ DEDENT
field       = NAME ":" type ( "=" expr )? NEWLINE

enum_decl   = "enum" NAME ":" NEWLINE INDENT variant+ DEDENT
variant     = NAME ( "(" variant_field,* ")" )? NEWLINE
variant_field = ( NAME ":" )? type
```

```tds
struct Motor:
    name: String
    voltage: Float
    running: Bool = false        // default value

enum Shape:
    Circle(radius: Float)        // named payload field
    Pair(Int, Int)               // positional payload fields
    Point                        // no payload
```

Default values are evaluated each time a struct is created, and can't
refer to variables.

## Types

```
type        = NAME ( "[" type,* "]" )? "?"*
            | "(" ")"
```

`List[Int]`, `Player?`, `()`. See [Types](types.md).

## Blocks

```
body        = NEWLINE INDENT stmt+ DEDENT      // indented block
            | simple_stmt NEWLINE              // inline, after the `:`
```

Every `:` that opens a body (`fn`, `if`, `else`, `while`, `for`, match
arms) can be followed either by an indented block or by one simple
statement on the same line:

```tds
if done: return
for x in xs: print(x)
```

When a block is used as a value (a function body, an `if` branch, a match
arm), its value is the value of its last statement if that statement is an
expression. Otherwise its value is `()`.

## Statements

```
stmt        = simple_stmt NEWLINE
            | "while" expr ":" body
            | "for" NAME "in" expr ":" body
            | expr_with_block                  // if / match ending in a block

simple_stmt = ( "let" | "var" ) NAME ( ":" type )? "=" expr
            | place assign_op expr
            | "return" expr?
            | "break"
            | "continue"
            | expr

assign_op   = "=" | "+=" | "-=" | "*=" | "/=" | "%="
place       = NAME | place "." NAME | place "[" expr "]"
```

- `let` declares a variable that can't change; `var` one that can. Every
  variable needs an initial value (`TDS0114`).
- A variable can be declared again with the same name; the new one shadows
  the old one.
- Variables are visible from their declaration to the end of the enclosing
  block.

## Expressions

```
expr        = literal
            | NAME
            | "(" expr ")"
            | "(" ")"                          // the unit value
            | "[" expr,* "]"                   // list
            | prefix_op expr
            | expr infix_op expr
            | expr "(" arg,* ")"               // call / construction
            | expr "." NAME "(" arg,* ")"      // method call
            | expr "." NAME                    // field
            | expr "[" expr "]"                // index
            | expr "!"                         // unwrap optional
            | "take" expr                      // move
            | if_expr
            | match_expr

arg         = ( NAME ":" )? mode? expr
prefix_op   = "-" | "not"
infix_op    = "or" | "and" | "==" | "!=" | "<" | "<=" | ">" | ">="
            | "??" | "..<" | "..." | "+" | "-" | "*" | "/" | "%"
```

### Precedence

From loosest to tightest:

| Operators | Associativity |
|---|---|
| `or` | left |
| `and` | left |
| `not` (prefix) | |
| `==` `!=` `<` `<=` `>` `>=` | none: `a < b < c` is an error (`TDS0107`) |
| `??` | right |
| `..<` `...` | none |
| `+` `-` | left |
| `*` `/` `%` | left |
| `-` `take` (prefix) | |
| calls, `[]`, `.`, `!` (postfix) | left |

So `not a == b` means `not (a == b)`, and `x > best ?? 0` means
`x > (best ?? 0)`.

### Calls and construction

A call `NAME(args)` is one of:

- a function call: arguments are positional; each is marked with the
  parameter's mode (`f(mut x)`, `g(take y)`), see
  [Values and ownership](ownership.md);
- struct construction: `Player(name: "Alex", health: 100)`; every field
  must be named, and fields with defaults may be left out;
- enum variant construction: `Circle(radius: 1.0)` or `Circle(1.0)`. The
  enum's name may be written too: `Shape.Circle(1.0)`, `Shape.Point`;
- a conversion: `Int(x)`, `Float(x)`, `String(x)`.

### `if`

```
if_expr     = "if" expr ":" branch ( "else" ( if_expr | ":" branch ) )?
branch      = simple_stmt | NEWLINE INDENT stmt+ DEDENT
```

`if` is an expression. Used as a value, it needs an `else` (`TDS0220`),
and both branches must have the same type:

```tds
let label = if score >= 100: "high" else: "low"
```

After an inline `if a: x`, the `else:` may go on the next line.

### `match`

```
match_expr  = "match" expr ":" NEWLINE INDENT arm+ DEDENT
arm         = pattern ":" branch NEWLINE?
pattern     = "_"
            | NAME                             // binding, or a unit variant
            | NAME "(" pattern,* ")"           // variant with fields
            | "-"? INT | STRING | CHAR | "true" | "false"
```

Arms are tried in order, and the first that matches runs. A `match` must be
exhaustive (`TDS0217`): it must cover every variant of an enum, both `true`
and `false`, or end with a catch-all (`_` or a name) for other types. An
arm that can never run is a warning (`TDS0239`).

A capitalized name in a pattern is always an enum variant; a lowercase name
binds the value to a new variable for that arm.

```tds
match shape:
    Circle(r): 3.14159 * r * r
    Rect(w, _): w
    Point: 0.0

match n:
    0: "zero"
    -1: "minus one"
    other: "the number {other}"
```
