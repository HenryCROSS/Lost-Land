# 工单清单

来源：`knowledge/audit/2026-08-17-doc-code-audit.md`。以下工单全部落在本次审计的禁区内（`.github/**`、`docs/superpowers/specs/**`、`crates/**`、`knowledge/README.md`），审计角色不动手改，仅产出可直接照做的工单。

---

### W-01 [CRITICAL] 补上 `cargo-llvm-cov` 覆盖率门禁

**文件**：`.github/workflows/ci.yml`

**现状**（第 12-32 行，`test` job 末尾）：

```yaml
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: 格式检查
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: 测试
        run: cargo test --workspace
```

**应改为**（在 `licenses` job 之后新增一个独立 job；不要塞进 `test` job，因为 `cargo-llvm-cov` 只需单平台跑一次，塞进双平台矩阵会白白跑两遍）：

```yaml
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@cargo-llvm-cov
      - name: 覆盖率（核心 crate ≥ 90%，其余 ≥ 80%，规格 §14.5）
        run: |
          cargo llvm-cov --workspace --fail-under-lines 80
          cargo llvm-cov --package ll-core --package ll-sim --package ll-world --fail-under-lines 90
```

**理由**：规格 §14.7 把覆盖率列为"任一项失败即阻断"的门禁之一，但该门禁自 CI 建立以来从未存在过（详见审计报告 C-1）。这是 9 项门禁里唯一一项**工具本身已经在依赖清单里、且不需要额外自研静态分析**的，实现成本最低，应优先补上。`--fail-under-lines` 的两条阈值直接对应规格 §14.5 原文的"整体 80% / 核心 crate 90%"。

**风险**：首次接入时如果当前实际覆盖率低于阈值，这个 job 会让 CI 变红——这本身就是发现问题（覆盖率一直没达标但没人知道），不是新引入的问题，建议先在本地跑一次 `cargo llvm-cov --workspace` 确认当前基线，若低于阈值需要先补测试或先把阈值设到当前基线再逐步收紧，而不是直接卡死流水线。

---

### W-02 [HIGH] 补齐或如实标注剩余 3 项 CI 门禁：无硬编码字符串检查 / 无手写欧氏距离检查 / 无死代码检查

**文件**：`docs/superpowers/specs/2026-08-16-lostland-design.md:435-451`（§14.7 表格）与 `.github/workflows/ci.yml`

**现状**（规格 §14.7 表格节选）：

```
| 无硬编码用户可见字符串 | 自研检查（§11.3） |
| 无手写欧氏距离 | 自研检查（§7.1） |
| 无死代码与过时文档 | 自研检查（§13） |
```

**问题**：这三项检查依赖的静态分析工具目前完全不存在——`tools/` 目录本身尚未创建（对应规格 §5 的 `tools/ll-datacheck`，规划在 P10，[2026-08-18 规格修订] 插入「物品与装备」新 P6 阶段后原 P9 顺移为 P10），全仓库搜索 `euclid`/`hardcode`/`deadcode` 关键词零命中。这不是"忘了接进 CI"，而是"检查本身还没被写出来"。

**两种应改为，二选一，由 P3/P4 阶段负责人裁定**：

**方案 A（推荐，成本低）**：把规格 §14.7 表格里这三行的措辞从"现状描述"改为"计划描述"，明确标注目标阶段，避免"任一项失败即阻断"这句话继续覆盖三个不存在的机制：

```
| 无硬编码用户可见字符串 | 自研检查（§11.3，**P10 `tools/ll-datacheck` 交付前不接入 CI**） |
| 无手写欧氏距离 | 自研检查（§7.1，**P10 `tools/ll-datacheck` 交付前不接入 CI**） |
| 无死代码与过时文档 | 自研检查（§13，**P10 `tools/ll-datacheck` 交付前不接入 CI**；`cargo check`/`clippy` 已覆盖 Rust 语言层面的死代码，此处特指"无引用的整个模块/文件"与"过时文档"两类工具查不到的情形） |
```

