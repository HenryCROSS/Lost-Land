# 批次 27 实施计划：把三张纯 CPU 视觉基准从历史留档搬成带比对的测试

**日期**：2026-08-31
**工作树**：`wt-visual`（分支 `wt-visual`，基于 `origin/main` 的 `0ce3736`）
**裁定依据**：[ADR 0030](../../../knowledge/decisions/0030-remove-examples-acceptance-demos.md)
「后果」一节列的三条路，所有者批准走第 2 条——**把生成逻辑搬成测试**。
**任务书本体**：[`crates/ll-game/tests/visual/README.md`](../../../crates/ll-game/tests/visual/README.md)

---

## 一、现状核实（本批开工前逐条 grep 过）

| 事实 | 判据 |
|---|---|
| `crates/ll-game/tests/visual/` 有三张 PNG | `surface_preview.png` 23411 B、`settlement_preview.png` 20140 B、`npc_roster_preview.png` 36258 B |
| 唯一生产者是三个 example target | `git log --oneline --diff-filter=D -- crates/ll-game/examples` → `58cd7ab` |
| 三个 example 源码在 git 里逐字节留着 | `git show 58cd7ab^:crates/ll-game/examples/{surface,settlement,npc_roster}_preview.rs` → 327 / 170 / 244 行 |
| 三张图纯 CPU、不需要 GPU/窗口 | 三份源码都只用 `image` + `PackedAtlas::canvas` 的像素拷贝，无 `wgpu`、无 `ll-platform` |
| `image` 已随 example 从 `ll-game` 的 dev-dependency 里删掉 | `crates/ll-game/Cargo.toml` 末尾那段注释就是删除记录 |
| 给机器看的那一半没受影响 | `tests/surface_render.rs` / `atlas_coverage.rs` / `npc_appearance.rs` 三个文件仍在 |
| `run_tests.sh` 就是 `cargo test --workspace` | 因此落在 `crates/ll-game/tests/` 下的 `#[test]` **自动**进常规门禁路径，不需要改脚本 |

**它卡住了谁**：地形美术变化那一批（同一种地形按位置哈希取多张贴图）会让
`surface_preview.png` 与 `settlement_preview.png` 过期，而今天没有任何人能重新出图。

---

## 二、A：把出图逻辑搬成 `#[test]`

### 落点：**一个**测试二进制，不是三个

`crates/ll-game/tests/visual_baselines.rs`，内含三条 `#[test]`；共用的比对/写盘
机制放在 `crates/ll-game/tests/visual_support/mod.rs`（子目录，cargo 不会把它
当成第四个测试 target）。

**为什么合成一个 target**：交接文档纪律第 8 条——`ll-game` 的测试二进制要链
257 个 object file 加一百多个 rlib，两三个并发就能把 MSVC 链接器挤爆
（`LNK1102`）。三个新 target 等于把这个已知的假失败源放大三倍，而三条测试
之间没有任何隔离需求。

### 硬约束：出图逻辑不许自己重推

README 强调 example「一行都没有自己重推」。搬家之后这一点不许退化，具体是
下面这张对照表——**每一行都是「本文件不做，调生产代码」**：

| 这件事 | 由谁算 |
|---|---|
| 世界长什么样 | `ll_game::world::build_new_world`（固定种子） |
| 图集怎么打 | `ll_game::app::load_sprite_sources` + `ll_render::atlas_pack::pack_atlas` |
| 地表内容画在哪一格、用哪个键 | `ll_game::surface_draw::surface_draws` |
| NPC 两层用哪两个键、谁压制谁 | `ll_game::surface_draw::npc_draws` + `SurfaceDraw::superseded_by` |
| 地形该查哪个图集键 | `ll_game::layout::terrain_atlas_key` |
| 世界坐标 → 屏幕坐标 | `ll_render::camera::Camera::world_to_screen` |
| 精灵落笔位置（锚点/脚印） | `ll_render::sprite::sprite_draw_position` |
| 谁挡住谁 | `ll_render::sprite::DrawOrder` + `footprint_bottom_screen_y`，`sort_by_key` |

