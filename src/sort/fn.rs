//! Sort to represent functions as values.
//!
//! To declare the sort, you must specify the exact number of arguments and the sort of each, followed by the output sort:
//! `(sort IntToString (Fn (i64) String))`
//!
//! To create a function value, use the `(function "name" [<partial args>])` primitive and to apply it use the `(call function arg1 arg2 ...)` primitive.
//! The number of args must match the number of arguments in the function sort.
//!
//!
//! The value is stored similar to the `vec` sort, as an index into a set, where each item in
//! the set is a `(Symbol, Vec<Value>)` pairs. The Symbol is the function name, and the `Vec<Value>` is
//! the list of partially applied arguments.
use std::sync::Mutex;

use crate::ast::Literal;

use super::*;

type ValueFunction = (Symbol, Vec<Value>);

#[derive(Debug)]
pub struct FunctionSort {
    name: Symbol,
    inputs: Vec<ArcSort>,
    output: ArcSort,
    functions: Mutex<IndexSet<ValueFunction>>,
}

impl FunctionSort {
    pub fn presort_names() -> Vec<Symbol> {
        vec!["fn".into(), "call".into()]
    }
    pub fn make_sort(
        typeinfo: &mut TypeInfo,
        name: Symbol,
        args: &[Expr],
    ) -> Result<ArcSort, TypeError> {
        if let [Expr::Call((), first, rest_args), Expr::Var((), output)] = args {
            let output_sort = typeinfo
                .sorts
                .get(output)
                .ok_or(TypeError::UndefinedSort(*output))?;
            let all_args = once(first).chain(rest_args.iter().map(|arg| {
                if let Expr::Var((), arg) = arg {
                    arg
                } else {
                    panic!("function sort must be called with list of input sorts");
                }
            }));
            let input_sorts = all_args
                .map(|arg| {
                    typeinfo
                        .sorts
                        .get(arg)
                        .ok_or(TypeError::UndefinedSort(*arg))
                        .map(|s| s.clone())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Arc::new(Self {
                name,
                inputs: input_sorts,
                output: output_sort.clone(),
                functions: Default::default(),
            }))
        } else {
            panic!("function sort must be called with list of input args and output sort");
        }
    }

    fn get_value(&self, value: &Value) -> ValueFunction {
        let functions = self.functions.lock().unwrap();
        let (name, args) = functions.get_index(value.bits as usize).unwrap();
        (*name, args.clone())
    }
}

impl Sort for FunctionSort {
    fn name(&self) -> Symbol {
        self.name
    }

    fn as_arc_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync + 'static> {
        self
    }

    fn is_container_sort(&self) -> bool {
        true
    }

    fn is_eq_container_sort(&self) -> bool {
        self.inputs.iter().any(|s| s.is_eq_sort())
    }

    fn serialized_name(&self, value: &Value) -> Symbol {
        self.get_value(value).0
    }

    fn inner_values(&self, value: &Value) -> Vec<(&ArcSort, Value)> {
        let input_values = self.get_value(value).1;
        self.inputs.iter().zip(input_values).collect()
    }

    fn canonicalize(&self, value: &mut Value, unionfind: &UnionFind) -> bool {
        let (name, inputs) = self.get_value(value);
        let (new_outputs, changed) = inputs.into_iter().zip(&self.inputs).fold(
            (vec![], false),
            |(mut outputs, changed), (mut v, s)| {
                outputs.push(v);
                (outputs, changed | s.canonicalize(&mut v, unionfind))
            },
        );
        *value = (name, new_outputs).store(self).unwrap();
        changed
    }

    fn register_primitives(self: Arc<Self>, typeinfo: &mut TypeInfo) {
        typeinfo.add_primitive(Ctor {
            name: "fn".into(),
            function: self.clone(),
            string: typeinfo.get_sort_nofail(),
        });
        typeinfo.add_primitive(FunctionCall {
            name: "call".into(),
            function: self.clone(),
        });
    }

    fn make_expr(&self, egraph: &EGraph, value: Value) -> (Cost, Expr) {
        let mut termdag = TermDag::default();
        let extractor = Extractor::new(egraph, &mut termdag);
        self.extract_expr(egraph, value, &extractor, &mut termdag)
            .expect("Extraction should be successful since extractor has been fully initialized")
    }

    fn extract_expr(
        &self,
        _egraph: &EGraph,
        value: Value,
        extractor: &Extractor,
        termdag: &mut TermDag,
    ) -> Option<(Cost, Expr)> {
        let (name, inputs) = ValueFunction::load(self, &value);
        let (cost, args) = inputs.into_iter().zip(&self.inputs).try_fold(
            (0usize, vec![Expr::Lit((), Literal::String(name))]),
            |(cost, mut args), (value, sort)| {
                let (new_cost, term) = extractor.find_best(value, termdag, sort)?;
                args.push(termdag.term_to_expr(&term));
                Some((cost.saturating_add(new_cost), args))
            },
        )?;

        Some((cost, Expr::call("function", args)))
    }
}

