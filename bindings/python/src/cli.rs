//! `cli_main`: the `virtx` command line, for the console script `pip install virtx` puts on
//! `PATH` (and `uvx virtx` runs).

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::get_runtime;

/// Run the `virtx` command line on `args` (without the program name) and return its exit
/// code. Output goes to this process's stdout and stderr.
///
/// Blocking, on the binding's runtime, with the GIL released: a command line has nothing to
/// interleave with.
#[pyfunction]
fn cli_main(py: Python<'_>, args: Vec<String>) -> i32 {
    py.detach(|| get_runtime().block_on(virtx::cli::run(args)))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(cli_main, m)?)?;
    Ok(())
}