同时在"任一项失败即阻断"前加一句"下表标注为 P10 交付前的三项除外"，避免继续误导。

**方案 B（成本高，尽快落地机器强制）**：提前把三个检查中最简单的一个做成 CI 脚本先顶上——"无手写欧氏距离"可以先用一条 `ripgrep` 规则近似（例如禁止在 `crates/**/src/**/*.rs` 中出现 `.powi(2)` 紧邻两次相加再开方的模式，或更简单地禁止裸写 `sqrt` 且不在 `ll_core::torus` 模块内），作为"总比没有强"的过渡方案，同时保留 `tools/ll-datacheck` 作为 P10 的完整实现目标。

**理由**：见审计报告 C-1——规格用绝对化措辞描述一个从未存在的机制，这正是本项目已经吃过一次亏的失败模式（"精灵批处理支持 4 份偏移副本"错了一整个阶段无人发现）的另一种形式：这次不是规格描述了实现里不存在的东西，而是规格描述了**CI 里不存在的强制力**，两者本质都是"文档与事实脱节、且没人反向核对"。

**风险**：方案 A 只改文字，风险低，但不解决"欧氏距离/硬编码字符串仍然可以无阻拦地进入代码库"的实际问题。方案 B 需要有人设计出可靠的静态检查规则，误报率控制不好反而会拖慢开发。

---

### W-03 [HIGH] 裁定：季节要不要接入时间轴调度器（规格 §7.2 vs 当前纯函数派生实现）

**文件**：`crates/ll-sim/src/effect.rs`（`Effect` 枚举定义处）与 `crates/ll-world/src/light.rs`（`season_light_scale` 定义处）

**现状**：

`crates/ll-sim/src/effect.rs:23-72` 的 `Effect` 枚举当前只有六个变体：`MoveTo`、`Damage`、`Kill`、`ScheduleNext`、`SetTerrain`、`AdjustWallet`，没有任何季节相关变体。

`crates/ll-world/src/light.rs` 的 `season_light_scale` 目前是纯函数（按世界时钟直接计算光照缩放系数，无需时间轴事件驱动）。

规格 §7.2（`docs/superpowers/specs/2026-08-16-lostland-design.md:202`）：

> 季节更替是时间轴上的一个定时事件，其 `Effect` 修改各城镇生产速率、地形通行性与野怪分布表。

`knowledge/handoff/p2-to-p3.md:91`：

> 第三项与 P3 直接相关——P3 要建时间轴调度器，届时必须决定：季节到底是纯函数派生（当前实现）还是时间轴事件（规格原文）？**两者不能都留着。**

**应改为**（二选一，需要 P3 实现者/架构决策者裁定，而非本工单代为决定）：

- **选项 1：接受规格原文**，在 `Effect` 枚举里新增 `Effect::SeasonChange { season: Season }`（或类似形态），由 `ll-sim::timeline` 在世界时钟跨季节边界时插入定时事件，`apply` 消费该 Effect 时更新城镇生产速率、地形通行性、野怪分布表等受季节影响的状态。`ll-world::light::season_light_scale` 若仍需要"即时查询当前季节"的能力（例如渲染层每帧要知道当前光照），可以保留一个只读的纯函数版本用于查询，但**驱动状态变化的一侧必须走 Effect**，避免出现"两条路径都能改变季节相关状态"的双头写入。
- **选项 2：正式废除规格原文**，在规格 §7.2 补一条类似 §7.1 已有的"原方案为……P2/P3 实施时改为……"的修正说明（参照规格文件第 195-197 行两处已有先例的措辞与格式），把"季节更替是时间轴事件"改写为"季节由世界时钟纯函数派生，理由是……"，并同步更新 `knowledge/handoff/p2-to-p3.md` 第四节，把这一项从"尚无人认领"移到"已裁定为纯函数派生"。

**理由**：见审计报告 H-3——P3 的时间轴基础设施（`Timeline`/`Effect`/`apply`）已经成型，这个决策的改造成本正在持续上升；再拖到 P3 之后，规格原文与代码现状的分歧会继续被传递到下一份交接清单，重复"没人反向核对规格"的风险。

