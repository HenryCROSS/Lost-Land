# 资产管理系统：Steel API 与全局资产 mod

**冻结于** 2026-08-20。核对提交 `main` 分支 HEAD `2d2dbcf`。**落地状态**：纯设计——本文档不重新设计资产 VFS 本身（它已经落地，见下），只补两处此前遗留的角落：① Steel 侧的资产注册 API（现状只有 JSON 清单一条路）；② 核实"全局资产做成一个 mod"这条路径是否真的零成本、还差什么。

## 已核实的现状（重要：与 `mod-package-structure.md` 冻结时不同）

`mod-package-structure.md`（冻结于 2026-08-20 早些时候，核对提交 `5769bae`）写作时，资产 VFS 是"规格已规划、`crates/ll-mod/src/` 无 `vfs.rs` 或等价实现"的纯设计。**这个现状已经变了，但改动还没有合入 `main`**：

- 分支 `wt-walkart`（提交 `f474e5c`，1196 测试全绿；对应工作树 `.claude/worktrees/manual-walkart`）已经落地资产 VFS：`crates/ll-mod/src/asset_vfs.rs`（解析 `assets/sprites/manifest.json`，`overrides/<目标命名空间>/sprites/` 同路径覆盖，按 `topo_sort` 确定性总序生效，冲突产出 `LoadStatus::Warning`）与 `crates/ll-render/src/atlas_pack.rs`（运行期货架式装箱）。`crates/ll-game/src/content.rs::load_content` 已经把两者接进真实装载流程：先跑 `pipeline::load_all`（脚本），再跑 `asset_vfs::build`（JSON 清单），两条冲突记录追加进同一份 `LoadReport`。`main` 分支（HEAD `2d2dbcf`）尚不包含这批代码——本文档核实代码现状时读的是 `wt-walkart`，不是 `main`；本文档冻结之后，等 `wt-walkart` 合入 `main`，下面的行号/路径引用应视为对该分支内容的引用。
- 已落地的形状比 `mod-package-structure.md` 四节最初设想的更简单：**没有 `.atlas.json` 图集清单**，精灵只声明 `name`/`file`/`pivot`/`footprint` 四要素，矩形怎么摆放完全交给运行期打包器决定；`tools/ll-artgen` 的主职因此从"生成图集"变成"生成松散贴图树 + 清单"。本文档尊重这个已经落地的更简单形状，不重新引入 `.atlas.json`。
- `ModManifest`/`topo::topo_sort`/`version_constraint`（依赖声明、`"0.3"` 精确匹配、`">=0.4"` 下限约束，命名空间去重/成环/缺失依赖/版本不满足四类校验，均导致"整批中止"）已落地——本文档二节（全局资产 mod）直接复用，不重新设计。
- `pipeline::load_all` 已核实：`entry_points` 为空是合法状态，直接判定 `LoadStatus::Loaded`（`crates/ll-mod/src/pipeline.rs:147`：「纯数据 mod（清单允许没有脚本入口，见 manifest.rs 文档），没有脚本可跑，直接算加载成功」）。
- 七个既有 `register-*` 脚本 API（`class`/`clip`/`quest`/`race`/`skill`/`subclass`/`terrain`，均在 `crates/ll-mod/src/script_*_api.rs`）都遵循同一个模式：`thread_local!` 存一个「当前调用窗口的活跃表」，`register_fn` 注册进 `ScriptEngine`，函数签名是一串简单类型（`String`/`i64`），返回 `Result<bool, String>`。**没有任何一个校验 `id` 参数的命名空间必须等于当前加载 mod 自己的命名空间**——这是既有代码的真实现状，本文档一节会指出为什么 `register-sprite` 不能照抄这一点。

---

## 一、Steel API 与配置文件并存

### 形状：`register-sprite`，照抄既有七个 `register-*` 的模式

```scheme
(register-sprite id file pivot-x pivot-y footprint-width footprint-height)
```

- `id`：完整命名空间标识符字符串（如 `"yourmod:lava_floor"`），与其余七个 `register-*` 一致——**但必须校验其命名空间等于当前正在加载的 mod 自己的命名空间，不满足则报错**，理由见下「为什么脚本这条路要比 JSON 更严格」。
- `file`：mod 自己 `assets/sprites/` 目录下的相对路径，复用 [`validate_relative_asset_path`]（`crates/ll-mod/src/asset_vfs.rs`，已是 `pub fn`）同一套路径校验——脚本这条路不能绕开路径穿越防线,不能自己另起一套校验。
- `pivot-x`/`pivot-y`/`footprint-width`/`footprint-height`：与 JSON 清单 `pivot`/`footprint` 两个字段语义完全相同，四个整数。
- 返回 `Result<bool, String>`，与其余七个一致。

