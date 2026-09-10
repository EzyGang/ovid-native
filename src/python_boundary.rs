use pyo3::{prelude::*, types::PyType};

pub(crate) fn add_exceptions<const N: usize>(
    module: &Bound<'_, PyModule>,
    exceptions: [Bound<'_, PyType>; N],
) -> PyResult<()> {
    for exception in exceptions {
        module.add(exception.name()?, exception)?;
    }
    Ok(())
}

pub(crate) fn request_operation<C, T, R, E, F>(
    py: Python<'_>,
    context: &C,
    request: &T,
    operation: F,
    map_error: fn(E) -> PyErr,
) -> PyResult<R>
where
    C: Clone + Send,
    T: Clone + Send,
    R: Send,
    E: Send,
    F: FnOnce(&C, T) -> Result<R, E> + Send,
{
    let context = context.clone();
    let request = request.clone();
    py.detach(move || operation(&context, request))
        .map_err(map_error)
}

macro_rules! register_exports {
    ($module:expr, classes [$($class:ty),* $(,)?], functions [$($function:ident),* $(,)?]) => {
        $( $module.add_class::<$class>()?; )*
        $( $module.add_function(pyo3::wrap_pyfunction!($function, $module)?)?; )*
    };
}

macro_rules! python_errors {
    ($error:ty; $( $name:ident: $pattern:pat => $message:expr ),* $(,)?) => {
        $(pyo3::create_exception!(_native, $name, pyo3::exceptions::PyException);)*

        pub(crate) fn add_exceptions(module: &pyo3::Bound<'_, pyo3::types::PyModule>) -> pyo3::PyResult<()> {
            crate::python_boundary::add_exceptions(module, [$(module.py().get_type::<$name>()),*])
        }

        pub(crate) fn to_python_error(error: $error) -> pyo3::PyErr {
            match error {
                $($pattern => $name::new_err($message)),*
            }
        }
    };
}

pub(crate) use {python_errors, register_exports};
