# 许可证归档 — `ll-text` 字体与图标字体资产入库

**日期**：2026-08-18
**对应任务**：Task 10（`ll-text` 地基）
**方式**：本文件归档的是**实际下载入库**的文件本身（不是采购前调研），字节数与许可证全文均为本次操作直接核实，不是转述。前置调研见
[文本与字体渲染管线](../pipelines/text-and-font-rendering.md) 与
[许可证扫描 — 文本渲染与字体调研](2026-08-17-text-font-license-scan.md)，本文件不重复那两份文档已有的候选对比，只记录「最终选了什么、从哪下载的、字节数多少、许可证全文放在哪」。

---

## 字体：思源黑体 Source Han Sans（Adobe 发行版）CN 地区子集

| 文件 | 来源 | 字节数 | SHA 来源核对方式 |
|---|---|---|---|
| `assets/fonts/SourceHanSansCN-Regular.otf` | `https://raw.githubusercontent.com/adobe-fonts/source-han-sans/release/SubsetOTF/CN/SourceHanSansCN-Regular.otf` | 8,429,224 字节 | 下载后本地 `ls -la` 字节数与 GitHub Contents API 返回的 `size` 字段一致（8429224），与前置调研文档记录的数字一致 |
| `assets/fonts/SourceHanSansCN-Bold.otf` | 同仓库同路径 `SourceHanSansCN-Bold.otf` | 8,569,308 字节 | 同上，与前置调研文档记录一致 |
| `assets/fonts/LICENSE-SourceHanSans.txt` | `https://raw.githubusercontent.com/adobe-fonts/source-han-sans/release/LICENSE.txt` | 4,463 字节 | 本次直接下载并读取全文，确认为 SIL Open Font License 1.1，版权声明行原文：「Copyright 2014-2025 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'.」 |

两个 OTF 文件合计约 **16.2 MB**（8,429,224 + 8,569,308 = 16,998,532 字节 ≈ 16.21 MB），与前置调研文档第 5.1 节的实测数字一致。**未做二次子集化**——理由见管线文档第 5.2 节：CN 地区子集本身覆盖已经足够宽，二次子集化换来的体积节省不值得冒「玩家自定义名字/mod 文本被切掉字形」的风险。

**许可证**：`OFL-1.1`，SPDX 已在 `deny.toml` 白名单内，无需改动。字体文件本身不是 Cargo 依赖，不进 `cargo deny check` 的扫描范围，但同样受本项目的许可证纪律约束——本文件即该纪律要求的存证。

---

## 图标字体：Tabler Icons 官方 webfont（默认描边粗细）

| 文件 | 来源 | 字节数 |
|---|---|---|
| `assets/icons/tabler-icons.ttf` | `https://cdn.jsdelivr.net/npm/@tabler/icons-webfont@3.46.0/dist/fonts/tabler-icons.ttf`（jsDelivr 镜像 npm 包 `@tabler/icons-webfont@3.46.0` 的产物；该包官方仓库 `packages/icons-webfont` 不直接提交构建产物到 git，构建产物随 npm 发布，故改用 jsDelivr 直接取发布版本的文件，版本号与前置调研文档核实的 `v3.46.0`/2026-07-28 一致） | 2,834,800 字节 |
| `assets/icons/LICENSE-TablerIcons.txt` | 同 npm 包根目录 `LICENSE` | 1,073 字节 |

**许可证**：`MIT`，下载后直接读取全文确认，版权声明行：「Copyright (c) 2020-2026 Paweł Kuna」。已在 `deny.toml` 白名单内。

**实测 PUA 码位**（本次从同一 npm 包的 `dist/tabler-icons.css` 里直接核对，不是转述文档）：

| 图标 | CSS 类名 | PUA 码位 |
|---|---|---|
| 设置 | `.ti-settings` | `U+EB20` |
| 关闭 | `.ti-x` | `U+EB55` |

两个码位均落在 Unicode 私用区（`U+E000`–`U+F8FF`），这是本任务「PUA 码位路由到图标字体」实测用的两个具体码位，实测结果见提交信息与代码注释。

**未下载的产物**：`tabler-icons.woff`/`.woff2`/`.svg`、200/300 描边粗细变体、filled 变体、CSS/HTML 辅助文件——本项目是原生 wgpu 渲染，不需要 web 字体格式或 CSS 辅助文件，`fontdb`/`cosmic-text` 直接吃 TTF/OTF，只取默认描边粗细的 `.ttf` 一份即够用。

---

## 结论

两笔资产共约 **19 MB**（字体 16.2MB + 图标字体 2.8MB），均已入库到 `assets/fonts/` 与 `assets/icons/`，许可证全文随资产一并入库（`LICENSE-SourceHanSans.txt`、`LICENSE-TablerIcons.txt`），不依赖运行环境已安装的系统字体。`deny.toml` 无需改动——`OFL-1.1`、`MIT` 均已在白名单内。
