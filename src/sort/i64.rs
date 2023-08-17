use crate::ast::Literal;

use super::*;

#[derive(Debug)]
pub struct I64Sort {
    name: Symbol,
}

impl I64Sort {
    pub fn new(name: Symbol) -> Self {
        Self { name }
    }
}

impl Sort for I64Sort {
    fn name(&self) -> Symbol {
        self.name
    }

    fn as_arc_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync + 'static> {
        self
    }

    #[rustfmt::skip]
    // We need the closure for division and mod operations, as they can panic.
    // cf https://github.com/rust-lang/rust-clippy/issues/9422
    #[allow(clippy::unnecessary_lazy_evaluations)]
    fn register_primitives(self: Arc<Self>, typeinfo: &mut TypeInfo) {
        typeinfo.add_primitive(TermOrderingMin {
           });
        typeinfo.add_primitive(TermOrderingMax {
           });

        type Opt<T=()> = Option<T>;

        add_primitives!(typeinfo, "+" = |a: i64, b: i64| -> i64 { a + b });
        add_primitives!(typeinfo, "-" = |a: i64, b: i64| -> i64 { a - b });
        add_primitives!(typeinfo, "*" = |a: i64, b: i64| -> i64 { a * b });
        add_primitives!(typeinfo, "/" = |a: i64, b: i64| -> Opt<i64> { (b != 0).then(|| a / b) });
        add_primitives!(typeinfo, "%" = |a: i64, b: i64| -> Opt<i64> { (b != 0).then(|| a % b) });

        add_primitives!(typeinfo, "&" = |a: i64, b: i64| -> i64 { a & b });
        add_primitives!(typeinfo, "|" = |a: i64, b: i64| -> i64 { a | b });
        add_primitives!(typeinfo, "^" = |a: i64, b: i64| -> i64 { a ^ b });
        add_primitives!(typeinfo, "<<" = |a: i64, b: i64| -> Opt<i64> { b.try_into().ok().and_then(|b| a.checked_shl(b)) });
        add_primitives!(typeinfo, ">>" = |a: i64, b: i64| -> Opt<i64> { b.try_into().ok().and_then(|b| a.checked_shr(b)) });
        add_primitives!(typeinfo, "not-i64" = |a: i64| -> i64 { !a });

        add_primitives!(typeinfo, "log2" = |a: i64| -> i64 { (a as i64).ilog2() as i64 });

        add_primitives!(typeinfo, "<" = |a: i64, b: i64| -> Opt { (a < b).then(|| ()) });
        add_primitives!(typeinfo, ">" = |a: i64, b: i64| -> Opt { (a > b).then(|| ()) });
        add_primitives!(typeinfo, "<=" = |a: i64, b: i64| -> Opt { (a <= b).then(|| ()) });
        add_primitives!(typeinfo, ">=" = |a: i64, b: i64| -> Opt { (a >= b).then(|| ()) });

        add_primitives!(typeinfo, "min" = |a: i64, b: i64| -> i64 { a.min(b) });
        add_primitives!(typeinfo, "max" = |a: i64, b: i64| -> i64 { a.max(b) });

        add_primitives!(typeinfo, "to-string" = |a: i64| -> Symbol { a.to_string().into() });

    }

    fn make_expr(&self, _egraph: &EGraph, value: Value) -> Expr {
        assert!(value.tag == self.name());
        Expr::Lit(Literal::Int(value.bits as _))
    }
}

impl IntoSort for i64 {
    type Sort = I64Sort;
    fn store(self, sort: &Self::Sort) -> Option<Value> {
        Some(Value {
            tag: sort.name,
            bits: self as u64,
        })
    }
}

impl FromSort for i64 {
    type Sort = I64Sort;
    fn load(_sort: &Self::Sort, value: &Value) -> Self {
        value.bits as Self
    }
}
