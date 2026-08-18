# 许可证与公告扫描 — P4 引入 `steel-core`

**日期**：2026-08-18
**命令**：`cargo deny check`

## 结果

```
advisories FAILED, bans ok, licenses FAILED, sources ok
```

**四项未能全过。** 按项目纪律（`knowledge/decisions/0001-steel-sandbox-verification.md`
所在批次的交接要求：「若发现许可证或安全公告问题，停下来报告，不要自行豁免」），
如实记录，**没有修改 `deny.toml` 去豁免以下发现**。

## `licenses` 失败：3 个 MPL-2.0 许可的传递依赖

`steel-core` 0.8.2 默认 feature 集（未开 `sync`）传递依赖 `im-rc`
（不可变数据结构库），而 `im-rc` 又依赖 `sized-chunks`、`bitmaps`。三者
均声明 `license = "MPL-2.0+"`：

```
bitmaps v2.1.0
├── im-rc v15.1.0
│   └── steel-core v0.8.2
│       └── ll-script v0.1.0
└── sized-chunks v0.6.5
    └── im-rc v15.1.0 (*)

im-rc v15.1.0
└── steel-core v0.8.2
    └── ll-script v0.1.0

sized-chunks v0.6.5
└── im-rc v15.1.0
    └── steel-core v0.8.2
        └── ll-script v0.1.0
```

`deny.toml` 白名单目前是 MIT / Apache-2.0 / Apache-2.0 WITH LLVM-exception /
BSD-2-Clause / BSD-3-Clause / ISC / Zlib / Unicode-3.0 / OFL-1.1，**不包含
MPL-2.0**。MPL-2.0 是文件级弱 copyleft（不同于 GPL 的项目级传染），比
GPL 宽松得多，但仍然是一种 copyleft，不在当前白名单内，需要人工判断是否
接受。

**未尝试规避**：切到 `steel-core` 的 `sync` feature 会把 `im-rc` 换成
`im`（同一作者 bodil 的另一个仓库），大概率是同样的 MPL-2.0 许可，未实测
验证；且 `sync` feature 会改变 `register_builtin_modules` 里一整段模块
注册路径（ADR 0012 记录的 `#[cfg(feature = "sync")]` 分支），牵动的面
比许可证问题本身更大，不应该为了绕开许可证审查顺手切一个从未验证过的
feature 组合。

## `advisories` 失败：4 条「已无人维护」公告

同样来自 `im-rc` 这条依赖链，均为 `unmaintained` 级别（不是已知漏洞，是
上游仓库已归档、不再更新）：

| 依赖 | 公告 | 说明 |
|---|---|---|
| `bincode` 1.3.3 | RUSTSEC-2025-0141 | 团队因骚扰事件停止维护；1.3.3 被其自身认定为「完整版本」 |
| `bitmaps` 2.1.0 | RUSTSEC-2026-0247 | 仓库已于 2026-05-03 归档 |
| `im-rc` 15.1.0 | RUSTSEC-2026-0250 | 仓库已于 2026-05-03 归档；`imbl` 是 `im`（非 `im-rc`）的维护中分支 |
| `sized-chunks` 0.6.5 | RUSTSEC-2026-0251 | 仓库已于 2026-05-03 归档 |

`deny.toml` 目前只配置了 `yanked = "deny"`，没有为 `unmaintained` 级别
显式配置策略——本次实测这四条在默认策略下即报 `FAILED`，而不是降级成
警告。

`bincode` 是 `steel-core` 自身序列化机制用的依赖（`register_builtin_modules`
调用链外，属于 steel-core 内部用它做常量池/字节码序列化），不是通过
`im-rc` 引入的，是独立的第五条问题依赖，但恰好也在这次扫描里一并报出。

## 其余两项

`bans ok`：`multiple-versions = "warn"` 口径下无新阻断（`bitflags` 1.3.2
与 2.13.1 并存的既有警告仍在，见 P0 扫描记录，未加重）。

`sources ok`：无新增来源。

## 结论与建议（留给项目所有者裁定）

**这是一个需要人工决策的真实阻断项，本次没有自行豁免。** 三条可能路径：

1. **把 MPL-2.0 加入 `deny.toml` 白名单，并接受四条 `unmaintained` 公告**
   （可以给 `[advisories]` 加 `ignore = [...]` 逐条豁免并写明理由）——
   最快，但需要项目所有者对「弱 copyleft + 已无人维护的传递依赖」这个
   风险组合点头。
2. **寻找不依赖 `im-rc` 的 Steel 发行方式**——未探明是否存在（`sync`
   feature 换 `im` 大概率同样受影响，见上）。
3. **放弃 Steel，换脚本引擎**——影响范围最大，会推翻 ADR 0001/0012
   已经做的全部实测工作。

本次扫描前，`ll-script` 相关的三次提交（`docs:` 探针结果、`feat:`
ScriptEngine、`feat:` 内存守卫、以及 mod API 表面的实现）均已完成并通过
`cargo fmt`/`cargo clippy`/`cargo test`，**唯独 `cargo deny check` 这一步
未通过**——如实报告，不倒填结果。
