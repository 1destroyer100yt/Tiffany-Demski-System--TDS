//! The TDS tree-walking interpreter.
//!
//! # Where this sits in the pipeline
//!
//! In Phase 1 the interpreter walks the AST directly, after the type
//! checker has accepted it. That's the seam for Phase 2: name resolution and
//! lowering will produce HIR and then TIR (a control-flow graph with
//! explicit moves, borrows and drops), and this interpreter will be rewritten
//! to execute TIR. The public API (`Interpreter::new`, `load_items`,
//! `run_main`, `run_stmt`) is meant to survive that change.
//!
//! # Values and parameter modes
//!
//! See [`value`] for how values are stored. Parameter modes are implemented
//! like this:
//!
//! * read: the callee gets a copy (an `Rc` clone, so it's cheap).
//! * take: the value is moved out of the caller's variable, which is left
//!   holding [`Value::Moved`].
//! * mut: the value is moved out of the caller's place for the duration of
//!   the call and written back when the call returns ("copy-in/copy-out").
//!   Because a `mut` borrow is exclusive, nobody can observe the difference
//!   from a real reference.

mod builtins;
pub mod value;

use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use tds_ast::*;
use tds_diagnostics::{codes, Diagnostic, Span};

pub use value::Value;
use value::{StructInfo, StructValue, VariantInfo, VariantValue};

/// Maximum call depth before reporting a stack overflow.
pub const MAX_CALL_DEPTH: usize = 10_000;

/// Native stack size the interpreter should run on so that
/// [`MAX_CALL_DEPTH`] nested calls fit. Callers run the interpreter on a
/// thread of this size.
pub const STACK_SIZE: usize = 1 << 30;

/// Why evaluation stopped early.
enum Flow {
    Break,
    Continue,
    Return(Value),
    /// A runtime panic.
    Panic(Box<Diagnostic>),
}

type Exec<T> = Result<T, Flow>;

fn internal<T>(message: impl Into<String>, span: Span) -> Exec<T> {
    Err(Flow::Panic(Box::new(
        Diagnostic::error(
            codes::INTERNAL_ERROR,
            format!("internal interpreter error: {}", message.into()),
        )
        .with_primary(span, "here")
        .with_note("the type checker should have rejected this program; this is a compiler bug"),
    )))
}

/// The variables of one function call. Scopes nest by truncating the list
/// back to a saved length when a block ends.
#[derive(Default)]
struct Frame {
    vars: Vec<(Rc<str>, Value)>,
}

/// A location that can be assigned to or lent with `mut`:
/// a variable followed by field and index steps.
struct Place {
    root: String,
    root_span: Span,
    steps: Vec<Step>,
}

enum Step {
    Field(String, Span),
    Index(i64, Span),
}

pub struct Interpreter<'out> {
    out: &'out mut dyn Write,
    args: Vec<String>,
    fns: HashMap<String, Rc<FnDecl>>,
    structs: HashMap<String, (Rc<StructInfo>, Rc<StructDecl>)>,
    /// Variant name → layout. Variant names are global in Phase 1.
    variants: HashMap<String, Rc<VariantInfo>>,
    /// Enum name → its variants, for `Shape.Circle`.
    enums: HashMap<String, Vec<Rc<VariantInfo>>>,
    /// Call frames. Frame 0 holds the REPL's top-level variables.
    frames: Vec<Frame>,
}

impl<'out> Interpreter<'out> {
    /// `args` are the program's command-line arguments, returned by `args()`.
    pub fn new(out: &'out mut dyn Write, args: Vec<String>) -> Interpreter<'out> {
        Interpreter {
            out,
            args,
            fns: HashMap::new(),
            structs: HashMap::new(),
            variants: HashMap::new(),
            enums: HashMap::new(),
            frames: vec![Frame::default()],
        }
    }

    /// Registers functions, structs and enums. Later definitions replace
    /// earlier ones with the same name (useful in the REPL).
    pub fn load_items<'a>(&mut self, items: impl IntoIterator<Item = &'a Item>) {
        for item in items {
            match item {
                Item::Fn(f) => {
                    self.fns.insert(f.name.name.clone(), Rc::new(f.clone()));
                }
                Item::Struct(s) => {
                    let info = Rc::new(StructInfo {
                        name: s.name.name.clone(),
                        field_names: s.fields.iter().map(|f| f.name.name.clone()).collect(),
                    });
                    self.structs
                        .insert(s.name.name.clone(), (info, Rc::new(s.clone())));
                }
                Item::Enum(e) => {
                    let mut infos = Vec::new();
                    for v in &e.variants {
                        let info = Rc::new(VariantInfo {
                            enum_name: e.name.name.clone(),
                            name: v.name.name.clone(),
                            field_names: v
                                .fields
                                .iter()
                                .map(|f| f.name.as_ref().map(|n| n.name.clone()))
                                .collect(),
                        });
                        self.variants.insert(v.name.name.clone(), info.clone());
                        infos.push(info);
                    }
                    self.enums.insert(e.name.name.clone(), infos);
                }
            }
        }
    }

