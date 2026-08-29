#!/usr/bin/env python3
r"""Rust 源码的近似解析与「线格式形状」抽取，供
`scripts/ci/check_save_schema_version.py` 使用。

# 为什么单独一个文件

判据（「形状变了就必须升 `CURRENT_SCHEMA_VERSION`」）与「怎么从 Rust
源码里把形状读出来」是两件独立会变的事：前者是策略，后者是解析细节。
本仓库的文件规模上限是 800 行，两者写在一起已经越界；拆开之后，读判据
的人不必翻过四百行正则，改解析的人也不会碰到策略。

# 解析到什么程度

**不是真正的 Rust 语法解析**，与 `scripts/ci/check_field_consumers.py`
同一类近似方法：去注释 → 正则找 `struct`/`enum`/`type` 行 → 括号配对取
出体。`cfg` 条件编译分支、宏生成的类型可能取错或取不到；取不到的名字会
表现为「不透明叶子」，调用方的 `--dump` 会列出来。

# 「线格式形状」为什么长这样

`postcard` 的字节布局只取决于三件事：字段的**顺序**、字段的**类型**、
枚举变体的**下标**。它**不写字段名**。因此本模块抽出的形状里没有字段
名——改名不改变任何一个字节，把改名判成「形状变了」是纯粹的假红。
详见调用方脚本头注释「线格式形状具体指什么」一节。
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field
from pathlib import Path

BACKSLASH = chr(92)

ITEM_RE = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?P<kind>struct|enum|type)[ \t]+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)",
    re.M,
)
IMPL_RE = re.compile(
    r"^[ \t]*impl(?:<[^>]*>)?[ \t]+(?P<trait>Serialize|Deserialize)"
    r"(?:<[^>]*>)?[ \t]+for[ \t]+(?P<name>[A-Za-z_][A-Za-z0-9_]*)",
    re.M,
)
IDENT_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*)\b")
WS_RE = re.compile(r"\s+")
ATTR_RE = re.compile(r"#\[[^\[\]]*(?:\[[^\[\]]*\][^\[\]]*)*\]")

# 会改变线格式的字段级 serde 属性。`default`/`rename` 之类刻意不计入：
# 前者在 postcard 上是空操作（本门禁存在的原因），后者只影响自描述格式。
WIRE_ATTR_KEYS = (
    "with",
    "serialize_with",
    "deserialize_with",
    "skip_serializing_if",
    "flatten",
)


# --------------------------------------------------------------------------
# 词法层
# --------------------------------------------------------------------------


def strip_comments(src: str) -> str:
    """去掉行注释与块注释，保留字符串字面量与换行结构。"""
    out: list[str] = []
    i, n = 0, len(src)
    in_str = False
    escaped = False
    while i < n:
        c = src[i]
        if in_str:
            out.append(c)
            if escaped:
                escaped = False
            elif c == BACKSLASH:
                escaped = True
            elif c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
            out.append(c)
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            if j < 0:
                i = n
            else:
                out.append("\n")
                i = j + 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            i = n if j < 0 else j + 2
            out.append(" ")
            continue
        out.append(c)
        i += 1
    return "".join(out)


def split_top(text: str) -> list[str]:
    """按嵌套深度为 0 的逗号切分。"""
    parts: list[str] = []
    depth = 0
    cur = ""
    for c in text:
        if c in "<([{":
            depth += 1
        elif c in ">)]}":
            depth -= 1
        if c == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += c
    if cur.strip():
        parts.append(cur)
    return parts


def matched_block(src: str, open_idx: int) -> tuple[str, int]:
    """从 `open_idx` 处的开括号做配对，返回 (体文本, 闭括号下标)。"""
    open_c = src[open_idx]
    close_c = {"{": "}", "(": ")"}[open_c]
    depth = 0
    k = open_idx
    while k < len(src):
        if src[k] == open_c:
            depth += 1
        elif src[k] == close_c:
            depth -= 1
            if depth == 0:
                break
        k += 1
    return src[open_idx + 1 : k], k


def collect_attrs(src: str, start: int) -> list[str]:
    """从类型定义的起始位置往回收集紧邻的属性。

    刻意不按行走：本仓库里 `#[derive(...)]` 常常换行写（例如
    `crates/ll-world/src/ownership.rs` 的 `Owner`），逐行判断「这一行是
    不是以 `#[` 开头」会把多行属性整个漏掉——漏掉的后果是「某个类型悄悄
    不再派生 `Serialize`」这类变化不进指纹。这里改成从右往左做方括号
    配对，多行与单行一视同仁。
    """
    attrs: list[str] = []
    i = start - 1
    while i >= 0:
        while i >= 0 and src[i].isspace():
            i -= 1
        if i < 0 or src[i] != "]":
            break
        depth = 0
        j = i
        while j >= 0:
            if src[j] == "]":
                depth += 1
            elif src[j] == "[":
                depth -= 1
                if depth == 0:
                    break
            j -= 1
        if j <= 0 or src[j - 1] != "#":
            break
        attrs.insert(0, WS_RE.sub(" ", src[j - 1 : i + 1]))
        i = j - 2
    return attrs


# --------------------------------------------------------------------------
# 类型定义索引
# --------------------------------------------------------------------------


@dataclass
class TypeDef:
    """一个 `struct`/`enum`/`type` 定义的近似解析结果。"""

    kind: str  # "struct" | "enum" | "alias"
    name: str
    file: str
    line: int
    attrs: list[str]
    body: str
    body_kind: str  # '{' | '(' | ';'
    alias_target: str = ""
    manual_serde: dict[str, str] = field(default_factory=dict)


def _parse_alias(src: str, m: re.Match, rel: str, line_idx: int) -> TypeDef:
    end = src.find(";", m.end())
    rhs = src[m.end() : end] if end > 0 else ""
    rhs = rhs.split("=", 1)[1] if "=" in rhs else ""
    return TypeDef(
        kind="alias",
        name=m.group("name"),
        file=rel,
        line=line_idx + 1,
        attrs=[],
        body="",
        body_kind=";",
        alias_target=WS_RE.sub(" ", rhs).strip(),
    )


def parse_defs(src: str, rel: str) -> list[TypeDef]:
    """抽取一份（已去注释的）源码里的全部类型定义。"""
    defs: list[TypeDef] = []
    for m in ITEM_RE.finditer(src):
        line_idx = src.count("\n", 0, m.start())
        if m.group("kind") == "type":
            defs.append(_parse_alias(src, m, rel, line_idx))
            continue
        j = m.end()
        while j < len(src) and src[j] not in "{(;":
            j += 1
        if j >= len(src):
            continue
        body_kind = src[j]
        body = "" if body_kind == ";" else matched_block(src, j)[0]
        defs.append(
            TypeDef(
                kind=m.group("kind"),
                name=m.group("name"),
                file=rel,
                line=line_idx + 1,
                attrs=collect_attrs(src, m.start()),
                body=body,
                body_kind=body_kind,
            )
        )
    return defs


def parse_manual_serde_impls(src: str) -> list[tuple[str, str, str]]:
    """抽取手写的 `impl Serialize/Deserialize for X`，返回 (类型, trait, 正文)。"""
    impls: list[tuple[str, str, str]] = []
    for m in IMPL_RE.finditer(src):
        j = src.find("{", m.end())
        if j < 0:
            continue
        body = matched_block(src, j)[0]
        impls.append((m.group("name"), m.group("trait"), WS_RE.sub(" ", body).strip()))
    return impls


def build_index(repo_root: Path, globs: list[str]) -> tuple[dict[str, list[TypeDef]], list[Path]]:
    """建立「类型名 → 定义列表」索引，并把手写 serde impl 的正文哈希挂上去。"""
    index: dict[str, list[TypeDef]] = {}
    impls: dict[str, dict[str, str]] = {}
    files: list[Path] = []
    for pattern in globs:
        files.extend(sorted(repo_root.glob(pattern)))
    for f in files:
        src = strip_comments(f.read_text(encoding="utf-8"))
        rel = str(f.relative_to(repo_root)).replace(BACKSLASH, "/")
        for d in parse_defs(src, rel):
            index.setdefault(d.name, []).append(d)
        for name, trait, body in parse_manual_serde_impls(src):
            digest = hashlib.sha256(body.encode("utf-8")).hexdigest()[:16]
            impls.setdefault(name, {})[trait] = digest
    for name, per_trait in impls.items():
        for d in index.get(name, []):
            d.manual_serde = dict(per_trait)
    return index, files


# --------------------------------------------------------------------------
# 形状抽取
# --------------------------------------------------------------------------


@dataclass
class Shape:
    kind: str
    container_attrs: list[str]
    members: list[dict]
    manual_serde: dict[str, str]
    refs: set[str] = field(default_factory=set)


def _split_attrs(chunk: str) -> tuple[list[str], str]:
    attrs = ATTR_RE.findall(chunk)
    rest = ATTR_RE.sub("", chunk).strip()
    return [WS_RE.sub(" ", a) for a in attrs], rest


def _is_skipped(attrs: list[str]) -> bool:
    """`#[serde(skip)]` 的成员真的不进字节流，不计入形状。"""
    for a in attrs:
        if "serde(" not in a:
            continue
        inner = a[a.find("(") + 1 : a.rfind(")")]
        for token in split_top(inner):
            if token.strip() == "skip":
                return True
    return False


def _wire_attrs(attrs: list[str]) -> list[str]:
    kept: list[str] = []
    for a in attrs:
        if "serde(" not in a:
            continue
        inner = a[a.find("(") + 1 : a.rfind(")")]
        for token in split_top(inner):
            if token.split("=")[0].strip() in WIRE_ATTR_KEYS:
                kept.append(WS_RE.sub(" ", token.strip()))
    return kept


def _norm_type(text: str) -> str:
    return WS_RE.sub(" ", text.strip())


def container_serde_attrs(attrs: list[str]) -> list[str]:
    """容器级、会影响线格式的属性：`#[serde(..)]` 与是否派生 Serialize/Deserialize。"""
    out: list[str] = []
    for a in attrs:
        if a.startswith("#[serde("):
            out.append(WS_RE.sub(" ", a))
        elif a.startswith("#[derive("):
            inner = a[a.find("(") + 1 : a.rfind(")")]
            traits = sorted(
                {
                    t.strip().rsplit("::", 1)[-1]
                    for t in inner.split(",")
                    if t.strip().rsplit("::", 1)[-1] in ("Serialize", "Deserialize")
                }
            )
            if traits:
                out.append("derive(" + ",".join(traits) + ")")
    return sorted(out)


def redirect_targets(attrs: list[str]) -> list[str]:
    """容器级 `#[serde(try_from/from/into = "X")]` 指向的中转类型。

    漏了这条边等于漏掉真正决定反序列化布局的那半边——`WorldState`
    自己就是这一类（`#[serde(try_from = "WorldStateRepr")]`）。
    """
    targets: list[str] = []
    for a in attrs:
        if not a.startswith("#[serde("):
            continue
        inner = a[a.find("(") + 1 : a.rfind(")")]
        for token in split_top(inner):
            if "=" not in token:
                continue
            key, _, value = token.partition("=")
            if key.strip() in ("try_from", "from", "into"):
                targets.append(value.strip().strip('"'))
    return targets


def _struct_members(defn: TypeDef) -> tuple[list[dict], set[str]]:
    members: list[dict] = []
    refs: set[str] = set()
    idx = 0
    for chunk in split_top(defn.body):
        attrs, rest = _split_attrs(chunk)
        if not rest or _is_skipped(attrs):
            continue
        if defn.body_kind == "{":
            if ":" not in rest:
                continue
            type_text = rest.split(":", 1)[1]
        else:
            type_text = re.sub(r"^pub(?:\([^)]*\))?\s+", "", rest)
        type_text = _norm_type(type_text)
        members.append({"i": idx, "ty": type_text, "attrs": _wire_attrs(attrs)})
        refs.update(IDENT_RE.findall(type_text))
        idx += 1
    return members, refs


def _variant_payload(payload_raw: str) -> list[str]:
    if not (payload_raw.startswith("(") or payload_raw.startswith("{")):
        return []
    named = payload_raw.startswith("{")
    payload: list[str] = []
    for sub in split_top(payload_raw[1:-1]):
        _, sub_rest = _split_attrs(sub)
        sub_rest = sub_rest.strip()
        if not sub_rest:
            continue
        if named and ":" in sub_rest:
            sub_rest = sub_rest.split(":", 1)[1]
        payload.append(_norm_type(sub_rest))
    return payload


def _enum_members(defn: TypeDef) -> tuple[list[dict], set[str]]:
    members: list[dict] = []
    refs: set[str] = set()
    idx = 0
    for chunk in split_top(defn.body):
        attrs, rest = _split_attrs(chunk)
        rest = rest.strip()
        if not rest or _is_skipped(attrs):
            continue
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s*(.*)$", rest, re.S)
        if not m:
            continue
        payload = _variant_payload(m.group(2).strip())
        members.append({"i": idx, "payload": payload, "attrs": _wire_attrs(attrs)})
        for p in payload:
            refs.update(IDENT_RE.findall(p))
        idx += 1
    return members, refs


def shape_of(defn: TypeDef) -> Shape:
    """求一个类型的线格式形状，以及它指向的类型名集合（闭包的边）。"""
    container = container_serde_attrs(defn.attrs)
    refs: set[str] = set()
    for target in redirect_targets(defn.attrs):
        refs.update(IDENT_RE.findall(target))

    if defn.kind == "alias":
        refs.update(IDENT_RE.findall(defn.alias_target))
        return Shape("alias", container, [{"target": defn.alias_target}], defn.manual_serde, refs)

    if defn.kind == "struct":
        members, member_refs = _struct_members(defn)
    else:
        members, member_refs = _enum_members(defn)
    refs.update(member_refs)
    return Shape(defn.kind, container, members, defn.manual_serde, refs)
