mod ast;
mod discovery;
mod fff;
mod search;
mod workspace;

use pyo3::prelude::*;

const API_VERSION: u16 = 11;

#[pyfunction]
#[must_use]
fn runtime_info() -> (&'static str, &'static str, u16) {
    (std::env::consts::OS, std::env::consts::ARCH, API_VERSION)
}

#[pymodule(gil_used = false)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(runtime_info, module)?)?;
    discovery::register_module(module)?;
    workspace::python::register(module)?;
    ast::register_module(module)?;
    fff::register_module(module)?;
    search::register_module(module)?;
    Ok(())
}
