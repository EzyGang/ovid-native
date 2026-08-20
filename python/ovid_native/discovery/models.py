from pathlib import Path
from typing import Literal

from ovid_core.models import BaseModel
from pydantic import Field


type FileDiscoveryCompletion = Literal['complete', 'file_limit_reached', 'deadline_reached']


class NamedFileDiscoveryRequest(BaseModel):
    filename: str = Field(min_length=1)
    max_depth: int = Field(default=4, ge=1, le=64)
    limit: int = Field(default=200, ge=1, le=10_000)
    timeout_seconds: float = Field(default=5.0, gt=0, le=30.0)


class NamedFileDiscoveryResult(BaseModel):
    paths: tuple[str, ...]
    completion: FileDiscoveryCompletion


class TextFile(BaseModel):
    path: Path
    content: str