### 两条路的关系：写进同一张表，不是两套并行清单

JSON 清单与脚本注册的产出，最终都要落进 `asset_vfs::build` 产出的同一份 `AssetVfs.sprites`（按 `ResolvedSprite::id` 排序、供 `atlas_pack::pack_atlas` 消费）——**不是"脚本资产"和"清单资产"分别打包成两张图集**，那样会让"mod 能不能覆盖另一个 mod 的脚本声明资产"这类问题在两套系统里各答一遍,制造不必要的复杂度。

**这要求一处具体的接线调整**（本文档只指出接线点，不代为实现）：目前 `content.rs::load_content` 是先跑 `load_all`（脚本执行）、再跑 `asset_vfs::build`（JSON 解析），两者互不知道对方。要让 `register-sprite` 在脚本执行期间就能查出"这个 id 是不是清单已经声明过"，**JSON 清单里"本 mod 自己声明的精灵"这一步（`register_own_sprites` 对应的逻辑）需要挪到脚本执行之前**，把解析结果通过一个类似 `set_active_target(RaceTable)` 的 thread-local 传给脚本引擎，`register-sprite` 才能在调用当下就检测到重复并返回 `Err`。这不是一次架构改动——`register_own_sprites` 本身已经是一个不依赖任何脚本求值的纯函数（只读文件），挪到 `load_all` 之前跑一次完全可行，只是调用顺序要换。

### 同一个资产 id 两边都声明了，谁赢？——报错，不是静默择优

**结论：`register-sprite` 若发现要注册的 `id` 已经被同一个 mod 的 JSON 清单声明过，返回 `Err`，与其余七个 `register-*` 遇到重复内容 id 时的既有纪律一致（`RaceTable::define`/`TerrainTable::define` 均拒绝重复定义，见 `pipeline.rs::reload_mod` 模块注释）。** 该 mod 的这条脚本入口执行失败，整个 mod 的装载状态变成 `Failed`——不是悄悄跳过一份声明。

**这与"两个 mod 覆盖同一份资产"是不同性质的问题，不能套用同一条规则**：

- **跨 mod 覆盖**（四节已有设计）：两个独立作者对同一份资产有正当的竞争关系（谁的换肤 mod 该生效），`topo_sort` 决出确定顺序，产出 `LoadStatus::Warning` 但不阻止装载——game 仍然要能跑起来，这是"必须可见，但不该拦下"的场景。
- **同一个 mod 内 JSON 与脚本都声明了同一个 id**：这不是两方竞争，是**同一个作者对同一个东西写了两次声明**——大概率是重构后忘记删旧声明，或者手滑复制粘贴。这类错误应该在装载期就响亮地报出来，逼作者修，而不是靠一条容易被忽略的 `Warning` 静默吞掉。`asset_vfs.rs` 现有的「同一份 JSON 清单内部两条同名条目→跳过后出现的一份、记警告」是**清单自身语法错误的容错**（"打包失败必须优雅"，针对不可信输入的单点降级），跟"作者在两个不同的声明通道里各写了一遍"不是同一件事，不该混用同一条纪律。

### 两条路的能力是否等价

**基本等价**：都能表达 `name`/`file`/`pivot`/`footprint` 四要素，JSON 能声明的东西脚本一定能声明（脚本只是把四个字段换成函数参数）。但脚本能做两件 JSON 天然做不到的事：

1. **条件/计算式生成**——例如按某个开关生成 N 个变体精灵（`for` 循环里调用 N 次 `register-sprite`），或者把 `footprint` 算成另一个已注册内容的函数。JSON 是纯数据文件，没有循环/条件这类表达力。
2. **与同一个入口脚本里的其它 `register-*` 调用协同**——例如注册一种新地形的同时紧跟着注册它的贴图，两件事写在同一处、同一次审阅就能看全，不需要在 `.scm` 和 `manifest.json` 两个文件之间来回切换核对。

反过来，JSON 有脚本代替不了的优势：**不需要写一行 Steel 代码，`tools/ll-artgen` 能直接产出/校验它**——这不是能力问题，是门槛问题，见下「为什么保留两条」。

