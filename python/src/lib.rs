//! Python bindings for push3-fast (see `push3_fast.pyi` for the API as Python sees it).
//!
//! What crosses threads is text and `Edn`: `Item` is Rc-based and is only ever built on the worker
//! that uses it. Evaluation and generation run with the GIL released.
//!
//! What must not end the Python process:
//! - A panic. This crate builds with panic = "unwind", a panic while handling one program becomes
//!   that program's error, and one anywhere else becomes a RuntimeError.
//! - A stack overflow, which cannot be caught. Parsing, printing and dropping recurse on the nesting
//!   of the text, so nesting is checked before parsing, and whatever may nest deeply is handled on a
//!   thread with a big stack, not on Python's.

use core3::edn::{self, Edn};
use core3::eval::{parallel, parse_program_line, Limits, Prepared, Problem as CoreProblem, Row};
use core3::hindsight::{generate, plushy_to_program as program_of_genes, Genome, Params, Spec, Stats};
use core3::Item;
use pyo3::exceptions::{PyOSError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyDict, PyList};
use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// The stack of the worker threads in `eval::parallel`, used for the helper thread here as well.
const BIG_STACK: usize = 1 << 28;
/// Nesting allowed in a program. A worker's stack holds 400,000 levels of parsing, running and dropping
/// (measured; 500,000 crashed), and past the real limit the process dies. No drawn program comes near.
const MAX_PROGRAM_DEPTH: usize = 100_000;
/// Nesting allowed in a problem or spec. They are parsed and dropped on Python's stack; the real ones
/// nest three or four levels.
const MAX_DATA_DEPTH: usize = 1000;
/// A genome's program may nest as deep as the genome is long, and it is parsed again on a stack like
/// a worker's.
const MAX_GENOME_SIZE: usize = MAX_PROGRAM_DEPTH;

fn panic_text(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast_ref::<&str>() {
        Some(s) => s.to_string(),
        None => payload.downcast_ref::<String>().cloned().unwrap_or_else(|| "unknown panic".into()),
    }
}

/// The deepest nesting of collections in `text`, read the way `edn::parse` reads it.
fn nesting_depth(text: &str) -> usize {
    let b = text.as_bytes();
    let (mut i, mut depth, mut deepest) = (0, 0usize, 0);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\\' => i += 1, // a character literal, `\(` is not an open paren
            b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    deepest
}

fn check_depth(text: &str, limit: usize) -> Result<(), String> {
    let d = nesting_depth(text);
    if d > limit {
        return Err(format!("nested {d} levels deep, the limit is {limit}"));
    }
    Ok(())
}

/// Runs `f` on a thread with a big stack. A panic there comes back as `Err`.
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(BIG_STACK)
            .spawn_scoped(s, f)
            .map_err(|e| format!("cannot start a thread: {e}"))?
            .join()
            .map_err(|p| format!("internal panic: {}", panic_text(p)))
    })
}

/// A panic while reading text means the text is bad (the reader indexes past the end of a truncated
/// `\u` escape, for one).
fn reading<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|p| Err(format!("unreadable EDN: {}", panic_text(p))))
}

fn value_error(e: String) -> PyErr {
    PyValueError::new_err(e)
}

fn thread_count(threads: Option<usize>) -> PyResult<usize> {
    match threads {
        Some(0) => Err(PyValueError::new_err("threads must be at least 1")),
        Some(n) => Ok(n),
        None => Ok(std::thread::available_parallelism().map_or(1, |n| n.get())),
    }
}

/// What the library would accept and then wrap around: its casts turn a negative `:max` into an endless
/// run and a negative `:num-outputs` or one above the 8 registers into a panic in a worker.
fn check_common(e: &Edn) -> Result<(), String> {
    if !matches!(e, Edn::Map(_)) {
        return Err("expected an EDN map".into());
    }
    if matches!(e.get("max"), Some(Edn::Int(n)) if *n < 0) {
        return Err(":max must not be negative".into());
    }
    if matches!(e.get("num-outputs"), Some(Edn::Int(n)) if !(0..=8).contains(n)) {
        return Err(":num-outputs must be between 0 and 8".into());
    }
    Ok(())
}

