use crate::fff::FffError;
use crate::python_boundary::python_errors;

python_errors! {
    FffError;
    NativeFffConfigurationError: FffError::Configuration(message) => message,
    NativeFffPathError: FffError::Path(message) => message,
    NativeFffQueryError: FffError::Query(message) => message,
    NativeFffPatternError: FffError::Pattern(message) => message,
    NativeFffLimitError: FffError::Limit(message) => message,
    NativeFffIndexNotReadyError: FffError::IndexNotReady => "FFF index is not ready",
    NativeFffClosedError: FffError::Closed => "FFF engine is closed",
    NativeFffCancelledError: FffError::Cancelled => "FFF operation cancelled",
    NativeFffRuntimeError: FffError::Runtime(message) => message,
    NativeFffStartupError: FffError::Startup(message) => message,
}
