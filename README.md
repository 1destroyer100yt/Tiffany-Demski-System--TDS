# TDS

TDS is a compiled systems programming language that reads like Python:
indentation for blocks, type inference, string interpolation. It has
ownership-based memory safety and no garbage collector. It's a hobby project
by Andrew Detroyer, built for fun and learning.

The full design is in [`docs/brief.md`](docs/brief.md). The language as
implemented so far is described in the [reference](docs/spec/README.md),
and the choices made along the way are in
[`docs/decisions.md`](docs/decisions.md).

```tds
struct Player:
    name: String
    health: Int

fn damage(player: mut Player, amount: Int):
    player.health -= amount

fn main():
    var player = Player(name: "Alex", health: 100)
    damage(mut player, 25)
    print("{player.name} has {player.health} HP")
```

## Status

**Phase 1 (prototype interpreter): done.** The exit test passes:
FizzBuzz, a file reader and sorts written in TDS run correctly, and there
are 91 error-message tests (one per error code).

- [x] Step 1: workspace + lexer (INDENT/DEDENT)
- [x] Step 2: parser + AST pretty-printer
- [x] Step 3: interpreter: expressions, statements, functions
- [x] Step 4: structs, enums/match, lists/ranges/for
- [x] Step 5: parameter modes and move tracking
- [x] Step 6: static type checker
- [x] Step 7: diagnostics (codes, snippets, help lines, recovery, `tds explain`)
- [x] Step 8: REPL
- [x] Step 9: reference and decision log

Next is Phase 2: HIR/TIR lowering, LLVM code generation, modules and
`tds build`.

## Getting started

You need a Rust toolchain (`cargo`).

```sh
cargo build --release
./target/release/tds run tests/run-pass/fizzbuzz.tds
./target/release/tds repl
./target/release/tds explain TDS0412
```

| Command | Does |
|---|---|
| `tds run <file.tds> [args...]` | check and run a program |
| `tds check <file.tds>` | check without running |
| `tds repl` | interactive prompt |
| `tds explain <code>` | explain an error code |
| `tds parse` / `tds tokens <file.tds>` | debugging views |

## Layout

| Path | What |
|---|---|
| `crates/tds_diagnostics` | `Span`, `Diagnostic`, the error-code registry and explanations |
| `crates/tds_lexer` | source → tokens, including INDENT / DEDENT / NEWLINE |
| `crates/tds_ast` | the syntax tree and its pretty-printer |
| `crates/tds_parser` | tokens → syntax tree, with error recovery |
| `crates/tds_check` | types, parameter modes, mutability and moves |
| `crates/tds_interp` | the tree-walking interpreter (replaced by TIR in Phase 2) |
| `crates/tds_cli` | the driver, the REPL and the `tds` binary |
| `tests/run-pass/` | programs and their expected output |
| `tests/ui/` | one program per error code, with snapshot-tested messages |
| `docs/spec/` | the language reference |

## Tests

```sh
cargo test
```

| Suite | Count |
|---|---|
| Lexer (INDENT/DEDENT edge cases, literals, recovery) | 94 |
| Parser (snapshot-tested AST dumps, errors, recovery) | 45 |
| Checker (move analysis, false-positive checks) | 35 |
| Run-pass programs with expected output | 17 |
| UI tests (one per error code, snapshot-tested) | 91 |
| REPL and command-line tests | 23 |

To accept changed error messages after reviewing them:
`INSTA_UPDATE=always cargo test`, then check the diff with git.