fn parse_problem(text: &str) -> Result<CoreProblem, String> {
    check_depth(text, MAX_DATA_DEPTH)?;
    reading(|| {
        let e = edn::parse(text)?;
        check_common(&e)?;
        // without :cases the library reads the map as a hindsight spec, which has nothing to score
        match e.get("cases").and_then(|c| c.items()) {
            Some([]) => return Err(":cases is empty".into()),
            Some(_) => {}
            None => return Err("missing :cases (a hindsight spec is not a problem)".into()),
        }
        CoreProblem::from_edn(&e)
    })
}

fn parse_spec(text: &str) -> Result<Spec, String> {
    check_depth(text, MAX_DATA_DEPTH)?;
    reading(|| {
        let e = edn::parse(text)?;
        check_common(&e)?;
        if matches!(e.get("inputs").and_then(|c| c.items()), Some([])) {
            return Err(":inputs is empty".into());
        }
        if matches!(e.get("max-initial-plushy-size"), Some(Edn::Int(n)) if *n < 0) {
            return Err(":max-initial-plushy-size must not be negative".into());
        }
        Spec::parse(text)
    })
}

/// One program on one worker. A program that fails, in any way, is that program's row.
fn evaluate_one(text: &str, prepared: &mut Prepared, limits: Limits) -> Result<Row, String> {
    // `Machine::reset` at the start of each case puts the worker's machine right after a panic
    catch_unwind(AssertUnwindSafe(|| {
        check_depth(text, MAX_PROGRAM_DEPTH)?;
        let (program, _) = parse_program_line(text)?;
        Ok(prepared.evaluate(&program, limits))
    }))
    .unwrap_or_else(|p| Err(format!("internal panic: {}", panic_text(p))))
}

fn evaluate_rows(py: Python<'_>, problem: &CoreProblem, programs: &[String], limits: Limits, threads: usize) -> PyResult<Vec<Result<Row, String>>> {
    py.detach(|| {
        catch_unwind(AssertUnwindSafe(|| parallel(problem, programs.len(), threads, |i, prepared| evaluate_one(&programs[i], prepared, limits))))
    })
    .map_err(|p| PyRuntimeError::new_err(format!("internal panic: {}", panic_text(p))))
}

fn row_dict<'py>(py: Python<'py>, row: &Result<Row, String>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match row {
        Ok(r) => {
            d.set_item("errors", PyList::new(py, &r.errors)?)?;
            d.set_item("total", r.total())?;
            d.set_item("solved", r.solved())?;
            d.set_item("steps", r.steps)?;
            d.set_item("cut", r.cut)?;
            d.set_item("overflow", r.overflow)?;
        }
        Err(e) => d.set_item("error", e)?,
    }
    Ok(d)
}

/// A NumPy array that owns a writable copy of `bytes`; no compile-time link to NumPy, which is only
/// imported when one of these is asked for.
fn numpy_array<'py>(numpy: &Bound<'py, PyModule>, bytes: &[u8], dtype: &str, shape: &[usize]) -> PyResult<Bound<'py, PyAny>> {
    if bytes.is_empty() {
        return numpy.call_method1("zeros", (shape.to_vec(), dtype));
    }
    let buffer = PyByteArray::new(numpy.py(), bytes);
    numpy.call_method1("frombuffer", (buffer, dtype))?.call_method1("reshape", (shape.to_vec(),))
}

fn ne_bytes<T: Copy, const N: usize>(values: impl Iterator<Item = T>, bytes: impl Fn(T) -> [u8; N]) -> Vec<u8> {
    values.flat_map(bytes).collect()
}

fn f64_bytes(values: impl Iterator<Item = f64>) -> Vec<u8> {
    ne_bytes(values, f64::to_ne_bytes)
}

fn i64_bytes(values: impl Iterator<Item = i64>) -> Vec<u8> {
    ne_bytes(values, i64::to_ne_bytes)
}

