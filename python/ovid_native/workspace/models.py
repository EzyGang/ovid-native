import secrets
from enum import StrEnum
from pathlib import Path

from ovid_core.models import BaseModel, BaseRootModel
from pydantic import Field


class WorkspaceSessionId(BaseRootModel[str]):
    @classmethod
    def new(cls) -> WorkspaceSessionId:
        return cls(secrets.token_urlsafe(24))


class WorkspaceOperation(StrEnum):
    FILES = 'files'
    SEARCH = 'search'
    AST = 'ast'
    FFF = 'fff'
    OBSERVATIONS = 'observations'
    SNAPSHOTS = 'snapshots'
    CHANGE_EVENTS = 'change_events'


class WorkspaceViewPurpose(StrEnum):
    SEARCH = 'search'
    AST = 'ast'
    FFF = 'fff'


class WorkspaceView(BaseModel):
    root: Path
    revision: str = Field(min_length=1)
    read_only: bool
