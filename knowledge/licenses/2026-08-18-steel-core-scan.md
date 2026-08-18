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

## 决策与执行（2026-08-18，项目所有者裁定）

### 甲案：许可证白名单加入 MPL-2.0

**裁定**：`deny.toml` 加入 `MPL-2.0`，接受这个许可证进入依赖树。

**理由（与 GPL 的实质区别）**：

| | GPL | MPL-2.0 |
|---|---|---|
| 链接进闭源程序 | 会传染 | 明确允许（§3.3） |
| 必须公开自身源码 | 是 | 否 |
| 义务范围 | 整个衍生作品 | 仅限被修改的那些文件 |

项目的许可证规矩原文是「宽松许可，无 GPL、无付费」——MPL-2.0 是文件级
弱 copyleft，不是 GPL 那种项目级传染。**唯一义务**：若修改
`im-rc`/`sized-chunks`/`bitmaps` 的源文件，须以 MPL-2.0 发布那些修改；
本项目不修改它们，义务不触发。

**边界（写清楚给后来者看）**：将来若真要 fork 或 patch 这三个库中的
任何一个，义务即刻生效——这是加白名单时接受的代价，见 `deny.toml`
对应注释。

### 4 条 unmaintained 公告：单独处理，不与许可证问题混在一起

**这是「无人维护」，不是「有漏洞」**——`RUSTSEC-2025-0141`（bincode）、
`RUSTSEC-2026-0247`（bitmaps）、`RUSTSEC-2026-0250`（im-rc）、
`RUSTSEC-2026-0251`（sized-chunks）四条公告的性质都是「上游仓库已归档/
团队停止维护」，当前**没有已知安全漏洞**。已在 `deny.toml` 的
`[advisories] ignore` 里逐条列出并注明理由，与许可证白名单是两个独立的
变更，性质不同不合并处理。

**重新评估触发条件**：以上任意一条若被 RustSec 升级成真实的漏洞类
公告（advisory 类型从 unmaintained 变成 vulnerability），必须立刻从
`ignore` 列表移除，重新评估影响面，不能到时候顺手继续忽略。

### 最终结果

```
advisories ok, bans ok, licenses ok, sources ok
```

四项全过。`deny.toml` 的改动本身（新增许可证 + advisories ignore 列表）
单独一次提交，附完整理由注释——不是「加一行了事」。
