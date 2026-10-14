# Build TDS — Full Project Brief for a Coding Agent

You are helping me build **TDS**, a compiled systems programming language. It is a hobby project, built for fun and learning. This brief contains the **entire design**, so you understand where every piece is going. However, you will build it **one phase at a time**. Do not jump ahead: every design decision you make now should leave room for the later phases described below.

Implementation language: **Rust** (Cargo workspace). Final backend: **LLVM** (via `inkwell`), plus **Cranelift** for fast debug builds later.

---

## PART 1 — WHAT TDS IS

### The core idea: one language, three levels of control

| Level | For | What you get | What you give up |
|---|---|---|---|
| 1 — Safe values | Beginners, apps, tools | Value semantics, parameter modes, no lifetime syntax, full memory safety | Structs can't hold references; functions can't return references |
| 2 — Explicit references | Libraries, performance code | Stored references with named lifetimes, custom allocators | Must write lifetime annotations |
| 3 — Unsafe | Drivers, FFI, embedded | Raw pointers, MMIO, inline asm, C interop | Compiler stops guaranteeing safety inside `unsafe:` blocks |

### Goals
- Memory safety and no data races outside `unsafe:`, with **no garbage collector**.
- Performance close to C++/Rust (ahead-of-time compiled, zero-cost abstractions).
- Reads like Python: indentation blocks, inference, string interpolation, little boilerplate.
- One binary, `tds`, for build, run, test, format, lint, docs and packages.
- Can run bare-metal: a `core`-only build needs no heap, no OS, no runtime.

### Non-goals (for 1.0)
- No GC, no exceptions, no hidden control flow.
- No implicit numeric conversions that can lose information.
- No custom optimizing backend (LLVM handles that).
- No async/await until after 1.0.

### Locked decisions (don't revisit without asking me)
| Area | Decision |
|---|---|
| Blocks | Indentation, 4 spaces. Tabs are a compile error. No braces, no semicolons. |
| Generics | Square brackets: `List[Int32]`, `fn largest[T: Ordered](...)` |
| Memory | Ownership + parameter modes (Level 1) + lifetimes (Level 2) |
| Errors | `Result[T, E]` + `?`; panics only for bugs |
| Backend | LLVM for release, Cranelift for debug (Phase 2+), TIR interpreter for Phase 1 / REPL / const fn |
| Compiler written in | Rust |
| Self-hosting | Not before 1.0 |

---

## PART 2 — THE LANGUAGE

### 2.1 Syntax tour

```tds
// Variables
let name = "Tiffany"          // immutable, inferred String
var score: Int32 = 100         // mutable
score += 10

// Functions
fn greet(name: String) -> String:
    return "Hello, {name}!"    // string interpolation: any expression in {}

fn square(x: Int32) -> Int32 = x * x   // single-expression form

// Control flow — if/match are expressions
let label = if score >= 100: "high" else: "low"

for i in 0..<10:     // exclusive range: 0..9
    print(i)
for i in 1...10:     // inclusive range: 1..10
    print(i)

while running:
    tick()

// Enums with payloads + exhaustive match
enum Shape:
    Circle(radius: Float64)
    Rect(w: Float64, h: Float64)
    Point

fn area(s: Shape) -> Float64:
    match s:
        Circle(r): 3.14159 * r * r
        Rect(w, h): w * h
        Point: 0.0

// Structs, interfaces, impls
struct Motor:
    name: String
    voltage: Float32
    max_rpm: UInt32
    running: Bool = false          // default field value

interface Controllable:
    fn start(mut self)
    fn stop(mut self)
    fn is_running(self) -> Bool = false   // default method

impl Controllable for Motor:
    fn start(mut self):
        self.running = true
    fn stop(mut self):
        self.running = false
    fn is_running(self) -> Bool = self.running

impl Motor:
    fn new(name: String, volts: Float32) -> Motor:
        return Motor(name: name, voltage: volts, max_rpm: 6000)

// Generics + optionals
fn largest[T: Ordered](items: List[T]) -> T?:
    var best: T? = none
    for item in items:
        if best == none or item > best!:
            best = item
    return best
// T? == Option[T]; `none` is empty; `x!` unwraps (panics on none); `a ?? b` = default

// Modules: one file = one module, private unless `pub`
use std.io
use motors.types.{Motor, Shape}

pub fn run_all(motors: mut List[Motor]):
    for m in motors:
        m.start()
```

