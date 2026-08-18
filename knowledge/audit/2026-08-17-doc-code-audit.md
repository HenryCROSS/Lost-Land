# 文档—代码一致性审计报告

**审计日期**：2026-08-17
**审计范围**：`docs/superpowers/specs/2026-08-16-lostland-design.md`、三份阶段交接清单、`knowledge/design/*.md`、`.github/workflows/ci.yml`、`cargo doc` 输出、代码内文档注释。
**审计时刻的仓库状态**：`main` 分支，HEAD `4bdd87d`（`feat: Effect 与 apply——世界状态唯一写入口`），P3（回合与战斗层）正在进行中，另有多名代理并行改动 `crates/**`、`docs/architecture/**` 等区域。本报告的代码事实以审计时刻的快照为准；若之后又有新提交，部分「已核实成立」的条目可能已经过时，请以文中给出的证据（文件:行号、commit hash）自行复核。

**方法论声明**：本次审计延续 `knowledge/handoff/p2-to-p3.md` 第六节确立的纪律——不只问「实现是否满足规格」，更要问「规格有没有被实现淘汰、或规格声称的机制根本不存在」。凡是"纪律保证"（靠约定、靠唯一写入口的自觉遵守）与"机器强制"（靠类型系统、靠 CI 阻断）混为一谈的地方，本报告会明确拆开说。

---

## CRITICAL

### C-1　规格 §14.7 声称的 9 项 CI 门禁，实际只有约 5 项存在机器强制

**现象**

规格原文（`docs/superpowers/specs/2026-08-16-lostland-design.md:435-451`）：

> 合并到 `main` 前必须全部通过，**任一项失败即阻断**：
>
> | 门禁 | 工具 |
> |---|---|
> | 编译无警告 | `cargo clippy -- -D warnings` |
> | 格式统一 | `cargo fmt --check` |
> | 全部测试通过 | `cargo test --workspace` |
> | 覆盖率达标 | `cargo-llvm-cov` |
> | 许可证合规 | `cargo-deny` |
> | 无硬编码用户可见字符串 | 自研检查（§11.3） |
> | 无手写欧氏距离 | 自研检查（§7.1） |
> | 无死代码与过时文档 | 自研检查（§13） |
> | 跨平台世界哈希一致 | Windows + Linux 双平台矩阵 |

实际 `.github/workflows/ci.yml`（全文仅 42 行，自 `87c6bbf`「建立 workspace 骨架与 CI 门禁」提交后**从未被修改过**，`git log --oneline -- .github/workflows/ci.yml` 只有这一条记录）只有两个 job：

```yaml
test:
  strategy:
    matrix:
      os: [ubuntu-latest, windows-latest]
  steps:
    - cargo fmt --all --check
    - cargo clippy --workspace --all-targets -- -D warnings
    - cargo test --workspace

licenses:
  steps:
    - cargo deny check
```

**证据**

- 逐项核对：格式统一 ✓、编译无警告/clippy ✓、全部测试通过 ✓、许可证合规 ✓。
- 跨平台世界哈希一致：**部分成立**。`cargo test --workspace` 在双平台矩阵各自执行，且 `crates/ll-world/tests/determinism.rs:31` 有一个写死的黄金摘要 `EXPECTED_WORLD_DIGEST`，两个平台各自跑测试时都要对上同一个常量，因此确实能在两平台间接互相印证。但这只是**测试作者恰好写对了断言方式带来的副作用**，规格原文写的是"双平台矩阵对比哈希"，而 CI 里并没有任何步骤把两个平台的实际输出互相 diff——如果哪天有人删掉这个黄金常量改成 `assert!(world.hash() != 0)` 之类的弱断言，这项门禁会在不改动 YAML 的情况下悄悄失效，没有任何机制会报警。
- 完全不存在（`.github` 目录下唯一文件即 `ci.yml`，全仓库搜索 `euclid`/`deadcode`/`hardcode` 关键词零命中，`tools/` 目录本身尚不存在）：
  - `cargo-llvm-cov` 覆盖率门禁
  - 无硬编码用户可见字符串检查
  - 无手写欧氏距离检查（§7.1 明确写着"此项由 CI 静态检查强制"）
  - 无死代码与过时文档检查

