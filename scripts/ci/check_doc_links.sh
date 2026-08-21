#!/usr/bin/env bash
# 文档断链检查：对应 .github/workflows/ci.yml `docs-and-links` job 的
# 「文档断链检查」步骤。
#
# 规格 §13：「代码卫生（持续执行）……文档与代码不一致即视为缺陷。」
# `cargo doc` 断链历史上真实出现过，`clippy`/`cargo test` 都不检查，只能
# 靠人工点开每一条文档链接核实，代码规模变大后必然会漏。
#
# 必须带 `--document-private-items`：本项目此前本地验证一直没加这个
# 参数，导致一批只在私有项之间互相引用、CI 才会检出的断链在多次提交里
# 从未被本地发现（2026-08 那次事故的直接起因）。`RUSTDOCFLAGS` 里
# `-D rustdoc::broken_intra_doc_links` 是真正的「断链」阻断项——链接目标
# 根本不存在；`-A rustdoc::private_intra_doc_links` 把「公开文档链到私有
# 项」单独降级为警告，这是可见性设计问题不是真正死链，调整 `pub` 或加
# `--document-private-items` 就能让它消失，噪音大、价值低，因此保持
# 与 CI 逐字一致的降级处理，不在本脚本里收紧或放宽。
set -euo pipefail

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export RUSTDOCFLAGS="${RUSTDOCFLAGS:--D rustdoc::broken_intra_doc_links -A rustdoc::private_intra_doc_links}"

cargo doc --workspace --no-deps --document-private-items
