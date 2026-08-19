# 过时标注整理清单

**整理日期**：2026-08-19
**整理范围**：`knowledge/design/`、`knowledge/handoff/`、`docs/architecture/`、`docs/superpowers/specs/`、`docs/superpowers/plans/`。
**整理时刻的仓库状态**：`main` 分支，HEAD `8916752`，747+ 测试全绿。
**方法论**：本次不删改被取代的内容（ADR 0013 立的规矩），只在原文旁标注「已过时」并指向替代者；落地状态类结论一律去 `crates/**` grep/读代码核实，不凭文档转述。

---

## 一、统一标注格式

本次约定并全文一致使用两种标注：

1. **内容被后续决定取代**（例如旧提法被 ADR 0018 收窄）：在受影响段落上方插入一行
   ```
   > **[已过时 YYYY-MM-DD]** 本节的 X 已被 Y 取代/收窄，见 Z。
   ```
   段内联的短标注（如表格单元格里放不下整段引用块）用方括号内联版：`[已过时 YYYY-MM-DD：原因，见 Z]`。

2. **落地状态复核更正**（沿用本仓库 `identity-and-ids.md` 已经在用的既有格式，本次只是把它推广到其余文档）：
   ```
   **落地状态**（YYYY-MM-DD 复核更正——原文「……」已过期）：新结论……
   ```

两种格式都不删除原文，只在其上方或内部插入说明，原有措辞原样保留。

---

## 二、「本体即 Mod」旧提法核实结果

核对范围：任务指定的 13 个文件，逐处读取上下文判断是否需要限定「玩法层」范围。

| 文件:行 | 结论 | 处理 |
|---|---|---|
| `knowledge/design/class-skill-quest-system.md:248` | 正确用法（职业/技能，玩法层内容） | 未改 |
| `knowledge/design/coordinate-system-and-layers.md:234,277` | 正确用法（`SpaceProfile`/生成器注册，玩法层内容） | 未改 |
| `knowledge/design/item-system.md:102` | 正确用法（物品品质倍率表，玩法层内容） | 未改 |
| `knowledge/design/race-system.md:31` | 正确用法（种族定义，玩法层内容） | 未改 |
| `knowledge/design/README.md:114` 起「本体即 Mod」小节 | 正确用法（六条例子全部是玩法层内容：品质表/文化定义/装备槽位/种族定义/势力实例/命名钩子） | 未改 |
| `knowledge/design/script-state-storage.md:133` | 正确用法（互操作场景描述，非字面重述原则） | 未改 |
| `knowledge/design/society-and-affiliation.md:98` | 正确用法（聚落生成器，玩法层内容） | 未改 |
| `knowledge/handoff/p3-to-p4.md:87` | 正确用法（`RaceDef` 注册表，玩法层内容） | 未改 |
| `knowledge/handoff/p4-to-p5.md:68,127` | 正确用法（已直接引用 ADR 0016，语境是技能效果这类玩法层内容） | 未改 |
| `docs/architecture/discrepancies.md:156-169`（第 8 条） | **需要标注**——逐字引用规格旧句「本体不享有特权通道」，未随 §10.3 一起改，虽然本条讨论对象（`TerrainKind`）本身是玩法层内容、结论不变，但引文已不是当前逐字文本 | **已标注**，指向 ADR 0018 与规格 §10.3 现行文本 |
| `docs/superpowers/specs/2026-08-16-lostland-design.md:39`（§2 决策清单表格） | **需要标注**——§10.3 正文已加「玩法层」限定语并补规格修订说明，但决策清单表格这一行的摘要没有跟着改，同一份文档内两处不一致 | **已标注**，行内补「（玩法层内成立）」+ 指回 §10.3/ADR 0018 |
| `docs/superpowers/specs/2026-08-16-lostland-design.md` §10.3 本体 | 已经是修订后的正确文本（本次核实,未发现需要重复标注之处） | 未改 |
| `docs/superpowers/plans/2026-08-16-p0-platform-foundation.md:1050` | 正确用法——只谈内容 ID 命名空间机制（本身是注册表内容的属性），不是「全部内容」的字面重述，不构成过度概括 | 未改 |
| `docs/superpowers/plans/2026-08-18-coordinate-system-rewrite.md:134` | 正确用法（profile intern 路径检验，玩法层内容） | 未改 |
| `docs/superpowers/plans/2026-08-18-p4-script-and-mod.md:80,90,351,390,555` | 正确用法（全文语境即 P4 内容注册表/mod 框架，天然限定在玩法层） | 未改 |
| `docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md`（多处） | 正确用法（`ClassDef`/`SkillDef`，玩法层内容，且已引用 ADR 0016） | 未改 |

**小结**：13 个指定文件逐处核实，**2 处需要标注**（均在 `docs/` 而非原始 13 文件清单内——原清单里的 13 处全部核实为正确用法，问题实际出在规格自身表格与 `discrepancies.md` 的历史引文两处未跟着 §10.3 同步），**11 处（原清单）确认为正确用法，未做任何改动**。