### 2.2 Type system

Statically typed. Types are inferred **inside function bodies**; function signatures and public struct fields are always written out.

| Type | Meaning |
|---|---|
| `Bool` | true/false |
| `Int8`…`Int64`, `Int` | signed ints; `Int` = pointer-sized |
| `UInt8`…`UInt64`, `UInt` | unsigned; `UInt` = pointer-sized, used for indexing |
| `Float32`, `Float64` | IEEE 754 |
| `Byte` | alias for `UInt8` |
| `Char` | Unicode scalar (4 bytes) |
| `String` / `Str` | owned UTF-8 / borrowed slice (never allocates) |
| `List[T]` | growable heap array (needs `alloc`) |
| `Array[T, N]` | fixed-size inline array (no heap) |
| `Slice[T]` | borrowed view |
| `Map[K, V]`, `Set[T]` | hash map / set (needs `alloc`) |
| `T?` = `Option[T]` | maybe-absent value; **no null in safe code** |
| `Result[T, E]` | success or error |
| `Box[T]` | single owned heap value |
| `Rc[T]`, `Arc[T]` | shared ownership (single / multi-thread), always explicit |
| `Ptr[T]`, `MutPtr[T]` | raw pointers, deref only in `unsafe:` |

Numeric rules:
- No implicit conversions. Widening: `Int64(x)`. Narrowing: `x.truncate[Int8]()`, `x.saturate[Int8]()`, `x.checked[Int8]()` → `Int8?`.
- Overflow panics in Development builds, wraps in Release (configurable: `overflow = "panic"`). `wrapping_add`, `saturating_add`, `checked_add` always exist.
- Integer literals default to `Int`, floats to `Float64`, but adopt the expected type when known (`let x: UInt8 = 200`).
- Division by zero always panics.

Built-in interfaces the compiler knows: `Copy`, `Clone`, `Drop`, `Equal`, `Ordered`, `Hash`, `Display`, `Send`, `Share`, `From`. Derivable with `@derive(Equal, Hash, Clone)`.

### 2.3 Memory model (the heart of TDS)

**Ownership rules (all levels):**
1. Every value has exactly one owner: a variable, field, or collection slot.
2. When the owner goes out of scope, the value is destroyed immediately (`Drop` runs), in reverse declaration order.
3. Passing or assigning a non-`Copy` value with `take` moves it; the old name is dead.
4. At any point a value has either many readers or exactly one writer, never both.
5. No reference outlives the value it points to.

