import json
from dataclasses import dataclass
from typing import Any, cast

from lark import Lark, Token, Tree, UnexpectedInput

from ovid_native import _native
from ovid_native.workspace.errors import WorkspacePatchError


HASHLINE_GRAMMAR = r"""
start: "*** Begin Patch" _NL section+ "*** End Patch" _NL?
section: header _NL statement+
header: HEADER
?statement: put_body | put_register | cut | remove | move
put_body: "PUT" _WS locator ":" _NL body+
put_register: "PUT" _WS locator (_WS REGISTER)? _NL
cut: "CUT" _WS cut_locator (_WS REGISTER)? _NL
remove: "REM" _NL
move: "MV" _WS destination _NL
?locator: range | block | before | after_block | after | beginning | ending
?cut_locator: range | block
range: LINE_HASH ".=" LINE_HASH      -> inclusive_range
block: LINE_HASH "*"                  -> syntax_block
before: "<" LINE_HASH                 -> before_line
after_block: ">" LINE_HASH "*"       -> after_syntax_block
after: ">" LINE_HASH                  -> after_line
beginning: "<^"                       -> beginning
ending: ">$"                          -> ending
body: BODY _NL
destination: JSON_STRING | DEST
HEADER: /\[(?:\\.|[^\]\r\n])+\#[0-9A-Fa-f]{4}\]/
LINE_HASH: /[1-9][0-9]*:[0-9A-Fa-f]{2}/
REGISTER: /@[A-Za-z0-9_][A-Za-z0-9_-]{0,63}/
BODY: /\+[^\r\n]*/
DEST: /(?:\\.|[^\s\r\n])+/
JSON_STRING: /"(?:\\.|[^"\\])*"/
_WS: /[ \t]+/
_NL: /\r?\n/
"""
_PARSER = Lark(HASHLINE_GRAMMAR, parser='lalr', lexer='contextual')
_MAX_INPUT_BYTES = 1024 * 1024
_MAX_OPERATIONS = 1024


@dataclass(frozen=True, slots=True)
class _Locator:
    kind: str
    start: int | None = None
    start_hash: str | None = None
    end: int | None = None
    end_hash: str | None = None


def parse_hashline(value: str) -> list[_native.NativeHashlineSection]:
    if len(value.encode()) > _MAX_INPUT_BYTES:
        raise WorkspacePatchError('Hashline input exceeds the 1048576-byte limit')

    try:
        document = _PARSER.parse(value)
    except UnexpectedInput as error:
        raise WorkspacePatchError(f'Malformed Hashline input at line {error.line}, column {error.column}') from error

    sections = [_section(cast(Tree[Any], child)) for child in document.children]
    operation_count = sum(len(section[2]) for section in sections)
    if operation_count > _MAX_OPERATIONS:
        raise WorkspacePatchError(f'Hashline input exceeds the {_MAX_OPERATIONS}-operation limit')
    paths = [section[0] for section in sections]
    if len(paths) != len(set(paths)):
        raise WorkspacePatchError('Hashline input contains duplicate path sections')
    return sections


def _section(tree: Tree[Any]) -> _native.NativeHashlineSection:
    header = cast(Tree[Any], tree.children[0])
    path, tag = _header(cast(Token, header.children[0]))
    operations = [_operation(cast(Tree[Any], child)) for child in tree.children[1:]]
    return path, tag, operations


def _header(token: Token) -> tuple[str, str]:
    inner = str(token)[1:-1]
    path_value, tag = inner.rsplit('#', 1)
    path = _unescape(path_value, kind='header path')
    return path, tag.upper()


def _operation(tree: Tree[Any]) -> _native.NativeHashlineOperation:
    kind = str(tree.data)
    if kind == 'remove':
        return _native_operation('remove')
    if kind == 'move':
        destination = _destination(cast(Tree[Any], tree.children[0]))
        return _native_operation('move', destination=destination)

    locator = _locator(cast(Tree[Any], tree.children[0]))
    if kind == 'put_body':
        body = tuple(str(cast(Tree[Any], child).children[0])[1:] for child in tree.children[1:])
        return _native_operation(locator.kind, locator=locator, body=body)
    register = _register(cast(Token, tree.children[1])) if len(tree.children) > 1 else None
    operation_kind = f'cut_{locator.kind.removeprefix("put_")}' if kind == 'cut' else locator.kind
    return _native_operation(operation_kind, locator=locator, register=register)


def _locator(tree: Tree[Any]) -> _Locator:
    kind = str(tree.data)
    if kind == 'beginning':
        return _Locator(kind='put_begin')
    if kind == 'ending':
        return _Locator(kind='put_end')

    start, start_hash = _line_hash(cast(Token, tree.children[0]))
    if kind == 'inclusive_range':
        end, end_hash = _line_hash(cast(Token, tree.children[1]))
        return _Locator('put_range', start, start_hash, end, end_hash)
    kinds = {
        'syntax_block': 'put_block',
        'before_line': 'put_before',
        'after_line': 'put_after',
        'after_syntax_block': 'put_after_block',
    }
    return _Locator(kinds[kind], start, start_hash)


def _native_operation(
    kind: str,
    *,
    locator: _Locator | None = None,
    body: tuple[str, ...] = (),
    register: str | None = None,
    destination: str | None = None,
) -> _native.NativeHashlineOperation:
    locator = locator or _Locator(kind)
    return (
        kind,
        locator.start,
        locator.start_hash,
        locator.end,
        locator.end_hash,
        list(body),
        register,
        destination,
    )


def _line_hash(token: Token) -> tuple[int, str]:
    line, short_hash = str(token).split(':', 1)
    return int(line), short_hash.upper()


def _register(value: Token) -> str:
    return str(value)[1:]


def _destination(tree: Tree[Any]) -> str:
    value = str(cast(Token, tree.children[0]))
    if value.startswith('"'):
        try:
            decoded = json.loads(value)
        except json.JSONDecodeError as error:
            raise WorkspacePatchError('Malformed Hashline move destination') from error
        return decoded
    return _unescape(value, kind='move destination')


def _unescape(value: str, *, kind: str) -> str:
    result: list[str] = []
    index = 0
    while index < len(value):
        if value[index] != '\\':
            result.append(value[index])
            index += 1
            continue
        if index + 1 == len(value) or value[index + 1] not in ('\\', ']'):
            raise WorkspacePatchError(f'Hashline {kind} contains an invalid escape')
        result.append(value[index + 1])
        index += 2
    return ''.join(result)
