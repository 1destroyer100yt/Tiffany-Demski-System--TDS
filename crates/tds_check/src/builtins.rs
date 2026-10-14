//! Signatures of built-in functions and methods. The interpreter
//! implements them in `tds_interp::builtins`; keep the two in sync.

use crate::types::Type;

/// The names of the built-in functions, for "did you mean" suggestions.
pub const FUNCTIONS: &[&str] = &[
    "print",
    "len",
    "Int",
    "Float",
    "String",
    "read_file",
    "args",
    "panic",
];

/// A built-in method's signature.
pub struct MethodSig {
    pub params: Vec<Type>,
    pub ret: Type,
    /// Whether the method changes its receiver (so the receiver must be a
    /// mutable variable).
    pub mutates: bool,
}

fn sig(params: Vec<Type>, ret: Type) -> Option<MethodSig> {
    Some(MethodSig {
        params,
        ret,
        mutates: false,
    })
}

/// Looks up `receiver.name(...)`.
pub fn method(receiver: &Type, name: &str) -> Option<MethodSig> {
    use Type::*;
    if name == "clone" {
        return sig(vec![], receiver.clone());
    }
    match (receiver, name) {
        (List(_), "len") => sig(vec![], Int),
        (List(_), "is_empty") => sig(vec![], Bool),
        (List(elem), "contains") => sig(vec![(**elem).clone()], Bool),
        (List(elem), "push") => Some(MethodSig {
            params: vec![(**elem).clone()],
            ret: Unit,
            mutates: true,
        }),
        (List(elem), "pop") => Some(MethodSig {
            params: vec![],
            ret: Type::optional((**elem).clone()),
            mutates: true,
        }),

        (String, "len") => sig(vec![], Int),
        (String, "is_empty") => sig(vec![], Bool),
        (String, "contains" | "starts_with" | "ends_with") => sig(vec![String], Bool),
        (String, "trim" | "upper" | "lower") => sig(vec![], String),
        (String, "lines") => sig(vec![], Type::list(String)),
        (String, "split") => sig(vec![String], Type::list(String)),
        (String, "chars") => sig(vec![], Type::list(Char)),
        (String, "to_int") => sig(vec![], Type::optional(Int)),
        (String, "to_float") => sig(vec![], Type::optional(Float)),

        (Int, "abs") => sig(vec![], Int),
        (Int, "min" | "max" | "pow") => sig(vec![Int], Int),

        (Float, "abs" | "sqrt" | "floor" | "ceil" | "round") => sig(vec![], Float),

        (Char, "is_digit" | "is_alphabetic" | "is_whitespace") => sig(vec![], Bool),

        _ => None,
    }
}

/// All method names available on a type, for "did you mean" suggestions.
pub fn method_names(receiver: &Type) -> Vec<&'static str> {
    const CANDIDATES: &[&str] = &[
        "len",
        "is_empty",
        "contains",
        "push",
        "pop",
        "starts_with",
        "ends_with",
        "trim",
        "upper",
        "lower",
        "lines",
        "split",
        "chars",
        "to_int",
        "to_float",
        "abs",
        "min",
        "max",
        "pow",
        "sqrt",
        "floor",
        "ceil",
        "round",
        "is_digit",
        "is_alphabetic",
        "is_whitespace",
        "clone",
    ];
    CANDIDATES
        .iter()
        .copied()
        .filter(|name| method(receiver, name).is_some())
        .collect()
}