/// A problem: its cases, output reader and error function. Built once, evaluated many times.
#[pyclass(frozen)]
struct Problem {
    inner: CoreProblem,
}

#[pymethods]
impl Problem {
    #[new]
    fn new(py: Python<'_>, edn_text: &str) -> PyResult<Problem> {
        let inner = py.detach(|| parse_problem(edn_text)).map_err(value_error)?;
        Ok(Problem { inner })
    }

    #[staticmethod]
    fn from_file(py: Python<'_>, path: std::path::PathBuf) -> PyResult<Problem> {
        let text = py.detach(|| std::fs::read(&path)).map_err(|e| match e.raw_os_error() {
            // OSError with (errno, text, path) becomes FileNotFoundError and so on
            Some(code) => PyOSError::new_err((code, e.to_string(), path.clone())),
            None => PyOSError::new_err(format!("{}: {e}", path.display())),
        })?;
        let text = String::from_utf8(text).map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))?;
        let inner = py.detach(|| parse_problem(&text)).map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))?;
        Ok(Problem { inner })
    }

    #[getter]
    fn n_cases(&self) -> usize {
        self.inner.n_cases()
    }

    /// Errors per program when none is cut: `n_cases`, or more on the circuit problems (one per bit).
    #[getter]
    fn n_errors(&self) -> usize {
        self.inner.n_errors()
    }

    fn __repr__(&self) -> String {
        format!("Problem(n_cases={})", self.inner.n_cases())
    }

    #[pyo3(signature = (programs, cap=None, screen=None, stack_limit=0, propeller_limits=false, threads=None))]
    fn evaluate<'py>(
        &self,
        py: Python<'py>,
        programs: Vec<String>,
        cap: Option<f64>,
        screen: Option<(usize, f64)>,
        stack_limit: usize,
        propeller_limits: bool,
        threads: Option<usize>,
    ) -> PyResult<Bound<'py, PyList>> {
        let threads = thread_count(threads)?;
        let limits = Limits { cap, screen, stack_limit, propeller_limits };
        let rows = evaluate_rows(py, &self.inner, &programs, limits, threads)?;
        let dicts = rows.iter().map(|r| row_dict(py, r)).collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, dicts)
    }

    #[pyo3(signature = (programs, cap=None, screen=None, stack_limit=0, propeller_limits=false, threads=None, fill=f64::NAN))]
    fn evaluate_arrays<'py>(
        &self,
        py: Python<'py>,
        programs: Vec<String>,
        cap: Option<f64>,
        screen: Option<(usize, f64)>,
        stack_limit: usize,
        propeller_limits: bool,
        threads: Option<usize>,
        fill: f64,
    ) -> PyResult<Bound<'py, PyDict>> {
        // before the work, so that a missing NumPy costs nothing
        let numpy = py.import("numpy")?;
        let threads = thread_count(threads)?;
        let limits = Limits { cap, screen, stack_limit, propeller_limits };
        let rows = evaluate_rows(py, &self.inner, &programs, limits, threads)?;
        let n = rows.len();
        let width = self.inner.n_errors();

        let errors = py.detach(|| {
            let mut errors = vec![fill; n * width];
            for (slot, row) in errors.chunks_exact_mut(width.max(1)).zip(&rows) {
                if let Ok(r) = row {
                    let len = r.errors.len().min(width);
                    slot[..len].copy_from_slice(&r.errors[..len]);
                }
            }
            f64_bytes(errors.into_iter())
        });
        let oks = || rows.iter().map(|r| r.as_ref().ok());
        let flag = |f: fn(&Row) -> bool| -> Vec<u8> { oks().map(|r| r.is_some_and(f) as u8).collect() };

        let d = PyDict::new(py);
        d.set_item("errors", numpy_array(&numpy, &errors, "float64", &[n, width])?)?;
        let one_column = |bytes: &[u8], dtype: &str| numpy_array(&numpy, bytes, dtype, &[n]);
        d.set_item("lengths", one_column(&i64_bytes(oks().map(|r| r.map_or(0, |r| r.errors.len() as i64))), "int64")?)?;
        d.set_item("total", one_column(&f64_bytes(oks().map(|r| r.map_or(fill, |r| r.total()))), "float64")?)?;
        d.set_item("solved", one_column(&i64_bytes(oks().map(|r| r.map_or(0, |r| r.solved() as i64))), "int64")?)?;
        d.set_item("steps", one_column(&i64_bytes(oks().map(|r| r.map_or(0, |r| r.steps as i64))), "int64")?)?;
        d.set_item("cut", one_column(&flag(|r| r.cut), "bool")?)?;
        d.set_item("overflow", one_column(&flag(|r| r.overflow), "bool")?)?;
        d.set_item("ok", one_column(&rows.iter().map(|r| r.is_ok() as u8).collect::<Vec<u8>>(), "bool")?)?;
        // only the programs that failed: {index: message}
        let messages = PyDict::new(py);
        for (i, r) in rows.iter().enumerate() {
            if let Err(e) = r {
                messages.set_item(i, e)?;
            }
        }
        d.set_item("messages", messages)?;
        Ok(d)
    }
}

