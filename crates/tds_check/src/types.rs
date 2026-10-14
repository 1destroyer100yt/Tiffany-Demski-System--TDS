//! The types the checker reasons about.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Char,
    String,
    Unit,
    List(Box<Type>),
    Optional(Box<Type>),
    /// The value of `a..<b` or `a...b`.
    Range,
    Struct(String),
    Enum(String),
    /// The type of expressions that never produce a value: `return`,
    /// `break`, `continue`, `panic(...)`. It fits wherever a value is
    /// expected.
    Never,
    /// Stands in for a type that couldn't be worked out because of an
    /// earlier error. It fits everywhere, so one mistake doesn't cascade.
    Error,
}

impl Type {
    pub fn list(elem: Type) -> Type {
        Type::List(Box::new(elem))
    }

    pub fn optional(inner: Type) -> Type {
        Type::Optional(Box::new(inner))
    }

    /// Types that are copied instead of moved, even by `take`.
    pub fn is_copy(&self) -> bool {
        match self {
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::Char
            | Type::Unit
            | Type::Range
            | Type::Never
            | Type::Error => true,
            Type::Optional(inner) => inner.is_copy(),
            _ => false,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }

    /// Whether a value of type `self` can be used where `want` is expected.
    /// The only implicit conversion is wrapping a value into an optional:
    /// an `Int` fits where an `Int?` is expected.
    pub fn fits(&self, want: &Type) -> bool {
        match (self, want) {
            (Type::Error | Type::Never, _) | (_, Type::Error) => true,
            (Type::List(a), Type::List(b)) => a.fits_exactly(b),
            (Type::Optional(a), Type::Optional(b)) => a.fits_exactly(b),
            (a, Type::Optional(b)) => a.fits_exactly(b),
            (a, b) => a == b,
        }
    }

    /// Like [`Type::fits`] but without wrapping into optionals: element
    /// types of lists must match exactly.
    fn fits_exactly(&self, want: &Type) -> bool {
        match (self, want) {
            (Type::Error | Type::Never, _) | (_, Type::Error) => true,
            (Type::List(a), Type::List(b)) | (Type::Optional(a), Type::Optional(b)) => {
                a.fits_exactly(b)
            }
            (a, b) => a == b,
        }
    }

    /// Strips one level of optional: `Int?` → `Int`.
    pub fn unwrap_optional(&self) -> Option<&Type> {
        match self {
            Type::Optional(inner) => Some(inner),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => f.write_str("Int"),
            Type::Float => f.write_str("Float"),
            Type::Bool => f.write_str("Bool"),
            Type::Char => f.write_str("Char"),
            Type::String => f.write_str("String"),
            Type::Unit => f.write_str("()"),
            Type::List(elem) => write!(f, "List[{elem}]"),
            Type::Optional(inner) => write!(f, "{inner}?"),
            Type::Range => f.write_str("Range"),
            Type::Struct(name) | Type::Enum(name) => f.write_str(name),
            Type::Never => f.write_str("Never"),
            Type::Error => f.write_str("{unknown}"),
        }
    }
}
