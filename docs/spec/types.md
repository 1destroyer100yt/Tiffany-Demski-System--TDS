# Types

TDS is statically typed. Types are worked out (inferred) inside function
bodies; function signatures and struct fields are always written out.

## The Phase 1 types

| Type | Values |
|---|---|
| `Int` | 64-bit signed integers. `Int64` is another name for it. |
| `Float` | 64-bit IEEE 754 floating point. `Float64` is another name for it. |
| `Bool` | `true`, `false` |
| `Char` | one Unicode scalar value: `'a'`, `'é'`, `'\n'` |
| `String` | UTF-8 text |
| `List[T]` | a growable list of `T` |
| `T?` | an optional `T`: a `T`, or `none`. `Option[T]` is another way to write it. |
| `()` | the unit type, with the single value `()`; "no value" |
| structs and enums | declared by the program |

Ranges (`0..<10`, `1...10`) also have a type, used by `for` loops.

Sized integers (`Int32`, `UInt8`, ...), `Float32`, `Str`, `Array`, `Slice`,
`Map`, `Set`, `Result`, `Box`, `Rc`, `Arc` and the pointer types come in
later phases (`TDS0234`).

## Inference

Inside a function body, a variable's type comes from its initial value:

```tds
let count = 3              // Int
let names = ["a", "b"]     // List[String]
var best: Int? = none      // the annotation is needed: `none` alone has no type
var xs: List[Int] = []     // likewise for an empty list
```

Where the context expects a particular type (an annotation, a parameter,
a return type, the other side of `==`), that type flows into the
expression. That's how `[]` and `none` get their types. Without context they
are an error (`TDS0221`).

## Conversions

TDS never converts between number types implicitly (`TDS0204`). Convert
explicitly:

| Conversion | Result |
|---|---|
| `Float(i)` | the `Int` as a `Float` (rounded to the nearest float) |
| `Int(f)` | the `Float` truncated toward zero; panics (`TDS0505`) if out of range or NaN |
| `Int(c)` | the `Char`'s code point |
| `String(x)` | any value as text, formatted like `print` |
| `s.to_int()`, `s.to_float()` | parse text; `none` if it isn't a number |

Integer literals are `Int` and float literals are `Float`; `1` can't be
used where a `Float` is expected (write `1.0`).

The one implicit conversion is wrapping a value into an optional: an `Int`
can be used where an `Int?` is expected.

## Optionals

`T?` holds a `T` or `none`. There is no `null` anywhere else.

| Operation | Meaning |
|---|---|
| `none` | the empty optional (needs a known type) |
| `x == none`, `x != none` | check for a value |
| `x!` | the value; panics (`TDS0503`) if `x` is `none` |
| `x ?? default` | the value, or `default` if `x` is `none` |

```tds
fn largest(items: List[Int]) -> Int?:
    var best: Int? = none
    for item in items:
        if best == none or item > best!:
            best = item
    return best

print(largest([3, 9, 2]) ?? 0)    // 9
```

Optionals can't be nested (`Int??` is `TDS0225`), and fields and methods
can't be used on an optional directly: unwrap it first.

## Numbers

- `+ - * / %` work on two `Int`s or two `Float`s.
- Integer overflow panics (`TDS0500`). Phase 1 behaves like a Development
  build.
- Integer `/` truncates toward zero, and integer division or remainder by
  zero panics (`TDS0501`). Float division follows IEEE 754
  (`1.0 / 0.0` is infinity).
- `-9223372036854775808` is a valid `Int` literal; `9223372036854775808`
  on its own is not (`TDS0235`).

## Strings

`+` joins two `String`s. Interpolation `"text {expr} text"` turns any value
into text, formatted like `print`. Strings can't be indexed yet; use
`.chars()`. See [Built-ins](builtins.md) for string methods.

## Equality and ordering

- `==` and `!=` compare any two values of the same type, structurally:
  lists element by element, structs field by field.
- `<`, `<=`, `>`, `>=` work on `Int`, `Float`, `Char` and `String`
  (`TDS0226`).
- `and`, `or` and `not` need `Bool`s. Conditions of `if` and `while` must
  be `Bool` (`TDS0212`): numbers, strings and lists are never "truthy".

## The never type

`return`, `break`, `continue` and `panic(...)` never produce a value. They
can stand wherever any type is expected:

```tds
let n = if ok: 1 else: panic("not ok")
```
