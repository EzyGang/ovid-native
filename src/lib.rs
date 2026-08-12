mod ast;

use pyo3::prelude::*;

const API_VERSION: u16 = 2;

#[pyfunction]
#[must_use]
fn runtime_info() -> (&'static str, &'static str, u16, Option<&'static str>) {
    (
        std::env::consts::OS,
        std::env::consts::ARCH,
        API_VERSION,
        Some("0.45.1"),
    )
}

#[pymodule(gil_used = false)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(runtime_info, module)?)?;
    ast::register_module(module)?;

    Ok(())
}
