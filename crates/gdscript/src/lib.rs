//! gdscript — a GDScript (.gd) parser and interpreter in pure Rust.
//!
//! The goal is NOT full GDScript coverage: it is exactly the subset needed
//! to let Godot-style scripts drive RoadKit — classes, functions, loops,
//! conditionals, `Vector3` math, strings, arrays and a host-object API
//! (`RoadPoint`, `RoadCurve`, …) provided by the embedder.
//!
//! Supported syntax:
//! - `extends X`, `class_name X` (ignored), top-level `var`/`const`
//! - `func name(args):` with indentation blocks
//! - `var x = ...`, assignment, `+= -= *= /=`
//! - `if / elif / else`, `while`, `for x in expr`, `break`, `continue`
//! - `return [expr]`, `pass`
//! - operators: + - * / % == != < <= > >= and or not, unary minus
//! - literals: int, float, "string" (escapes), true/false, `[a, b, c]`
//! - member access + calls: `obj.prop`, `obj.method(...)`, `Klass.new()`
//! - builtins: print, str, len, range, Vector3, randf, randf_range,
//!   abs, floor, min, max, clamp, sqrt, sin, cos, deg_to_rad
//!
//! Host objects implement [`GodotObject`]; the embedder registers
//! constructors (e.g. `RoadPoint`, `RoadCurve`) that scripts instantiate
//! with `Klass.new()` — exactly the Godot idiom.

pub mod ast;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod value;

pub use ast::{BinOp, Expr, FuncDecl, Script, Stmt, UnOp};
pub use interp::{Interpreter, CtorFn};
pub use lexer::{lex, Tok, Token};
pub use parser::parse;
pub use value::{GodotObject, Value, Vec3Val};

/// Convenience: parse + run `entry` (default `_ready`) on a fresh interpreter.
pub fn run_source(
    src: &str,
    entry: &str,
    register: &mut dyn FnMut(&mut Interpreter),
) -> Result<Vec<String>, String> {
    let script = parse(src)?;
    let mut it = Interpreter::new();
    register(&mut it);
    let out = it.run(&script, entry)?;
    let mut lines = it.take_output();
    lines.push(format!("[exit] {}", out.to_display()));
    Ok(lines)
}
