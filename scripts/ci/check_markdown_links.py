#!/usr/bin/env python3
"""检查 docs/ 与 knowledge/ 下 Markdown 文件里的相对链接是否指向存在的文件。

# 不检查会漏掉什么

规格 §13 要求"过时的知识库文件必须删除或更新，不得留存"。这两个目录是
项目的知识库与规格文档：文件一旦被改名、移动或删除，所有指向它的旧链接
就变成死链——读者点进去只会得到一个 404 或者在编辑器里跳到不存在的路径，
但没有任何自动化机制会告诉你这件事。`clippy`/`cargo test`/`cargo doc`
都不检查 Markdown 文件，这类问题只能靠人工点开每一个链接才能发现，
项目规模变大后必然会漏、会累积。

# 检查什么

- 遍历 `docs/` 与 `knowledge/` 下所有 `*.md` 文件。
- 提取形如 `[文字](路径)` 与 `[文字]: 路径`（引用式链接定义）的链接。
- 只检查**相对路径**链接（不含 `://` 的 scheme、不是 `mailto:`）。
- 去掉 `#anchor` 片段后，相对链接发起文件所在目录解析，检查目标文件/
  目录是否存在。

# 不检查什么（已知局限，非本次范围）

- 不校验锚点（`#some-heading`）本身是否真的存在于目标文件里，只检查
  目标文件本身是否存在——锚点校验需要解析目标 Markdown 的标题树，
  误报风险更高，留待后续需要时再做。
- 不检查绝对路径链接（`/foo/bar`）与外部 URL 是否可达（外部链接可能
  因网络原因误报，不适合在 CI 里做强校验）。
- 不检查图片语法 `![alt](path)` 之外、正文中裸露出现的路径字符串。

# 豁免

如果某条链接是刻意保留的历史死链（例如说明"此路径曾经存在，现已废弃"
这种引用本身就是文档内容的一部分），在链接所在行末尾加注释标记
`<!-- link-exempt -->` 放行。

# 围栏代码块

审计类文档经常用 ```` ```markdown ... ``` ```` 引用别的文件里的原文
（比如"现状是这样、应改为这样"），引用出来的文字里可能包含链接语法，
但那只是被引用的文本，不是本文档自己的导航链接，按本文档所在目录解析
必然是错的——这类内容一律跳过，不当作真实链接检查。
"""

from __future__ import annotations

import re
import sys
import urllib.parse
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

REPO_ROOT = Path(__file__).resolve().parents[2]
SCAN_ROOTS = ["docs", "knowledge"]
EXEMPT_MARKER = "link-exempt"

# `[text](target)` 与 `[text]: target`（引用式定义）。
LINK_PATTERN = re.compile(
    r"(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)" r"|^\s*\[[^\]]+\]:\s*(\S+)",
    re.MULTILINE,
)


def is_external_or_special(target: str) -> bool:
    if not target or target.startswith("#"):
        return True
    if "://" in target:
        return True
    if target.startswith("mailto:") or target.startswith("tel:"):
        return True
    if target.startswith("/"):
        # 绝对路径链接：不在本次检查范围（见文件头注释）。
        return True
    return False


def find_dead_links() -> list[tuple[Path, int, str, Path]]:
    dead: list[tuple[Path, int, str, Path]] = []

    for root_name in SCAN_ROOTS:
        root = REPO_ROOT / root_name
        if not root.is_dir():
            continue
        for md_file in sorted(root.rglob("*.md")):
            lines = md_file.read_text(encoding="utf-8", errors="replace").splitlines()
            in_fence = False
            fence_marker = ""
            for line_no, line in enumerate(lines, start=1):
                stripped = line.strip()
                fence_match = re.match(r"^(```+|~~~+)", stripped)
                if fence_match:
                    marker = fence_match.group(1)[:3]
                    if not in_fence:
                        in_fence = True
                        fence_marker = marker
                    elif marker == fence_marker:
                        in_fence = False
                    continue
                if in_fence:
                    continue

                if EXEMPT_MARKER in line:
                    continue
                for match in LINK_PATTERN.finditer(line):
                    target = match.group(1) or match.group(2)
                    if target is None:
                        continue
                    target = target.strip("<>")
                    if is_external_or_special(target):
                        continue

                    target_no_fragment = target.split("#", 1)[0]
                    if not target_no_fragment:
                        continue
                    target_no_fragment = urllib.parse.unquote(target_no_fragment)

                    resolved = (md_file.parent / target_no_fragment).resolve()
                    if not resolved.exists():
                        dead.append((md_file, line_no, target, resolved))

    return dead


def main() -> int:
    dead = find_dead_links()
    if not dead:
        print("未发现 Markdown 死链（docs/、knowledge/）。")
        return 0

    for md_file, line_no, target, resolved in dead:
        rel = md_file.relative_to(REPO_ROOT)
        print(
            f"::error file={rel},line={line_no}::"
            f"死链：链接目标 '{target}' 解析为 '{resolved}'，该文件/目录不存在。"
            f"请修正链接路径，或如果目标文件已被移除，一并更新/删除本条引用"
            f"（规格 §13：过时知识库文件不得留存）。若这是刻意保留的历史引用，"
            f"在行尾加 <!-- link-exempt --> 标记放行。"
        )

    print(f"\n共发现 {len(dead)} 处死链，见上方逐条标注。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
