//! Walking every expression in a module.

use crate::*;

/// Calls `f` on every expression in the module, outer expressions before
/// the expressions inside them. Includes struct field defaults.
pub fn for_each_expr_mut(module: &mut Module, f: &mut impl FnMut(&mut Expr)) {
    for item in &mut module.items {
        match item {
            Item::Fn(func) => block(&mut func.body, f),
            Item::Struct(s) => {
                for field in &mut s.fields {
                    if let Some(default) = &mut field.default {
                        expr(default, f);
                    }
                }
            }
            Item::Enum(_) => {}
        }
    }
}

/// Gives every expression in the module a unique id, in source order.
pub fn number_exprs(module: &mut Module) -> u32 {
    let mut next = 0;
    for_each_expr_mut(module, &mut |e| {
        e.id = ExprId(next);
        next += 1;
    });
    next
}

fn block(b: &mut Block, f: &mut impl FnMut(&mut Expr)) {
    for s in &mut b.stmts {
        stmt(s, f);
    }
}

fn stmt(s: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
    match &mut s.kind {
        StmtKind::Let { init, .. } => expr(init, f),
        StmtKind::Assign { target, value, .. } => {
            expr(target, f);
            expr(value, f);
        }
        StmtKind::Expr(e) => expr(e, f),
        StmtKind::While { cond, body } => {
            expr(cond, f);
            block(body, f);
        }
        StmtKind::For { iter, body, .. } => {
            expr(iter, f);
            block(body, f);
        }
        StmtKind::Return(Some(e)) => expr(e, f),
        StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
    }
}

fn expr(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    f(e);
    match &mut e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::None
        | ExprKind::Unit
        | ExprKind::Name(_) => {}
        ExprKind::Str(segments) => {
            for segment in segments {
                if let StrSegment::Expr(inner) = segment {
                    expr(inner, f);
                }
            }
        }
        ExprKind::List(items) => {
            for item in items {
                expr(item, f);
            }
        }
        ExprKind::Unary { expr: inner, .. } | ExprKind::Unwrap(inner) | ExprKind::Take(inner) => {
            expr(inner, f)
        }
        ExprKind::Field { expr: inner, .. } => expr(inner, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr(lhs, f);
            expr(rhs, f);
        }
        ExprKind::Range { start, end, .. } => {
            expr(start, f);
            expr(end, f);
        }
        ExprKind::Index { expr: inner, index } => {
            expr(inner, f);
            expr(index, f);
        }
        ExprKind::Call { callee, args } => {
            expr(callee, f);
            for arg in args {
                expr(&mut arg.value, f);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            expr(receiver, f);
            for arg in args {
                expr(&mut arg.value, f);
            }
        }
        ExprKind::If { cond, then, else_ } => {
            expr(cond, f);
            block(then, f);
            if let Some(e) = else_ {
                expr(e, f);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            expr(scrutinee, f);
            for arm in arms {
                block(&mut arm.body, f);
            }
        }
        ExprKind::Block(b) => block(b, f),
    }
}