**判断**

以代码（CI 配置文件的真实内容）为准。规格文本在陈述一个从未存在过的现状，且用的是"任一项失败即阻断"这种不留余地的措辞。这不是某一版本曾经对、后来漂移的问题——`git log` 显示 CI 文件建立以来就是这个样子，说明规格写这句话的时候描述的从来就是意图而非实况，且没有人在之后的任何一次评审里把这句话改成符合实况的表述。

**影响**

这正是本项目历史上已经吃过一次亏的失败模式的重演——"精灵批处理需支持最多 4 份偏移副本"那条错了整整一个阶段、五轮评审无一发现，原因正是"没人问反过来的问题"。这里的风险更隐蔽：CI 绿灯本身会被当作"门禁已生效"的证明，但绿灯只覆盖了 9 项里的 5 项。具体后果：

- 硬编码用户可见字符串可以无限制地进入代码库而不被任何自动机制拦截，直到某次人工审查或者玩家反馈才会被发现——而规格 §11.3 把它当作"i18n 双语并行开发以尽早暴露"的核心手段，这个手段现在是空的。
- 手写欧氏距离——规格原文明确警告"遗漏一处就会产生『小地图上很近但寻路绕半个世界』这类极难定位的缺陷"——目前完全没有自动防线，只能靠代码审查肉眼发现。
- 覆盖率门禁不存在意味着 §14.5 的"核心 crate ≥ 90%"要求没有任何强制力，纯粹依赖自觉。
- 死代码与过时文档检查不存在，意味着规格 §13"持续清理"本身也只是纪律层面的承诺，没有机器兜底——而本报告 M-3、M-4 两条恰好就是"过时文档没被发现"的活例子。

**处置**：待办工单 W-01、W-02（见 `worklist.md`）。CI 文件与规格文本均属禁区，本次审计不动手改。

---

## HIGH

### H-1　p0-to-p1 交接清单三处"P1 第一天必须改动"的接口债，实际早已修复，但清单未标注

**现象**

`knowledge/handoff/p0-to-p1.md` 第一节列出三处"P1 第一天必须改动的平台层接口"：`AppHandler` 拿不到 `Window`、没有 `Resized`/`ScaleFactorChanged` 事件转发、`on_frame` 不带帧号；并在同节末尾附带一条"属于死代码，按规格 §13 应当清理"的 `PlatformError::WindowCreation`。

**证据**

`crates/ll-platform/src/window.rs` 当前内容（第 115-142 行 `AppHandler` trait 定义）：

```rust
pub trait AppHandler {
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>);
    fn on_resize(&mut self, size: PhysicalSize<u32>);
    fn on_frame(&mut self, frame: FrameId, input: &InputState) -> FrameOutcome;
    fn on_exit(&mut self);
}
```

`crates/ll-platform/src/lib.rs` 的 `PlatformError` 枚举中 `WindowCreation` 变体已不存在（`grep -n "WindowCreation" crates/ll-platform/src/lib.rs` 零命中）。

`git log --oneline -S"WindowCreation" -- crates/ll-platform/src/lib.rs` 定位到提交 **`0c9fb27`**（`refactor: 平台层为接入渲染做接口改造`），其提交信息逐条对应交接清单里的三项："回调拿不到 Window（surface 建不出来）、没有尺寸变化转发（窗口一改大画面就拉伸或崩）、on_frame 没有时间基准"，并同时移除了 `WindowCreation` 死代码。

**判断**

代码已经完全解决了清单描述的问题，清单文本却仍停留在"这是 P1 要做的事"的语气，没有任何标注说明已经修复。

**影响**

过时的交接清单比没有交接清单更危险（规格 §13 原话）。任何后续读者（包括审计角色自己）如果不去逐项核实代码，会误以为这是尚待处理的接口债，可能重复排查一个已经不存在的问题，或者在评估"P1 是否还有遗留风险"时把已修复项计入未修复。

**处置**：**已由本次审计修复**——已在 `knowledge/handoff/p0-to-p1.md` 第一节标注三项均已修复，附提交哈希 `0c9fb27`。

