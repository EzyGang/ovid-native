class SearchError(Exception):
    pass


class SearchConfigurationError(SearchError):
    pass


class SearchPathError(SearchError):
    pass


class SearchPatternError(SearchError):
    pass


class SearchLimitError(SearchError):
    pass


class SearchCancelledError(SearchError):
    pass


class SearchReadError(SearchError):
    pass