**Level 1: parameter modes** (instead of Rust's `&` / `&mut`)

| Mode | Declared | Caller writes | Meaning |
|---|---|---|---|
| read (default) | `p: Player` | `f(player)` | shared read-only borrow for the call |
| mut | `p: mut Player` | `f(mut player)` | exclusive borrow; caller sees changes |
| take | `p: take Player` | `f(take player)` | ownership moves in |

```tds
struct Player:
    name: String
    health: Int32

fn damage(player: mut Player, amount: Int32):
    player.health -= amount

fn main():
    var player = Player(name: "Alex", health: 100)
    damage(mut player, 25)    // mutation visible at the call site
    print(player.health)      // 75
```

At Level 1, functions return by ownership only, and structs can't hold references. **This is the key rule:** it makes Level 1 checkable with no lifetime syntax at all. `Copy` types (numbers, Bool, Char, small plain structs) are copied rather than borrowed.

**Level 2: stored references with lifetimes**

```tds
struct Parser['src]:
    text: ref['src] Str
    pos: UInt

fn first_word['a](text: ref['a] Str) -> ref['a] Str:
    let end = text.find(' ') ?? text.len()
    return text[0..<end]
```

Lifetime elision follows Rust's rules: with one reference input, the output borrows from it; with a `self` borrow, the output borrows from `self`. Anything else must be written out.

**Safe escape hatches:** `Rc`/`Arc` + `Cell`/`Mutex`; `std.mem.Arena` for region-style allocation; explicit allocators (`List[Int32].with_allocator(arena)`).

**How it's checked:** a non-lexical dataflow analysis on TIR, like Rust's NLL/Polonius. A borrow ends at its last use, not at the end of the block.

### 2.4 Error handling

```tds
enum ConfigError:
    Missing(path: String)
    Invalid(line: UInt, reason: String)

fn load_config(path: Str) -> Result[Config, ConfigError]:
    let text = fs.read_text(path)?      // ? returns early on Err
    return parse(text)

fn main() -> Result[(), Error]:
    let cfg = load_config("tds.toml")?
    let port = cfg.port ?? 8080
    match connect(port):
        Ok(conn): serve(take conn)
        Err(e): print("failed: {e}")
```

- `?` converts errors through `From`.
- `Error` is a standard boxed error type for apps.
- Panics unwind on desktop/server and abort on embedded; `panic = "abort"` forces abort everywhere.
- Embedded targets provide a `@panic_handler`.

### 2.5 Concurrency

Data races are compile errors. `Send` (can move to another thread) and `Share` (can be read from several threads) are auto-derived from a type's fields.

```tds
use std.thread
use std.sync.{Mutex, Arc, channel}

fn main():
    let counter = Arc(Mutex(0))
    thread.scope(fn(s):
        for _ in 0..<4:
            let c = counter.clone()
            s.spawn(fn():
                c.lock() += 1
            )
    )
    print(counter.lock())   // 4

    let (tx, rx) = channel[String]()
    thread.spawn(fn(): tx.send("hello"))
    print(rx.recv()!)
```

Phase 4 adds threads, scoped threads, Mutex, RwLock, atomics, channels, and maybe `par_for`. Phase 5 adds `@interrupt` handlers and critical sections. Async comes after 1.0.

### 2.6 Unsafe, C interop, hardware (Level 3)

`unsafe:` blocks and `unsafe fn`. Each needs a `// SAFETY:` comment, and `tds check` warns if it's missing.

`unsafe:` unlocks exactly these:
- raw pointer deref and arithmetic
- calling `unsafe fn` / `extern`
- `static var`
- volatile access and inline asm
- manual `Send`/`Share` impls

```tds
@repr(C)
struct GpioRegs:
    mode: Volatile[UInt32]
    output: Volatile[UInt32]

const GPIOA: MutPtr[GpioRegs] = MutPtr(0x4002_0000)

pub fn led_on():
    unsafe:
        // SAFETY: GPIOA is a valid, aligned peripheral address on this chip
        GPIOA.deref().output.write(1 << 5)

extern "C":
    fn strlen(s: Ptr[CChar]) -> UInt

@export("tds_add")
pub extern "C" fn add(a: Int32, b: Int32) -> Int32 = a + b

unsafe:
    asm("wfi")
```

`tds bindgen header.h` generates externs; `tds build --emit header` writes a `.h`.

Library layers:

| Layer | Needs | Provides |
|---|---|---|
| `core` | nothing | primitives, Option, Result, Array, Slice, Str, interfaces, math |
| `alloc` | an allocator | Box, List, String, Map, Rc, Arc |
| `std` | an OS | files, net, threads, time, processes |

Embedded packages set `std = false` and optionally register a `@global_allocator`.

---

## PART 3 — HOW THE COMPILER WORKS

### 3.1 Pipeline

```
.tds source
  → Lexer + parser              tokens (with INDENT/DEDENT) → AST; syntax errors with recovery
  → Name resolution → HIR       imports resolved; for, ?, ??, ranges desugared to core forms
  → Type checker → typed HIR    inference, generics, interface lookup, numeric rules
  → Lowering → TIR              control-flow graph, explicit moves / borrows / drops
  → Ownership checker (on TIR)  moves, borrows, lifetimes, Send/Share, unsafe boundaries
  → Monomorphization + TIR passes   generics specialized, inlining, const folding
  → Backends:
       LLVM            release builds, every target  → native exe / lib / object
       Cranelift       fast dev builds               → native exe / lib / object
       TIR interpreter REPL, const fn, Phase 1 bring-up
```

The ownership checker runs on TIR because borrows are about control flow. **In Phase 1 it's a pass that accepts everything**, so the pipeline shape never changes.

### 3.2 Compiler design rules
- **Query-based and incremental**, like Rust's `salsa`: each stage is a memoized query. Editing one function re-checks only its dependents, which powers the LSP. (You may start without salsa in Phase 1, but keep stages as pure functions over inputs so they can become queries later.)
- **One interpreter, three jobs:** Phase 1 execution, `tds repl`, compile-time `const fn`.
- **Error recovery everywhere:** one typo gives one error, not fifty.
- **Diagnostics are a product:** every error has a code (`TDS0412`), a primary label, a plain-language note, and a suggested fix when possible. `tds explain CODE` prints the long form. Use `ariadne` or `codespan-reporting`.

Example target quality:
```
error[TDS0412]: `player` was moved and can't be used here
  --> src/main.tds:9:11
   |
 7 |     save(take player)
   |          ----------- ownership moved into `save` here
 9 |     print(player.health)
   |           ^^^^^^ used after the move
   = help: pass a read borrow instead: `save(player)`
   = help: or keep a copy first: `save(take player.clone())`
```

### 3.3 Lexer notes
- Track an indentation stack. At each logical line start, compare its indent to the stack top: emit INDENT, or one DEDENT per popped level, or an error if it doesn't match any level.
- Inside open `(` `[` `{`, newlines and indentation are ignored, so long expressions can span lines.
- Lines may also continue after a trailing binary operator or comma.
- String interpolation: lex `"...{expr}..."` into parts and parse the embedded expressions.

### 3.4 Repository layout

```
tds/
├── crates/
│   ├── tds_lexer/        tds_parser/       tds_ast/
│   ├── tds_hir/          tds_resolve/      tds_types/
│   ├── tds_tir/          tds_borrowck/     tds_interp/
│   ├── tds_codegen_llvm/ tds_codegen_cl/   tds_diagnostics/
│   ├── tds_driver/       tds_lsp/          tds_fmt/
│   └── tds_cli/          (the `tds` binary)
├── library/
│   ├── core/  alloc/  std/      (written in TDS once the compiler can)
├── tests/
│   ├── ui/          source + expected error output (snapshot tests, `insta`)
│   ├── run-pass/    programs + expected stdout
│   └── borrowck/    one test per ownership rule
├── docs/spec/       language reference, updated with every language change
├── docs/book/       beginner guide
└── docs/decisions.md   log of design decisions made during implementation
```

Only create the crates a phase needs. The rest appear when their phase starts.

---

## PART 4 — TOOLING (the end goal)

| Command | Does |
|---|---|
| `tds new hello` | new project (`--lib`, `--embedded`) |
| `tds run` | dev build + run |
| `tds build --release` | optimized build; `--target <triple>` cross-compiles |
| `tds check` | type + ownership checks, no codegen |
| `tds test` | runs `@test` functions |
| `tds bench` | runs `@bench` functions |
| `tds fmt` | one style, no options |
| `tds lint` | extra lints |
| `tds doc` | HTML docs from `///`; examples are tested |
| `tds add http` | add dependency, update `tds.lock` |
| `tds explain TDS0412` | long error explanation |
| `tds repl` | interactive prompt |

`tds.toml`:
```toml
[package]
name = "motor-controller"
version = "0.1.0"
edition = "2027"

[dependencies]
http = "1.2"
serial = { git = "https://github.com/example/serial", tag = "v0.4" }

[profile.release]
opt-level = 3
overflow = "panic"
lto = true

[target.embedded]
triple = "thumbv7em-none-eabihf"
std = false
panic = "abort"
```

Project layout: `tds.toml`, `tds.lock`, `src/main.tds`, `src/lib.tds`, `tests/`, `benches/`, `examples/`, `build/`.

The language server reuses the same incremental compiler as `tds check`. Target features: diagnostics, hover types, go-to-definition, rename, completion, and inlay hints for inferred types and parameter modes.

---

## PART 5 — ROADMAP

Don't start a phase until the previous one passes its exit test.

| Phase | Builds | Exit test |
|---|---|---|
| **1 · Prototype** | Lexer, parser, AST, basic type checking, TIR interpreter, diagnostics, REPL | FizzBuzz, a file reader, a TDS-written sort, and 50+ error-message tests pass |
| **2 · Native compiler** | HIR→TIR lowering made real, LLVM codegen, modules/imports, debug info, `tds build` | Standalone binary that steps cleanly in lldb/gdb |
| **3 · Safety** *(hardest)* | Parameter modes enforced statically, ownership checker, lifetimes, enums, generics, interfaces | One test per ownership rule; 3 real programs ported (CLI, HTTP server sketch, driver sketch) |
| **4 · Practical** | Collections, I/O, threads, packages, `tds test`/`fmt`, LSP | A CLI tool and an HTTP server written in TDS |
| **5 · Systems** | Unsafe, C FFI, allocators, SIMD, no-std, cross-compile, Cranelift | LED + UART driver on an ARM Cortex-M board |
| **6 · Stable 1.0** | Spec frozen, editions, benchmarks | Within ~10% of C++ on the benchmark suite |

Rules for every phase:
- Every new error gets a `tests/ui/` test.
- Every language change updates `docs/spec/` in the same commit.
- CI on Linux, macOS, Windows from Phase 2.

---

## PART 6 — YOUR JOB RIGHT NOW: PHASE 1

Build a **working interpreter for a subset of TDS**, with the architecture already shaped for the full pipeline.

### Phase 1 subset
- `let`/`var`, optional annotations; types Int (i64), Float (f64), Bool, String, List[T], `()`
- Arithmetic, comparison, `and`/`or`/`not`, compound assignment
- String interpolation
- Functions (block and `= expr` forms), recursion
- `if`/`else if`/`else` as statements and expressions; `while`, `for x in list`, ranges `..<` and `...`, `break`, `continue`
- `struct` with defaults, named-arg construction, field access and mutation
- `enum` with payloads, exhaustive `match`
- **Parameter modes** `read` (default) / `mut` / `take` with call-site markers. Enforce them in the interpreter or checker: no assigning to read params, "used after move" after `take`.
- Built-ins: `print`, `len`, `.push()`, `.pop()`, indexing
- No implicit numeric conversions; `Float(x)`, `Int(x)`
- Optional stretch: `Option`/`T?`, `??`, `!`

### Phase 1 crates
`tds_lexer`, `tds_parser`, `tds_ast`, `tds_check`, `tds_interp`, `tds_diagnostics`, `tds_cli`. You may interpret the AST directly in Phase 1, but put a clear seam where HIR/TIR lowering will go in Phase 2.

### Phase 1 CLI
`tds run file.tds`, `tds check file.tds`, `tds repl` (multi-line blocks).

### Phase 1 tests
- run-pass: hello world, FizzBuzz, recursive fib, struct + mut param, enum + match, interpolation, ranges, a sort written in TDS
- ui: one failing test per error code (aim for 50+), snapshot-tested with `insta`
- lexer unit tests for INDENT/DEDENT edge cases (blank lines, comments, nested blocks, dedenting several levels at once, tabs, misalignment)

### Build order
1. Workspace + lexer (INDENT/DEDENT) + tests
2. Parser (recursive descent + Pratt for expressions) + AST pretty-printer for tests
3. Interpreter: expressions → statements → functions
4. Structs → enums/match → lists/ranges/for
5. Parameter modes and move tracking
6. Static type checker
7. Diagnostics polish (codes, snippets, help lines, recovery)
8. REPL
9. `docs/spec.md` and `docs/decisions.md` brought up to date

### How to work
- Commit after each step with all tests passing.
- When the brief doesn't answer a design question, pick the simplest option that doesn't block later phases, log it in `docs/decisions.md`, and keep going.
- Prefer clear, well-commented code over clever code. This is a learning project.
- **Start with step 1, and show me the lexer tests passing before moving on.**