/// A kept record, taken apart. `program` is dropped here, on the big stack, with its `Edn`.
struct Record {
    line: String,
    program: String,
    plushy: Option<Vec<String>>,
    examples: Vec<(Vec<Edn>, Edn)>,
}

fn record(line: String) -> Result<Record, String> {
    let e = edn::parse(&line)?;
    let program = edn::edn_text(e.get("program").ok_or("record without :program")?);
    let plushy = e.get("plushy").and_then(|g| g.items()).map(|g| g.iter().map(edn::edn_text).collect());
    let mut examples = Vec::new();
    for x in e.get("examples").and_then(|x| x.items()).ok_or("record without :examples")? {
        let inputs = x.get("inputs").and_then(|i| i.items()).ok_or("example without :inputs")?.to_vec();
        examples.push((inputs, x.get("output").ok_or("example without :output")?.clone()));
    }
    Ok(Record { line, program, plushy, examples })
}

fn edn_value(py: Python<'_>, e: &Edn) -> PyResult<Py<PyAny>> {
    use pyo3::IntoPyObjectExt;
    match e {
        Edn::Nil => Ok(py.None()),
        Edn::Bool(b) => b.into_py_any(py),
        Edn::Int(i) => i.into_py_any(py),
        Edn::Float(f) => f.into_py_any(py),
        Edn::Char(c) => c.to_string().into_py_any(py),
        Edn::Str(s) => s.into_py_any(py),
        Edn::List(v) | Edn::Vector(v) => {
            let items = v.iter().map(|x| edn_value(py, x)).collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, items)?.into_py_any(py)
        }
        // keywords and maps do not occur as inputs or outputs of the modelled types
        other => edn::edn_text(other).into_py_any(py),
    }
}

fn record_dict<'py>(py: Python<'py>, r: &Record) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("edn", &r.line)?;
    d.set_item("program", &r.program)?;
    d.set_item("plushy", &r.plushy)?;
    let examples = PyList::empty(py);
    for (inputs, output) in &r.examples {
        let x = PyDict::new(py);
        let inputs = inputs.iter().map(|v| edn_value(py, v)).collect::<PyResult<Vec<_>>>()?;
        x.set_item("inputs", inputs)?;
        x.set_item("output", edn_value(py, output)?)?;
        examples.append(x)?;
    }
    d.set_item("examples", examples)?;
    Ok(d)
}

