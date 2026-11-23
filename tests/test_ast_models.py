from typing import get_args

import pytest
from pydantic import ValidationError

from ovid_native.ast import (
    AstIssueKind,
    AstLimits,
    AstPosition,
    AstRewritePreviewRequest,
    AstScanOptions,
    AstSearchRequest,
    AstStrictness,
    ast_grep_version,
    supported_ast_languages,
)


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