### 命名空间必须匹配当前 mod——为什么脚本这条路要比 JSON、也比既有 `register-*` 更严格

已核实：JSON 路径的命名空间归属是**结构性**保证的——`register_own_sprites` 从"这份清单在哪个 mod 的目录里"直接决定 `owner_namespace`，清单文件本身写的是裸名字（不含命名空间），**没有办法从清单内容伪造一个别的命名空间**。而脚本路径若照抄其余七个 `register-*` 现有的宽松行为（`id` 参数不做命名空间归属校验，任何 mod 的脚本理论上都能传一个别的命名空间字符串），`register-sprite` 就会打开一个 JSON 路径原本不存在的口子：**一个 mod 的脚本可以直接 `(register-sprite "lostland:sprites/terrain/lava" ...)`，静默顶替本体资产，且不会走 `overrides/` 目录、不会产生 `AssetVfsBuildResult.conflicts` 里那条 `Warning`**——因为现有冲突检测只扫描 `overrides/` 目录下的真实文件，根本不知道脚本调用过什么参数。这是一条本设计必须堵上的具体可见性缺口，即便其余七个 `register-*` 尚未处理同类问题（它们注册的是"内容"不是"资产"，不像资产覆盖那样已经有一整套"必须可见"的既有纪律要维护一致）。

**结论：`register-sprite` 的 `id` 参数必须解析出命名空间，且强制等于当前加载 mod 自己的命名空间，不等则拒绝**——mod 想换别的命名空间的资产，仍然只能走 `assets/overrides/<目标命名空间>/sprites/` 这一条已有、可见、纳入拓扑序决胜的路径，不能用脚本抄近路。

### 为什么保留两条，不砍掉一条

**被否决：只保留脚本，砍掉 JSON 清单。** 否决理由：

- 破坏"资产 VFS 不需要求值任何脚本就能回答'这个 mod 有没有某资产'"这条既有设计意图（`mod-package-structure.md` 一节原文：「VFS 要能在不求值任何脚本、不解析清单里某个可选字段的前提下，回答……」）——如果精灵只能靠脚本声明，查一个 mod 有什么资产就必须真的跑一遍 Steel VM，装载管理界面的资产预览、`tools/ll-artgen` 之类的离线工具都做不到。
- `tools/ll-artgen` 现在的产出物就是松散贴图树 + JSON 清单——没有 JSON 路径，这个已经存在的工具链会失去落点，得再造一个"JSON→Steel 代码生成器"这类多余的中间层。
- 绝大多数精灵是"美术画完图、量出锚点和占地格数，照实填四个字段"，没有任何条件/计算需求——强迫这类最常见的场景写 Steel 代码，是拿脚本的灵活性去换美术流程的门槛，得不偿失。

**被否决：只保留 JSON 清单，砍掉脚本。** 否决理由：

- 清单是纯数据，天生表达不了"依条件生成""与同一处的其它内容注册协同"这类需求。这类需求目前不算迫切（`mods/example_mod` 没有这种用例），但保留这条路的边际成本很低——`register-sprite` 只是又一个 `register-*` 函数，复用完全相同的模式，没有引入新机制；一旦真的需要，没有这条路是"能不能做到"的差别，不是"效率高低"的差别，不对称，不该现在砍掉。
- 现有七个 `register-*` 已经确立"内容注册走脚本"是这个项目的通用范式（`class`/`clip`/`quest`/`race`/`skill`/`subclass`/`terrain` 全部可以，也应该可以，走脚本）。资产作为第八类被声明的东西，若唯独它没有脚本入口，会是一处说不清道理的不对称。

---

## 二、全局资产 = 一个 mod

项目所有者自己给出的解法（"或者做一个 mod 当全局资产使用？"）依赖两个前提，逐一核实：

### 核实 1：只有资产、没有脚本的 mod 现在能装载吗——能

`crates/ll-mod/src/pipeline.rs:147`（`load_all`）与 `:206`（`reload_mod`）均已核实：`manifest.entry_points.is_empty()` 时直接判定 `LoadStatus::Loaded`，不报错。`manifest.rs` 测试 `assert!(manifest.dependencies.is_empty() && manifest.entry_points.is_empty())` 进一步确认清单解析层面也接受空 `entry_points`。一个只有 `mod.toml` + `assets/sprites/` 的目录，是一个完全合法、能正常装载成功的 mod。

