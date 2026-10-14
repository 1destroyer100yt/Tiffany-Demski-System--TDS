# Built-ins

## Functions

| Function | Type | Description |
|---|---|---|
| `print(x)` | any → `()` | prints `x` and a newline; `print()` prints an empty line |
| `len(x)` | `List[T]` or `String` → `Int` | number of elements, or of characters |
| `Int(x)` | `Int`, `Float`, `Char` → `Int` | see [conversions](types.md#conversions) |
| `Float(x)` | `Int`, `Float` → `Float` | |
| `String(x)` | any → `String` | the value as text |
| `read_file(path)` | `String` → `String?` | the whole file, or `none` if it can't be read |
| `args()` | → `List[String]` | the program's command-line arguments |
| `panic(message)` | `String` → never | stops the program with `TDS0506` |

A function declared by the program with the same name replaces the
built-in one.

## Methods

Every value has `.clone()`, which returns a copy.

### `List[T]`

| Method | Returns | Notes |
|---|---|---|
| `xs.len()` | `Int` | |
| `xs.is_empty()` | `Bool` | |
| `xs.contains(x)` | `Bool` | uses `==` |
| `xs.push(x)` | `()` | adds to the end; `xs` must be changeable |
| `xs.pop()` | `T?` | removes and returns the last element, or `none` if empty; `xs` must be changeable |

Elements are read and written with `xs[i]`, where `0 <= i < len(xs)`
(`TDS0502` otherwise).

### `String`

| Method | Returns |
|---|---|
| `s.len()` | `Int` (characters) |
| `s.is_empty()` | `Bool` |
| `s.contains(t)`, `s.starts_with(t)`, `s.ends_with(t)` | `Bool` |
| `s.trim()`, `s.upper()`, `s.lower()` | `String` |
| `s.lines()` | `List[String]` |
| `s.split(sep)` | `List[String]` |
| `s.chars()` | `List[Char]` |
| `s.to_int()` | `Int?` |
| `s.to_float()` | `Float?` |

### `Int`

| Method | Returns |
|---|---|
| `n.abs()` | `Int` (panics on overflow) |
| `n.min(m)`, `n.max(m)` | `Int` |
| `n.pow(e)` | `Int` (panics on overflow) |

### `Float`

`x.abs()`, `x.sqrt()`, `x.floor()`, `x.ceil()`, `x.round()`, each
returning `Float`.

### `Char`

`c.is_digit()`, `c.is_alphabetic()`, `c.is_whitespace()`, each returning
`Bool`.

## How values print

`print`, interpolation and `String(x)` format values the same way:

| Value | Printed as |
|---|---|
| `42`, `-7` | `42`, `-7` |
| `10.0`, `0.1 + 0.2` | `10.0`, `0.30000000000000004` |
| `"text"`, `'c'` | `text`, `c` |
| `[1, 2]`, `["a"]` | `[1, 2]`, `["a"]` (text is quoted inside lists) |
| a struct | `Player(name: "Alex", health: 75)` |
| a variant | `Circle(radius: 1.0)`, `Pair(1, 2)`, `Point` |
| `none`, `()` | `none`, `()` |
