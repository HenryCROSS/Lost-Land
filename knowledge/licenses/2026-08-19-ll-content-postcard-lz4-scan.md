# 许可证 / 安全公告扫描 — ll-content 引入 postcard/lz4_flex

**日期**：2026-08-19
**提交**：见本文件所在提交（`feat: 新建 ll-content crate 与存档头骨架`）
**命令**：`cargo deny check`

## 背景

规格 §11.2 指定存档主体用 `postcard` 编码、`lz4_flex` 压缩。P5-A 计划任务 1
（`.superpowers/sdd/2026-08-19-p5-save-format-and-identity/task-1-brief.md`
对应的实施计划条目）要求本任务在 `Cargo.toml` 声明这两个依赖并核实
`cargo deny check` 通过，不能假设"规格写了就一定过"。

## 第一次尝试：默认 feature 触发 unmaintained 公告

最初声明为 `postcard = { version = "1", features = ["alloc"] }`（不关闭默认
feature）。`cargo deny check` 报告：

```
error[unmaintained]: atomic-polyfill is unmaintained
├ ID: RUSTSEC-2023-0089
├ atomic-polyfill v1.0.3
  └── heapless v0.7.17
      └── postcard v1.1.3
          └── ll-content v0.1.0
advisories FAILED, bans ok, licenses ok, sources ok
```

根因：`postcard` 的 `default = ["heapless-cas"]`（见其 `Cargo.toml.orig`
`[features]` 一节）无条件拉入 `heapless`，而 `heapless 0.7.17` 依赖
`atomic-polyfill`——该 crate 已被作者归档，RUSTSEC-2023-0089 标记为
unmaintained、且**无安全升级路径**（"No safe upgrade is available!"）。

**未采用「加入 `deny.toml` 忽略列表」的方案**：`deny.toml` 现有的 4 条
unmaintained 忽略（steel-core 依赖链）都是"该依赖链本身别无选择"的情形；
这里不是——`postcard` 自己的 `heapless` feature 完全是可选的，本项目是纯
桌面应用，不需要它面向的 no_std/嵌入式场景，能够也应该直接避开，而不是
把一条本可避免的 unmaintained 依赖也堆进豁免列表。

## 修正：关闭默认 feature，改用 `use-std`

```toml
postcard = { version = "1", default-features = false, features = ["use-std"] }
lz4_flex = "0.11"
```

`use-std = ["serde/std", "alloc"]`（`postcard` 自身 `[features]` 定义）足以
提供 `to_allocvec`/`from_bytes` 这类基于 `Vec<u8>`/`std` 的 API，且不触碰
`heapless`/`heapless-cas` 这两个 feature，从依赖树上彻底移除
`heapless`→`atomic-polyfill` 这条链。`cargo tree -p ll-content` 核实
`heapless`/`atomic-polyfill` 均不再出现在依赖树中。

## 许可证结果

| crate | 版本 | 许可证（SPDX） | 白名单内 |
|---|---|---|---|
| `postcard` | 1.1.3 | `MIT OR Apache-2.0` | 是 |
| `cobs`（postcard 依赖） | 0.3.0 | `MIT OR Apache-2.0` | 是 |
| `thiserror`（cobs 依赖，仅 postcard 内部使用，不影响本项目手写错误类型的既有约定） | 2.0.20 | `MIT OR Apache-2.0` | 是 |
| `lz4_flex` | 0.11.6 | `MIT` | 是 |
| `twox-hash`（lz4_flex 依赖） | 2.1.3 | `MIT` | 是 |

全部落在 `deny.toml` 现有白名单（`MIT`、`Apache-2.0`），**不需要修改
`deny.toml`**。

## 最终结果

```
advisories ok, bans ok, licenses ok, sources ok
```

`cargo deny check` 退出码 0。仍存在的告警（`winnow` 两个版本并存、
`arrayvec` 两个版本并存等）与本次改动无关——分别来自 `ll-mod` 既有的
`toml` 依赖与 `ll-render`/`ll-script` 既有的图形/脚本依赖链，
`bans.multiple-versions = "warn"` 下均为非阻断警告，早于本次改动已存在。

## 执行时的工作区状态

单机顺序开发，无并行编辑 `Cargo.toml` 的其他任务。
