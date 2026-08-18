# P0 → P1 交接清单

**冻结于** 2026-08-16，P0 平台地基阶段结束时。
**读者**：P1（渲染与动画层）的规划者与实现者。

这份清单来自 P0 最终评审后留下的已知问题与前瞻判断。它不是待办列表，而是**P1 第一天就会撞上的东西**——提前知道能省下返工。

---

## 一、P1 第一天必须改动的平台层接口

> **[2026-08-17 doc-code-audit 更新] 本节四项均已修复**，修复于提交 `0c9fb27`（`refactor: 平台层为接入渲染做接口改造`）。`AppHandler::on_resume` 现接收 `Arc<Window>`，新增 `on_resize` 转发 `Resized`/`ScaleFactorChanged`，`on_frame` 携带整数 `FrameId`，`PlatformError::WindowCreation` 已随该提交移除。详见 `knowledge/audit/2026-08-17-doc-code-audit.md` 的 H-1。以下原文保留供追溯问题的历史背景。

`ll-platform` 当前的接口是按「P0 不渲染」设计的，接入 wgpu 时三处必然要动：

1. **`AppHandler` 拿不到 `Window`**。wgpu 需要 `Window` 来创建 surface，而当前 `Window` 被 `App` 私有持有，回调里够不着。
2. **没有 `Resized` / `ScaleFactorChanged` 事件转发**。窗口尺寸变化时 surface 必须重建，否则画面拉伸或崩溃。
3. **`on_frame` 不带帧号**。动画播放需要一个整数帧计数器；**它必须是整数**，不能用墙钟时间的浮点秒数——那会让动画状态无法安全地进入世界状态。

改这三处时请一并处理下面这条：`PlatformError::WindowCreation` 变体现已无构造点（建窗失败只走日志后退出），属于死代码，按规格 §13 应当清理。

## 二、渲染层最容易无声破防的一处

**`ll-core` 没有提供「定点 → 像素」的换算，这是刻意的。**

P1 做子格插值（角色在两格之间平滑移动）时会需要浮点。**浮点结果绝不得回流世界状态**——这是全项目最容易在不知不觉中破坏跨平台确定性的地方，因为它看起来完全无害：

```
错误：world.entity_pos = lerp(from, to, t)   // t 是浮点，世界状态被污染
正确：world.entity_pos 始终是整数格坐标
      渲染层自己算插值，结果只用于本帧绘制，不写回
```

判断标准很简单：**这个浮点值会不会被存档序列化？** 会，就是错的。

## 三、Y 排序的稳定性要求

混合尺寸精灵（普通单位 16×24 占 1 格，重点目标 32×48 占 2×2 格）需要 Y 排序解决遮挡。两条硬性要求：

- 排序键用精灵的**脚底 Y**，不是精灵原点 Y。用原点会让高精灵在视觉上错误地挡住前排单位。
- **必须配实体 ID 作第二排序键**，构成稳定全序。否则同一世界状态可能画出不同的遮挡关系——这既是视觉抖动，也会让视觉回归测试无法冻结基准。

## 四、性能与接口债（P0 遗留，P1 需处理）

| 项 | 现状 | P1 的动作 |
|---|---|---|
| 主循环无节流 | `ControlFlow::Poll` + 每帧 `request_redraw`，当前吃满一核 | P0 无渲染时无害，**接 wgpu 前必须加帧预算** |
| 通道类型外泄 | `crossbeam` 的 `Sender`/`Receiver` 被直接 re-export 为公开 API | 上游升大版本即成为本 crate 的破坏性变更；考虑包一层自有类型 |
| 通道无背压 | `unbounded` 无容量上限 | 资产加载队列若无背压，快速切场景时内存会涨；至少补文档说明 |

## 五、持续性纪律

- **每个阶段闸口手工实跑 `cargo deny check`**。feature 合并会让已移除的依赖悄悄回归——`ttf-parser` 正是靠裁 winit 的 feature 才移除的（见 [0003](../decisions/0003-winit-dependency-policy.md)），而 P1 引入 wgpu 与字体库时极可能重新拉回同类依赖。
- **跨平台确定性基准不得随手更新**。`crates/ll-core/tests/determinism.rs` 顶部写明了规矩：测试挂了要排查根因，不允许把期望值改成实际值。

## 六、已知且已接受的代价

- **Linux Wayland 下窗口标题栏不显示标题文字**。这是 `wayland-csd-adwaita-notitle` 的代价，为移除已停止维护的 `ttf-parser` 而付出。装饰边框仍在。P6 接入 i18n 后此现象依旧存在，**不是缺陷**。

---

## 相关文档

- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) — 唯一真相源
- [0002 — 世界状态一律用整数](../decisions/0002-integer-only-world-state.md) — 第二节的完整论证
- [0003 — winit 版本与 feature 策略](../decisions/0003-winit-dependency-policy.md) — 第六节代价的由来
