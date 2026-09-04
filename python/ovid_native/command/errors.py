class CommandError(Exception):
    pass


class CommandConfigurationError(CommandError):
    pass


class CommandPathError(CommandError):
    pass


class CommandExecutionError(CommandError):
    pass


class CommandCancelledError(CommandError):
    pass


class CommandClosedError(CommandError):
    pass