    /// Calls `fn main()`.
    pub fn run_main(&mut self, span: Span) -> Result<(), Diagnostic> {
        let Some(main) = self.fns.get("main").cloned() else {
            return Err(
                Diagnostic::error(codes::INTERNAL_ERROR, "no `main` function")
                    .with_primary(span, "here"),
            );
        };
        match self.call_fn(&main, &[], span) {
            Ok(_) => Ok(()),
            Err(Flow::Panic(diag)) => Err(*diag),
            Err(_) => Err(Diagnostic::error(
                codes::INTERNAL_ERROR,
                "`break` or `continue` escaped a function",
            )),
        }
    }

    /// Runs one top-level REPL statement in the REPL's own scope. Returns
    /// the value of an expression statement, if it isn't `()`.
    pub fn run_stmt(&mut self, stmt: &Stmt) -> Result<Option<Value>, Diagnostic> {
        let result = match &stmt.kind {
            StmtKind::Expr(e) => self.eval(e).map(Some),
            _ => self.exec(stmt).map(|_| None),
        };
        match result {
            Ok(Some(Value::Unit)) => Ok(None),
            Ok(v) => Ok(v),
            Err(Flow::Panic(diag)) => Err(*diag),
            Err(_) => Err(Diagnostic::error(
                codes::INTERNAL_ERROR,
                "`break`, `continue` or `return` outside a function",
            )
            .with_primary(stmt.span, "here")),
        }
    }

