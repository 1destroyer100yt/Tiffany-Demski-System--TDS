# Long explanations for `tds explain CODE`

Each section starts with `## TDSxxxx` and runs until the next one.

## TDS0001

A tab character was found in the source code.

TDS uses indentation to mark blocks, and it only accepts spaces, so code
looks the same in every editor. Tabs are allowed inside strings and
comments.

Fix: configure your editor to insert 4 spaces when you press Tab.

## TDS0002

A line ends a block, but doesn't line up with any block it could return to.

    fn main():
        if ready:
            start()
          stop()        // 6 spaces: neither 4 nor 8

When a block ends, the next line must start at exactly the same column as
an enclosing block. Here, `stop()` should be at 4 spaces (after the `if`)
or 8 spaces (inside it).

## TDS0003

A block is indented by something other than 4 spaces.

    fn main():
      print(1)          // 2 spaces

Every level of indentation in TDS is exactly 4 spaces deeper than the line
that opens it.

## TDS0004

This character can't start any TDS token.

A common case is `;`. TDS doesn't use semicolons: a statement ends at the
end of the line.

## TDS0005

A string literal has no closing `"` on the same line.

    print("hello)

Strings can't span lines. To put a line break inside a string, write `\n`.

## TDS0006

A backslash escape in a string or char literal isn't one TDS knows.

The valid escapes are `\n` (newline), `\t` (tab), `\r`, `\0`, `\\`, `\"`,
`\'`, `\{`, `\}` and `\u{1F600}` (a Unicode code point in hex).

To write a backslash itself, double it: `"C:\\path"`.

## TDS0007

A `{` in a string starts an interpolation, but there's no `}` to end it on
the same line.

    print("hi {name")

Close the interpolation with `}`, or write `\{` if you wanted a literal
brace.

## TDS0008

A closing bracket `)`, `]` or `}` has no opening bracket to match.

    print(1))

Remove the extra bracket.

## TDS0009

A closing bracket is the wrong kind for the bracket it closes.

    let xs = [1, 2)

Square brackets close with `]`, parentheses with `)`, braces with `}`.

## TDS0010

