#!/usr/bin/env bash
# 许可证与安全公告扫描：对应 .github/workflows/ci.yml `licenses` job 的
# 「许可证与安全公告扫描」步骤。
#
# 依赖 `cargo-deny`（CI 用 `taiki-e/install-action@cargo-deny` 安装；
# 本地请 `cargo install cargo-deny --locked`）。检查内容由仓库根
# `deny.toml` 决定，本脚本不重复那份配置，只是同一条命令的单一入口。
set -euo pipefail

cargo deny check
