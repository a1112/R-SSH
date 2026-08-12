"""Canonical Rust #[test] function-body extraction for Task 10 provenance."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass


BODY_CODEC = "rust-itemfn-block-lf/v1"


class BodyCodecError(ValueError):
    pass


@dataclass(frozen=True)
class Token:
    text: str
    start: int
    end: int
    identifier: bool = False


def test_body_sha256s(source_bytes: bytes) -> dict[str, str]:
    source = source_bytes.decode("utf-8").replace("\r\n", "\n")
    bodies = test_bodies(source)
    return {
        name: hashlib.sha256(body.encode("utf-8")).hexdigest()
        for name, body in bodies.items()
    }


def test_bodies(source: str) -> dict[str, str]:
    tokens = _tokens(source)
    matches: dict[str, str] = {}
    pending_test = False
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.text == "#" and index + 1 < len(tokens) and tokens[index + 1].text == "[":
            end = _matching(tokens, index + 1, "[", "]")
            attribute = tokens[index + 2 : end]
            if len(attribute) == 1 and attribute[0].identifier and attribute[0].text == "test":
                pending_test = True
            index = end + 1
            continue
        if token.identifier and token.text == "fn":
            if not pending_test:
                index += 1
                continue
            name_index = index + 1
            if name_index >= len(tokens) or not tokens[name_index].identifier:
                context = source[token.start : token.start + 80].replace("\n", "\\n")
                raise BodyCodecError(
                    f"function without an identifier at byte {token.start}: {context}"
                )
            body_start = _function_body_start(tokens, name_index + 1)
            body_end = _matching(tokens, body_start, "{", "}")
            name = tokens[name_index].text
            body = source[tokens[body_start].start : tokens[body_end].end]
            if name in matches:
                raise BodyCodecError(f"duplicate #[test] function name: {name}")
            matches[name] = body
            pending_test = False
        index += 1
    return matches


def _function_body_start(tokens: list[Token], start: int) -> int:
    parentheses = 0
    brackets = 0
    for index in range(start, len(tokens)):
        text = tokens[index].text
        if text == "(":
            parentheses += 1
        elif text == ")":
            parentheses -= 1
        elif text == "[":
            brackets += 1
        elif text == "]":
            brackets -= 1
        elif text == "{" and parentheses == 0 and brackets == 0:
            return index
        elif text == ";" and parentheses == 0 and brackets == 0:
            break
    raise BodyCodecError("#[test] function without a body")


def _matching(tokens: list[Token], start: int, opening: str, closing: str) -> int:
    if tokens[start].text != opening:
        raise BodyCodecError(f"expected {opening}")
    depth = 0
    for index in range(start, len(tokens)):
        if tokens[index].text == opening:
            depth += 1
        elif tokens[index].text == closing:
            depth -= 1
            if depth == 0:
                return index
    raise BodyCodecError(f"unterminated {opening}")


def _tokens(source: str) -> list[Token]:
    tokens: list[Token] = []
    index = 0
    while index < len(source):
        char = source[index]
        if char.isspace():
            index += 1
            continue
        if source.startswith("//", index):
            newline = source.find("\n", index + 2)
            index = len(source) if newline < 0 else newline + 1
            continue
        if source.startswith("/*", index):
            index = _skip_block_comment(source, index)
            continue
        literal_end = _literal_end(source, index)
        if literal_end is not None:
            index = literal_end
            continue
        if char == "_" or char.isalpha():
            end = index + 1
            while end < len(source) and (source[end] == "_" or source[end].isalnum()):
                end += 1
            tokens.append(Token(source[index:end], index, end, True))
            index = end
            continue
        tokens.append(Token(char, index, index + 1))
        index += 1
    return tokens


def _skip_block_comment(source: str, start: int) -> int:
    depth = 1
    index = start + 2
    while index < len(source):
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
            if depth == 0:
                return index
        else:
            index += 1
    raise BodyCodecError("unterminated block comment")


def _literal_end(source: str, start: int) -> int | None:
    raw = _raw_string_end(source, start)
    if raw is not None:
        return raw
    for prefix in ('b"', 'c"', '"'):
        if source.startswith(prefix, start):
            return _quoted_end(source, start + len(prefix) - 1, '"')
    if source.startswith("b'", start):
        return _quoted_end(source, start + 1, "'")
    if source[start] == "'":
        closing = _quoted_end(source, start, "'", allow_missing=True)
        if closing is not None:
            return closing
    return None


def _raw_string_end(source: str, start: int) -> int | None:
    prefix_end = start
    if source.startswith("br", start) or source.startswith("cr", start):
        prefix_end += 2
    elif source.startswith("r", start):
        prefix_end += 1
    else:
        return None
    hashes = 0
    while prefix_end + hashes < len(source) and source[prefix_end + hashes] == "#":
        hashes += 1
    quote = prefix_end + hashes
    if quote >= len(source) or source[quote] != '"':
        return None
    terminator = '"' + ("#" * hashes)
    end = source.find(terminator, quote + 1)
    if end < 0:
        raise BodyCodecError("unterminated raw string")
    return end + len(terminator)


def _quoted_end(
    source: str, start: int, quote: str, *, allow_missing: bool = False
) -> int | None:
    index = start + 1
    while index < len(source):
        char = source[index]
        if char == "\\":
            index += 2
            continue
        if char == quote:
            return index + 1
        if char == "\n":
            break
        index += 1
    if allow_missing:
        return None
    raise BodyCodecError(f"unterminated {quote} literal")
