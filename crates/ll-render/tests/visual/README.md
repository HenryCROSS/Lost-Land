# 视觉回归基准

`baseline/` 下的 PNG 是**视觉回归基准**，来自
`crates/ll-render/examples/p1_acceptance.rs`：运行该 demo 后按 M 键
（见该文件顶部文档「按键替代：M 而非 F2」）把当前离屏渲染目标
（`ll_render::target::RenderTarget::read_pixels`）存成 PNG。

`p1_acceptance.png` 是这条通路产出的基准：640×360，地形棋盘格铺满
视口、boss（重点目标）与 hero（普通单位，当时处于 `hero_idle_0` 帧）
均在画面内且互不重叠。

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
自查表）。当前的比对方式是：跑一次 `p1_acceptance` demo，按 M 存一张
新 PNG，与 `baseline/` 下的基准肉眼或用外部工具逐像素对比。