    // ------------------------------------------------------------------
    // Variables
    // ------------------------------------------------------------------

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().unwrap()
    }

    fn define(&mut self, name: &str, value: Value) {
        self.frame().vars.push((Rc::from(name), value));
    }

    fn lookup_slot(&mut self, name: &str) -> Option<&mut Value> {
        self.frame()
            .vars
            .iter_mut()
            .rev()
            .find(|(n, _)| &**n == name)
            .map(|(_, v)| v)
    }

    /// Runs `f` in a nested scope: variables it defines are dropped after.
    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> Exec<T>) -> Exec<T> {
        let mark = self.frame().vars.len();
        let result = f(self);
        self.frame().vars.truncate(mark);
        result
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn exec(&mut self, stmt: &Stmt) -> Exec<()> {
        match &stmt.kind {
            StmtKind::Let { name, init, .. } => {
                let value = self.eval(init)?;
                self.define(&name.name, value);
            }
            StmtKind::Expr(e) => {
                self.eval(e)?;
            }
            StmtKind::Assign { target, op, value } => self.assign(target, *op, value)?,
            StmtKind::While { cond, body } => loop {
                if !self.eval_bool(cond)? {
                    break;
                }
                match self.scoped(|i| i.exec_block(body)) {
                    Ok(()) | Err(Flow::Continue) => {}
                    Err(Flow::Break) => break,
                    Err(other) => return Err(other),
                }
            },
            StmtKind::For { var, iter, body } => self.exec_for(var, iter, body)?,
            StmtKind::Return(value) => {
                let value = match value {
                    Some(e) => self.eval(e)?,
                    None => Value::Unit,
                };
                return Err(Flow::Return(value));
            }
            StmtKind::Break => return Err(Flow::Break),
            StmtKind::Continue => return Err(Flow::Continue),
        }
        Ok(())
    }

    fn exec_block(&mut self, block: &Block) -> Exec<()> {
        for stmt in &block.stmts {
            self.exec(stmt)?;
        }
        Ok(())
    }

    fn exec_for(&mut self, var: &Ident, iter: &Expr, body: &Block) -> Exec<()> {
        // Each kind of iterable becomes a stream of values. The list is a
        // cheap snapshot, so changing it inside the loop can't break
        // iteration.
        let iterable = self.eval(iter)?;
        let run = |interp: &mut Self, item: Value| -> Exec<bool> {
            let result = interp.scoped(|i| {
                i.define(&var.name, item);
                i.exec_block(body)
            });
            match result {
                Ok(()) | Err(Flow::Continue) => Ok(true),
                Err(Flow::Break) => Ok(false),
                Err(other) => Err(other),
            }
        };
        match iterable {
            Value::List(items) => {
                for item in items.iter() {
                    if !run(self, item.clone())? {
                        break;
                    }
                }
            }
            Value::Range {
                start,
                end,
                inclusive,
            } => {
                let end = i128::from(end) + i128::from(inclusive);
                let mut i = i128::from(start);
                while i < end {
                    if !run(self, Value::Int(i as i64))? {
                        break;
                    }
                    i += 1;
                }
            }
            Value::Str(s) => {
                for c in s.chars() {
                    if !run(self, Value::Char(c))? {
                        break;
                    }
                }
            }
            _ => return internal("`for` over a value that isn't iterable", iter.span),
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr, op: AssignOp, value: &Expr) -> Exec<()> {
        let rhs = self.eval(value)?;
        let place = self.place(target)?;
        let new_value = match op.binary_op() {
            None => rhs,
            Some(bin) => {
                let current = self.place_mut(&place)?.clone();
                self.binary_values(bin, current, rhs, target.span.to(value.span))?
            }
        };
        *self.place_mut(&place)? = new_value;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Places
    // ------------------------------------------------------------------

    /// Evaluates an assignable expression to a [`Place`]. Index
    /// expressions are evaluated now, so the place is stable.
    fn place(&mut self, e: &Expr) -> Exec<Place> {
        match &e.kind {
            ExprKind::Name(name) => Ok(Place {
                root: name.clone(),
                root_span: e.span,
                steps: Vec::new(),
            }),
            ExprKind::Field { expr, field } => {
                let mut place = self.place(expr)?;
                place
                    .steps
                    .push(Step::Field(field.name.clone(), field.span));
                Ok(place)
            }
            ExprKind::Index { expr, index } => {
                let mut place = self.place(expr)?;
                let i = self.eval_int(index)?;
                place.steps.push(Step::Index(i, index.span));
                Ok(place)
            }
            _ => internal("expression is not a place", e.span),
        }
    }

    /// Follows a place to the value it names, copying shared data on the
    /// way down (copy-on-write) so the value can be changed in place.
    fn place_mut(&mut self, place: &Place) -> Exec<&mut Value> {
        let Some(mut slot) = self.lookup_slot(&place.root) else {
            return internal(
                format!("unknown variable `{}`", place.root),
                place.root_span,
            );
        };
        for step in &place.steps {
            slot = match (slot, step) {
                (Value::Struct(s), Step::Field(name, span)) => {
                    let s = Rc::make_mut(s);
                    match s.info.field_names.iter().position(|f| f == name) {
                        Some(i) => &mut s.fields[i],
                        None => return internal(format!("no field `{name}`"), *span),
                    }
                }
                (Value::List(items), Step::Index(i, span)) => {
                    let items = Rc::make_mut(items);
                    let len = items.len();
                    match usize::try_from(*i).ok().filter(|&i| i < len) {
                        Some(i) => &mut items[i],
                        None => return index_out_of_bounds(*i, len, *span),
                    }
                }
                (Value::Moved, _) => return use_after_move(&place.root, place.root_span),
                (_, step) => {
                    let span = match step {
                        Step::Field(_, s) | Step::Index(_, s) => *s,
                    };
                    return internal("invalid place", span);
                }
            };
        }
        Ok(slot)
    }

    /// Takes the value out of a place, leaving `Moved` behind.
    fn take_from_place(&mut self, place: &Place) -> Exec<Value> {
        let slot = self.place_mut(place)?;
        if matches!(slot, Value::Moved) {
            return use_after_move(&place.root, place.root_span);
        }
        Ok(std::mem::replace(slot, Value::Moved))
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn eval_bool(&mut self, e: &Expr) -> Exec<bool> {
        match self.eval(e)? {
            Value::Bool(b) => Ok(b),
            _ => internal("expected a Bool", e.span),
        }
    }

    fn eval_int(&mut self, e: &Expr) -> Exec<i64> {
        match self.eval(e)? {
            Value::Int(n) => Ok(n),
            _ => internal("expected an Int", e.span),
        }
    }

    fn eval(&mut self, e: &Expr) -> Exec<Value> {
        match &e.kind {
            ExprKind::Int(n) => match i64::try_from(*n) {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => internal("integer literal out of range", e.span),
            },
            ExprKind::Float(x) => Ok(Value::Float(*x)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Char(c) => Ok(Value::Char(*c)),
            ExprKind::None => Ok(Value::None),
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Str(segments) => {
                let mut text = String::new();
                for segment in segments {
                    match segment {
                        StrSegment::Text(t) => text.push_str(t),
                        StrSegment::Expr(e) => text.push_str(&self.eval(e)?.to_string()),
                    }
                }
                Ok(Value::str(&text))
            }
            ExprKind::Name(name) => self.eval_name(name, e.span),
            ExprKind::List(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item)?);
                }
                Ok(Value::list(values))
            }
            ExprKind::Unary { op, expr } => self.eval_unary(*op, expr, e.span),
            ExprKind::Binary { op, lhs, rhs } => self.eval_binary(*op, lhs, rhs, e.span),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => Ok(Value::Range {
                start: self.eval_int(start)?,
                end: self.eval_int(end)?,
                inclusive: *inclusive,
            }),
            ExprKind::Call { callee, args } => self.eval_call(callee, args, e.span),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => self.eval_method(receiver, method, args, e.span),
            ExprKind::Field { expr, field } => self.eval_field(expr, field),
            ExprKind::Index { expr, index } => {
                let list = self.eval(expr)?;
                let i = self.eval_int(index)?;
                match list {
                    Value::List(items) => {
                        match usize::try_from(i).ok().and_then(|i| items.get(i)) {
                            Some(v) => Ok(v.clone()),
                            None => index_out_of_bounds(i, items.len(), index.span),
                        }
                    }
                    _ => internal("indexing a value that isn't a list", e.span),
                }
            }
            ExprKind::Unwrap(inner) => match self.eval(inner)? {
                Value::None => Err(Flow::Panic(Box::new(
                    Diagnostic::error(codes::UNWRAP_NONE, "unwrapped `none`")
                        .with_primary(e.span, "this value was `none`")
                        .with_help("use `?? default` to supply a fallback value instead"),
                ))),
                v => Ok(v),
            },
            ExprKind::Take(inner) => self.eval_take(inner),
            ExprKind::If { cond, then, else_ } => {
                if self.eval_bool(cond)? {
                    self.eval_block(then)
                } else {
                    match else_ {
                        Some(else_) => self.eval(else_),
                        None => Ok(Value::Unit),
                    }
                }
            }
            ExprKind::Match { scrutinee, arms } => self.eval_match(scrutinee, arms, e.span),
            ExprKind::Block(block) => self.eval_block(block),
        }
    }

    /// Evaluates a block in its own scope; its value is the last
    /// expression statement, or `()`.
    fn eval_block(&mut self, block: &Block) -> Exec<Value> {
        self.scoped(|i| {
            let Some((last, rest)) = block.stmts.split_last() else {
                return Ok(Value::Unit);
            };
            for stmt in rest {
                i.exec(stmt)?;
            }
            match &last.kind {
                StmtKind::Expr(e) => i.eval(e),
                _ => {
                    i.exec(last)?;
                    Ok(Value::Unit)
                }
            }
        })
    }

    fn eval_name(&mut self, name: &str, span: Span) -> Exec<Value> {
        if let Some(slot) = self.lookup_slot(name) {
            if matches!(slot, Value::Moved) {
                return use_after_move(name, span);
            }
            return Ok(slot.clone());
        }
        if let Some(info) = self.variants.get(name) {
            if info.field_names.is_empty() {
                return Ok(Value::Variant(Rc::new(VariantValue {
                    info: info.clone(),
                    fields: Vec::new(),
                })));
            }
        }
        internal(format!("unknown name `{name}`"), span)
    }

    /// `take x`: moves out of a variable. Copy values are just copied.
    fn eval_take(&mut self, e: &Expr) -> Exec<Value> {
        if let ExprKind::Name(name) = &e.kind {
            if let Some(slot) = self.lookup_slot(name) {
                if slot.is_copy() {
                    return Ok(slot.clone());
                }
                let place = self.place(e)?;
                return self.take_from_place(&place);
            }
        }
        self.eval(e)
    }

    fn eval_field(&mut self, expr: &Expr, field: &Ident) -> Exec<Value> {
        // `Shape.Point` or `Shape.Circle` (the latter only as a callee).
        if let ExprKind::Name(enum_name) = &expr.kind {
            if self.lookup_slot(enum_name).is_none() {
                if let Some(variants) = self.enums.get(enum_name) {
                    if let Some(info) = variants.iter().find(|v| v.name == field.name) {
                        return Ok(Value::Variant(Rc::new(VariantValue {
                            info: info.clone(),
                            fields: Vec::new(),
                        })));
                    }
                }
            }
        }
        match self.eval(expr)? {
            Value::Struct(s) => match s.info.field_names.iter().position(|f| *f == field.name) {
                Some(i) => Ok(s.fields[i].clone()),
                None => internal(format!("no field `{}`", field.name), field.span),
            },
            _ => internal(format!("no field `{}`", field.name), field.span),
        }
    }

    fn eval_unary(&mut self, op: UnaryOp, operand: &Expr, span: Span) -> Exec<Value> {
        // `-9223372036854775808` is a valid Int even though the literal on
        // its own isn't.
        if let (UnaryOp::Neg, ExprKind::Int(n)) = (op, &operand.kind) {
            return match 0i64.checked_sub_unsigned(*n) {
                Some(v) => Ok(Value::Int(v)),
                None => internal("integer literal out of range", span),
            };
        }
        match (op, self.eval(operand)?) {
            (UnaryOp::Neg, Value::Int(n)) => match n.checked_neg() {
                Some(v) => Ok(Value::Int(v)),
                None => overflow("negation", span),
            },
            (UnaryOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => internal("bad operand for unary operator", span),
        }
    }

    fn eval_binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Exec<Value> {
        // Short-circuiting operators evaluate the right side only if needed.
        match op {
            BinaryOp::And => return Ok(Value::Bool(self.eval_bool(lhs)? && self.eval_bool(rhs)?)),
            BinaryOp::Or => return Ok(Value::Bool(self.eval_bool(lhs)? || self.eval_bool(rhs)?)),
            BinaryOp::Coalesce => {
                return match self.eval(lhs)? {
                    Value::None => self.eval(rhs),
                    v => Ok(v),
                };
            }
            _ => {}
        }
        let a = self.eval(lhs)?;
        let b = self.eval(rhs)?;
        self.binary_values(op, a, b, span)
    }

    fn binary_values(&mut self, op: BinaryOp, a: Value, b: Value, span: Span) -> Exec<Value> {
        use BinaryOp::*;
        match op {
            Eq => return Ok(Value::Bool(a.equals(&b))),
            Ne => return Ok(Value::Bool(!a.equals(&b))),
            Lt | Le | Gt | Ge => {
                let Some(ord) = a.compare(&b) else {
                    // NaN compares false with everything.
                    if matches!((&a, &b), (Value::Float(_), Value::Float(_))) {
                        return Ok(Value::Bool(false));
                    }
                    return internal("values can't be ordered", span);
                };
                return Ok(Value::Bool(match op {
                    Lt => ord.is_lt(),
                    Le => ord.is_le(),
                    Gt => ord.is_gt(),
                    _ => ord.is_ge(),
                }));
            }
            _ => {}
        }
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                let result = match op {
                    Add => x.checked_add(y),
                    Sub => x.checked_sub(y),
                    Mul => x.checked_mul(y),
                    Div | Rem if y == 0 => {
                        return Err(Flow::Panic(Box::new(
                            Diagnostic::error(codes::DIVISION_BY_ZERO, "division by zero")
                                .with_primary(span, "the divisor is zero"),
                        )));
                    }
                    Div => x.checked_div(y),
                    Rem => x.checked_rem(y),
                    _ => return internal("bad integer operator", span),
                };
                match result {
                    Some(v) => Ok(Value::Int(v)),
                    None => overflow(op.symbol(), span),
                }
            }
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                Rem => x % y,
                _ => return internal("bad float operator", span),
            })),
            (Value::Str(x), Value::Str(y)) if op == Add => Ok(Value::str(&format!("{x}{y}"))),
            _ => internal(format!("bad operands for `{}`", op.symbol()), span),
        }
    }

    fn eval_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], span: Span) -> Exec<Value> {
        let value = self.eval(scrutinee)?;
        for arm in arms {
            let mut bindings = Vec::new();
            if self.match_pattern(&arm.pattern, &value, &mut bindings) {
                return self.scoped(|i| {
                    for (name, v) in bindings {
                        i.define(&name, v);
                    }
                    i.eval_block(&arm.body)
                });
            }
        }
        internal("no match arm matched", span)
    }

    fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Value,
        bindings: &mut Vec<(String, Value)>,
    ) -> bool {
        match (&pattern.kind, value) {
            (PatternKind::Wildcard, _) => true,
            (PatternKind::Name(name), Value::Variant(v))
                if self.is_variant_of(name, &v.info.enum_name) =>
            {
                v.info.name == *name
            }
            (PatternKind::Name(name), _) => {
                bindings.push((name.clone(), value.clone()));
                true
            }
            (PatternKind::Variant { name, args }, Value::Variant(v)) => {
                v.info.name == name.name
                    && args.len() == v.fields.len()
                    && args
                        .iter()
                        .zip(&v.fields)
                        .all(|(p, f)| self.match_pattern(p, f, bindings))
            }
            (PatternKind::Int(n), Value::Int(m)) => n == m,
            (PatternKind::Str(s), Value::Str(t)) => **s == **t,
            (PatternKind::Bool(a), Value::Bool(b)) => a == b,
            (PatternKind::Char(a), Value::Char(b)) => a == b,
            _ => false,
        }
    }

    fn is_variant_of(&self, name: &str, enum_name: &str) -> bool {
        self.enums
            .get(enum_name)
            .is_some_and(|variants| variants.iter().any(|v| v.name == name))
    }

    // ------------------------------------------------------------------
    // Calls
    // ------------------------------------------------------------------

    fn eval_call(&mut self, callee: &Expr, args: &[Arg], span: Span) -> Exec<Value> {
        match &callee.kind {
            ExprKind::Name(name) if self.lookup_slot(name).is_none() => {
                if let Some(f) = self.fns.get(name).cloned() {
                    return self.call_fn(&f, args, span);
                }
                if let Some((info, decl)) = self.structs.get(name).cloned() {
                    return self.construct_struct(info, &decl, args);
                }
                if let Some(info) = self.variants.get(name).cloned() {
                    return self.construct_variant(info, args);
                }
                self.call_builtin(name, args, span)
            }
            ExprKind::Field { expr, field } => {
                if let ExprKind::Name(enum_name) = &expr.kind {
                    let info = self
                        .enums
                        .get(enum_name)
                        .and_then(|vs| vs.iter().find(|v| v.name == field.name))
                        .cloned();
                    if let Some(info) = info {
                        return self.construct_variant(info, args);
                    }
                }
                internal("calling a value that isn't a function", callee.span)
            }
            _ => internal("calling a value that isn't a function", callee.span),
        }
    }

    /// Calls a user function. `mut` arguments are written back to their
    /// places when the call returns.
    fn call_fn(&mut self, f: &FnDecl, args: &[Arg], span: Span) -> Exec<Value> {
        // Two phases, like Rust's two-phase borrows: first evaluate the
        // read and take arguments, then take the `mut` places just before
        // the call. That way `sort(mut xs, len(xs))` works: the read of `xs`
        // is finished before the exclusive borrow starts.
        let mut values: Vec<Option<Value>> = Vec::with_capacity(args.len());
        for (param, arg) in f.params.iter().zip(args) {
            values.push(match param.mode {
                Mode::Read => Some(self.eval(&arg.value)?),
                Mode::Take => Some(self.eval_take(&arg.value)?),
                Mode::Mut => None,
            });
        }
        let mut write_back = Vec::new();
        for (index, (param, arg)) in f.params.iter().zip(args).enumerate() {
            if param.mode == Mode::Mut {
                let place = self.place(&arg.value)?;
                values[index] = Some(self.take_from_place(&place)?);
                write_back.push((index, place));
            }
        }

        if self.frames.len() > MAX_CALL_DEPTH {
            return Err(Flow::Panic(Box::new(
                Diagnostic::error(codes::STACK_OVERFLOW, "stack overflow")
                    .with_primary(span, format!("more than {MAX_CALL_DEPTH} nested calls"))
                    .with_note(
                        "this is usually a recursive function that never reaches its base case",
                    ),
            )));
        }

        let mut frame = Frame::default();
        for (param, value) in f.params.iter().zip(values) {
            let value = value.expect("every argument was evaluated");
            frame.vars.push((Rc::from(param.name.name.as_str()), value));
        }
        self.frames.push(frame);
        let result = self.eval_block(&f.body);
        let frame = self.frames.pop().unwrap();

        let value = match result {
            Ok(v) | Err(Flow::Return(v)) => v,
            Err(other) => return Err(other),
        };
        // Parameters are always the first variables of the frame.
        for (index, place) in write_back {
            let new_value = frame.vars[index].1.clone();
            *self.place_mut(&place)? = new_value;
        }
        Ok(value)
    }

    fn construct_struct(
        &mut self,
        info: Rc<StructInfo>,
        decl: &StructDecl,
        args: &[Arg],
    ) -> Exec<Value> {
        let mut fields: Vec<Option<Value>> = vec![None; decl.fields.len()];
        for (position, arg) in args.iter().enumerate() {
            let index = match &arg.label {
                Some(label) => match decl.fields.iter().position(|f| f.name.name == label.name) {
                    Some(i) => i,
                    None => return internal(format!("no field `{}`", label.name), label.span),
                },
                None => position,
            };
            fields[index] = Some(self.eval_arg(arg)?);
        }
        let mut values = Vec::with_capacity(fields.len());
        for (slot, field) in fields.into_iter().zip(&decl.fields) {
            let value = match (slot, &field.default) {
                (Some(v), _) => v,
                // Defaults are evaluated in an empty frame, so they can't
                // see the caller's variables.
                (None, Some(default)) => {
                    self.frames.push(Frame::default());
                    let v = self.eval(default);
                    self.frames.pop();
                    v?
                }
                (None, None) => {
                    return internal(format!("missing field `{}`", field.name.name), field.span)
                }
            };
            values.push(value);
        }
        Ok(Value::Struct(Rc::new(StructValue {
            info,
            fields: values,
        })))
    }

    fn construct_variant(&mut self, info: Rc<VariantInfo>, args: &[Arg]) -> Exec<Value> {
        let mut fields: Vec<Option<Value>> = vec![None; info.field_names.len()];
        for (position, arg) in args.iter().enumerate() {
            let index = match &arg.label {
                Some(label) => match info
                    .field_names
                    .iter()
                    .position(|f| f.as_deref() == Some(&label.name))
                {
                    Some(i) => i,
                    None => return internal(format!("no field `{}`", label.name), label.span),
                },
                None => position,
            };
            fields[index] = Some(self.eval_arg(arg)?);
        }
        let mut values = Vec::with_capacity(fields.len());
        for field in fields {
            match field {
                Some(v) => values.push(v),
                None => return internal("missing variant field", Span::empty(0)),
            }
        }
        Ok(Value::Variant(Rc::new(VariantValue {
            info,
            fields: values,
        })))
    }

    /// An argument stored into a struct or variant: `take` moves, anything
    /// else copies.
    fn eval_arg(&mut self, arg: &Arg) -> Exec<Value> {
        match arg.mode {
            Mode::Take => self.eval_take(&arg.value),
            _ => self.eval(&arg.value),
        }
    }
}