**风险**：选项 1 需要设计"城镇生产速率、地形通行性、野怪分布表"这几个目前尚不存在的数据结构如何被一个 Effect 统一驱动，工作量不小，可能超出 P3（回合与战斗层）的范围而更适合放到实际引入这些系统的阶段（生产/城镇经济属 P9，[2026-08-18 规格修订] 插入「物品与装备」新 P6 阶段后原 P8 顺移为 P9，野怪分布属未明确阶段）；选项 2 工作量小但需要有人愿意正式承认"规格这条从一开始就不会照原文实现"。两者都需要架构层面的裁定，不是纯粹的代码改动可以单方面决定的。

---

### W-04 [MEDIUM] `knowledge/README.md` 设计索引补上两份遗漏文档

**文件**：`knowledge/README.md`（"## 索引 → ### 设计"一节）

**现状**：

```markdown
### 设计

- [物品系统](design/item-system.md) — 定义与实例分离、堆叠规则、归属、耐久、地面老化
- [装备栏位与占位掩码](design/equipment-slots.md) — 22 槽位；一条掩码规则同时覆盖双手武器与全身甲
- [角色属性系统](design/attribute-system.md) — DnD 六维骨架、三系攻防、四种穿透、衍生属性绝不入存档
```

**应改为**：

```markdown
### 设计

- [物品系统](design/item-system.md) — 定义与实例分离、堆叠规则、归属、耐久、地面老化
- [装备栏位与占位掩码](design/equipment-slots.md) — 22 槽位；一条掩码规则同时覆盖双手武器与全身甲
- [角色属性系统](design/attribute-system.md) — DnD 六维骨架、三系攻防、四种穿透、衍生属性绝不入存档
- [智能体目标与经济](design/agent-goals-and-economy.md) — 目标栈结构、需求分解、任务发布机制、经济模型
- [社会与归属](design/society-and-affiliation.md) — 势力/宗教/行会/文化/家族/职业六类归属共用一套结构
```

（后两条的一句话摘要建议由实际读过全文的人核校用词是否准确，此处只是按文档标题与开篇段落给出的初稿。）

**理由**：见审计报告 M-3——`knowledge/design/` 目录实际有五份文档，索引只列了三份，且遗漏的两份已经被 `crates/ll-world/src/entity/affiliation.rs` 与 `crates/ll-world/src/entity/goal.rs` 的模块文档直接引用为"完整语义冻结"的权威出处，不是可有可无的边缘文档。

**风险**：纯文本改动，无代码风险。

---

### W-05 [HIGH] 「禁止 HashMap/HashSet 迭代顺序参与逻辑判断」这条规则不是任何一个已定义约束——五处代码引用分裂成两种错误编号

**[2026-08-18 更新]** 规格 §4 已新增 **C5**，专门收编这条规则（`docs/superpowers/specs/2026-08-16-lostland-design.md:133-134`）。下方「判断」与「应改为」两节已按此更新——**结论从「C3 更站得住脚」改为「统一改标 C5」**，`crates/**` 内五处引用的实际改码仍待执行（本次更新只改规格与本工单文本，不改 `crates/**`，见权限边界）。

**文件与行号**：

| 文件 | 行号 | 当前标注 |
|---|---|---|
| `crates/ll-world/src/state.rs` | 196 | C3 |
| `crates/ll-sim/examples/p3_acceptance/turn.rs` | 356 | C3 |
| `crates/ll-mod/src/registry.rs` | 66 | C4 |
| `crates/ll-mod/src/topo.rs` | 5 | C4 |
| `crates/ll-world/src/entity/id.rs` | 30 | C4 |
| `crates/ll-sim/src/intent.rs` | 9（相关但独立的第二处错误，见下） | C4 |

**现状**：

规格 §4「硬性约束」（`docs/superpowers/specs/2026-08-16-lostland-design.md:129-132`）是 C1–C4 的唯一权威定义，逐条抄录：

