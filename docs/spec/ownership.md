# Values and ownership

This chapter covers Level 1 of the TDS memory model: values, parameter
modes and moves. Stored references with lifetimes (Level 2) and `unsafe`
(Level 3) come in later phases.

## Value semantics

Every value has exactly one owner: a variable, a struct field or a list
slot. Assigning or passing a value normally gives the receiver its own
copy:

```tds
var a = [1, 2]
var b = a            // b is a separate list
b.push(3)
print(a)             // [1, 2]
```

Copies are cheap in the prototype (data is shared until one side changes
it). Phase 3's ownership checker may require `.clone()` or `take` in some
of these places; `.clone()` already works on every value.

## Mutability

- `let` variables can't change (`TDS0400`); `var` variables can.
- Loop variables and names bound by `match` patterns can't change.
- Parameters follow their mode (below).

Changing a field or list element counts as changing the variable that holds
it: `p.health -= 1` needs `p` to be changeable, and so does `xs.push(1)`.

## Parameter modes

A parameter is declared with one of three modes, and the call site must
say the same mode:

| Mode | Declared | Caller writes | Meaning |
|---|---|---|---|
| read | `p: Player` | `f(player)` | the function can look at the value but not change it |
| mut | `p: mut Player` | `f(mut player)` | the function can change the caller's value; the change is visible after the call |
| take | `p: take Player` | `f(take player)` | the value moves into the function; the caller can't use it afterwards |

```tds
fn damage(player: mut Player, amount: Int):
    player.health -= amount

fn main():
    var player = Player(name: "Alex", health: 100)
    damage(mut player, 25)
    print(player.health)      // 75
```

The rules:

- A read parameter can't be changed (`TDS0401`) or moved away (`TDS0409`).
- A `mut` argument needs `mut` at the call (`TDS0403`); a `take` argument
  needs `take` (`TDS0404`). Marking an argument that the parameter doesn't
  ask for is an error (`TDS0405`).
- A `mut` argument must be a variable, a field or a list element
  (`TDS0406`) that is allowed to change (`TDS0407`). Its type must match
  the parameter's exactly.
- A `take` parameter is owned by the function, which may change it.
- Copy types (`Int`, `Float`, `Bool`, `Char`, `()`, ranges, and optionals
  of these) are copied, not moved. A `take` parameter of a Copy type needs
  no marker, and the caller's variable stays usable.

## Moves

`take x` moves the value out of `x`. It can be used in arguments, in
struct and variant fields (`Wrapper(item: take item)`), with `push`
(`xs.push(take item)`), and on its own (`let b = take a`).

After a move, the variable can't be used (`TDS0412`) until it is given a
new value:

```tds
save(take player)
print(player.health)          // error[TDS0412]: `player` was moved
player = Player(name: "Sam", health: 100)
print(player.health)          // fine again
```

The checker follows each path through the code:

- after `if`/`match`, a variable moved in any branch counts as moved;
- a branch that ends in `return`, `break`, `continue` or a panic doesn't
  count;
- the right side of `and`, `or` and `??` might not run, so a move there
  counts as "maybe moved", which is also an error to use afterwards;
- a variable declared outside a loop and moved inside it is an error
  (`TDS0411`), because the next iteration would use it, unless the
  iteration gives it a new value or `break`s first.

`take` only moves whole variables. Moving a field or list element would
leave a hole (`TDS0408`); move a copy instead with `take p.name.clone()`.

## Borrow conflicts

While a call holds a variable through a `mut` argument, the same variable
can't be another argument of that call (`TDS0410`):

```tds
copy_into(mut xs, xs)          // error
sort(mut xs, 0, len(xs) - 1)   // fine: len(xs) is finished before the call starts
```

## How it runs

The interpreter implements the modes like this: read arguments are copied,
take arguments are moved, and `mut` arguments are moved into the call and
written back when it returns. Because a `mut` borrow is exclusive, this
behaves exactly like passing a reference. The arguments of a call are
evaluated left to right, and `mut` places are taken last.