A bracket is opened but never closed before the end of the file.

    print(len([1, 2])

Add the missing closing bracket.

## TDS0011

An integer literal has more than 64 bits.

The largest number that can be written is 18446744073709551615, and the
largest `Int` is 9223372036854775807.

## TDS0012

A number is malformed: a prefix with no digits (`0x`), an exponent with no
digits (`1e`), or letters right after the digits (`12abc`).

Numbers look like `42`, `1_000_000`, `0xFF`, `0b1010`, `0o17`, `3.14` or
`1.5e-3`.

## TDS0013

A string has `{}` with nothing inside.

    print("value: {}")

Interpolation needs an expression: `"value: {x}"`. For literal braces,
write `\{\}`.

## TDS0014

A char literal must hold exactly one character: `'a'`, `'\n'`, `'é'`.

    let c = 'ab'      // two characters
    let d = ''        // none

For text, use a string: `"ab"`.

## TDS0100

An expression was expected, but the code ends or something else follows.

    let x = 1 +

Here the `+` needs a right-hand side.

## TDS0101

The parser expected one specific token and found another.

    fn main()
        print(1)

The function header needs a `:` before its body. The message says what
was expected.

## TDS0102

A line ends with `:` (opening a block), but the next line isn't indented.

    if ready:
    start()

Indent the body of the block by 4 more spaces.

## TDS0103

A line is indented more than the line before it, but the line before it
doesn't open a block with `:`.

    print(1)
        print(2)

Line it up with the previous line.

## TDS0104

A statement appears at the top level of a file.

The top level of a TDS file only declares things: functions, structs and
enums. The program runs from `fn main()`:

    fn main():
        print("hello")

(The REPL is different: there you can type statements directly.)

## TDS0105

The left side of `=` isn't something that can be assigned to.

You can assign to a variable (`x = 1`), a field (`p.health = 1`) or a list
element (`xs[0] = 1`), but not to a computed value like `1 + 1` or `f()`.

## TDS0106

This feature exists in the TDS design, but not in the Phase 1 prototype.

The message says which phase of the roadmap adds it. For example, `impl`
blocks and generics arrive in Phase 3, modules in Phase 2, and closures in
Phase 4.

## TDS0107

Comparisons can't be chained.

    if 0 < x < 10:

Write the two comparisons separately and combine them with `and`:

    if 0 < x and x < 10:

## TDS0108

A type was expected here, such as `Int`, `List[String]` or `Player?`.

## TDS0109

At the top level, only `fn`, `struct` and `enum` declarations are allowed
(optionally with `pub` in front).

## TDS0110

A positional argument comes after a named one.

    Point(x: 1, 2)

Put positional arguments first, or name all of them.

## TDS0111

A function parameter has no type.

    fn double(x) -> Int:

Function signatures are always written out in full, so each parameter
needs a type: `fn double(x: Int) -> Int:`.

## TDS0112

`else` appears without an `if` right before it.

Usually the `else` is indented differently from its `if`. They must line
up:

    if ready:
        start()
    else:
        wait()

## TDS0113

`=` was used in a condition.

    if x = 1:

`=` assigns a value. To compare, use `==`:

    if x == 1:

## TDS0114

A variable is declared without a value.

    var total: Int

Every TDS variable holds a value from the moment it exists, so there's
never an uninitialized variable to read by accident:

    var total = 0

## TDS0115

`mut` (or `take`) appears where it isn't allowed.

`mut` and `take` are parameter modes: they appear in a function's parameter
list (`p: mut Player`) and at the call site (`damage(mut player, 5)`). To
declare a variable that can change, use `var`.

## TDS0116

A `match` arm must start with a pattern: a variant (`Circle(r)`), a literal
(`0`, `"en"`, `true`), a name that binds the value (`other`) or `_`.

## TDS0200

This name isn't a variable, function, struct, enum variant or built-in in
scope.

Check the spelling; the message suggests a close match when it finds one.
Variables are only visible after they're declared and inside the block
that declares them.

## TDS0201

This type doesn't exist.

The Phase 1 types are `Int`, `Float`, `Bool`, `Char`, `String`, `List[T]`,
`T?` (optional), `()`, and your own structs and enums.

## TDS0202

Two declarations have the same name, or a declaration reuses a built-in
type name.

Functions, structs, enums and enum variants share one namespace in Phase 1,
so every name must be unique.

## TDS0203

A value has a different type than the context expects.

    fn greet(name: String) -> String = "hi {name}"
    greet(42)       // expects a String, got an Int

To turn any value into text, use `String(x)` or interpolation `"{x}"`.

## TDS0204

`Int` and `Float` are mixed without an explicit conversion.

TDS never converts between number types on its own, because conversions
can lose information. Convert explicitly:

    let half = Float(count) / 2.0
    let whole = Int(3.9)          // 3: truncates toward zero

For a literal, write it in the right form: `2.0` instead of `2`.

## TDS0205

A function or variant was called with the wrong number of arguments.

## TDS0206

The struct (or variant) has no field with this name.

The message lists the fields that do exist, or suggests a close match.

## TDS0207

A struct is created without giving a value for a field that has no default.

    struct Player:
        name: String
        health: Int
        level: Int = 1

    Player(name: "Alex")              // error: no health
    Player(name: "Alex", health: 10)  // fine: level defaults to 1

## TDS0208

The same field is given twice when creating a struct or variant.

## TDS0209

Struct fields must be named when creating a struct.

    Point(1, 2)          // error
    Point(x: 1, y: 2)    // fine

Naming fields keeps construction readable and safe when fields are
reordered. (Enum variants may use positional values.)

## TDS0210

This type has no method with this name.

Phase 1 has built-in methods on lists, strings and numbers, such as
`xs.push(x)`, `s.trim()` and `n.abs()`. User-defined methods (`impl`
blocks) arrive in Phase 3; until then, write a function that takes the
value as a parameter.

## TDS0211

Something that isn't a function is being called with `(...)`.

## TDS0212

An `if` or `while` condition must be a `Bool`.

TDS doesn't treat numbers, strings, lists or optionals as true or false.
Say what you mean:

    if count != 0:
    if not name.is_empty():
    if result != none:

## TDS0213

This operator doesn't work on these types.

Arithmetic needs two `Int`s or two `Float`s. `+` also joins two `String`s.
`and` and `or` need `Bool`s. To combine text and numbers, use interpolation:

    "Score: {score}"

## TDS0214

A unary operator is applied to the wrong type: `-` needs a number and
`not` needs a `Bool`.

## TDS0215

Indexing with `[...]` works on lists, with an `Int` index.

Strings can't be indexed yet. Use `.chars()` to get a list of characters.

## TDS0216

`for` loops work over lists, ranges and strings.

    for i in 0..<10:       // 0 to 9
    for i in 1...10:       // 1 to 10
    for item in items:
    for c in "text":

## TDS0217

A `match` must handle every possible value.

    match shape:
        Circle(r): ...
        // Rect and Point are missing

Add an arm for each missing case, or a final `_:` arm for everything else.
Matches on numbers and strings always need a `_` (or a name) arm.

## TDS0218

The pattern can't match the type of the value being matched.

A capitalized name in a pattern is an enum variant, and it must be a
variant of the enum being matched. Use a lowercase name to bind the value
to a variable instead.

## TDS0219

A variant pattern has the wrong number of fields.

    enum Shape:
        Rect(w: Float, h: Float)

    match s:
        Rect(w): ...       // error: Rect has 2 fields
        Rect(w, _): ...    // fine: `_` ignores a field

## TDS0220

An `if` without `else` is used as a value.

    let label = if score > 10: "high"

When the condition is false there would be no value. Add an `else` branch:

    let label = if score > 10: "high" else: "low"

## TDS0221

The type of an empty list or of `none` can't be worked out.

    var items = []

Add a type annotation:

    var items: List[Int] = []
    var best: Int? = none

## TDS0222

A value is needed, but this code doesn't produce one.

This usually means a function with a return type can reach its end
without returning. A block's value is its last expression, so either end
the function with an expression of the right type or `return` one on
every path.

## TDS0223

`break` and `continue` only work inside `while` and `for` loops.

## TDS0224

An optional operation is used on a value that isn't optional.

`x!` unwraps an optional, `x ?? default` supplies a default for `none`, and
only optional types like `Int?` can be `none`. If the value can never be
`none`, use it directly.

## TDS0225

Optionals can't be nested: `Int??` isn't allowed in Phase 1.

## TDS0226

Only `Int`, `Float`, `Char` and `String` can be ordered with `<`, `<=`, `>`
and `>=`.

## TDS0227

The program has no `main` function. A program starts running at:

    fn main():
        ...

## TDS0228

`main` must take no parameters and return nothing. Read command-line
arguments with the built-in `args()`.

## TDS0229

Two parameters of one function have the same name.

## TDS0230

This conversion isn't possible.

`Int(x)` converts from `Float` (truncating) and `Char` (giving the code
point). `Float(x)` converts from `Int`. To parse text, use
`text.to_int()` or `text.to_float()`, which return `none` if the text isn't
a number.

## TDS0231

Function arguments are passed by position, not by name.

    area(w: 2, h: 3)     // error
    area(2, 3)           // fine

Named arguments are for creating structs and enum variants.

## TDS0232

A struct or enum contains itself directly, so it would be infinitely large.

    struct Node:
        value: Int
        next: Node?        // a Node inside a Node inside a Node...

Store the children in a list, which keeps them on the heap:

    struct Node:
        value: Int
        children: List[Node]

`Box[T]` arrives in Phase 4.

## TDS0233

A type has the wrong number of type arguments: `List` needs exactly one
(`List[Int]`), and `Int` takes none.

## TDS0234

This type is part of TDS, but not of the Phase 1 prototype.

Phase 1 has one integer type, `Int` (64-bit, signed), and one float type,
`Float` (64-bit). Sized types like `Int32`, `UInt8` and `Float32`, and
library types like `Map`, `Result` and `Box`, arrive in later phases.

## TDS0235

This integer literal doesn't fit in `Int`, which goes up to
9223372036854775807.

## TDS0236

A variant is used with the wrong shape: a variant with fields used without
values (`Circle` instead of `Circle(1.0)`), or a variant without fields
called with parentheses (`Point()` instead of `Point`).

## TDS0237

A function or type name is used as a value.

Functions can't be stored in variables until Phase 4 (closures). To use a
function, call it: `double(2)`. To create a struct value, write
`Player(name: "Alex", health: 10)`.

## TDS0238

`return` doesn't match the function's return type.

A function without `-> Type` can't return a value, and a function with a
return type must return a value of that type.

## TDS0239

This match arm can never run, because an earlier arm already matches every
value it could match. This is a warning: the program still runs.

## TDS0400

A variable declared with `let` can't change.

    let score = 10
    score += 5          // error

Declare it with `var` if it needs to change:

    var score = 10

## TDS0401

A parameter without a mode is a read-only borrow: the function can look at
the value but not change it.

To change the caller's value, declare the parameter `mut` and mark the
argument at the call site:

    fn damage(player: mut Player, amount: Int):
        player.health -= amount

    damage(mut player, 25)

## TDS0402

A method that changes its receiver (like `push` or `pop`) is called on
something that can't change: a `let` variable, a read parameter, a loop
variable, or a temporary value. Store the value in a `var` first.

## TDS0403

The function declares this parameter `mut`, so the call must say `mut`
too.

    damage(player, 25)        // error
    damage(mut player, 25)    // fine

The marker makes it visible at the call site that the function may change
the variable.

## TDS0404

The function declares this parameter `take`, so the call must say `take`.

    save(player)              // error
    save(take player)         // fine: player moves into save
    save(take player.clone()) // fine: a copy moves in; player stays

After `take`, the variable can't be used until it gets a new value. (Copy
types like `Int` don't need the marker.)

## TDS0405

The argument's marker doesn't match the parameter's mode.

A read parameter takes no marker, a `mut` parameter needs `mut`, and a
`take` parameter needs `take`. Built-in functions only read their
arguments, and struct fields can't be `mut`.

## TDS0406

Only something stored in a variable can be lent with `mut`: a variable, a
field of one (`mut p.stats`) or a list element (`mut team[0]`). A
temporary value would be thrown away after the call, so the changes would
be lost.

## TDS0407

A value lent with `mut` must be allowed to change, but this variable can't
change: it was declared with `let`, or it's a read parameter or a loop
variable. Declare it with `var`, or make the parameter `mut`.

## TDS0408

`take` can only move a whole variable, not a field or list element, because
that would leave a hole in the struct or list.

Move a copy instead: `take p.name.clone()`.

## TDS0409

A read or `mut` parameter is only borrowed: the caller still owns the value,
so the function can't give it away.

To take ownership, declare the parameter `take`:

    fn relay(name: take String):
        keep(take name)

Or move a copy: `keep(take name.clone())`.

## TDS0410

A variable lent with `mut` to a call is also used by another argument of
the same call.

    copy_into(mut xs, xs)

While the function holds a `mut` borrow, it must be the only way to reach
the value. Make a copy first:

    let source = xs
    copy_into(mut xs, source)

Reading the variable inside another argument's expression, like
`sort(mut xs, len(xs))`, is fine: that read finishes before the call.

## TDS0411

A variable declared outside a loop is moved inside it.

    let message = "hi"
    for i in 0..<3:
        consume(take message)     // the 2nd iteration has nothing to move

Move a copy each time (`take message.clone()`), give the variable a new
value before the iteration ends, or `break` after moving it.

## TDS0412

A variable is used after its value was moved away with `take`.

    save(take player)
    print(player.health)          // player no longer holds a value

Either lend it for reading instead (`save(player)`, if `save` only needs
to read), or move a copy and keep the original
(`save(take player.clone())`), or give the variable a new value before
using it again.

## TDS0500

Integer arithmetic overflowed: the result doesn't fit in `Int`, which holds
values from -9223372036854775808 to 9223372036854775807.

The prototype behaves like a Development build: overflow is a panic, not a
silent wrap-around.

## TDS0501

An integer was divided by zero (with `/` or `%`). Check the divisor first.

## TDS0502

A list was indexed outside `0..<len`. Indexes start at 0, so the last
element of a list `xs` is `xs[len(xs) - 1]`.

## TDS0503

`x!` was used when `x` was `none`.

`!` says "I'm sure this has a value". If it might not, use a default with
`x ?? fallback`, or check first with `if x != none:`.

## TDS0504

The program made more than 10,000 nested calls. This is almost always a
recursive function that never reaches its base case.

## TDS0505

A number conversion failed because the value doesn't fit, like
`Int(1e300)`, or isn't a number (`NaN`).

## TDS0506

The program called `panic("...")`. Panics are for bugs: situations the
program can't continue from.

## TDS0599

The interpreter reached a state that the checker should have ruled out.
This is a bug in the TDS prototype, not in your program. Please report it,
with the program that caused it.
