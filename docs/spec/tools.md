# Tools

## The `tds` command

| Command | Does |
|---|---|
| `tds run <file.tds> [args...]` | checks the program and, if there are no errors, runs `main`. `args` are returned by `args()`. |
| `tds check <file.tds>` | checks types, parameter modes and moves without running |
| `tds repl` | starts the interactive prompt |
| `tds explain <code>` | prints the long explanation of an error code, like `tds explain TDS0412` |
| `tds parse <file.tds>` | prints the syntax tree (for debugging) |
| `tds tokens <file.tds>` | prints the tokens (for debugging) |

Exit status is 0 on success, and 1 on a compile error or a runtime panic.
Diagnostics go to stderr, in color when stderr is a terminal. Warnings
don't stop a program from running.

## The REPL

```text
$ tds repl
TDS 0.1.0 REPL. Type :help for help, :quit to leave.
>>> let x = 40
>>> x + 2
42
>>> fn double(n: Int) -> Int:
...     return n * 2
...
>>> double(x)
80
```

- Declarations and statements can be typed directly. Top-level variables
  and declarations last for the whole session; functions can't see the
  session's variables.
- The value of an expression is printed. Strings and chars are shown
  quoted, so `"5"` and `5` look different.
- A line ending in `:` starts a block; finish it with an empty line. Open
  brackets and trailing operators continue onto the next line.
- An entry with errors runs nothing. An entry that panics partway through
  is undone, as if it had never been typed (its output stays).
- Declaring a function, struct or enum again replaces the old one.
- Commands: `:help`, `:reset` (forget everything), `:quit` (or Ctrl-D).
