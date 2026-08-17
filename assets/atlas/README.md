# 占位图集

`placeholder.png` 与 `placeholder.json` 是**程序生成的临时占位资产**，
不是美术成品。

- `placeholder.png`：64×64 RGBA PNG，由 `ll-render` Task 5 提交时的一段
  一次性脚本（未随本次提交保留）生成，纯色块拼出四块内容：16×24 的
  「普通单位」、32×48 的「重点目标」、两块 16×16 地形。每个单位顶部
  留了一小块浅色标记，仅用来在测试截图里辨认朝向，不代表任何美术设计。
- `placeholder.json`：与上图配套的 [`ll_render::atlas::AtlasMetadata`]
  元数据，条目名 `hero_idle_0`、`boss_idle_0`、`terrain_grass`、
  `terrain_dirt` 是内部标识符，供渲染层单测与集成测试引用。

## 待办

美术资产到位后：
1. 用真实图集替换 `placeholder.png` 与 `placeholder.json`。
2. 检查所有引用这些条目名的测试/代码是否需要同步改名。
3. 删除本文件，或更新为正式图集的说明。

在此之前，请勿删除或依赖这两个文件的具体像素内容——它们只保证
「结构合法、可解析」，不保证任何视觉效果。