---

### H-2　p2-to-p3 交接清单"DrawOrder.foot_y 在环面上不成立"，实际早已修复，但清单未标注

**现象**

`knowledge/handoff/p2-to-p3.md` 第一节第 1 条描述："`ll-render::sprite` 把 `foot_y` 定义为『世界纵坐标』……P3 必须在引入第二个实体之前处理：`foot_y` 应当用相机相对的屏幕纵坐标，而不是世界纵坐标。"

**证据**

`crates/ll-render/src/sprite.rs:93-104`（`DrawOrder::new` 的文档注释）：

```rust
/// `foot_y` 应为精灵脚底（而非图像原点）的**屏幕**纵坐标（相机
/// 相对），不是世界纵坐标。
///
/// 必须用屏幕坐标：环面世界里 `y = 世界高度 − 1` 与 `y = 0` 在屏幕上
/// 相邻却相差整个世界高度，用世界坐标会让跨南北接缝的排序反转，
/// 接缝北侧的单位被南侧的错误遮挡。
///
/// 屏幕坐标由 `Camera::world_to_screen` 得出，它已处理环面最短位移，
/// 因此接缝两侧的相邻格在屏幕上也相邻。
```

实际调用点 `crates/ll-world/examples/p2_acceptance/main.rs:266-271`（地形）与 `284-294`（玩家）均先调用 `camera.world_to_screen(pos)` 取得屏幕坐标 `sy`/`tile_y`，再用它构造 `DrawOrder`，未见任何调用点直接传世界 y。

`git log --oneline --all -- crates/ll-render/src/sprite.rs` 中提交 **`c0768af`**（`fix: DrawOrder 的排序键改用屏幕纵坐标`）即此修复。

**判断**

代码与文档双双已经改对，清单仍在用"P3 必须处理"的措辞描述一个已经不存在的缺陷。

**影响**

同 H-1：P3 实现者若信了这条清单，可能会去"修"一个已经修好的问题，或者反过来因为花时间核实后发现"清单是错的"而对整份交接清单的可信度产生怀疑，进而对清单里仍然有效的条目也打折扣看待——这是过时文档最贵的隐性成本：拖累了其余仍然正确的内容。

**处置**：**已由本次审计修复**——已在 `knowledge/handoff/p2-to-p3.md` 第一节第 1 条标注已修复，附提交哈希 `c0768af`。

---

### H-3　规格 §7.2「季节应为时间轴事件」的决策点，P3 进行到一半仍未敲定，且窗口正在关闭

**现象**

规格 §7.2（`docs/superpowers/specs/2026-08-16-lostland-design.md:202`）："季节更替是时间轴上的一个定时事件，其 `Effect` 修改各城镇生产速率、地形通行性与野怪分布表。"

`knowledge/handoff/p2-to-p3.md` 第四节把这一条列为"规格里尚无人认领"的三项之一，并在第 91 行明确指出："第三项与 P3 直接相关——P3 要建时间轴调度器，届时必须决定：季节到底是纯函数派生（当前实现）还是时间轴事件（规格原文）？**两者不能都留着。**"

**证据**

P3（`ll-sim`）目前已经落地了完整的 Intent-Effect-Apply 骨架：`crates/ll-sim/src/effect.rs:23-72` 定义的 `Effect` 枚举有 `MoveTo`、`Damage`、`Kill`、`ScheduleNext`、`SetTerrain`、`AdjustWallet` 六个变体，`crates/ll-sim/src/timeline.rs`、`crates/ll-sim/src/apply.rs` 均已实现。**枚举里没有任何季节相关变体**（`grep -n "Season\|season"` 在 `effect.rs` 零命中）。

与此同时 `crates/ll-world/src/light.rs` 里的 `season_light_scale` 仍是纯函数（`grep -rln "season" crates/ll-world/src crates/ll-sim/src` 只命中 `light.rs` 一处）。

**判断**

