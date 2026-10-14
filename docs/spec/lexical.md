# Lexical structure

This chapter describes how TDS source text is split into tokens.

## Source files

Source files are UTF-8. Lines end with `\n`, `\r\n` or `\r`. Tab characters
are not allowed outside string literals and comments (`TDS0001`).

## Comments

`//` starts a comment that runs to the end of the line. `///` starts a doc
comment. There are no block comments.

## Layout

Blocks are marked by indentation, not braces. The lexer turns indentation into
`INDENT` and `DEDENT` tokens and ends each logical line with `NEWLINE`.

- Each block is indented exactly **4 spaces** deeper than the line that opens
  it (`TDS0003`).
- When a block ends, the next line must line up with an enclosing block
  (`TDS0002`). Ending several blocks at once is allowed.
- Blank lines and lines holding only a comment are ignored, whatever their
  indentation.
- Inside `( )`, `[ ]` and `{ }`, line breaks and indentation are ignored.
- A line that ends in `,` or a binary operator continues on the next line,
  and the next line's indentation is ignored. The operators are
  `+ - * / % & | ^ << >> == != < <= > >= = += -= *= /= %= . ..< ... -> ?? and or`.
- At the end of the file, every open block is closed.

```tds
fn main():                  // fn main ( ) : NEWLINE
    let total = 1 +         // INDENT let total = 1 +
        2                   //   2 NEWLINE
    print(total)            // print ( total ) NEWLINE
                            // DEDENT EOF
```

## Identifiers and keywords

An identifier starts with a letter or `_` and continues with letters, digits
and `_`. Letters may be any Unicode alphabetic characters.

Reserved keywords:

```
and break const continue else enum extern false fn for if impl in interface
let match mut none not or pub ref return self static struct take true unsafe
use var while _
```

## Literals

**Integers:** `42`, `1_000_000`, `0xFF`, `0b1010`, `0o17`. Underscores may
appear after the first digit. The value must fit in 64 bits (`TDS0011`).
Negative numbers are written with the unary `-` operator.

**Floats:** `3.14`, `0.5`, `1e10`, `1.5e-3`. A fraction needs digits on both
sides of the dot, so `1..<10` is a range and `5.abs()` is a method call.

A number followed straight away by a letter is an error (`TDS0012`).

**Strings:** `"..."` on a single line (`TDS0005`). Escapes:

| Escape | Meaning |
|---|---|
| `\n` `\t` `\r` `\0` | newline, tab, carriage return, NUL |
| `\\` `\"` `\'` | backslash, double quote, single quote |
| `\{` `\}` | literal braces |
| `\u{1F600}` | Unicode scalar value, 1-6 hex digits |

Any other escape is an error (`TDS0006`).

**Interpolation:** `{expr}` inside a string embeds any expression:
`"Hello, {name}!"`, `"{a + b}"`, `"{greet("Bob")}"`. The braces must be
closed on the same line (`TDS0007`) and can't be empty (`TDS0013`).

**Chars:** `'a'`, `'\n'`, `'é'`: exactly one character (`TDS0014`).

**Lifetimes:** `'src`, a quote followed by an identifier with no closing
quote. Used by Level 2 references.

## Operators and punctuation

```
+  -  *  /  %  &  |  ^  ~  <<  >>
== != <  <= >  >=
=  += -= *= /= %=
(  )  [  ]  {  }  ,  :  .  ..  ..<  ...  ->  @  ?  ??  !
```

`;` is not used in TDS (`TDS0004`, with a hint).