测试文件自己只做三件事：摆场景（哪一格放什么）、把矩形按算好的位置拷进画布、
整数放大存盘。这与被删的 example 逐字同一分工。

### 不新增 example target

ADR 0030 明令禁止，`scripts/ci/check_no_examples.sh` 会拦（它不接受「加进某张
清单」这种消红方式）。本批一个 `[[example]]` 段都不加。

### 依赖

`crates/ll-game/Cargo.toml` 的 `[dev-dependencies]` 加回
`image = { version = "0.25", default-features = false, features = ["png"] }`
——与 `ll-render`、`tools/ll-artgen` 同一份版本与 feature 集合，避免同一个 crate
在工作区里被解析成两份。Cargo.toml 末尾那段「随 example 一并移除」的注释按
纪律第 9 条**原文保留、原地追加更正**并指回本计划。

---

## 三、B：比对基准 + 环境变量覆盖

### 判据：**逐像素相等**（先比尺寸，再比 RGBA 四通道）

比的是**解码后的像素**，不是 PNG 文件字节。理由两条：

1. **PNG 编码字节不是稳定判据**：`image` crate 换一个小版本就可能改压缩参数，
   同一份像素编出来的字节不同。比字节会在与画面无关的地方变红。
2. **像素应当逐位确定**：这三张图的每一个像素都来自
   `PackedAtlas::canvas.get_pixel()` 的整数拷贝——没有浮点、没有插值、没有
   抗锯齿、没有 alpha 混合运算（透明像素是**跳过**不是混合）。上游的世界生成
   有黄金基准盯着确定性，图集打包不经任何哈希容器（约束 C5）。因此
   **逐像素相等是这条路径应当能达到的最强判据**，实测是否真的如此见落地报告。

**为什么不设容差**：本会话已出现 15 次「测试全绿但保护不存在」，而本批最可能的
形状恰恰是「比对写得太宽松 ⇒ 图画坏了也绿」。容差与「只比尺寸+哈希」都属于
把咬合力主动放掉。逐像素相等是唯一不需要论证「多大差异算差异」的判据；如果
将来真的出现无害抖动，那时再带着实测证据放宽，比现在预防性放宽安全。

### 红了之后人要能一眼看出差在哪

比对失败时往 `target/visual-baselines/` 写三份产物并在断言消息里给出路径：

- `<name>.actual.png`——本次实际产出（可直接与仓库里那张并排看）；
- `<name>.diff.png`——不同的像素涂成洋红、相同的像素压暗，差异区域一眼可见；
- 断言消息里带**不同像素数 / 总像素数**与**前若干个不同点的坐标与两侧 RGBA**。

只说「不一样」不算数——那正是 README「比对失败时的处置规矩」要人做判断的那
一步所需要的输入。

### 有意更新：`LL_BLESS_VISUAL=1`

置为 `1` 时把本次产出**写回基准路径**并通过，同时打印「覆盖了哪张、原来多少
像素不同」。规矩仍按 README：**绝不允许「看着不一样就重新跑一遍覆盖」**，
`--bless` 只在人已经判断过「是有意的视觉调整」之后用，并在提交信息里写清
改了什么、为什么。

### 门禁化

三条测试是 `crates/ll-game/tests/` 下的普通 `#[test]`，`scripts/ci/run_tests.sh`
就是 `cargo test --workspace`，**因此它们自动进常规路径**。本批不加 `#[ignore]`、
不加任何「拿不到就跳过」的分支——那正是 ADR 0022 要根除的假测试形状，而这三
张图不需要 GPU，没有任何可跳过的理由。

### 基准图尺寸取舍

三张图沿用 example 原来的 `ZOOM = 4` 整数放大，**不降尺寸**。取舍：

- 存 1× 图能省 16 倍像素、信息量完全等价（最近邻放大不产生新信息），但
  **会让本批 C 这一步失去判据**——每一个像素的坐标都变了，没法把新产出与
  仓库里那三张历史图逐张对差异。