时间轴调度器（决策所需的机制本身）已经就位，但"季节到底走哪条路"这个决策既没有被显式采纳（没有 `Season` Effect），也没有被显式放弃（规格原文没删，`ll-world::light` 没有任何"本处有意保留纯函数派生，季节不作为 Effect"的说明性注释）。这是"两者都还没决定"的中间态，而不是"两者都留着"——但危险程度相当：随着 P3 的时间轴基础设施定型，"要不要把季节接进时间轴"这件事的改造成本只会越来越高，决策窗口正在关闭而没人显式记录这一点。

**影响**：P3 收尾产出下一份交接清单（P3→P4）时，如果不在其中显式回答这个问题（连同给出理由），这条悬而未决的决策会被静默地传递到 P4，而 P4 的交接清单读者未必知道要回头去查 P2→P3 清单的第四节——决策记录本身有随阶段推进而失踪的风险。届时"规格 §7.2 明确写着季节是时间轴事件"这句话依然摆在那里，成为下一个"五轮评审无人发现"的候选项。

**处置**：待办工单 W-03（见 `worklist.md`）——这需要 P3 实现者或架构决策者裁定，本次审计无权替其决定，也不动 `crates/**`。

---

## MEDIUM

### M-1　`cargo doc` 断链现状：核实为 1 处真断链 + 9 处私有项链接（较 P2 收尾时记录的数字多 1），且该检查仍从未进入 CI

**现象**

`knowledge/handoff/p2-to-p3.md` 第五节记录 P2 收尾时跑 `cargo doc --workspace --no-deps` 发现"1 处真正的断链"与"8 处『公开文档链到私有项』"警告。

**证据**

本次审计重新执行 `cargo doc --workspace --no-deps 2>&1`，实际输出：

- 真断链 1 处，位置不变：`crates/ll-render/src/batch.rs:77` 的文档链接 `` [`tests::实例结构体大小符合着色器预期`] `` 指向 `crates/ll-render/src/batch.rs:347` 的 `#[cfg(test)] mod tests` 内的测试函数——rustdoc 默认不为测试模块生成文档，链接目标不在文档作用域内，报 `no item named 'tests' in scope`。
- 私有项链接警告 **9 处**（不是 8 处）：
  - `crates/ll-core/src/time.rs:85` → `crate::light::DAYLIGHT_THRESHOLD`
  - `crates/ll-world/src/chunk.rs:57`（同一行内 2 处）→ `MIN_WORLD_WIDTH`、`MIN_WORLD_HEIGHT`
  - `crates/ll-world/src/entity/arena.rs:103` → `Slot::Retired`
  - `crates/ll-world/src/fov.rs:28` → `Slope`
  - `crates/ll-world/src/terrain.rs:17`、`:26`、`:32` → `Self::is_known`（同一符号三处引用）
  - `crates/ll-render/src/batch.rs:17` → `grow_capacity`

**判断**

多出的 1 处来自 P3 期间新增的 `ll-world::entity` 模块（`arena.rs`、`fov.rs` 的两处新增链接抵消后净增 1）。这不是"记录错了"，而是**验证了 P2→P3 清单第五节末尾那句判断本身**："这类问题 `clippy` 与 `cargo test` 都不检查，所以积累了整个项目周期无人发现"——现在有了第二次实证：P2 收尾到 P3 进行到一半这段时间里，警告数又在无人察觉的情况下涨了一个,因为它确实没有任何自动化拦截点。

**影响**：不修不会立刻造成运行时问题，但每多一个阶段，这份"应该断但没人管"的清单只会更长。等到某个阶段真正需要靠 `cargo doc` 生成对外文档（例如给 mod 作者用）时，会一次性面对远比现在更大的清理量，而且到那时很难分辨哪些是"一直存在的技术债"、哪些是"这次改动引入的新问题"。

**处置**：存疑待裁定——是否现在就把 `cargo doc` 接入 CI（真断链失败、私有链接降级为 warning，P2→P3 清单第五节末尾已给出这个具体建议）。这涉及 `.github/workflows/ci.yml`，属禁区，已并入 W-01 工单。

---

### M-2　规格 §7.1「气候周期性条带」与 §7.3「地形光照透过率」核实仍是零实现、零阶段认领