fn index_out_of_bounds<T>(index: i64, len: usize, span: Span) -> Exec<T> {
    Err(Flow::Panic(Box::new(
        Diagnostic::error(
            codes::INDEX_OUT_OF_BOUNDS,
            format!("index {index} is out of bounds"),
        )
        .with_primary(
            span,
            match len {
                0 => "the list is empty".to_string(),
                1 => "the list has 1 element".to_string(),
                n => format!("the list has {n} elements"),
            },
        )
        .with_note(if len == 0 {
            "an empty list has no valid indexes".to_string()
        } else {
            format!("valid indexes are 0..<{len}")
        }),
    )))
}

fn overflow<T>(what: &str, span: Span) -> Exec<T> {
    Err(Flow::Panic(Box::new(
        Diagnostic::error(
            codes::INTEGER_OVERFLOW,
            format!("integer overflow in `{what}`"),
        )
        .with_primary(span, "the result doesn't fit in `Int`")
        .with_note("`Int` holds values from -9223372036854775808 to 9223372036854775807"),
    )))
}

fn use_after_move<T>(name: &str, span: Span) -> Exec<T> {
    Err(Flow::Panic(Box::new(
        Diagnostic::error(
            codes::INTERNAL_ERROR,
            format!("`{name}` was used after being moved"),
        )
        .with_primary(span, "value was moved")
        .with_note("the ownership checker should have caught this; this is a compiler bug"),
    )))
}
