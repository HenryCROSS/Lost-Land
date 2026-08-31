# 0030 — 去掉 `examples/` 验收 demo，验收改由实机试玩 + 分层测试承担

**日期**：2026-08-29
**状态**：已生效
**裁定人**：项目所有者
**规格**：修改 §15 的硬性要求一句、§17 风险登记「逐层铺地基导致长期无可见产出」一行的缓解措施
**影响范围**：`crates/*/examples/`（全部删除，14 个 target、33 个文件、10206 行）、
`crates/{ll-content,ll-render,ll-sim,ll-text,ll-ui,ll-world}/Cargo.toml`、
`scripts/ci/run_acceptance_demos.sh` → `scripts/ci/check_no_examples.sh`、
`scripts/ci/run_all.sh`、`.github/workflows/ci.yml`、`assets/atlas/README.md`、
各 `crates/*/tests/visual/README.md`
**实施计划**：[`docs/superpowers/plans/2026-08-29-batch13-example-cleanup.md`](../../docs/superpowers/plans/2026-08-29-batch13-example-cleanup.md)

## 裁定原文

> 我觉得应该要去掉 example。然后有用的东西搬迁了。剩下的后面考虑。

## 背景：被推翻的是哪一句

规格 §15 原文：

> 每阶段在本规格批准后各自产出独立实施计划。**每阶段必须交付可独立运行的 `examples/` 验收 demo。**

§17 风险登记给出了它的理由：

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| 逐层铺地基导致长期无可见产出 | 中 | 每阶段强制交付 `examples/` demo |

**这条要求当初是对的**：P0–P5 期间没有可运行的本体二进制，如果没有 demo，
「铺了三个月地基、屏幕上什么都没有」是一个真实的风险。

## 为什么现在改

### 一、原始理由已经由别的东西达成

本体二进制早已可运行，项目所有者在实机试玩并逐条报缺陷。近几个会话的缺陷
清单——「所有 NPC 长得一模一样」「目前的贴图有点丑了」「关不上的门没有反馈」
「纯中文存档名退化成 save-2」——**没有一条来自任何一个 demo**，全部来自实机
试玩。「长期无可见产出」这个风险的缓解措施，事实上已经换成了「本体二进制随时
可玩」。

### 二、2026-08-28 的新门禁把 demo 分成了两类，其中一类价值为零

`run_acceptance_demos.sh` 落地时必须维护两张清单，实测比例是：

- **RUN_LIST（无头、纯断言、自己会退出）：2 个**——`ll-content` 的两个 P5 demo；
- **SKIP_LIST（开窗 / 要 GPU / 等人输入）：12 个**——P0–P4 与 `p5_coordinate`、
  `mixed_text_demo`、三个 preview、三个 probe。

SKIP_LIST 里那 12 个**在 CI 上一次都没跑过、也永远不会跑**。它们的成本是真实
的：进每一次 `cargo test --workspace` 与 `cargo clippy --all-targets` 的编译，
而 `ll-game` 的测试二进制在本地已经会因链接器内存不足而假失败
（见 `knowledge/handoff/2026-08-28-session-handoff.md` 纪律第 8 条）。

而 RUN_LIST 那两个的价值**并不来自「它是 example」**——来自「它是无头、有断言、
会自己退出的验证代码」。那正是 `tests/` 的定义。把它们搬进 `tests/`，价值一分
不少，还额外拿到了 `cargo test` 的并行执行与逐条失败报告。

### 三、这条要求在 P6/P7 已经被沉默地违反过

`knowledge/handoff/2026-08-28-session-handoff.md` 第四节第 7 条记着：P6、P7
两个阶段**都没有交付 demo**，而 §15 是硬性要求——这被登记为**一次未经记录的
规格变更**。本 ADR 存在的一半理由就是不让它变成第二次：正文可以改，但必须留下
「什么时候、因为什么、由谁裁定」。

## 决定

### 一、删除全部 `examples/`，把有真实断言且无头跑得动的搬进 `tests/`

搬入 29 条断言，落点四处：

| 到 | 条数 | 搬的是什么 |
|---|---|---|
| `crates/ll-content/tests/save_acceptance.rs` | 8 | 存档往返逐位一致、缺 mod 按内容类型降级的完整矩阵、生成期 mod 硬门禁、模式单向降级 |
| `crates/ll-content/tests/gameplay_acceptance.rs` | 1 | 职业/分支技能树/副职/网状任务/存档往返串成一条链 |
| `crates/ll-ui/tests/mod_load_pipeline.rs` | 8 | 真实 `load_all` 跑三个 mod 根目录（含两个失败夹具目录） |
| `crates/ll-sim/tests/coordinate_layers_e2e.rs` | 12 | 跨区块流式加载、进出 Interior、世界地图探索记忆、层属性 → 视野半径 |

**没有一条断言在搬迁中被弱化。** 反例验证见下方「后果」一节。

### 二、`run_acceptance_demos.sh` 保留成更弱的形式：`check_no_examples.sh`

那个门禁做两件事，**只有一件失去了对象**：

- 「真的运行 RUN_LIST 里的 demo」——失去对象。它要防的失败模式（断言藏在
  `main()` 里没人跑）在断言搬进 `tests/` 之后，在结构上不再存在。
- 「每个 example 必须被显式分类，否则报错」——**它自己的脚本头注释把这一半
  称作「本门禁真正的价值所在」**，而本裁定让它更重要了：工作区里冒出一个
  example，本身就该当场变红。

新脚本把判据收紧成「一个 example target 都不许有」，**且不接受「加进某张清单」
这种消红方式**——要重新引入 example，需要一次新的裁定并同批更新本 ADR 与
规格 §15。

### 三、规格 §15 与 §17 同批修改，并在原句处留下指向本 ADR 的标记

