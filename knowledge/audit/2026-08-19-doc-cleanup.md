# 文档清理清单（方针变更：删除过时内容，不再标注）

**整理日期**：2026-08-19
**整理范围**：`knowledge/`（全部）、`docs/architecture/`、`docs/qa/`、`docs/superpowers/specs/`、`docs/superpowers/plans/`。
**整理时刻的仓库状态**：`main` 分支，HEAD `22b7c4d`，802 测试全绿，10 个 crate，26 份 ADR。
**方针变更**：上一轮整理（`2026-08-19-stale-annotation-sweep.md`）约定「不删改被取代的内容，只标注『已过时』」。项目所有者本轮改为相反方针：**过时内容直接删除或改写为当前状态，不再加注释**——注释会累积、污染读文档者（含后续代理）的上下文，git 历史已经完整保留了旧内容。**唯一例外**：`knowledge/decisions/` 下的 ADR 不删，只把长篇「已过时」注释块压缩成一行状态指针，正文保持原样。

---

## 一、上一轮加的「已过时」注释块——按新方针处理

| 位置 | 处理 |
|---|---|
| `knowledge/decisions/0012-steel-capability-surface-verification.md` | ADR，**压缩**为标题下方一行状态指针（原 3 行注释块） |
| `knowledge/handoff/p4-to-p5.md`（两处，`#[serde(skip)]` 三处债务 / `terrain_table` 校验点） | **删除注释块，正文改写为当前已解决状态**，不再保留「待办」框架 |
| `docs/architecture/05-integer-discipline.md`（浮点边界规则） | **删除注释块，正文直接改写**为含 ADR 0020 三处允许浮点位置的现行规则 |
| `docs/architecture/discrepancies.md` 条目 8（`TerrainKind` 注册表迁移） | **删除注释块，整条改写**为「已解决」历史记录 |
| `docs/superpowers/specs/2026-08-16-lostland-design.md:39`（§2 决策清单「Mod」行） | **删除内联注释，表格单元格直接改写**为现行「玩法层内成立」文本 |

## 二、落地状态核实并更正

- `knowledge/design/item-system.md` 头部「本文档全部内容仍待 P2/P5 实现验证」→ **改为 P6**（P2 只要求类型布局定稿，P5 从未实现物品系统；`p5-to-p6.md` 已点出此处需要更正）。
- `docs/architecture/discrepancies.md`：条目 1（CI 门禁「9 项只落地约 5 项」）核对 `.github/workflows/ci.yml` 现有 6 个 job 后**改写为「8/9 项已落地，仅 i18n 检查仍是 warn 模式」**；条目 5（季节是否走时间轴事件）核对后**改写为「已裁定为纯函数派生，规格已同步改写」**；条目 7（`resolve` 未实现）核对 `crates/ll-sim/src/resolve.rs` 已存在多个阶段后**改写为「已实现」**；条目 4、6（气候条带、光照透过率）按历轮交接清单复核结果更新阶段认领信息。
- `docs/qa/03-CI门禁对照与补齐方案.md`：这是一份为「9 项只落地 5 项」写的补齐方案，其中三个 job 的 YAML 草案已被基本照抄采纳（提交 `aa4b36e`、`a20889e`）。**整份重写**：删除已实现的补齐方案段落（原 262 行的第 3、4 节，约 190 行），保留现状对照表并更新为当前 6-job 状态，只留两个仍然开放的缺口（i18n 检查转正、死代码检查未接入）。

**最离谱的一处**：`docs/qa/03-CI门禁对照与补齐方案.md` 通篇是"如何补齐 CI 门禁"的详细 YAML 方案，而这些方案实际已经被原样采纳实现——文档却仍停留在"待补齐"的语气，是本轮篇幅删减最大的一份（262 行→约 50 行）。

## 三、交接清单已完成却仍标待办的债务

`knowledge/handoff/p4-to-p5.md` 二节两条 `#[serde(skip)]`/`terrain_table` 债务，见第一节表格——已从「待办 + 事后追加的已过时注释」两段式，改写为一次性陈述当前已解决状态。

## 四、`knowledge/README.md` 索引重建

索引与 `knowledge/` 实际文件逐一核对后重写：

| 类别 | 索引原有条目数 | 实际文件数 | 补齐后 |
|---|---|---|---|
| 决策记录 | 11（0001–0011） | 26（0001–0026） | 26 |
| 许可证 | 1 | 6 | 6 |
| 设计 | 11 | 16（含 `README.md`/`conflicts.md`） | 16 |
| 审计 | 2 | 4（含本文档与上一轮整理清单） | 4 |
| 阶段交接 | 3（p0→p1、p1→p2、p2→p3） | 6（另有 p3→p4、p4→p5、p5→p6） | 6 |

`knowledge/design/README.md`（设计文档总索引）本身也漏了一份：`class-skill-quest-system.md`（职业/技能树/副职/任务系统）从未被并入其「十四份文档」的统一索引——它在核心概念对照表、阅读顺序、落地状态速览三处均无条目。本轮已作为第十五份补齐，索引标题与计数一并更正为「十五份」。

## 五、死链检查

`python3 scripts/ci/check_markdown_links.py`：整理过程中发现并修正 3 处既有死链（与本次清理内容无关的历史遗留路径错误：`docs/architecture/03-invariants.md`、`knowledge/design/buffs-and-triggers.md` 两处相对路径多写了一层 `../`），另有 1 处是本次新增索引条目指向本文档自身、在写入前的暂时性死链。**清理完成后复检为 0 处死链**。

## 六、保留未改动、留待人工判断的内容

- `knowledge/decisions/0016-mod-performance-tiers-by-declaration.md`：上一轮整理已指出其守门规则原句是 ADR 0018 之前的未限定表述，但 ADR 0018 自己已明确记录「不直接修改 0016 正文，两者叠加读才是完整决定」——这是已经做出的裁定，不是遗漏，本轮未动。
- `knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md` `opens_into` 一节称「仍是真洞」——本轮未重新核实是否已有通用交互规则声明式机制落地，不确定，保留。
- `docs/architecture/discrepancies.md` 条目 2、3（`ll-world` crate 职责描述、`ll-sim` 依赖顺序精度）：均为架构性描述，不涉及随阶段推进变化的实现进度，本轮未重新核实，维持原状。
- `docs/qa/01`/`02`/`04` 三份文档本轮未逐份核对，`04-覆盖率与缺失测试层.md` 里的 L6 端到端测试空白盘点很可能已被 `p5-to-p6.md` 记录的 `e2e_save_cycle.rs` 部分填补，但填补程度需要单独核实，本轮未展开。

## 相关文档

- [上一轮整理：过时标注整理清单](2026-08-19-stale-annotation-sweep.md) — 标注而非删改的方法论，本轮方针与此相反
- [2026-08-17 文档—代码一致性审计报告](2026-08-17-doc-code-audit.md)
- [知识库总索引](../README.md)
