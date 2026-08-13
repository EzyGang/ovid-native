use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use crate::fff::FffError;

create_exception!(_native, NativeFffConfigurationError, PyException);
create_exception!(_native, NativeFffPathError, PyException);
create_exception!(_native, NativeFffQueryError, PyException);
create_exception!(_native, NativeFffPatternError, PyException);
create_exception!(_native, NativeFffLimitError, PyException);
create_exception!(_native, NativeFffIndexNotReadyError, PyException);
create_exception!(_native, NativeFffClosedError, PyException);
create_exception!(_native, NativeFffCancelledError, PyException);
create_exception!(_native, NativeFffRuntimeError, PyException);
create_exception!(_native, NativeFffStartupError, PyException);

pub(crate) fn add_exceptions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add(
        "NativeFffConfigurationError",
        py.get_type::<NativeFffConfigurationError>(),
    )?;
    module.add("NativeFffPathError", py.get_type::<NativeFffPathError>())?;
    module.add("NativeFffQueryError", py.get_type::<NativeFffQueryError>())?;
    module.add(
        "NativeFffPatternError",
        py.get_type::<NativeFffPatternError>(),
    )?;
    module.add("NativeFffLimitError", py.get_type::<NativeFffLimitError>())?;
    module.add(
        "NativeFffIndexNotReadyError",
        py.get_type::<NativeFffIndexNotReadyError>(),
    )?;
    module.add(
        "NativeFffClosedError",
        py.get_type::<NativeFffClosedError>(),
    )?;
    module.add(
        "NativeFffCancelledError",
        py.get_type::<NativeFffCancelledError>(),
    )?;
    module.add(
        "NativeFffRuntimeError",
        py.get_type::<NativeFffRuntimeError>(),
    )?;
    module.add(
        "NativeFffStartupError",
        py.get_type::<NativeFffStartupError>(),
    )?;
    Ok(())
}

pub(crate) fn to_python_error(error: FffError) -> PyErr {
    match error {
        FffError::Configuration(message) => NativeFffConfigurationError::new_err(message),
        FffError::Path(message) => NativeFffPathError::new_err(message),
        FffError::Query(message) => NativeFffQueryError::new_err(message),
        FffError::Pattern(message) => NativeFffPatternError::new_err(message),
        FffError::Limit(message) => NativeFffLimitError::new_err(message),
        FffError::IndexNotReady => NativeFffIndexNotReadyError::new_err("FFF index is not ready"),
        FffError::Closed => NativeFffClosedError::new_err("FFF engine is closed"),
        FffError::Cancelled => NativeFffCancelledError::new_err("FFF operation cancelled"),
        FffError::Runtime(message) => NativeFffRuntimeError::new_err(message),
        FffError::Startup(message) => NativeFffStartupError::new_err(message),
    }
}
