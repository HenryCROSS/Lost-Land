# 0003 — winit 版本与 feature 策略

**日期**：2026-08-16
**状态**：已生效
**影响范围**：`ll-platform`，间接影响全部上层

## 背景

P0 收尾时 `cargo deny check advisories` 由通过转为失败，命中
**RUSTSEC-2026-0192**：`ttf-parser` 被作者宣布停止维护、不再修复，
且**没有安全的升级版本**。

依赖链：
```
ll-platform → winit → sctk-adwaita → ab_glyph → owned_ttf_parser → ttf-parser
```

`sctk-adwaita` 是 winit 在 Linux Wayland 下绘制客户端窗口装饰所用；
`wayland-csd-adwaita` 是 winit 的**默认 feature**，它同时启用
`sctk-adwaita/ab_glyph`——后者才是 `ttf-parser` 的真正来源，用途是画标题文字。

## 决定

### 一、版本锁 0.30 稳定线

`winit = "0.30"`。查证时最新发布版是 **0.31.0-beta.2**（发布已逾八个月，
至今未 stable）。**地基层不压 beta。**

### 二、显式 feature 列表，裁掉 Adwaita 标题文字

```toml
winit = { version = "0.30", default-features = false, features = [
    "rwh_06", "x11", "wayland", "wayland-dlopen", "wayland-csd-adwaita-notitle",
] }
```

与默认集合逐项比对：**仅把 `wayland-csd-adwaita` 换成
`wayland-csd-adwaita-notitle`，其余四项一字不差。**

- `rwh_06` **绝不能删**——wgpu 靠它取 raw-window-handle
- winit 对 `sctk-adwaita` 的依赖声明带 `default-features = false`，
  故 `-notitle`（feature 列表只有 `["sctk-adwaita"]`）仍保留窗口装饰，只丢标题文字
- Windows 与 macOS 的依赖图**改动前后逐字相同**——其余项均在
  `cfg(all(unix, not(any(redox, wasm, android, ios, macos))))` 门控下

### 三、裁剪而非豁免

**不在 `deny.toml` 中豁免该公告。** 豁免是掩盖，裁剪是真正把包移出依赖树。
本项目自绘全部 UI（cosmic-text + Fusion Pixel Font），不需要 winit 画标题文字。

## 被否决的替代方案

`winit 0.31` 另计——它不是换库，是同一个库的下一条版本线，仍是 beta（发布已逾
八个月未 stable），故不在下表中，理由见「决定・一」。

真正的换库候选，2026-08 实查：

| 候选 | 版本 | 许可证 | 否决理由 |
|---|---|---|---|
| sdl3 | 0.18.4 | MIT | 需要 SDL3 这个 C 库，各平台要处理捆绑/构建，违背纯 Rust 跨平台目标 |
| sdl2 | 0.38.0 | MIT | 同上，且已是上一代，上游重心在 SDL3 |
| glfw | 0.62.0 | Apache-2.0 | 同为 C 依赖；GLFW 本体最近稳定版是 2024-02 的 3.4，活跃度低于 winit |
| miniquad | 0.4.11 | MIT/Apache-2.0 | 自带 GL 渲染器，与既定的 wgpu 方案冲突 |
| tao | 0.36.0 | Apache-2.0 | Tauri 对 winit 的分叉，面向桌面应用（菜单/托盘）而非游戏，依赖树更大 |

换 SDL/GLFW 都会引入 C 依赖，与「纯基础库、好跨平台」的项目约束冲突；miniquad
自带渲染器与 wgpu 方案二选一冲突；tao 面向桌面应用而非游戏。
winit 侧 rust-windowing 组织活跃（每周五 UTC 15:00 例会，各平台 issue 持续处理），
是 wgpu 生态事实标准，故结论仍是留在 winit，只裁 feature。

## 架构保障

只有 `ll-platform` 这一个 crate 直接依赖 `winit`；上层只看得到
`AppHandler` / `InputState` / `GameKey` 这几个平台无关的抽象。将来若真要换窗口库，
改动面收在这一个 crate 内，不会波及整个项目——这也是敢把 winit 锁在稳定线、
只做 feature 裁剪而不是急着换库的底气所在。

## 后果与持续风险

**代价**：Wayland 下窗口装饰栏不显示标题文字（X11 与 Windows 由服务端绘制，不受影响）。
**P7 联动**（[2026-08-18 规格修订] 插入「物品与装备」新 P6 阶段后，i18n/UI 层原 P6 顺移为 P7）：接入 i18n 设置真实标题后，**Wayland 用户仍然看不到标题**。
届时若 `ttf-parser` 已有维护继任者，可改回 `wayland-csd-adwaita`。

**必须持续盯的风险 —— feature 合并（unification）**：
将来任何一个依赖以默认 feature 拉 winit，`wayland-csd-adwaita` 会**重新启用**，
`ttf-parser` 悄悄回到依赖树。`cargo deny check advisories` 能抓到——
但仓库目前**没有 git 远程，CI 不在任何地方运行**。

> **在 git 远程建立之前，`cargo deny check` 必须在每个阶段闸口手工实跑一次。**

另注：显式列 feature 意味着 winit 0.30.x 后续若新增默认 feature，不会自动获得。