**现象**：`knowledge/handoff/p2-to-p3.md` 第四节记录这两项"规格写了但从未实现、且没有任何阶段认领"。

**证据**：全仓库搜索确认仍然成立——

- `grep -rn "气候\|climate\|赤道\|极圈\|tropic" crates/ll-world/src crates/ll-core/src` 零命中。
- `grep -rn "透过率\|transmit\|light_transparency" crates/ll-world/src crates/ll-core/src` 零命中；`crates/ll-world/src/terrain.rs` 的 `TerrainKind` 目前只有 `blocks_sight`/`blocks_move`/`move_cost` 三项属性（见 `knowledge/handoff/p2-to-p3.md` 第二节表格），没有透过率字段。

**判断**：与 P2 收尾时记录的状态一致，不是新发现，只是核实"仍未过期"。规格 §15 阶段表里 P0-P9 都没有认领这两项。

**影响**：这两条会一直躺在规格里，直到某次实现真正需要它们（气候条带涉及 §7.1 的世界观设定"没有边缘、首尾相接的土地"；光照透过率会影响 FOV 与光照系统的后续扩展）时才被人重新翻出来核对——如果那时忘了看这份交接清单，很可能重新走一遍"规格是否已被淘汰"的调查流程。

**处置**：存疑，无需处置——继续如实记录在案即可，不构成需要立刻处理的缺陷（两项本就未到认领阶段）。

---

### M-3　`knowledge/README.md` 的设计文档索引遗漏两份已被代码引用的设计文档

**现象**：`knowledge/README.md`"索引 → 设计"一节只列出三份文档（物品系统、装备栏位、属性系统），但 `knowledge/design/` 目录实际有五份文件。

**证据**：

```
$ ls knowledge/design
agent-goals-and-economy.md   456 行
attribute-system.md          154 行
equipment-slots.md           103 行
item-system.md               139 行
society-and-affiliation.md   464 行
```

`knowledge/README.md` "### 设计" 一节只有：

```
- [物品系统](design/item-system.md) — ...
- [装备栏位与占位掩码](design/equipment-slots.md) — ...
- [角色属性系统](design/attribute-system.md) — ...
```

`agent-goals-and-economy.md`、`society-and-affiliation.md` 均未出现。而这两份文档并非"纯纸面设计"——`crates/ll-world/src/entity/affiliation.rs:1-9` 与 `crates/ll-world/src/entity/goal.rs:1-7` 的模块文档都明确写着"完整语义冻结在 `knowledge/design/society-and-affiliation.md`"/"`knowledge/design/agent-goals-and-economy.md`"，即代码本身已经把这两份文档当作权威出处引用。

**判断**：`knowledge/README.md` 自己写着"每一份文件都应该在半年后仍然有用""发现文档与代码不一致时按缺陷处理"——但它自己的索引已经与 `knowledge/design/` 目录的实际内容不一致，且不一致的恰好是两份已被代码直接引用为"唯一语义出处"的文档。

**影响**：新读者按 README 索引浏览知识库时会误以为设计文档只有三份，找不到入口了解归属系统与经济/目标系统的设计意图——而这两个系统的字段布局已经提前埋入了 P3 的 `Agent` 类型（`affiliation.rs`、`goal.rs`），后续阶段（P7/P8）实现者若不知道有这两份文档存在，可能凭空重新设计一遍已经冻结的语义。

**处置**：待办工单 W-04（见 `worklist.md`）——`knowledge/README.md` 明确列在本次审计的禁区清单内，不动手改。

---

### M-4　`knowledge/licenses/` 的许可证扫描定格于 P0，P1–P3 新增依赖未见后续复核记录

**现象**：规格 §3 末尾写明："上表中除 `steel-core` 已实测确认外，其余版本与许可证均需在 P0 阶段由 `cargo-deny` 首次扫描正式核验，核验结果回写本表。"隐含的持续纪律是每个阶段新增依赖都要过一遍扫描。

**证据**：`knowledge/licenses/` 目录下唯一文件是 `2026-08-16-p0-scan.md`，日期即 P0 收尾当天。当前 `Cargo.lock` 已有 325 个锁定包（`grep -c "^name = " Cargo.lock`），而 P1 引入的 `wgpu`（及其庞大的传递依赖树：`naga`、各平台图形后端等）、P2/P3 陆续引入的依赖，均未见对应的许可证扫描报告归档。

