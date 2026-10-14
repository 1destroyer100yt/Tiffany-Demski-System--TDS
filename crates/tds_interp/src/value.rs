//! Runtime values.
//!
//! TDS has value semantics: `let b = a` gives `b` its own copy. To make that
//! cheap, compound values live behind `Rc` and are copied lazily: cloning a
//! `Value` only bumps a reference count, and a write goes through
//! `Rc::make_mut`, which copies the data only if someone else still shares it
//! (copy-on-write).

use std::cmp::Ordering;
use std::fmt::{self, Write};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(Rc<str>),
    List(Rc<Vec<Value>>),
    Struct(Rc<StructValue>),
    Variant(Rc<VariantValue>),
    /// The empty optional. A present optional is just the value itself.
    None,
    Range {
        start: i64,
        end: i64,
        inclusive: bool,
    },
    /// Left behind in a variable after `take`. Reading it is a bug that the
    /// checker rules out; the interpreter reports it as a backstop.
    Moved,
}

/// The layout of a struct type, shared by all its values.
#[derive(Debug)]
pub struct StructInfo {
    pub name: String,
    pub field_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StructValue {
    pub info: Rc<StructInfo>,
    pub fields: Vec<Value>,
}

/// The layout of one enum variant.
#[derive(Debug)]
pub struct VariantInfo {
    pub enum_name: String,
    pub name: String,
    /// `None` for positional fields like `Pair(Int, Int)`.
    pub field_names: Vec<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct VariantValue {
    pub info: Rc<VariantInfo>,
    pub fields: Vec<Value>,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(items))
    }

    /// Plain data that is copied rather than moved by `take`.
    pub fn is_copy(&self) -> bool {
        matches!(
            self,
            Value::Unit
                | Value::Int(_)
                | Value::Float(_)
                | Value::Bool(_)
                | Value::Char(_)
                | Value::None
                | Value::Range { .. }
        )
    }

    /// Structural equality. Phase 1 lets `==` compare any two values of the
    /// same type; Phase 3 will require the `Equal` interface.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) | (Value::None, Value::None) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Struct(a), Value::Struct(b)) => {
                Rc::ptr_eq(&a.info, &b.info)
                    && a.fields.iter().zip(&b.fields).all(|(x, y)| x.equals(y))
            }
            (Value::Variant(a), Value::Variant(b)) => {
                Rc::ptr_eq(&a.info, &b.info)
                    && a.fields.iter().zip(&b.fields).all(|(x, y)| x.equals(y))
            }
            (
                Value::Range {
                    start: s1,
                    end: e1,
                    inclusive: i1,
                },
                Value::Range {
                    start: s2,
                    end: e2,
                    inclusive: i2,
                },
            ) => s1 == s2 && e1 == e2 && i1 == i2,
            _ => false,
        }
    }

    /// Ordering for `<` and friends: numbers, chars and strings only.
    pub fn compare(&self, other: &Value) -> Option<Ordering> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Char(a), Value::Char(b)) => Some(a.cmp(b)),
            (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// Renders the value as `print` shows it. Strings and chars inside
    /// lists and structs are quoted; at the top level they are not.
    fn write(&self, out: &mut String, nested: bool) {
        match self {
            Value::Unit => out.push_str("()"),
            Value::Int(n) => write!(out, "{n}").unwrap(),
            Value::Float(x) => write!(out, "{x:?}").unwrap(),
            Value::Bool(b) => write!(out, "{b}").unwrap(),
            Value::Char(c) if nested => write!(out, "{c:?}").unwrap(),
            Value::Char(c) => out.push(*c),
            Value::Str(s) if nested => write!(out, "{:?}", &**s).unwrap(),
            Value::Str(s) => out.push_str(s),
            Value::None => out.push_str("none"),
            Value::Moved => out.push_str("<moved>"),
            Value::Range {
                start,
                end,
                inclusive,
            } => {
                let op = if *inclusive { "..." } else { "..<" };
                write!(out, "{start}{op}{end}").unwrap();
            }
            Value::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write(out, true);
                }
                out.push(']');
            }
            Value::Struct(s) => {
                out.push_str(&s.info.name);
                out.push('(');
                for (i, (name, value)) in s.info.field_names.iter().zip(&s.fields).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write!(out, "{name}: ").unwrap();
                    value.write(out, true);
                }
                out.push(')');
            }
            Value::Variant(v) => {
                out.push_str(&v.info.name);
                if v.fields.is_empty() {
                    return;
                }
                out.push('(');
                for (i, (name, value)) in v.info.field_names.iter().zip(&v.fields).enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if let Some(name) = name {
                        write!(out, "{name}: ").unwrap();
                    }
                    value.write(out, true);
                }
                out.push(')');
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        self.write(&mut out, false);
        f.write_str(&out)
    }
}