---

## 三、落地状态复核结果

逐份核实 `knowledge/design/*.md` 的「落地状态」字段，以 `crates/**` 实际结构/字段/函数为准。

| 文件 | 原状态 | 复核结论 | 依据 | 处理 |
|---|---|---|---|---|
| `coordinate-system-and-layers.md` | 纯设计，尚无代码 | **已完整落地**（15 个任务全部完成） | `crates/ll-world/src/{space,space_profile,interior,zone}.rs`，提交 `8ec08de`…`260d7ca`+`0b3244e` | **已更新** |
| `class-skill-quest-system.md` | 纯设计 | **部分落地**（`ClassDef`/`SkillDef` 已落地，`SubclassDef`/`QuestDef` 未落地） | `crates/ll-mod/src/{class,skill}.rs`，提交 `580a7bd`/`c10a3e9` | **已更新** |
| `script-entity-handles-and-batch-queries.md` | 纯设计，不要求本次实现 | **部分落地**（三/四节句柄机制+`Intent::Attack` 已落地，五节批量查询原语未落地） | `crates/ll-script/src/api/handle.rs`（提交 `ac27217`）、`crates/ll-sim/src/intent.rs`；`query.rs` 无 `filter`/`average` 系列函数 | **已更新** |
| `conflicts.md` | 无落地状态字段 | 本质是历史交叉核对清单，非实现型设计文档；「待裁定」已全部裁定，「设计缺口」仍开放 | README.md 开篇说明 + 各条目末尾裁定记录 | **已补状态行** |
| `script-state-storage.md` | 用户提示称「缺失」 | **实际已有**，只是字段名叫「状态」不叫「落地状态」，内容本身准确（已落地，P5 批次 D） | `crates/ll-world/src/script_state.rs` 等，与文档描述一致，已实测核对 | **未改**（内容准确，仅字段命名与其余文档不完全一致，留作后续统一格式时的小改，本次未动手改，避免无意义 churn） |
| 其余 12 份（`agent-goals-and-economy`/`attribute-system`/`buffs-and-triggers`/`combat-three-axis`/`equipment-slots`/`identity-and-ids`/`item-system`/`naming-and-localization`/`race-system`/`society-and-affiliation`/`world-history`/`README`） | — | 逐份核实，落地状态描述与代码现状一致（`identity-and-ids.md` 此前已有 2026-08-19 复核记录，其余各份的「未落地」结论经 grep 类型名/字段名确认仍成立） | 见各文档「落地状态」行自带依据 | 未改 |

**最离谱的一处**：`coordinate-system-and-layers.md` 写着「纯设计，尚无代码」，但坐标系重写的 15 个任务实际已经全部完成、代码基线测试数从 495/496 涨到验收 demo 落地时的 597——文档说的是「设计提案」，代码已经是「当前的实际模型」，落差最大。

---

## 四、其余过时内容核查

### 4.1 阶段编号（P6 插入后的顺移）

`docs/superpowers/plans/` 四个文件的 24 处旧编号已由此前提交 `5924a83`（"docs: 修正 P0-P4 实施计划里对 P6-P9 的旧编号前瞻引用"）修正过，本次复核未在 `docs/superpowers/plans/`、`knowledge/design/`、`knowledge/handoff/` 内发现新的漏网旧编号（这几类文档里的顺移引用普遍已经带着「[2026-08-18 规格修订] 原 PX 顺移为 PY」这个统一格式的旁注）。

**发现一处漏网**：`docs/architecture/03-invariants.md:118`「后台推进离屏世界……属于 P8 阶段」——该文档 C1-C4 部分冻结于 2026-08-17（早于 P6 插入的 2026-08-18 修订），C5 一节虽在修订当天补充，但 C4 一节本身未同批更新。核实规格现行阶段表：新 P8 是「随从与行为树」，后台推进离屏世界（三档 LOD、惰性追赶，规格 §9.1/§9.2）属于新 P9（智能体经济）。**已更正为 P9，并加注说明**。

### 4.2 约束编号（C1–C5）

全仓库搜索「四条约束」「C1–C4」（不含 C5）在本次范围内**零命中**——此前的工单 W-05（`worklist.md`）已经处理过 `crates/**` 内的编号错标，文档层面的 C1–C5 引用（`03-invariants.md`、各设计文档「实现阶段」行、`2026-08-18-coordinate-system-rewrite.md`、`2026-08-19-p5-gameplay-systems.md` 等）经抽查均已是 C1–C5 五条一起提及，未发现遗漏 C5 的旧表述。

### 4.3 ADR 0002 / 0020 交叉引用