/// Draws `n` programs and keeps those that make a usable problem of their own outputs.
#[pyfunction]
#[pyo3(signature = (spec_edn_text, n=1000, seed=1, examples=10, min_examples=5, max_size=None, min_distinct=2, stack_limit=100_000, threads=None, genome="plushy", length=(5, 60)))]
fn hindsight<'py>(
    py: Python<'py>,
    spec_edn_text: &str,
    n: usize,
    seed: u64,
    examples: usize,
    min_examples: usize,
    max_size: Option<usize>,
    min_distinct: usize,
    stack_limit: usize,
    threads: Option<usize>,
    genome: &str,
    length: (usize, usize),
) -> PyResult<(Bound<'py, PyList>, Bound<'py, PyDict>)> {
    let threads = thread_count(threads)?;
    if !matches!(genome, "plushy" | "tree") {
        return Err(PyValueError::new_err(format!("genome must be \"plushy\" or \"tree\", not {genome:?}")));
    }
    if length.0 > length.1 {
        return Err(PyValueError::new_err("length is (low, high) with low <= high"));
    }
    if length.1 > MAX_GENOME_SIZE {
        return Err(PyValueError::new_err(format!("length is above the limit of {MAX_GENOME_SIZE}")));
    }
    enum Failed {
        BadInput(String),
        Internal(String),
    }
    let done: Result<(Vec<Record>, Stats), Failed> = py.detach(|| {
        let spec = parse_spec(spec_edn_text).map_err(Failed::BadInput)?;
        let genome = if genome == "tree" {
            Genome::Tree
        } else {
            // as the CLI does: the argument, else the problem's own, else the library's default
            let max_size = max_size.or(spec.max_size).unwrap_or(100);
            if max_size > MAX_GENOME_SIZE {
                return Err(Failed::BadInput(format!("max_size {max_size} is above the limit of {MAX_GENOME_SIZE}")));
            }
            Genome::Plushy { max_size }
        };
        let params = Params { genome, n, seed, examples, min_examples, length, min_distinct, stack_limit, threads };
        let (lines, stats) = catch_unwind(AssertUnwindSafe(|| generate(&spec, &params)))
            .map_err(|p| Failed::Internal(format!("internal panic: {}", panic_text(p))))?;
        let records = on_big_stack(|| lines.into_iter().map(record).collect::<Result<Vec<_>, _>>())
            .map_err(Failed::Internal)?
            .map_err(|e| Failed::Internal(format!("a record does not read back: {e}")))?;
        Ok((records, stats))
    });
    let (records, stats) = done.map_err(|f| match f {
        Failed::BadInput(e) => PyValueError::new_err(e),
        Failed::Internal(e) => PyRuntimeError::new_err(e),
    })?;
    let dicts = records.iter().map(|r| record_dict(py, r)).collect::<PyResult<Vec<_>>>()?;
    let s = PyDict::new(py);
    s.set_item("kept", stats.kept)?;
    s.set_item("too_few_outputs", stats.too_few_outputs)?;
    s.set_item("too_uniform", stats.too_uniform)?;
    s.set_item("limit_hit", stats.limit_hit)?;
    Ok((PyList::new(py, dicts)?, s))
}

/// Propeller's `plushy->push`: a genome (an EDN vector) to a program (EDN text).
#[pyfunction]
fn plushy_to_program(py: Python<'_>, genes_edn: &str) -> PyResult<String> {
    py.detach(|| {
        check_depth(genes_edn, MAX_PROGRAM_DEPTH)?;
        // Items and their deep drop stay on this thread; only the text comes back
        on_big_stack(|| {
            reading(|| {
                let e = edn::parse(genes_edn)?;
                let genes = e.items().ok_or("a genome is an EDN vector")?;
                if genes.len() > MAX_GENOME_SIZE {
                    return Err(format!("{} genes, the limit is {MAX_GENOME_SIZE}", genes.len()));
                }
                let genes = genes.iter().map(Item::from_edn).collect::<Result<Vec<_>, _>>()?;
                Ok(program_of_genes(&genes).to_edn_string())
            })
        })?
    })
    .map_err(value_error)
}

/// The implemented instruction names.
#[pyfunction]
fn names() -> Vec<String> {
    core3::all_names()
}

#[pymodule]
fn push3_fast(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Problem>()?;
    m.add_function(wrap_pyfunction!(hindsight, m)?)?;
    m.add_function(wrap_pyfunction!(plushy_to_program, m)?)?;
    m.add_function(wrap_pyfunction!(names, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
