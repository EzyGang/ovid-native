class FileDiscoveryError(Exception):
    pass


class FileDiscoveryConfigurationError(FileDiscoveryError):
    pass


class FileDiscoveryPathError(FileDiscoveryError):
    pass


class FileDiscoveryReadError(FileDiscoveryError):
    pass


class FileDiscoveryEncodingError(FileDiscoveryReadError):
    pass


class FileDiscoveryCancelledError(FileDiscoveryError):
    pass
