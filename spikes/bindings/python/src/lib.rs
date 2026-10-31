//! Python host for toolchain check 3 (PyO3 + maturin).

use pyo3::prelude::*;

#[pyfunction]
fn summarize(source: &str) -> String {
    spike_core::summarize(source)
}

#[pymodule]
fn spike_python(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(summarize, module)?)?;
    Ok(())
}