### 核实 2：mod A 能不能引用 mod B 的资产——能，且这是默认行为，不是特别打通的特性

`asset_vfs::build` 按拓扑序遍历**全部**已发现的 mod（不只是当前正在处理的那一个），把每个 mod 自己声明的精灵累加进**同一份** `sprites: Vec<ResolvedSprite>`，寻址方式统一用完整 `namespace:path` 字符串（非本体命名空间的精灵，`atlas_name` 恒定是这个完整字符串，见 `asset_vfs.rs` 模块文档「为什么本体资产用裸名字，mod 资产用完整命名空间字符串」）。`atlas_pack::pack_atlas` 随后把这份**已经合并了全部已装载 mod** 的列表整体打成**一张共享图集**（模块文档原文：「本模块因此不区分'本体贴图'与'mod 贴图'……本体只是这批来源里 `namespace == "lostland"` 的那一部分」）。**换句话说，"某个内容能不能引用另一个命名空间下的贴图"这件事在图集这一层从一开始就没有边界**——任何一个已装载 mod 的精灵，天生就和其它所有已装载 mod 的精灵活在同一张纹理、同一套查找键空间里。

**端到端的证据**：`crates/ll-game/src/layout.rs::terrain_atlas_key` 已经示范了这条路径真实可用——地形种类若不在本体写死的静态映射表里（说明它是 mod 注册的自定义地形），回退到 `Registry::resolve(kind.index())` 拿回完整命名空间 ID 字符串，直接当图集查找键用。测试 `mod注册的地形回退到registry查出完整命名空间字符串` 验证了 `examplemod:lava_floor` 这个字符串确实能从 `Registry` 反查出来，且与 `asset_vfs` 的命名约定完全对齐。这证明：一个内容类型（这里是地形）注册在某个命名空间下，它关联的贴图**不要求**来自同一个命名空间——完全可以来自另一个 mod（包括"全局资产 mod"）。

**但有一处需要如实标注的缺口，不是"全局资产 mod"独有，是既有引用规则遗留的通用问题**：`mod-package-structure.md` 四节已经写下规则——「这类引用只应该出现在声明了对应依赖的 mod 里……具体校验时机留给实现任务」，**这条校验至今没有实现**，也已核实确实找不到任何代码在做"检查某个字符串参数是否落在声明的依赖命名空间内"这类事——事实上这也很难通用地做：一个 `register-*` 函数的某个 `String` 参数"是不是"一次资产/内容引用，引擎本身并不知道，需要每个消费该字符串的 API 各自决定要不要做这层校验。**后果是**：mod A 若引用了"全局资产 mod"里的一张图，却没有在 `[dependencies]` 里声明依赖它，现在的失败语义是——若全局资产 mod 缺失，装载期不会报错，只会在渲染时悄悄查不到图集条目，走既有"查不到条目就跳过绘制"的降级路径，玩家看到的是一格没有贴图，而不是一条清楚的"缺 mod"提示。这个问题在两个普通 mod 互相引用时同样存在，全局资产场景只是让后果更容易被撞见（资产可能是内容的必要部分，不只是换皮）。

### 结论：两条都通，"全局资产做成 mod"是零成本路径

白拿的机制，逐项列清楚：

| 机制 | 来源 | 全局资产 mod 怎么用 |
|---|---|---|
| 命名空间隔离 | `NamespacedId` | 全局资产 mod 有自己的命名空间（如 `common_assets`），与其它内容/资产不会撞名 |
| 版本号 | `ModManifest.version` | 全局资产 mod 也能声明版本，消费方可以用 `[dependencies]` 约束"至少要这个版本" |
| 依赖声明与版本约束 | `[dependencies]` + `version_constraint.rs` | 消费方 mod 声明依赖全局资产 mod（写法与依赖任何其它 mod 完全一样） |
| 拓扑顺序 | `topo::topo_sort` | 决定全局资产 mod 与其它 mod 在覆盖冲突场景下谁先谁后生效，不需要专门规则 |
| 装载失败语义 | `LoadStatus::{Loaded,Warning,Failed}` | 一个只带资产、没有脚本的 mod，装载失败只可能因为清单本身解析错误或声明的依赖不满足，走的是完全既有的三态 |
| 存档-mod 集合硬门禁 | 内容哈希 + 生成期/当前 mod 集合双记录（`identity-and-ids.md`「存档与 mod 集合」） | 全局资产 mod 即使自己不注册任何 `ContentIndex`，仍然通过版本号/是否存在参与"这份存档记得的 mod 集合，和现在装的是不是同一回事"这一判断，不需要特殊对待 |

