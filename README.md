# TDS

TDS is a compiled systems programming language with Python-like syntax,
ownership-based memory safety and no garbage collector. It's a hobby project
by Andrew Detroyer, built for fun and learning.

The full design brief is in [`docs/brief.md`](docs/brief.md).

## Status

Phase 1 (prototype interpreter) is in progress.

- [x] Step 1: workspace + lexer (INDENT/DEDENT) + tests
- [ ] Step 2: parser + AST pretty-printer
- [ ] Step 3: interpreter: expressions, statements, functions
- [ ] Step 4: structs, enums/match, lists/ranges/for
- [ ] Step 5: parameter modes and move tracking
- [ ] Step 6: static type checker
- [ ] Step 7: diagnostics polish
- [ ] Step 8: REPL
- [ ] Step 9: spec and decision log up to date

## Layout

| Path | What |
|---|---|
| `crates/tds_diagnostics` | `Span`, `Diagnostic` and the error-code registry |
| `crates/tds_lexer` | source → tokens, including INDENT / DEDENT / NEWLINE |
| `docs/spec/` | language reference |
| `docs/decisions.md` | design decisions made during implementation |

## Building

```sh
cargo test
```
