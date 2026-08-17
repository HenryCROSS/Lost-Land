# 视觉回归基准

`baseline/` 下的 PNG 是**视觉回归基准**，来自
`crates/ll-world/examples/p2_acceptance/main.rs`：运行该 demo 后按
F2 把当前离屏渲染目标（`ll_render::target::RenderTarget::read_pixels`）
存成 PNG。

`p2_acceptance.png` 是这条通路产出的基准：640×360，正午光照下的
512×320 环面大陆一角——出生点附近的地形铺满相机视口，只有落在当前
视野（`ll_world::fov::compute_fov`）内的格子被点亮，视野之外保持黑色；
出生点旁人工摆放的山脊（[`ll_world::terrain::TerrainKind::MOUNTAIN`]）
挡住了它背后的格子；左上角是不受光照调制的小地图
（`ll_world::overview::continent_map` 的下采样概览）。

## 比对失败时的处置规矩

渲染或世界生成改动导致某次比对与基准不一致时：

1. **先判断是有意的视觉调整还是缺陷**，不要假设任何一种。
2. 只有确认是**有意调整**才更新基准，并在提交信息里说明改了什么、
   为什么——这条说明和代码改动同等重要，日后排查回归全靠它。
3. **绝不允许「测试挂了就重新截图覆盖」**。不经判断直接覆盖基准，
   等于删掉了这道防线本身——基准存在的意义就是拦住未经确认的视觉
   改动，覆盖掉不一致的基准而不追问原因，会让防线名存实亡。

这与 `crates/ll-render/tests/visual/README.md`、
`crates/ll-core/tests/determinism.rs` 顶部对黄金基准的规矩是同一条：
基准是需要被保护的资产，不是可以随手刷新的缓存。

## 当前比对方式：人工

本仓库尚未接入 CI 自动像素比对（需要无头 GPU，如 lavapipe 或 WARP，
验证成本较高，留给 P1 收尾后单独处理，见
`crates/ll-render/tests/visual/README.md` 与
`.superpowers/sdd/2026-08-17-p1-render-animation/task-9-brief.md` 的
自查表——这个未决项跨阶段延续，尚未被单独处理）。当前的比对方式是：
跑一次 `p2_acceptance` demo，按 F2 存一张新 PNG，与 `baseline/` 下的
基准肉眼或用外部工具逐像素对比。

## 这张基准与 `ll-render` 的 `p1_acceptance.png` 的区别

`p1_acceptance.png` 验的是渲染层本身（棋盘格地形、动画、Y 排序遮挡、
相机绕回），世界层用的是手写棋盘格，不是真实生成的地形。
`p2_acceptance.png` 验的是世界层产出接上渲染层之后的完整链路：真实
噪声生成的环面地形、分块存储、阴影投射视野、昼夜四季光照——棋盘格在
这里不适用，因为验收点本身就是「地形分布看起来像真实地形，而不是
人工图案」。两张基准职责不同，互不替代。