**判断**：这不是"文档说错了"，而是"文档没跟上"——规格要求的持续纪律（每次新增依赖后归档扫描结果）目前只执行了一次。由于 `cargo-deny` 已经是 CI 的机器强制门禁（`licenses` job），**理论上**不合规的许可证会在合并时被直接挡下，所以当前合规性本身大概率没有被破坏；但 `knowledge/licenses/` 作为"逐依赖许可证结论"的归档本身已经脱节，无法反映 wgpu 一族依赖的核验结果。

**影响**：`knowledge/README.md` 把 `licenses/` 的写入时机定义为"每次新增依赖后"——目前的状态与这条约定不符，长期来看会让人误以为"许可证核验只做过一次、之后就没管"，即便 CI 一直在挡。这属于"纪律保证"没有跟上"机器强制"的典型例子：CI 挡住了坏依赖，但知识库没有记录"我们验证过 wgpu 一族是干净的"这件事本身。

**处置**：存疑——`knowledge/licenses/` 既不在本次审计明确的可写清单内，也不在明确的禁区清单内（原任务只列出 `crates/**`、根 `Cargo.toml`、`docs/superpowers/specs/**`、`docs/architecture/**`、`knowledge/decisions/**`、`knowledge/design/**`、`docs/qa/**`、`knowledge/README.md`、`.github/**`、`.superpowers/**` 为禁区，`knowledge/crates/**` 为可写）。出于谨慎，本次未动手改写此目录，仅记录发现，是否补一份 P1-P3 累计扫描报告请裁定。

---

## LOW

### L-1　p2-to-p3 交接清单"`entry.footprint` 被 demo 焊死为 1×1"，核实仍然成立，不需处置

**现象**：`knowledge/handoff/p2-to-p3.md` 第一节第 2 条描述 P2 验收 demo 硬编码单位占地为 1×1，提醒 P3 加入敌人/随从时应从图集条目读取真实 `footprint`。

**证据**：`crates/ll-world/examples/p2_acceptance/main.rs:285-288` 目前依然是：

```rust
let footprint = Footprint {
    width: 1,
    height: 1,
};
```

且全仓库搜索 `footprint` 关键词在 `crates/ll-sim`、`crates/ll-world/src/entity` 下零命中——P3 目前还没有触及"实体如何在渲染层取用 footprint"这块集成。

**判断**：这条提醒依然完全适用，不是过时内容，只是尚未到验证时机（P3 目前的重心是 `Intent`/`Effect`/`Timeline` 骨架，还没有产出渲染集成 demo）。

**影响**：无当前影响，纯粹是前瞻提醒；一旦 P3 或后续阶段开始渲染非 1×1 单位（Boss、精英），若忽略此条会立刻复现规格 §12.1 警告过的错位问题。

**处置**：无需处置，本次核实后维持原状，不做任何修改。

---

## 附录 A：设计文档 vs 实现落地对照表

以 `knowledge/design/*.md` 五份文档为对象，核对截至审计时刻 `crates/**` 的实际落地程度。