**还差什么**（两条，均已在上面核实小节论证过，这里只汇总）：

1. **依赖声明是否被真正引用，装载期不校验**——通用缺口，不是全局资产 mod 独有，全局资产场景只是让后果更严重。
2. **目前只有地形这一种内容类型真正走通了"引用别的命名空间资产"这条路径**（`terrain_atlas_key` 的 `Registry::resolve` 回退）。其它未来内容类型（技能图标、物品图标等）想引用全局资产 mod 里的图，需要各自照抄这个回退模式接线——这不是全局资产 mod 本身缺了什么，是这些内容类型自己大多还没实现（物品系统仍是纯设计），消费端还没走到需要这条路的那一步。

---

## 三、还需不需要"非 mod"的全局位置——不需要

**结论：不需要，理由是"少一个机制比多一个好"，且已核实 mod 目录本身已经满足两类候选需求。**

### 核实：mod 是不是已经满足"玩家自己丢进去的贴图包"这个需求

一个玩家想换皮，需要的东西——命名空间、覆盖目标路径、装载器怎么发现它——与"一个 mod 作者写一个 reskin mod"完全是同一件事：把资产放进 `assets/overrides/<目标命名空间>/sprites/` 下（四节已有设计），配一份最小的 `mod.toml`。**结构上没有第二种"玩家贴图包"，它就是一个 mod。**

### 核实：mod 是不是已经满足"跨存档共享"这个需求

mod 的装载发生在**进程层面**，不是按存档 scope 的——一份存档只是**记录**"生成这份存档时用过哪个 mod 集合"（`identity-and-ids.md`「存档与 mod 集合」），不是把 mod 内容拷贝进存档。任何装在 `mods/` 目录下的东西，天然对当前进程打开的**全部**存档可见。"跨存档共享"不是一个还没被满足的需求，是 mod 目录这个既有机制的默认行为。

### 唯一真实的顾虑：门槛，不是能力——用工具解决，不是新机制

期望一个普通玩家手写合法的 `mod.toml`（`namespace`/`version` 字段、TOML 语法）确实比"把图片拖进一个文件夹"门槛更高。**但这是 UX/工具问题，不是缺一层机制的证据**：`tools/ll-artgen` 已经承担"生成松散贴图树 + 清单"这个职责，顺手再生成一份最小 `mod.toml`（或者加载管理界面提供一个"导入贴图包"向导，自动填好这几行）就能把门槛压到跟"拖进一个文件夹"几乎一样低——不需要为此在 `ll-mod` 里另开一条平行的"非 mod 资产加载"代码路径。

### 被否决的方案

**加一个 `<游戏目录>/global_assets/` 之类的固定目录，不需要 `mod.toml`，直接扫描 `assets/` 结构。** 否决理由：这会制造出两套"资产从哪来"的解析逻辑——mod 的 `asset_vfs` 一套，这个新目录再一套。两者迟早要在覆盖规则、冲突警告、确定性总序上各自实现一遍，或者被迫共享代码却又不共享"命名空间"这个前提（这个新目录没有 namespace，"覆盖谁""被谁覆盖""拓扑序放在哪一步"都需要另外发明规则）。这完全是在重复"全局资产 mod"已经免费解决的问题，只是换了个不需要写 `mod.toml` 的说法——不值得为省几行 TOML 文本引入第二套机制。

---

## 四、资产种类的边界——现在只覆盖精灵，不纳入音频/字体/数据表

逐项核实：

### 音频——零实现，谈"怎么声明"为时过早

`crates/` 下没有任何 `ll-audio` 或音频相关 crate；`asset_vfs.rs` 只解析 `sprites/manifest.json` 与 `overrides/<ns>/sprites/`；`mod-package-structure.md` 一节的目录布局草图里画过 `audio/<相对路径>.ogg`，但从未落地为代码。**没有播放管线，就没有任何东西会消费"音频资产声明"这份数据**——声明了也没人读。现在设计音频资产的字段形状，是在给一个还不存在的消费方猜它需要什么，猜错的成本比现在不猜、等播放管线立项时一起设计更高。

### 字体——已有独立且完整的既定方案，不属于本设计范围

