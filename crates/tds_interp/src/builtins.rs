//! Built-in functions (`print`, `len`, conversions, ...) and built-in
//! methods on lists, strings and numbers.
//!
//! The type checker has the matching signatures in `tds_check::builtins`;
//! the two lists must be kept in sync.

use std::rc::Rc;

use tds_ast::{Arg, Expr, ExprKind, Ident};
use tds_diagnostics::{codes, Diagnostic, Span};

use crate::{internal, overflow, Exec, Flow, Interpreter, Value};

impl Interpreter<'_> {
    pub(crate) fn call_builtin(&mut self, name: &str, args: &[Arg], span: Span) -> Exec<Value> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(&arg.value)?);
        }
        let arg_span = |i: usize| args.get(i).map_or(span, |a| a.value.span);

        match (name, values.as_slice()) {
            ("print", []) => {
                let _ = writeln!(self.out);
                Ok(Value::Unit)
            }
            ("print", [v]) => {
                let _ = writeln!(self.out, "{v}");
                Ok(Value::Unit)
            }
            ("len", [Value::List(items)]) => Ok(Value::Int(items.len() as i64)),
            ("len", [Value::Str(s)]) => Ok(Value::Int(s.chars().count() as i64)),
            ("Int", [Value::Int(n)]) => Ok(Value::Int(*n)),
            ("Int", [Value::Char(c)]) => Ok(Value::Int(*c as i64)),
            ("Int", [Value::Float(x)]) => {
                // `as` saturates, so check the range first. Truncates toward zero.
                if x.is_finite() && *x >= -9.223_372_036_854_776e18 && *x < 9.223_372_036_854_776e18
                {
                    Ok(Value::Int(*x as i64))
                } else {
                    Err(Flow::Panic(Box::new(
                        Diagnostic::error(
                            codes::CONVERSION_FAILED,
                            format!("can't convert {x:?} to `Int`"),
                        )
                        .with_primary(arg_span(0), "out of range for `Int`")
                        .with_help("check the value first, or keep it as a `Float`"),
                    )))
                }
            }
            ("Float", [Value::Int(n)]) => Ok(Value::Float(*n as f64)),
            ("Float", [Value::Float(x)]) => Ok(Value::Float(*x)),
            ("String", [v]) => Ok(Value::str(&v.to_string())),
            ("read_file", [Value::Str(path)]) => Ok(match std::fs::read_to_string(&**path) {
                Ok(text) => Value::str(&text),
                Err(_) => Value::None,
            }),
            ("args", []) => Ok(Value::list(
                self.args.iter().map(|a| Value::str(a)).collect(),
            )),
            ("panic", [Value::Str(message)]) => Err(Flow::Panic(Box::new(
                Diagnostic::error(codes::EXPLICIT_PANIC, format!("panic: {message}"))
                    .with_primary(span, "panicked here"),
            ))),
            _ => internal(format!("unknown function `{name}`"), span),
        }
    }

    pub(crate) fn eval_method(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &[Arg],
        span: Span,
    ) -> Exec<Value> {
        // `Shape.Circle(1.0)` parses as a method call on the enum's name.
        if let ExprKind::Name(enum_name) = &receiver.kind {
            if self.lookup_slot(enum_name).is_none() {
                let info = self
                    .enums
                    .get(enum_name)
                    .and_then(|variants| variants.iter().find(|v| v.name == method.name))
                    .cloned();
                if let Some(info) = info {
                    return self.construct_variant(info, args);
                }
            }
        }

        // Methods that change a list work on the variable in place.
        if matches!(method.name.as_str(), "push" | "pop") {
            return self.list_mutation(receiver, method, args, span);
        }

        let target = self.eval(receiver)?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(&arg.value)?);
        }
        let name = method.name.as_str();
        if name == "clone" && values.is_empty() {
            // Values are copy-on-write, so a clone is just another handle.
            return Ok(target);
        }

        match (&target, name, values.as_slice()) {
            // ---- List ----
            (Value::List(items), "len", []) => Ok(Value::Int(items.len() as i64)),
            (Value::List(items), "is_empty", []) => Ok(Value::Bool(items.is_empty())),
            (Value::List(items), "contains", [v]) => {
                Ok(Value::Bool(items.iter().any(|x| x.equals(v))))
            }

            // ---- String ----
            (Value::Str(s), "len", []) => Ok(Value::Int(s.chars().count() as i64)),
            (Value::Str(s), "is_empty", []) => Ok(Value::Bool(s.is_empty())),
            (Value::Str(s), "contains", [Value::Str(t)]) => Ok(Value::Bool(s.contains(&**t))),
            (Value::Str(s), "starts_with", [Value::Str(t)]) => Ok(Value::Bool(s.starts_with(&**t))),
            (Value::Str(s), "ends_with", [Value::Str(t)]) => Ok(Value::Bool(s.ends_with(&**t))),
            (Value::Str(s), "trim", []) => Ok(Value::str(s.trim())),
            (Value::Str(s), "upper", []) => Ok(Value::str(&s.to_uppercase())),
            (Value::Str(s), "lower", []) => Ok(Value::str(&s.to_lowercase())),
            (Value::Str(s), "lines", []) => Ok(Value::list(s.lines().map(Value::str).collect())),
            (Value::Str(s), "split", [Value::Str(sep)]) => {
                if sep.is_empty() {
                    return internal("split on an empty separator", span);
                }
                Ok(Value::list(s.split(&**sep).map(Value::str).collect()))
            }
            (Value::Str(s), "chars", []) => Ok(Value::list(s.chars().map(Value::Char).collect())),
            (Value::Str(s), "to_int", []) => {
                Ok(s.trim().parse::<i64>().map_or(Value::None, Value::Int))
            }
            (Value::Str(s), "to_float", []) => {
                Ok(s.trim().parse::<f64>().map_or(Value::None, Value::Float))
            }

            // ---- Int ----
            (Value::Int(n), "abs", []) => n
                .checked_abs()
                .map(Value::Int)
                .map_or_else(|| overflow("abs", span), Ok),
            (Value::Int(n), "min", [Value::Int(m)]) => Ok(Value::Int(*n.min(m))),
            (Value::Int(n), "max", [Value::Int(m)]) => Ok(Value::Int(*n.max(m))),
            (Value::Int(n), "pow", [Value::Int(e)]) => {
                match u32::try_from(*e).ok().and_then(|e| n.checked_pow(e)) {
                    Some(v) => Ok(Value::Int(v)),
                    None if *e < 0 => internal("negative exponent", span),
                    None => overflow("pow", span),
                }
            }

            // ---- Float ----
            (Value::Float(x), "abs", []) => Ok(Value::Float(x.abs())),
            (Value::Float(x), "sqrt", []) => Ok(Value::Float(x.sqrt())),
            (Value::Float(x), "floor", []) => Ok(Value::Float(x.floor())),
            (Value::Float(x), "ceil", []) => Ok(Value::Float(x.ceil())),
            (Value::Float(x), "round", []) => Ok(Value::Float(x.round())),

            // ---- Char ----
            (Value::Char(c), "is_digit", []) => Ok(Value::Bool(c.is_ascii_digit())),
            (Value::Char(c), "is_alphabetic", []) => Ok(Value::Bool(c.is_alphabetic())),
            (Value::Char(c), "is_whitespace", []) => Ok(Value::Bool(c.is_whitespace())),

            _ => internal(format!("unknown method `{name}`"), method.span),
        }
    }

    /// `list.push(x)` and `list.pop()`: change the list where it lives.
    fn list_mutation(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &[Arg],
        span: Span,
    ) -> Exec<Value> {
        let pushed = match args.first() {
            Some(arg) => Some(self.eval_arg(arg)?),
            None => None,
        };
        if !is_place(receiver) {
            return internal(
                "`push` / `pop` on a value that isn't a variable",
                receiver.span,
            );
        }
        let place = self.place(receiver)?;
        let Value::List(items) = self.place_mut(&place)? else {
            return internal(
                format!("`{}` on a value that isn't a list", method.name),
                span,
            );
        };
        let items = Rc::make_mut(items);
        match (method.name.as_str(), pushed) {
            ("push", Some(v)) => {
                items.push(v);
                Ok(Value::Unit)
            }
            ("pop", None) => Ok(items.pop().unwrap_or(Value::None)),
            _ => internal("bad arguments to a list method", span),
        }
    }
}

fn is_place(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Name(_) => true,
        ExprKind::Field { expr, .. } | ExprKind::Index { expr, .. } => is_place(expr),
        _ => false,
    }
}
