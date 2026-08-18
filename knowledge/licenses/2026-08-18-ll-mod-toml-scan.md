# 许可证扫描 — ll-mod 引入 toml/serde

**日期**：2026-08-18
**提交**：见本文件所在提交（`feat: ll-mod 内容注册表核心`）
**命令**：`cargo deny check`（在共享工作区、`ll-text` 同批并行开发的状态下执行——见下方「执行时的工作区状态」）

## 背景

`ll-mod` 的 mod 清单解析（task-6-brief.md）采用 TOML 格式，新增两个顶层依赖：

- `serde`（`derive` feature）——已是工作区既有依赖（`ll-core` 的可选 feature、
  `ll-world`/`ll-sim` 等已启用），这里只是 `ll-mod` 自己新增一份直接依赖声明，
  不引入新的许可证面。
- `toml` 0.9 —— 新引入。传递依赖新增 `toml_datetime`、`toml_writer`、
  `toml_parser`、`winnow`（0.7 与 1.0 两个版本并存）、`serde_spanned`。

## 许可证结果

逐一核实（`cargo deny check` 的 `licenses` 项 + 本地缓存的 `Cargo.toml` 原文
交叉核对）：

| crate | 版本 | 许可证（SPDX） | 白名单内 |
|---|---|---|---|
| `toml` | 0.9.12+spec-1.1.0 | `MIT OR Apache-2.0` | 是 |
| `toml_datetime` | 0.7.5+spec-1.1.0 | `MIT OR Apache-2.0` | 是 |
| `toml_writer` | 1.1.2+spec-1.1.0 | `MIT OR Apache-2.0` | 是 |
| `toml_parser` | 1.1.3+spec-1.1.0 | `MIT OR Apache-2.0` | 是 |
| `winnow` | 1.0.4 / 0.7.15 | `MIT` | 是 |
| `serde_spanned` | 1.1.1 | `MIT OR Apache-2.0` | 是 |

六项全部落在 `deny.toml` 现有白名单（`MIT`、`Apache-2.0`），**不需要修改
`deny.toml`**。`cargo deny check` 的 `licenses` 项本身也报告 `ok`。

`winnow` 出现两个版本（0.7.15 由 `toml` 引入，1.0.4 由 `ll-text` 的
`toml_edit`/`cosmic-text` 依赖链引入）——`bans.multiple-versions = "warn"`
下这只是一条非阻断警告，两个版本许可证一致，不构成风险。

## 执行时的工作区状态（如实记录）

本次 `cargo deny check` 是在 `ll-text` crate 同批并行开发、尚未完成的状态下
跑的（另一位代理正在编辑 `crates/ll-text/`）。结果：

```
advisories FAILED, bans ok, licenses ok, sources ok
```

`advisories` 失败**与本次改动无关**：失败源是 `RUSTSEC-2026-0192`
（`ttf-parser` 停止维护），来自 `ll-text → cosmic-text → fontdb → ttf-parser`
这条依赖链，不在 `ll-mod` 引入的任何依赖路径上。`ll-mod` 自身引入的
`toml`/`serde` 依赖链没有触发任何 advisory。这条失败留给 `ll-text` 那批工作
自行处理（不是本次改动的范围，也不应该在这里代为豁免）。
