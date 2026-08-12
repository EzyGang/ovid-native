from importlib.metadata import metadata
from typing import get_args

import pytest
from ovid_core.models import BaseModel
from pydantic import ValidationError

from ovid_native.search import (
    GlobFileType,
    GlobMatch,
    GlobOrder,
    GlobRequest,
    GlobResult,
    GlobToolContent,
    GlobToolResult,
    GrepContextLine,
    GrepFileCoverage,
    GrepFileMatches,
    GrepLargeFileMode,
    GrepMatch,
    GrepPatternMode,
    GrepPosition,
    GrepRange,
    GrepRegexEngine,
    GrepRequest,
    GrepResult,
    GrepToolContent,
    GrepToolRequest,
    GrepToolResult,
    SearchCompletion,
    SearchLimits,
    SearchScanOptions,
)


PUBLIC_MODELS = (
    SearchScanOptions,
    SearchLimits,
    GlobRequest,
    GlobMatch,
    GlobResult,
    GrepRequest,
    GrepToolRequest,
    GrepPosition,
    GrepRange,
    GrepContextLine,
    GrepMatch,
    GrepFileCoverage,
    GrepFileMatches,
    GrepResult,
    GlobToolContent,
    GrepToolContent,
    GlobToolResult,
    GrepToolResult,
)


def test_search_models_are_immutable_strict_ovid_models() -> None:
    for model in PUBLIC_MODELS:
        assert issubclass(model, BaseModel)
        assert model.model_config['frozen'] is True
        assert model.model_config['extra'] == 'forbid'

    request = GlobRequest()
    with pytest.raises(ValidationError):
        GlobRequest(extra=True)
    with pytest.raises(ValidationError):
        request.limit = 1


def test_search_models_validate_bounds_and_defaults() -> None:
    with pytest.raises(ValidationError):
        SearchLimits(max_scan_files=0)
    with pytest.raises(ValidationError):
        GlobRequest(limit=0)
    with pytest.raises(ValidationError):
        GrepRequest(pattern='x', file_offset=-1)

    direct = GrepRequest(pattern='x')
    tool = GrepToolRequest(pattern='x')
    assert direct.mode == 'regex'
    assert tool.mode == 'auto'
    assert direct.scan == SearchScanOptions()
    assert direct.scan is not GrepRequest(pattern='y').scan


def test_search_literal_contracts_and_distribution_profiles() -> None:
    assert set(get_args(SearchCompletion)) == {'complete', 'file_limit_reached', 'deadline_reached'}
    assert set(get_args(GlobOrder)) == {'path', 'modified_desc'}
    assert set(get_args(GlobFileType)) == {'file', 'directory', 'any'}
    assert set(get_args(GrepPatternMode)) == {'regex', 'literal', 'auto'}
    assert set(get_args(GrepRegexEngine)) == {'rust', 'pcre2'}
    assert set(get_args(GrepLargeFileMode)) == {'skip', 'prefix'}

    profiles = metadata('ovid-native').get_all('Provides-Extra')
    assert profiles is not None
    assert {'ast', 'search', 'all'} <= set(profiles)
