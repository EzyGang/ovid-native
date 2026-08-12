class AstError(Exception):
    pass


class AstConfigurationError(AstError):
    pass


class AstPathError(AstError):
    pass


class AstLanguageError(AstError):
    pass


class AstPatternError(AstError):
    pass


class AstLimitError(AstError):
    pass


class AstProposalNotFoundError(AstError):
    pass


class AstProposalExpiredError(AstError):
    pass


class AstProposalStaleError(AstError):
    pass


class AstWriteError(AstError):
    pass
