# 许可证扫描 — 文本渲染与字体调研

**日期**：2026-08-17
**提交**：`ae6d7133e767b314918a2ce7e424eb9458133d36`
**方式**：手工核对，非 `cargo-deny` 自动扫描——本轮涉及的字体文件与 Rust crate 均**尚未**加入依赖树/资产目录，这是采购前的调研记录，不是既成事实的扫描报告。字体文件也是依赖，走本项目既有的许可证纪律（详见 [P0 收尾扫描](2026-08-16-p0-scan.md)）。

对应管线设计见 [文本与字体渲染管线](../pipelines/text-and-font-rendering.md)，本文件只归档许可证结论本身。

---

## Rust crate（文本栈）

| crate | 许可证（SPDX，`crates.io` API 实测） | `deny.toml` 白名单内 | 核实来源 |
|---|---|---|---|
| `cosmic-text` | `MIT OR Apache-2.0` | 是（MIT、Apache-2.0 均已在白名单） | `https://crates.io/api/v1/crates/cosmic-text` |
| `glyphon` | `MIT OR Apache-2.0 OR Zlib` | 是 | `https://crates.io/api/v1/crates/glyphon` |
| `fontdue` | `MIT OR Apache-2.0 OR Zlib` | 是 | `https://crates.io/api/v1/crates/fontdue` |
| `ab_glyph` | `Apache-2.0` | 是 | `https://crates.io/api/v1/crates/ab_glyph` |

四项均落在现有白名单（MIT / Apache-2.0 / Zlib），**不需要修改 `deny.toml`**。实际引入依赖后仍需按纪律跑一次 `cargo deny check`（仓库目前无远程，CI 不会自动跑，见 P0 扫描的提醒，此条仍然有效）。

## 字体文件

| 字体 | 许可证（SPDX） | RFN（保留字体名）条款 | 核实来源 | 核实强度 |
|---|---|---|---|---|
| **思源黑体 Source Han Sans（Adobe 发行版）** — 推荐 | `OFL-1.1` | 有，保留名为「Source」（原文："Reserved Font Name 'Source'"，"Source" 为 Adobe 在美国及/或其他国家的商标） | `LICENSE.txt`（`release` 分支）原文直接读取 | 高（一手文件原文） |
| Noto Sans CJK（Google 发行版） | `OFL-1.1` | 许可证类型确认为 OFL-1.1，但具体保留名字符串未在 `LICENSE` 文件的版权声明行里直接核实到 | `notofonts/noto-cjk` 仓库 `Sans/LICENSE` 原文读取 | 中（许可证类型高、RFN 具体字符串未核实） |
| HarmonyOS Sans（华为） | 非 SPDX 标准许可证，自定义 EULA | 不适用（非 OFL） | 许可证条款镜像页面 `sheep-realms.github.io` | 中（非官方一手页面，但引用了条款原文） |
| 阿里巴巴普惠体 3.0（阿里巴巴） | 非 SPDX 标准许可证，自定义免费商用协议 | 不适用 | 中文字体资讯站二手转述，未找到官方一手许可证全文页面 | 低 |
| Inter（拉丁文补充候选，未采纳为主字体） | `OFL-1.1` | 未核实具体保留名 | GitHub 仓库 `rsms/inter`，License Squirrel 等二手汇总页面交叉核对 | 中 |
| Zpix（最像素，已否决） | 非开源许可，分层收费（个人/教育免费，商用单产品 RMB 7000 / USD 1000） | 不适用 | 官方仓库 `SolidZORO/zpix-pixel-font` README | 高 |
| Fusion Pixel Font（缝合怪像素字体，方向变更前的候选，已不采纳） | `OFL-1.1` | 直接读取 `LICENSE-OFL` 原文，**版权声明行未见到「with Reserved Font Name」这类具体保留名声明**（与思源黑体的写法不同），可能意味着该项目未在版权声明里正式保留字体名——这一发现与本项目原本的假设（比照多数 OFL 字体的常见惯例）有出入，供后续若重新考虑像素字体路线时参考 | `LICENSE-OFL` 原文（`TakWolf/fusion-pixel-font` 仓库） | 高（一手文件原文，但需注意"未见到声明"不等于"确认没有"，可能是该项目采用了非标准位置声明 RFN，本条不作为决定性结论） |
| Ark Pixel Font（方向变更前的候选，已不采纳） | `OFL-1.1` | 未逐一核实 | `TakWolf/ark-pixel-font` 仓库页面 | 中（README 摘要，未直接读 LICENSE 原文） |
| Cubic 11 / 俐方體 11 號（方向变更前的候选，已不采纳） | `OFL-1.1` | 有，保留名为「Cubic」「俐方体」 | `ACh-K/Cubic-11` 仓库页面 | 中（README 摘要，未直接读 LICENSE 原文） |

## 图标字体（功能性 UI 图标）

只针对功能性 UI 图标（设置、关闭、音量一类），不适用于游戏内容图标（物品/技能/状态效果必须手绘像素图）。判定标准：优先无署名义务的宽松许可（MIT/ISC/Apache-2.0），CC BY 4.0 一类视为协议风险，非必要不采纳；必须有官方字体文件形态（`.ttf`/`.woff`）。

