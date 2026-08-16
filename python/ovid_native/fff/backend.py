from ovid_native.fff.capability import FffCapability
from ovid_native.fff.errors import FffIndexNotReadyError, FffStartupError
from ovid_native.search.capability import SearchCapability
from ovid_native.workspace.models import WorkspaceSession


async def select_fff_search_backend[Deps](
    *,
    workspace: WorkspaceSession,
    workspace_name: str = 'default',
    include_glob_with_fff: bool = True,
) -> FffCapability[Deps] | SearchCapability[Deps]:
    try:
        await workspace.fff.start()
        await workspace.fff.wait_ready()
    except FffStartupError, FffIndexNotReadyError:
        return SearchCapability(workspace=workspace_name)

    return FffCapability(
        workspace=workspace_name,
        include_glob=include_glob_with_fff,
    )
