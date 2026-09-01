# 视觉回归基准

`baseline/` 下的 PNG 是**视觉回归基准**，来自
`crates/ll-render/examples/p1_acceptance/main.rs`：运行该 demo 后按
F2 把当前离屏渲染目标（`ll_render::target::RenderTarget::read_pixels`）
存成 PNG。

`p1_acceptance.png` 是这条通路产出的基准：640×360，地形棋盘格铺满
视口、boss（重点目标）与 hero（普通单位，当时处于 `hero_idle_0` 帧）
均在画面内且互不重叠。

## 已知过期：玩家贴图重画（2026-08-28）

**`baseline/p1_acceptance.png` 里画的 hero 是重画之前那一版**（一整块
实心钢蓝矩形）。所有者裁定「目前的贴图有点丑了」之后，八张 `hero_*`
被重画成有轮廓、有明暗的人形，`assets/atlas/placeholder.png` 因此变了，
这张基准与它已经对不上。

按本文件下面那三条规矩，第 1、2 条**已经执行完**：这是一次经所有者裁定
的、有意的视觉调整，不是缺陷；改了什么、为什么，记在
`assets/atlas/README.md` 的「玩家贴图重画」一节与
`tools/ll-artgen/src/sprite.rs` 模块文档里。缺的只有「重新截一张」这一步
——`p1_acceptance` 要开真实窗口、要图形适配器（见
`scripts/ci/run_acceptance_demos.sh` 的 `SKIP_LIST`），当时的开发环境
拿不到 GPU，截不出新图。

**处置：宁可留一张写明「已过期」的旧基准，也不写一张没人真正看过的新
基准。** 这张图只能用来看「布局有没有乱」（地形棋盘、boss 与 hero 的相对
位置），**不能**用来比 hero 那块像素。

**〔2026-08-29 补记〕** 「下一次在有 GPU 的机器上跑 `p1_acceptance` 的人
截一张新图覆盖它」这条指示**已经执行不了**：`p1_acceptance` 这个 example
target 已随所有者裁定删除，见本文件末尾「生产者已删除」一节。要重拍这张
图，得先按那一节的方式把生产者恢复出来。

生成侧的重冻证据（`assets/atlas/placeholder.png` 那一半）走的是四步，
实跑记录见玩家贴图重画那个提交的提交信息。

## 比对失败时的处置规矩

渲染改动导致某次比对与基准不一致时：

1. **先判断是有意的视觉调整还是缺陷**，不要假设任何一种。
2. 只有确认是**有意调整**才更新基准，并在提交信息里说明改了什么、
   为什么——这条说明和代码改动同等重要，日后排查回归全靠它。
3. **绝不允许「测试挂了就重新截图覆盖」**。不经判断直接覆盖基准，
   等于删掉了这道防线本身——基准存在的意义就是拦住未经确认的视觉
   改动，覆盖掉不一致的基准而不追问原因，会让防线名存实亡。

这与 `crates/ll-core/tests/determinism.rs` 顶部对黄金基准的规矩是
同一条：基准是需要被保护的资产，不是可以随手刷新的缓存。

## 当前比对方式：人工

本仓库尚未接入 CI 自动像素比对（需要无头 GPU，如 lavapipe 或 WARP，
验证成本较高，留给 P1 收尾后单独处理，见
`.superpowers/sdd/2026-08-17-p1-render-animation/task-9-brief.md` 的
自查表）。原先的比对方式是：跑一次 `p1_acceptance` demo，按 F2 存一张
新 PNG，与 `baseline/` 下的基准肉眼或用外部工具逐像素对比——**这条路径
自 2026-08-29 起需要先恢复生产者才能走**，见下一节。

## 生产者已删除（2026-08-29 批次 13）

**`baseline/p1_acceptance.png`** 的唯一生产者是 ``ll-render:p1_acceptance`（按 F2 存图）`。
2026-08-29 项目所有者裁定去掉 `examples/`（原话「我觉得应该要去掉 example。
然后有用的东西搬迁了。剩下的后面考虑。」），那个 target 随之删除，见
[ADR 0030](../../../../knowledge/decisions/0030-remove-examples-acceptance-demos.md)。

**图本身一张没删，删的是生产者。** 也就是说：这张基准现在**无法重新生成**，
在有人按上面的方式恢复生产者、或另立一套无头像素比对之前，它只能当作
**只读的历史留档**看——发现不一致时无法「重新截一张对比」，只能靠读代码判断。
（这张图要真实窗口 + 图形适配器才截得出来，无头 CI 上 `request_adapter` 会失败，
硬搬成测试只能写出「拿不到适配器就跳过」的假测试，正是 ADR 0022 要根除的东西。）

这不是一次「顺手删掉」——ADR 0030「后果」一节列了三条路（保留那一个 example /
把生成逻辑搬成测试 / 放弃这张基准）各自的代价，并明确写着**本批次不替所有者
做选择**，只做最保守、最容易反转的那一侧：图留着，生产者删掉，恢复方式写在
这里。

恢复被删的生产者（git 里逐字节留着，与提交哈希无关）：

```bash
git log --oneline --diff-filter=D -- crates/ll-render/examples
git show <那个提交>^:crates/ll-render/examples/p1_acceptance/main.rs
```