| 候选 | 许可证（SPDX，一手来源核实） | 官方字体文件 | 核实来源 |
|---|---|---|---|
| **Tabler Icons** — 推荐 | `MIT` | 是（`packages/icons-webfont`） | `LICENSE` 原文 + `packages/` 目录列表，`github.com/tabler/tabler-icons` |
| Bootstrap Icons | `MIT` | 是（`font/fonts/*.woff*`，GitHub API 实测字节数） | `LICENSE` 原文 + 目录字节数，`github.com/twbs/icons` |
| Remix Icon | 非标准 SPDX，「Remix Icon License v1.0」（2026-01 换版，issue #1069 一手确认），署名可选非强制 | 是（`fonts/*.ttf`/`*.woff*`） | `License` 原文 + `fonts/` 目录列表，`github.com/Remix-Design/RemixIcon` |
| Phosphor Icons | `MIT` | 是（官方 `@phosphor-icons/web`） | `phosphor-icons/homepage` 仓库 `LICENSE` 原文；字体文件见 `phosphor-icons/web` 仓库 |
| Material Symbols | `Apache-2.0` | 是（官方可变字体，`variablefont/*.ttf`/`*.woff2`，GitHub API 实测字节数） | `google/material-design-icons` 仓库 `LICENSE` 原文 + 目录字节数 |
| Lucide | `ISC`（主体）+ `MIT`（约 140 个 Feather 派生图标） | **否**，仅 SVG/框架组件，无官方字体文件（`packages/` 逐一核实） | `LICENSE` 原文，`github.com/lucide-icons/lucide` |
| Feather Icons | `MIT` | **否**，仅 SVG，无官方字体文件（仓库根目录逐一核实） | `LICENSE` 原文，`github.com/feathericons/feather` |
| Font Awesome Free（未在候选清单内，一并核实） | **分层**：代码 MIT；SVG/JS 图标 CC BY 4.0；**字体文件（.ttf/.woff）走 SIL OFL 1.1，不是 CC BY 4.0** | 是 | `LICENSE.txt` 原文，`github.com/FortAwesome/Font-Awesome` |

**结论**：推荐 **Tabler Icons**（`MIT`，6,184 个图标，官方维护 webfont 包，最近发布 2026-07-28，本文档要求的十二类功能图标逐一核实全部存在）。**Font Awesome Free 需要单独澄清一点**：其图标整体常被认为是 CC BY 4.0，但那只适用于 SVG/JS 形态；字体文件形态实际走 SIL OFL 1.1，若只用字体文件形态、不碰 SVG/JS 资产，许可证本身并不构成排除理由——本文档仍不推荐它，但理由是图标规模与字体文件按风格拆分带来的整合成本，不是许可证。

Lucide 与 Feather Icons 均无官方字体文件形态，若日后风格评估更偏好它们，需要额外走「自建 SVG→字体转换管线」这条路，本次调研没有评估这条路径的成本。

## 「操作系统字体」这一类，评估过但不采纳

项目所有者最初提到希望用「操作系统那种字体」的观感，核实后确认这指的是视觉风格，不是真的要打包系统字体本身——两款代表性的系统中文黑体都不满足分发条件：

| 字体 | 许可证结论 | 核实来源 | 核实强度 |
|---|---|---|---|
| 微软雅黑（Microsoft YaHei） | 版权归北大方正，微软的授权范围限于「Windows 系统内嵌使用」，不含向第三方软件/游戏分发；随游戏分发需要另向 Monotype 购买扩展授权，即付费，撞上「不接受付费」的硬标准 | Microsoft 官方 Q&A 论坛帖子与中文行业资讯交叉印证 | 中（社区问答交叉印证，非官方一手 EULA 原文） |
| 苹方 PingFang | Apple 字体 EULA 限定使用范围在「运行 Apple 软件的兼容设备上显示/打印内容」，明确禁止提取、修改、再分发字体文件 | Apple 开发者论坛与用户社区讨论帖交叉印证 | 中（未核实到 PingFang 具体这一款字体的内嵌权限位标记） |

结论：两者均不采纳，游戏必须打包自带思源黑体，不依赖运行环境已安装的系统字体（缺字体的机器上会显示缺字方块，这条不需要额外核实）。详细推理见 [文本与字体渲染管线](../pipelines/text-and-font-rendering.md) 第 4.3 节。

## 结论

**不需要修改 `deny.toml` 白名单**——推荐的思源黑体走 `OFL-1.1`，已在 P0 扫描时预留（虽然当时的注释写的是"为将来的 Fusion Pixel Font 等预留"，这处注释现在**已经过时**：预留的许可证类别没变，但具体会用到它的字体从 Fusion Pixel Font 换成了思源黑体。`knowledge/licenses/2026-08-16-p0-scan.md` 是一份带日期的历史扫描记录，本文件不去改它，只在此处指出这处过时的地方，供后续有人整理 `deny.toml` 注释时参考）。文本栈四个 crate 也都落在 MIT/Apache-2.0/Zlib 范围内，同样不需要改白名单。

**HarmonyOS Sans 与阿里巴巴普惠体即便宣传"免费商用"，也不满足本项目的许可证判定标准**——不是标准 SPDX 许可证，且都明确禁止对字体文件做修改（含子集化），与本项目的资产打包管线（[文本与字体渲染管线](../pipelines/text-and-font-rendering.md) 第 5 节）直接冲突，两者均不采纳。

## 重要提醒（沿用 P0 扫描的提醒，本轮调研不改变这一现实）

仓库目前**没有配置 git 远程**，`cargo-deny` 相关的 CI job **不会在任何地方执行**。真正把这些依赖加进 `Cargo.toml`/资产目录时，**必须手工实跑一次 `cargo deny check`**，不能假设本文件的手工核对结果等价于一次自动扫描通过。