| 设计文档 | 规格实现阶段 | 落地程度 | 证据 |
|---|---|---|---|
| `item-system.md`（物品系统） | P5 | **零实现** | 全仓库搜索 `ItemDef`/`ItemStack`/`ItemLocation`/`Owner`（物品归属枚举）均零命中。纯设计。 |
| `equipment-slots.md`（装备栏位） | P5 | **零实现** | 全仓库搜索 `SlotMask`/`EquipSlot`/`equip_mask` 均零命中。纯设计。 |
| `attribute-system.md`（属性系统） | P3（骨架）+ P5（技能树） | **字段布局已落地，公式与衍生属性未落地** | `crates/ll-world/src/entity/stats.rs` 已实现 `BaseStats`（STR/DEX/CON/INT/WIS/CHA 六项整数字段 + `BASELINE` 常量），模块文档自述"具体的伤害/判定公式属于后续批次"。三系攻防、四种穿透、衍生属性纯函数、幸运机制、次级属性、d20 判定——设计文档二至九节全部尚未落地。 |
| `society-and-affiliation.md`（社会与归属） | P8（`society-and-affiliation.md` 内注明） | **字段布局已落地，关系/声望逻辑未落地** | `crates/ll-world/src/entity/affiliation.rs` 已实现 `AffiliationKind`（势力/宗教/行会/文化/家族/职业六类）与 `Affiliation { kind, org, standing }`，模块文档明确写着"实现阶段是 P8，本任务只建 P3 建 `Agent` 时必须已经存在的字段布局"。声望如何传播、如何驱动交易折扣/治安反应等行为逻辑尚未落地。 |
| `agent-goals-and-economy.md`（智能体目标与经济） | P8（目标栈）/ 贯穿（钱包） | **目标栈仅字段布局，钱包机制已有实质实现** | 目标栈：`crates/ll-world/src/entity/goal.rs` 仅 `Goal { kind, params, progress, priority }` 字段布局，模块文档自述"P3 阶段这个栈可以留空"。钱包：`crates/ll-world/src/entity/thin.rs` 已实现薄层人口的**惰性追赶钱包公式**（`wallet_of`、`batch_update_wallets`，按 §9.2 E2"薄个体+厚群体"的设计原话落地），`crates/ll-world/src/entity/agent.rs:39` 厚层 `Agent` 也有 `wallet: i64` 字段，`crates/ll-sim/src/effect.rs` 已有 `AdjustWallet` Effect。但市场节点、供需定价、价格稳定器、`ll-econsim` 压测——设计文档描述的完整经济模型（对应规格 §9.3）仍是零实现。 |

**这张表要传达的信息**：物品/装备两个系统目前**完全是纸上谈兵**，任何"物品系统已经有雏形"的印象都是错的；属性/归属/目标三个系统是**刻意提前预埋的字段布局**（P3 阶段的既定策略，规格 §15 P5/P8 行的"迁移债务"条目对此有明确交代，不是范围蔓延）；经济系统里恰好**钱包这一块是例外**，已经有可运行的薄层批量结算机制，比其余几项设计文档超前不少。

---

## 附录 B：交接清单其余条目核实结果（未在正文单列的部分）

以下条目本次逐一核实，**结论为"仍然成立/仍然有效"，不构成新发现，也不需要修改**，仅作记录：

- `p1-to-p2.md`「无独显环境的软件后端回退仍未验证」——CI 中确实没有任何 `llvmpipe`/`lavapipe`/`LIBGL_ALWAYS_SOFTWARE` 相关配置，视觉回归（L7）测试层本身也未接入 CI，与"仍未验证"的记录一致。
- `p1-to-p2.md`「`Camera::visible_tiles` 世界尺寸下限」——已被 `ChunkGrid::new`（`crates/ll-world/src/chunk.rs`）在构造点拒绝过小尺寸，`crates/ll-world/tests/determinism.rs:104-150` 四个边界测试覆盖，与 P2→P3 清单第三节记录一致，机制健在。
- `p0-to-p1.md`「crossbeam 通道类型外泄」「通道无背压」——`crates/ll-platform/src/jobs.rs:23` 仍然是 `pub use crossbeam_channel::{Receiver, Sender, unbounded as channel};`，两条风险均未处理。这属于交接清单本就标注为"P1 需处理"的**未完成技术债**而非文档失实，不算脱节，仅记录在案。
- `p2-to-p3.md` 第四节三项"规格无人认领"条目：光照透过率、气候条带两项见正文 M-2；季节时间轴事件一项见正文 H-3（已升级为独立发现，因为它现在有了新证据——P3 的 Effect 骨架已经成型却仍未回应这个决策点）。

---

## 相关文档

- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md)
- [P0 → P1 交接清单](../handoff/p0-to-p1.md)（本次审计已更新）
- [P1 → P2 交接清单](../handoff/p1-to-p2.md)
- [P2 → P3 交接清单](../handoff/p2-to-p3.md)（本次审计已更新）
- [工单清单](worklist.md)