§15 那句改为：验收由「本体二进制实机试玩 + 分层自动化测试（`tests/` +
`crates/*/tests/visual/` 的像素基准）」承担，不再要求 `examples/` demo。
§17 那一行的**缓解措施**同步改写，**风险等级不动**——本裁定改的是缓解手段，
不是风险本身。

## 后果

### 一、八张冻结像素基准的唯一生产者随之消失（**已知代价，留给下一次裁定**）

| 基准 | 唯一生产者（已删） | 重新生成需要 |
|---|---|---|
| `crates/ll-render/tests/visual/baseline/p1_acceptance.png` | `ll-render:p1_acceptance` | 真实窗口 + GPU，按 F2 |
| `crates/ll-world/tests/visual/baseline/p2_acceptance.png` | `ll-world:p2_acceptance` | 同上 |
| `crates/ll-sim/tests/visual/baseline/p3_acceptance.png` | `ll-sim:p3_acceptance` | 同上 |
| `crates/ll-ui/tests/visual/baseline/p4_acceptance.png` | `ll-ui:p4_acceptance` | 同上 |
| `crates/ll-text/tests/visual/mixed_text_demo.png` | `ll-text:mixed_text_demo` | GPU 适配器（不需要窗口） |
| `crates/ll-game/tests/visual/surface_preview.png` | `ll-game:surface_preview` | **纯 CPU，不需要 GPU** |
| `crates/ll-game/tests/visual/settlement_preview.png` | `ll-game:settlement_preview` | **纯 CPU** |
| `crates/ll-game/tests/visual/npc_roster_preview.png` | `ll-game:npc_roster_preview` | **纯 CPU** |

**八张 PNG 一张都没删。** 本批次的处置是「图留着，生产者删掉，恢复方式写进各
`tests/visual/README.md`」——这是最保守、最容易反转的一侧，**不是对三条路的
选择**。三条路各自的代价见实施计划第三节：

1. 保留那一两个 example（裁定只执行一半）；
2. 把生成逻辑搬成测试（前五张**做不到**——无头 CI 拿不到适配器，只能写成
   「拿不到就跳过」的假测试，正是 ADR 0018 要根除的东西；后三张技术上可行，
   但要新造一套「比对基准 + 环境变量覆盖」的机制）；
3. 放弃基准（`docs/qa/04-覆盖率与缺失测试层.md` 把 L7 视觉回归列为已知缺口
   并给了补法，删图等于把那条路的起点也删掉）。

**恢复任何一个被删的生产者**（git 里逐字节留着）：

```bash
git log --oneline --diff-filter=D -- crates/ll-render/examples
git show <那个提交>^:crates/ll-render/examples/p1_acceptance/main.rs
```

### 二、`assets/atlas/placeholder.png` 的**消费者**归零（图本身不受影响）

它的生产者是 `tools/ll-artgen::generate_legacy_shared_atlas`，**不是 example**，
因此仍可随时重新生成。但删 example 之后，`crates/` 下再没有任何一处读它——
`boss_idle_0` 的身份从「两个验收 demo 的测试夹具」变成「零消费者」。所有者
早前那句「现在应该不太需要 boss」的**结论**（留图）不变，但它的**理由**
（删它要连带重冻 p3 基准）已经作废，`assets/atlas/README.md` 同批更正。

**「不要往那张共享画布里塞新内容」这条纪律不但不作废，理由还更强了**：四张
基准 PNG 还在，画布一动它们就再也对不上，而现在连重截的生产者都没有了。

### 三、测试函数数净减 51，被 `cargo test` 执行的断言数上升

| 项 | 数 |
|---|---|
| 改前 | 2768 |
| 删掉的 example 内 `#[test]` | −80 |
| 搬入 `tests/` | +29 |
| 改后 | **2717** |

那 51 条**没有一条覆盖生产代码**：它们测的是随 demo 一起删掉的 demo 自有代码
（棋盘格、巡逻三角波、内置点阵字体、两行 AI、demo 自己那张地形→图集条目映射
表、demo 自己的出生点搜索与山脊雕刻、demo 自己的动画接线层），以及 9 条被
`ll-render/src/sprite.rs` 与 `ll-render/tests/sprite_blackbox.rs` **更强地**
覆盖的精灵换算/绘制顺序断言（那边锁精确数值与全序属性，demo 这边只锁方向性）。
逐 target 的核实见实施计划第一节。

反过来，`p5_save_acceptance` 的 **35 条断言此前 `cargo test` 一条都跑不到**
（只有 `run_acceptance_demos.sh` 跑），现在全部进了 `cargo test --workspace`。

### 四、四条 crate 依赖边被顺带清掉

`ll-world` 与 `ll-sim` 的 dev-dependency 里各有一条 `ll-render`（`ll-world`
还有 `ll-platform`），**唯一用途是 demo 的开窗渲染**。删掉之后，世界层与模拟层
回到「不依赖渲染/平台层」这条分层约束的干净状态。`ll-text` 的 `pollster`、
四个 crate 的 `image` 同理。

## 相关

- [ADR 0018](0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)——反例
  验证是硬要求。本批次对搬入的 29 条断言做了全量反例验证：机械取反全部判据后
  **29 条全红、0 条还绿**，另有四次生产侧改坏，命中面逐条符合预期。
- [ADR 0025](0025-demo-interaction-verification-forbids-sendkeys.md)——禁止用
  `SendKeys` 盲注按键做 demo 验收。那条 ADR 的对象（开窗 demo）已随本裁定
  消失，但它确立的方法论**正是本次搬迁的依据**：不模拟按键，直接驱动
  `Intent → resolve → Effect → apply` 这条与真实按键完全相同的链路。