`crates/ll-text/src/fonts.rs` 已核实：思源黑体（正文/标题）与 Tabler Icons（功能性图标）都用 `include_bytes!` 在**编译期**烧进二进制，走 `knowledge/pipelines/text-and-font-rendering.md` 定的独立管线——不经过 mod 的 `assets/` 目录，不参与资产 VFS 的覆盖/冲突逻辑，甚至不是运行期资源。字体要不要开放给 mod 替换，是这份既有管线文档自己的决定范围，本文档不越界重复设计。

### 纯数据表——核实：没有找到直接的既有裁定，但已有机制足够覆盖这个需求

没有找到一条专门讨论"纯数据表资产"的既有 ADR/设计文档——这条结论是本文档核实后自己给出的，不是复述已有裁定。若"纯数据表"指的是"mod 想声明一批结构化数值内容（不是精灵、不是脚本控制流）"，这个需求已经被 `register-*` 系列 + [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)/[0017](../decisions/0017-tiered-declarations-materialize-columnar.md) 的分档物化系统整个覆盖——mod 作者表达"这是一张地形表/技能表"，用的正是一批 `register-terrain`/`register-skill` 调用（一档，声明式，注册期物化进 Rust 列式表）。再开一条"松散数据表文件资产"的第三条路径，会与"脚本/清单注册"变成两条表达同一件事（结构化内容声明）的重叠机制——这正是应该避免的重复，YAGNI。

### 将来加音频，这套设计要不要推倒重来——不需要

本设计（以及它承接的、已经落地的资产 VFS 实现）在"资产种类"这个维度上其实早就是可以参数化的形状，只是当前只填了精灵一种：

- **目录约定不是本文档新增的假设**——`mod-package-structure.md` 一节的目录布局图从一开始就把 `audio/` 和 `sprites/` 画在同一张图里，同一套「固定目录名、覆盖走 `overrides/<目标命名空间>/<种类>/`」的组织约定。
- **覆盖解析/冲突可见/路径穿越校验都是通用骨架，不含任何"这是精灵"的假设**——`topo_sort` 确定性总序、`LoadStatus::Warning`、`Component` 逐段路径校验，这三样在 `asset_vfs.rs` 里都不依赖精灵这个具体资产种类。把 `SPRITES_DIR`/`SpritePivot`/`SpriteFootprint` 换成音频专属的目录名与字段（音量、循环点之类），就是同一套骨架的又一份实例，不是重新设计。
- **Steel 侧模式同理**——`register-sprite` 是"第八个 `register-*`"，将来 `register-audio-clip` 会是"第九个"，复用完全相同的 `thread_local!` 活跃表 + `register_fn` + `Result<bool, String>` 惯例，不需要发明新的脚本 API 风格。

**唯一需要动代码、不是动设计的地方**：`asset_vfs::build`/`register_own_sprites`/`apply_overrides` 目前是精灵专属的具体实现（硬编码 `SPRITES_DIR` 常量与 `ResolvedSprite` 结构体）。真加音频时，需要把这三个函数按"资产种类"参数化一次，或者干脆平行复制一份 `AudioVfs`——这是一次局部重构（改函数签名/拆结构体），不是推倒重来：`AssetVfs` 这个总容器概念、"mod 目录固定子目录"这条组织约定、拓扑序决胜的覆盖规则，三者都不需要变。

---

## 相关文档

- [mod 包结构与资产 VFS](mod-package-structure.md) —— 目录布局、清单字段、入口点分类、资产 VFS 的引用/覆盖约定与依赖版本约束的原始设计；本文档在其已经落地的实现之上补 Steel API 与全局资产两处角落
- [身份与 ID 空间](identity-and-ids.md) —— 存档与 mod 集合的内容哈希、生成期/当前 mod 集合双记录，本文档二节"存档 mod 硬门禁"直接复用
- [0016 — mod 性能分档按声明方式](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017 — 分档声明物化为列式存储](../decisions/0017-tiered-declarations-materialize-columnar.md) —— `register-sprite` 作为一档声明式注册的既有分级依据，本文档四节"纯数据表"结论的依据
- [0018 — 引擎层与玩法层的脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) —— 资产声明属于玩法层可注册内容的一种，判据与既有七个 `register-*` 一致
- [0019 — 被禁能力须给替代品或理由](../decisions/0019-denied-capability-needs-substitute-or-justification.md) —— J 组"文件系统访问→资产 VFS 是替代品"的原始裁定，本文档核实到该替代品已经落地（`wt-walkart` 分支）
