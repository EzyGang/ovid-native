class FffError(Exception):
    pass


class FffConfigurationError(FffError):
    pass


class FffPathError(FffError):
    pass


class FffQueryError(FffError):
    pass


class FffPatternError(FffError):
    pass


class FffLimitError(FffError):
    pass


class FffIndexNotReadyError(FffError):
    pass


class FffClosedError(FffError):
    pass


class FffCancelledError(FffError):
    pass


class FffRuntimeError(FffError):
    pass


class FffStartupError(FffRuntimeError):
    pass