- 三张图现在合计约 80 KB，即使名册涨到九个种族也在一两百 KB 量级，对仓库
  体积不构成问题。
- 图的既定用途是「给人看」（README 首句），1× 的 16 像素瓦片看不出门把手、
  窗棂、胸口 6×6 徽记的形状。

---

## 四、C：确认三张图今天仍然是对的

`58cd7ab` 之后主干走了二十多个提交，其中至少这几批会动到这三张图：

| 提交 | 可能影响哪张 |
|---|---|
| `8f3a202` 五个新种族（骆驼人/猫人/欧克/蜥蜴人/鱼人） | `npc_roster`（行数） |
| `b85330c` 批次 24 合并（117 张合成图） | `npc_roster`（每一格的画法） |
| `06e4de4` 据点有街道、`e40cd6a` 建筑类型由文化声明、`d893866` 按类型摆家具 | `surface`（世界内容） |
| `8716f26` 势力播种进世界状态与哈希 | `surface`（编年史 RNG 流 ⇒ 地形与出生点） |
| `90fdb6a` 沙漠文化沙民 | `surface` |

**判据用 README 自己写的「应当能一眼看见什么」**：五样东西 / 九样据点建筑地形 /
种族 × 职业点名册（**现在应当是九个种族起，不是四个**）。

逐张判断差异属于哪一类：

- **主干正确演进** ⇒ 用 `LL_BLESS_VISUAL=1` 更新基准，并在提交信息里**逐条**
  说明每一处差异对应哪一批改动；
- **真的画坏了** ⇒ **停下来报告**，不把 bug 冻进基准。

---

## 五、反例验证（ADR 0022，硬要求）

**注意编号**：讲反例验证的是
[ADR 0022](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)，
不是 ADR 0018——仓库刚在批次 25 扫除过 68 处这个误引，本批不跟着写错。

必验的三条（任务点名）：

| # | 改坏什么 | 期望 |
|---|---|---|
| ① | 把某张基准 PNG 换成另一张图 | 那条测试红，且红的理由是「像素不同」而不是「尺寸不同」「文件读不出来」 |
| ② | `LL_BLESS_VISUAL=1` | 基准真的被覆盖成新产出，再跑一次默认路径变绿 |
| ③ | 改坏 `ll_game::surface_draw` 里某一处生产代码 | `surface`（与受影响的其它张）跟着红——证明出图真的走了生产代码 |

外加每条测试各自的机制反例（尺寸不一致这一支、比对函数本身空转这一支）。

**每一条都要确认红的原因是我以为的那个**，不能只看到红就算过——本会话已出现
「测试红了但理由是错的」与「断言在主干未改动时就已经是红的」两种事故，因此
**写断言之前先跑基线**。

---

## 六、门禁与提交

- 改前基线：本批自己跑一次 `bash scripts/ci/run_tests.sh` 记数，不抄别人的数字
  （纪律第 4 条）。
- 提交前 `bash scripts/ci/run_all.sh` 必须 exit 0。
- 文件行数棘轮：新文件不许超 800 非空非注释行；真超了就拆，`--bless` 的理由
  不许留空。
- 不许动 `surface_render.rs` / `atlas_coverage.rs` / `npc_appearance.rs` 的既有
  断言（README 明说那一半没受影响）。可以加，不改。
- A / B / C 分成多个提交，中文提交信息。**不 push、不合并 main。**

## 七、不碰什么

`LostLand`（main 工作树）与 `wt-dialogue3`（并行批次，在改
`crates/ll-sim/src/dialogue.rs`、`crates/ll-world` 的 `Agent`、存档 schema、
`crates/ll-game/src/dialogue_screen.rs`，并会重冻
`crates/ll-game/tests/populated_determinism.rs`）。本批的落点与它零交集。

---

## 八、落地记录

（落地后回填：三条测试的实际落点、逐像素确定性实测结论、三张图逐张的差异
清单、反例验证结果、改前/改后测试数、门禁 exit code、规格没裁定而临时选的
做法。）
