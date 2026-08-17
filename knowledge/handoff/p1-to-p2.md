# P1 → P2 交接清单

**冻结于** 2026-08-17，P1 渲染与动画层结束时。
**读者**：P2（世界与地形层 `ll-world`）的规划者与实现者。

---

## 一、P2 第一天就会撞上的三件事

### 1. 子格插值的浮点绝不能回流世界状态

P0 交接清单已强调过一次，P1 没有触碰它（P1 只做整格对齐绘制），因此这条**原封不动移交给 P2**——而 P2 做角色平滑移动时一定会撞上。

```
错误：world.entity_pos = lerp(from, to, t)   // t 是浮点，世界状态被污染
正确：world.entity_pos 始终是整数格坐标
      渲染层自己算插值，结果只用于本帧绘制，不写回
```

判断标准：**这个浮点值会不会被 `serde` 写进存档？** 会，就是错的。

### 2. `Camera::visible_tiles` 有一个隐含的世界尺寸下限

它以相机为中心取 **43×25 格**（由 `LOGICAL_WIDTH/TILE_SIZE/2+1` 与 `LOGICAL_HEIGHT/TILE_SIZE/2+1` 各向两侧展开得出）。

**世界任一维度小于这个跨度时会产出重复的 `TorusPos`**，而 `world_to_screen` 对同一坐标只给一个位置，结果是地形填不满、留黑块。

P1 的验收 demo 靠 `WORLD_WIDTH = 48`、`WORLD_HEIGHT = 32` 规避。P2 生成真实世界时**必须保证两个维度都大于该跨度**，或改造 `visible_tiles` 使其在小世界下正确处理重复坐标。

### 3. 世界状态的形状已被三份设计文档预定

物品、装备、属性三个系统的类型布局已经冻结。P2 建 `WorldState` 时**必须按它们的形状预留位置**，否则 P5 落地时要返工整个世界层。

要点：`ItemDef` 在注册表、`ItemStack` 在世界状态；22 个装备槽位用 `u32` 位掩码；**衍生属性绝不进世界状态**，由纯函数从基础属性 + 装备 + 状态每次重算。

---

## 二、渲染层已就绪的能力（P2 可直接用）

| 能力 | 入口 |
|---|---|
| 环面坐标 → 屏幕像素 | `Camera::world_to_screen`，**已处理跨接缝最短路径，不需要画多份拷贝** |
| 视口内瓦片枚举 | `Camera::visible_tiles`（注意上面的尺寸下限） |
| 图集查询与 UV | `Atlas::uv_rect(name)`，**已含半 texel 内缩**，不要自己再算一遍 |
| 精灵视觉尺寸 | `AtlasEntry::sprite_size()` |
| 逻辑占地格数 | `AtlasEntry.footprint` |
| 排序键 | `DrawOrder::new(layer, foot_y, entity)`——**必须传脚底 Y，必须带实体号** |
| 批量提交 | `SpriteBatch::push` + `flush`，整帧一次 draw call |
| 动画取帧 | `Playback::current_frame(clips, FrameId)`，整数帧号驱动 |
| 离屏渲染与呈现 | `RenderTarget` + `GpuContext::acquire_frame` → `blit_to` → `queue().present` |
| 视觉回归钩子 | `RenderTarget::read_pixels`，离屏格式固定 `TARGET_FORMAT`（跨平台可比对） |

`ll-render` 已 `pub use wgpu;`，下游**不要**自带 wgpu 依赖。

---

## 三、已知风险与未验证项

| 项 | 状态 |
|---|---|
| **无独显环境的软件后端回退** | **仍未验证**。CI 视觉回归自动比对的前置条件就是它，P2 若要接 CI 视觉比对，必须先验这个 |
| 非 sRGB surface 平台的画面偏暗 | 已缓解：`gpu.rs` 优先选 sRGB 变体，找不到才退回并 `tracing::warn!`。**不要在着色器里加手动 gamma 兜底**——那会在正常路径上双重转换、画面过亮 |
| 窗口最小化是否真送 `(0,0)`、多显示器缩放是否重复触发 `resumed` | 未实机验证。零尺寸防线已在 `GpuContext::resize` 建好 |
| `read_pixels` 内部用 `expect` 而非返回 `Result` | 已知接受。若 P2 的视觉回归需要区分「基础设施故障」与「渲染结果不符」，再改签名 |
| `GameKey` 不认识 F1–F12 | demo 用 M 键代替存图。P6 做按键重绑定时一并加 |

---

## 四、必须继续遵守的纪律

- **每个阶段闸口手工实跑 `cargo deny check`**。feature 合并会让已移除的依赖悄悄回归——`ttf-parser` 就是靠裁 winit 的 feature 才移除的。
- **跨平台确定性基准与视觉回归基准都不得随手更新**。测试挂了先排查根因；确认是有意调整才更新基准，并在提交信息里说明改了什么、为什么。
- **只有 `ll-platform` 接触窗口库**。P1 曾出现 `ll-render` 直接依赖 winit，已收回改为经 re-export。
- **公开 API 若暴露第三方类型，就该 re-export 那个 crate**。`ll-render` 因此 re-export 了 wgpu。

---

## 五、给 P2 计划作者的一条方法论

P1 期间出现了**三次同类计划缺陷**，全部是计划作者的疏漏，都不是实现者的错：

1. 误删 Esc 退出通道——照上一阶段的**计划文本**写，没照**当前代码**
2. `ll-render` 直接依赖 winit——没走一遍「这个类型从哪来」
3. `GpuContext` 拿不到窗口帧——管线每段都设计了，**没走一遍完整调用链**

共同点：**按「模块清单」思维写接口，而不是按「调用链」思维**。列出 `device()`、`queue()`、`surface_format()` 看起来很完整，直到有人真要把画面显示出来，才发现从头到尾没有一条路径走得通。

这类缺陷的特征是：**编译通过、测试全绿、每个模块单独看都正确**。只有当有人真把它们串起来用，才会发现中间断了一节——这也正是「每阶段必须交付可独立运行的验收 demo」的价值所在。

**因此 P2 的计划必须在「自查」一节写出一条从输入到输出的完整调用链**，逐个 API 点名，确认每一步的参数都能从上一步拿到。这是三次返工换来的教训。

---

## 相关文档

- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) — 唯一真相源
- [P0 → P1 交接](p0-to-p1.md) — 其中「浮点不得回流」一条仍然有效
- [物品系统](../design/item-system.md)
- [装备栏位与占位掩码](../design/equipment-slots.md)
- [角色属性系统](../design/attribute-system.md)
- [世界状态一律用整数](../decisions/0002-integer-only-world-state.md)