- `knowledge/handoff/p4-to-p5.md` 已经正确交代了 ADR 0020「部分取代 ADR 0002」这层关系（该文档第四十三行）。
- `docs/architecture/05-integer-discipline.md`（冻结于 2026-08-17，早于 ADR 0020 的 2026-08-18）「规则」一节原文「浮点只允许出现在渲染层与音频层」，没有提到 ADR 0020 新增的第三处允许浮点的位置（脚本内部计算）。**已标注**，指向 ADR 0020，并明确核心规则本身（`WorldState` 零浮点）不受影响，只是允许浮点的位置清单少了一项。
- 其余引用 ADR 0002 的文件（`attribute-system.md`/`conflicts.md`/`item-system.md`/`race-system.md`/`society-and-affiliation.md`/`world-history.md`/`p0-to-p1.md`/`p1-to-p2.md`/`p2-to-p3.md`/`03-invariants.md`/`07-determinism.md`/`architecture/README.md`）逐一核实，均只引用「世界状态零浮点」这条未被触碰的核心规则，与脚本层浮点边界无关，**不需要标注**。

### 4.4 交接清单里已完成却仍标「待办」的债务

`knowledge/handoff/p4-to-p5.md` 第二节，两条：

1. **三处 `#[serde(skip)]` 待还**（`population`/`actors`/`terrain_table`）——`population`/`actors` 已按原计划补齐序列化往返（提交 `e9d0940`）；`terrain_table` 走的是文档自己建议的另一条路（补显式校验点而非摘 `#[serde(skip)]`，提交 `fd8a908`）。**已标注**，逐条说明处理方式的差异。
2. **`terrain_table` 缺「是否已重新灌入」的显式校验点**——已落地为 `WorldState::assert_terrain_table_loaded`（提交 `fd8a908`），且该提交顺带核实了 `surface_profile` 字段不适用同款校验（类型设计不对称，已如实记录为独立开放项）。**已标注**。

VM 强制重建（`rebuild_all_engines_after_load`）核实**未发现**被误标为待办的地方——规格 §4 C1 修订说明（2026-08-19）与 `script-state-storage.md` 状态行均已正确记录为已落地，未做改动。

---

## 五、`knowledge/decisions/` 三份存疑文件（只报告，不改动）

按任务要求只读核实，不动手改：

- **0012**（Steel 标准库能力面实测）：末尾「追加实测三」有一节「与『本体即 Mod』的联系（规格 §10.3，项目所有者指出）」，逐字引用旧版「本体即 Mod」的推理（未加「玩法层」限定），写于 ADR 0018（2026-08-18）之前。建议后续处理时在该节补一句指回 ADR 0018 的forward 指针，说明这里的论证在 ADR 0018 之后应理解为限定在玩法层范围内（本 ADR 讨论的白名单本身是引擎层机制，不受这层限定影响，但援引的「本体即 Mod」原则字面表述已经过期）。
- **0016**（mod 性能分档按声明方式）：守门规则原句「若本体需要一个 mod 够不着的性能档位，那是 API 缺陷」同样是 ADR 0018 之前的未限定表述。**但这是 ADR 0018 自己明确讨论过的情况**——0018「与 0016 的关系」一节已经写明「不直接修改 0016 正文……本 ADR 记录的是后续对适用范围的澄清，两者叠加读才是完整决定」，即 0016 保持原文、由 0018 单方面收窄是**已经做出的裁定**，不是遗漏。若要处理，建议只在 0016 顶部加一个指向 0018 的 forward 指针（方便只读 0016 的人知道要去看 0018），而不是改动 0016 正文本身——这与 0018 自己的处理原则一致。
- **0018**（引擎层/玩法层边界划分）：本次核实未发现明显过时内容——它是「本体即 Mod」问题的源头修订文档，`opens_into` 一节描述的「仍是真洞」核实后**当前代码仍然成立**（未发现后续有通用交互规则声明式机制落地），归类判据表格本身未随后续系统开发被验证过这一点也在文档内如实自陈。列在这里仅因任务原文点名要求核对，未发现需要标注之处；若确有具体疑虑，建议另行说明想核对的具体段落。

---

## 六、下次整理时的参考

- 本次核实的「本体即 Mod」13 个指定文件全部正确，问题反而出在未被指定的 `docs/superpowers/specs/2026-08-16-lostland-design.md` 决策清单表格与 `docs/architecture/discrepancies.md`——下次同类扫描应把「规格里的摘要表格是否跟着正文一起改」也列入常规检查项，不能只查设计文档正文。
- `docs/architecture/` 下写于规格修订前一天（2026-08-17）的文档，普遍需要额外核对是否踩中 2026-08-18 那批修订（P6 插入、C5 新增、ADR 0018/0020）——本次发现的两处遗漏（`03-invariants.md` 阶段号、`05-integer-discipline.md` 浮点边界）都属于这一类「冻结时间刚好卡在修订前」的文档。
- `script-state-storage.md` 的字段名「状态」与其余设计文档惯用的「落地状态」不一致，内容本身准确，未处理，留作下次统一格式时的候选项。
