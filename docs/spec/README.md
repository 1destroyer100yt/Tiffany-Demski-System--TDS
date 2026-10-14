# TDS language reference (Phase 1)

This is the reference for the language as the Phase 1 prototype implements
it. The full design, including later phases, is in [the brief](../brief.md);
choices made during implementation are logged in
[decisions.md](../decisions.md).

Every language change updates this reference in the same commit.

| Chapter | Contents |
|---|---|
| [Lexical structure](lexical.md) | Source text, comments, indentation, literals, operators |
| [Syntax](syntax.md) | Declarations, statements, expressions, patterns, precedence |
| [Types](types.md) | The Phase 1 types, inference, conversions, optionals |
| [Values and ownership](ownership.md) | Value semantics, parameter modes, moves, mutability |
| [Built-ins](builtins.md) | Built-in functions and methods |
| [Errors and panics](errors.md) | How errors are reported, and the list of error codes |
| [Tools](tools.md) | The `tds` command and the REPL |

## What Phase 1 covers

The Phase 1 subset is: `let`/`var`; `Int`, `Float`, `Bool`, `Char`,
`String`, `List[T]`, `()` and optionals `T?`; arithmetic, comparison and
logic; string interpolation; functions (block and `= expr` forms) and
recursion; `if`/`else` as statements and expressions; `while`, `for`,
ranges, `break`, `continue`; structs with defaults and named construction;
enums with payloads and exhaustive `match`; the parameter modes read,
`mut` and `take`, checked statically; and the built-ins listed in
[Built-ins](builtins.md).

Features from later phases (generics, interfaces, `impl`, modules,
closures, `Result` and `?`, references and lifetimes, `unsafe`) are
recognized and rejected with error `TDS0106`, which names the phase that
adds them.

## A taste

```tds
struct Player:
    name: String
    health: Int
    level: Int = 1

enum Shape:
    Circle(radius: Float)
    Rect(w: Float, h: Float)
    Point

fn damage(player: mut Player, amount: Int):
    player.health -= amount

fn area(s: Shape) -> Float:
    match s:
        Circle(r): 3.14159 * r * r
        Rect(w, h): w * h
        Point: 0.0

fn main():
    var player = Player(name: "Alex", health: 100)
    damage(mut player, 25)
    print("{player.name} has {player.health} HP")   // Alex has 75 HP

    for s in [Circle(radius: 1.0), Rect(w: 2.0, h: 3.0), Point]:
        print(area(s))
```