impl IntoSort for ValueFunction {
    type Sort = FunctionSort;
    fn store(self, sort: &Self::Sort) -> Option<Value> {
        let mut functions = sort.functions.lock().unwrap();
        let (i, _) = functions.insert_full(self);
        Some(Value {
            tag: sort.name,
            bits: i as u64,
        })
    }
}

impl FromSort for ValueFunction {
    type Sort = FunctionSort;
    fn load(sort: &Self::Sort, value: &Value) -> Self {
        sort.get_value(value)
    }
}

/// Takes a string and any number of partially applied args of any sort and returns a function
struct FunctionCTorTypeConstraint {
    name: Symbol,
    function: Arc<FunctionSort>,
    string: Arc<StringSort>,
}

impl TypeConstraint for FunctionCTorTypeConstraint {
    fn get(&self, arguments: &[AtomTerm]) -> Vec<Constraint<AtomTerm, ArcSort>> {
        // Must have at least one arg (plus the return value)
        if arguments.len() < 2 {
            vec![Constraint::Impossible(
                constraint::ImpossibleConstraint::ArityMismatch {
                    atom: core::Atom {
                        head: self.name,
                        args: arguments.to_vec(),
                    },
                    expected: 2,
                    actual: arguments.len(),
                },
            )]
        } else {
            vec![
                Constraint::Assign(arguments[0].clone(), self.string.clone()),
                Constraint::Assign(
                    arguments[arguments.len() - 1].clone(),
                    self.function.clone(),
                ),
            ]
        }
    }
}

// (fn "name" [<arg1>, <arg2>, ...])
struct Ctor {
    name: Symbol,
    function: Arc<FunctionSort>,
    string: Arc<StringSort>,
}

impl PrimitiveLike for Ctor {
    fn name(&self) -> Symbol {
        self.name
    }

    fn get_type_constraints(&self) -> Box<dyn TypeConstraint> {
        Box::new(FunctionCTorTypeConstraint {
            name: self.name,
            function: self.function.clone(),
            string: self.string.clone(),
        })
    }

    fn apply(&self, values: &[Value], _egraph: &mut EGraph) -> Option<Value> {
        let name = Symbol::load(&self.string, &values[0]);
        (name, values[1..].to_vec()).store(&self.function)
    }
}

// (call <function> [<arg1>, <arg2>, ...])
struct FunctionCall {
    name: Symbol,
    function: Arc<FunctionSort>,
}

impl PrimitiveLike for FunctionCall {
    fn name(&self) -> Symbol {
        self.name
    }

    fn get_type_constraints(&self) -> Box<dyn TypeConstraint> {
        let mut sorts: Vec<ArcSort> = vec![self.function.clone()];
        sorts.extend(self.function.inputs.clone());
        sorts.push(self.function.output.clone());
        SimpleTypeConstraint::new(self.name(), sorts).into_box()
    }

    fn apply(&self, values: &[Value], egraph: &mut EGraph) -> Option<Value> {
        let (name, mut args) = ValueFunction::load(&self.function, &values[0]);

        let types: Vec<_> = args
            .iter()
            // get the sorts of partially applied args
            .map(|arg| egraph.get_sort_from_value(arg).unwrap().clone())
            // combine with the args for the function call and then the output
            .chain(self.function.inputs.clone())
            .chain(once(self.function.output.clone()))
            .collect();

        args.extend_from_slice(&values[1..]);

        Some(call_fn(egraph, &name, types, args))
    }
}

/// Call function (either primitive or eqsort) <name> with value args <args> and return the value.
///
/// Does this in a similar way to how merge functions are resolved, using the stack and actions,
/// so that we can re-use the logic for primitive and regular functions.
fn call_fn(egraph: &mut EGraph, name: &Symbol, types: Vec<ArcSort>, args: Vec<Value>) -> Value {
    // Make a call with temp vars as each of the args
    let resolved_call = ResolvedCall::from_resolution(name, types.as_slice(), egraph.type_info());
    let arg_vars: Vec<_> = types
        .into_iter()
        // Skip last sort which is the output sort
        .take(args.len())
        .enumerate()
        .map(|(i, sort)| ResolvedVar {
            name: format!("__arg_{}", i).into(),
            sort,
        })
        .collect();
    let binding = IndexSet::from_iter(arg_vars.clone());
    let resolved_args = arg_vars
        .into_iter()
        .map(|v| ResolvedExpr::Var((), v))
        .collect();
    let expr = ResolvedExpr::Call((), resolved_call, resolved_args);
    // Similar to how the merge function is created in `Function::new`
    let (actions, mapped_expr) = expr
        .to_core_actions(
            egraph.type_info(),
            &mut binding.clone(),
            &mut ResolvedGen::new(),
        )
        .unwrap();
    let target = mapped_expr.get_corresponding_var_or_lit(egraph.type_info());
    let program = egraph.compile_expr(&binding, &actions, &target).unwrap();
    // Similar to how the `MergeFn::Expr` case is handled in `Egraph::perform_set`
    let mut stack = vec![];
    // Run action on cloned EGraph to avoid modifying the original
    egraph
        .run_actions(&mut stack, &args, &program, true)
        .unwrap();
    stack.pop().unwrap()
}