```
C1 — Steel 脚本不得持有任何跨帧的隐式状态。
C2 — 时间轴队列中只能存放纯数据，禁止闭包与裸指针。
C3 — 所有随机性必须来自按实体 ID 派生的确定性流；禁止使用全局 RNG 流，
     否则并行化与读档重放都会分叉。
C4 — 后台推进离屏世界时，必须推进到一个确定的世界时刻 T，不得
     「能跑多少跑多少」。
```

规格全文（含 §4 与全仓库搜索 `HashMap\|HashSet\|迭代顺序`，命中仅 §5 crate 分层表里一句不带编号的旁白）**从未把「禁止 HashMap/HashSet 迭代顺序参与逻辑判断」定义为 C1–C4 中的任何一个**——这条规则是历次实现中自然长出来的约定俗成，但没有人把它正式写回规格，导致后续贡献者各自猜测该标哪个号：`state.rs:196`、`turn.rs:356` 标 C3；`registry.rs:66`、`topo.rs:5`、`id.rs:30` 标 C4——五处三比二地分裂，且**没有一种标法字面对得上规格原文**：C3 字面只讲「随机性/RNG 流」，C4 字面只讲「后台推进到确定 tick」，都不是「容器迭代顺序」。

**独立的第二处错误（同一批问题的另一个症状）**：`crates/ll-sim/src/intent.rs:9` 把「`DetRng::for_entity` 派生的随机数序列在重放时必然相同」标为 **C4**——但这恰好就是规格原文 C3 的字面定义（"所有随机性必须来自按实体 ID 派生的确定性流"）。也就是说 `intent.rs` 把本该是 C3 的内容错标成 C4，而前述五处又把不属于 C3/C4 任何一个的 HashMap 规则分别错标成 C3 或 C4——两类错误方向相反，凑在一起会让读者更难猜出规律。

**判断**：

1. `intent.rs:9` 是单纯的编号写错，**应改为 C3**（字面完全对应，无需解释）——这一条与 HashMap 问题无关，独立成立，结论不受下一条更新影响。
2. HashMap/HashSet 迭代顺序规则**字面上不属于 C1–C4 中任何一个**。~~若必须从现有四个里选一个最接近的，C3 更站得住脚~~——**这条结论已被规格修订取代**：规格 §4 现已新增专门的 **C5** 收编这条规则（措辞见规格原文），不再需要从 C1–C4 里"矮子里拔将军"。**五处 HashMap 相关引用应统一改标为 C5**，而不是此前建议的 C3——继续标 C3 会与 C3 的字面定义（RNG 流）混在一起，反而掩盖了这是两条独立约束（触发源不同：一个是随机性没走确定性流，一个是容器迭代顺序被当成了顺序输入）的事实。

**应改为**：

```
crates/ll-world/src/state.rs:196        C3 → 改为 C5
crates/ll-sim/examples/p3_acceptance/turn.rs:356   C3 → 改为 C5
crates/ll-mod/src/registry.rs:66        C4 → 改为 C5
crates/ll-mod/src/topo.rs:5             C4 → 改为 C5（含模块文档标题「确定性」一节正文）
crates/ll-world/src/entity/id.rs:30     C4 → 改为 C5
crates/ll-sim/src/intent.rs:9           C4 → 改为 C3（独立的字面对应错误，与 HashMap 问题无关，不属于「选哪个更合理」的判断，是纯粹改正）
```

**风险**：六处均为注释/文档字符串改动，不改变任何运行时行为，编译不受影响，零测试影响。真正的风险在于**不改**：规格现在已经有 C5 这个正式条文号了，`crates/**` 里五处仍标着 C3/C4 的引用会继续误导——读者按编号去规格 §4 查，查到的是 RNG 流或后台推进 tick 的定义，对不上注释想表达的意思；建议这份工单被执行时，一并检查是否还有本次搜索未覆盖到的新增引用（本次搜索范围是 `crates/**/*.rs` 全文 `grep -rn "约束 C\[0-9\]\|规格 C\[0-9\]"`，如实记录搜索命令供复核）。
