from datetime import UTC, datetime
from importlib.metadata import metadata
from typing import get_args

import pytest
from ovid_core.models import BaseModel
from pydantic import ValidationError

from ovid_native.ast import (
    AstCapture,
    AstChange,
    AstFileChange,
    AstIssue,
    AstIssueKind,
    AstLanguageInfo,
    AstLimits,
    AstMatch,
    AstPosition,
    AstRange,
    AstRewriteApplyRequest,
    AstRewriteApplyResult,
    AstRewriteApplyToolContent,
    AstRewriteApplyToolResult,
    AstRewriteOperation,
    AstRewritePreview,
    AstRewritePreviewRequest,
    AstRewritePreviewToolContent,
    AstRewritePreviewToolResult,
    AstScanOptions,
    AstSearchRequest,
    AstSearchResult,
    AstSearchToolContent,
    AstSearchToolResult,
    AstStrictness,
    ast_grep_version,
    supported_ast_languages,
)


PUBLIC_MODELS = (
    AstLanguageInfo,
    AstPosition,
    AstRange,
    AstCapture,
    AstIssue,
    AstScanOptions,
    AstLimits,
    AstSearchRequest,
    AstMatch,
    AstSearchResult,
    AstRewriteOperation,
    AstRewritePreviewRequest,
    AstChange,
    AstFileChange,
    AstRewritePreview,
    AstRewriteApplyRequest,
    AstRewriteApplyResult,
    AstSearchToolContent,
    AstRewritePreviewToolContent,
    AstRewriteApplyToolContent,
    AstSearchToolResult,
    AstRewritePreviewToolResult,
    AstRewriteApplyToolResult,
)


def test_public_models_are_immutable_strict_ovid_models() -> None:
    for model in PUBLIC_MODELS:
        assert issubclass(model, BaseModel)
        assert model.model_config['frozen'] is True
        assert model.model_config['extra'] == 'forbid'

    position = AstPosition(line=1, column=1, byte_offset=0)
    with pytest.raises(ValidationError):
        AstPosition(line=1, column=1, byte_offset=0, extra=True)
    with pytest.raises(ValidationError):
        position.line = 2


def test_models_validate_bounds_and_range_descriptions() -> None:
    with pytest.raises(ValidationError):
        AstLimits(max_matches=0)
    with pytest.raises(ValidationError):
        AstSearchRequest(pattern='$A', limit=0)
    with pytest.raises(ValidationError):
        AstRewritePreviewRequest(operations=(), scan=AstScanOptions())

    fields = AstPosition.model_fields
    assert fields['line'].description == 'One-based source line.'
    assert fields['column'].description == 'One-based Unicode source column.'
    assert fields['byte_offset'].description == 'Zero-based UTF-8 byte offset.'


def test_supported_language_metadata_and_embedded_version() -> None:
    languages = supported_ast_languages()
    python = next(language for language in languages if language.identifier == 'python')
    assert {'py', 'python'} <= set(python.aliases)
    assert {'py', 'pyi'} <= set(python.extensions)
    assert ast_grep_version() == '0.45.1'
    assert set(get_args(AstStrictness)) == {'cst', 'smart', 'ast', 'relaxed', 'signature', 'template'}
    assert 'parse_error' in get_args(AstIssueKind)


def test_distribution_exposes_ast_and_all_profiles() -> None:
    profiles = metadata('ovid-native').get_all('Provides-Extra')
    assert profiles is not None
    assert {'ast', 'all'} <= set(profiles)


def test_rewrite_models_preserve_timezone_and_hashes() -> None:
    change = AstFileChange(path='a.py', original_sha256='a' * 64, updated_sha256='b' * 64, replacements=1)
    preview = AstRewritePreview(
        proposal_id='proposal',
        changes=(),
        files=(change,),
        total_replacements=1,
        files_searched=1,
        expires_at=datetime.now(UTC),
    )
    result = AstRewriteApplyResult(proposal_id=preview.proposal_id, files=preview.files, total_replacements=1)
    assert result.files[0].updated_sha256 == 'b' * 64
