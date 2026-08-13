from ovid_native.fff.capability import FffCapability
from ovid_native.fff.engine import FffEngine
from ovid_native.fff.errors import FffIndexNotReadyError, FffStartupError
from ovid_native.search.capability import SearchCapability
from ovid_native.search.engine import SearchEngine


async def select_fff_search_backend[Deps](
    *,
    fff_engine: FffEngine,
    native_engine: SearchEngine,
    include_glob_with_fff: bool = True,
) -> FffCapability[Deps] | SearchCapability[Deps]:
    try:
        await fff_engine.start()
        await fff_engine.wait_ready()
    except FffStartupError, FffIndexNotReadyError:
        await fff_engine.close()
        return SearchCapability(engine=native_engine)

    return FffCapability(
        engine=fff_engine,
        glob_engine=native_engine,
        include_glob=include_glob_with_fff,
    )
