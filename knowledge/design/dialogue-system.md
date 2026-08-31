# NPC 对话系统

**冻结于** 2026-08-30。核对提交 `5ea4281`（`main` 分支 HEAD，工作树 `wt-dialogue`）。

**落地状态**：**纯设计，零代码。** 全仓库没有任何 `Intent::Talk`/`Interact`、没有对话相关的
`Effect`、没有对话内容 schema、没有 `mods/*/dialogues.json5`、没有任何类型。
`grep -ri "dialogue\|对话"` 在 `crates/` 下的全部命中都是注释里的**预告**，逐条如下——
这几条注释同时也是本文档的需求来源，不要当成已有实现：

- `crates/ll-game/src/world.rs:653`、`crates/ll-world/src/entity/affiliation.rs:45`——同一条所有者
  裁定的引文：「玩家可以没有势力归属，这个可以通过后面和据点的管理者对话加入。」两处都紧挨着
  `affiliations: Vec::new()` 这行硬编码。
- `crates/ll-sim/src/effect.rs:622`——`Effect::TransferOwnership` 自陈「没有对话系统（赠送需要一个
  『NPC 决定要不要给你』的交互载体）」。
- `crates/ll-game/tests/door_interaction.rs:5-8`——门进交互列表那一批如实写下「本批次只做开关门，
  NPC 对话是一个全仓库零实现的系统」。
- `crates/ll-world/src/entity/agent.rs:192`——`race` 字段文档把「剧情/势力对话分支的解锁条件」
  列进未来消费者。

**已核实的现状**（本文档全部结论建立在这些之上，读的时候请以代码为准，不要以本节的转述为准）：

| 事实 | 位置 |
|---|---|
| `Agent` **没有 `name` 字段**；姓名是 `naming.rs` 的纯函数，生产路径零调用 | `crates/ll-world/src/entity/agent.rs:98` 起；`crates/ll-world/src/naming.rs:69/78/84` |
| `Agent` **也没有「我属于哪座据点」字段**——`NpcProfile.home` 在物化时被丢掉 | `crates/ll-mod/src/roster.rs:600`（`NpcProfile.home: WorldId`）vs `roster.rs:869` 起的 `build_npc_agent` |
| `Agent.affiliations` 唯一生产者是 NPC 物化时挂的**一条文化归属**；`AffiliationKind::Faction` 零生产者 | `crates/ll-mod/src/roster.rs:900`；`crates/ll-world/src/entity/affiliation.rs:35-49` |
| `OrgInstance` 类型在，**全仓库零构造点**（仅定义、re-export、两处 `#[cfg(test)]`）；`WorldState` 里没有 org 表 | `crates/ll-world/src/entity/org.rs:21` |
| `SettlementSite.id` 是现成的 `WorldId`，由编年史计数器分配，与势力/家族共用同一号段 | `crates/ll-world/src/settlement.rs:206` |
| 「据点管理者」是名册序号 0 的固定角色，每座还有人住的据点恰好一位 | `crates/ll-mod/src/roster.rs:157`（`STEWARD_INDEX`）、`:298`（`SettlementRoles::steward`） |
| `QuestNodeDef` 只有三个字段：`id`/`prerequisites`/`condition`。**没有奖励、没有发布者、没有文案键** | `crates/ll-mod/src/quest.rs:124-139` |
| 任务进度是 `Agent.mod_state` 里一条 `Int(1)`，经 `Effect::SetModState` 落地；**没有「接取」这个状态** | `crates/ll-sim/src/quest.rs:162`/`:181`；`crates/ll-sim/src/effect.rs:243` |
| `Effect::TransferOwnership` 已落地（`apply` 侧接线在 `apply.rs:434`），**`resolve` 侧零产出点、无对应 `Intent`** | `crates/ll-sim/src/effect.rs:650` |
| `interact_entries` 只扫 `world.ground_items` + 地形门，**从不看 `world.actors`** | `crates/ll-game/src/player_action.rs:463-489` |
| `InteractTarget` 四变体，`Door` 是「目标不是一件物品」的先例，`item_def()` 返回 `Option` | `crates/ll-game/src/player_action.rs:293-366` |
| i18n 只有一条通道：`Catalog::resolve` / `resolve_with_args`，**带具名参数的插值今天就能用** | `crates/ll-i18n/src/lib.rs:159`/`:167`；真实调用点 `crates/ll-game/src/menu_screen.rs:343` |
| **mod 的 `.ftl` 装载零实现**：唯一装载点只读本体一个目录；`mods/*/` 下连 `locales/` 目录都没有 | `crates/ll-game/src/lib.rs:477`（`Catalog::load_dir(&paths.locales_root)`）；`ls mods/*/` |
| 硬编码用户可见字符串门禁**只扫 `crates/*/src/**/*.rs` 里的 CJK 字面量**，且是 warn 模式 | `scripts/ci/check_i18n_strings.py` |
| `UiMode` 栈已落地（`Menu`/`TextEntry` 两变体），`InputContext` 三变体 | `crates/ll-ui/src/widget/ui_mode.rs:75`；`crates/ll-platform/src/keybind.rs:132` |
| 内容数据文件走一张引擎侧固定表；新增一类内容要同时动内容哈希的分类枚举 | `crates/ll-mod/src/content_data.rs:177`（`CONTENT_FILES`）；`crates/ll-mod/src/content_hash.rs:816`（`ContentTableKind`） |

**本文档不管什么**：对话框的**布局、字号、行数、翻页、光标行为**——那是 `ll-ui` 的职责，
与本文档并行的 `wt-uxdesign` 批次正在写 `knowledge/design/ui-and-navigation.md`。本文档只在
三节末尾给出**对布局的约束**（文本长度、参数插值、不得截断），不越界画界面。

---

## 零、所有者裁定原文与本文档的任务

> 「我希望交互也能包括和 NPC 对话」
>
> （问到范围时，从甲/乙/丙三档里选了**丙**）：能说话 + 选项分支 + **能触发实际后果**
> （加入势力、接任务、交易）
>
> 「玩家可以没有势力归属，这个可以通过后面和据点的管理者对话加入。」
>
> 「你参考一般的游戏看看怎么处理，但是注意 i18n 的问题。」

丙档的全部价值在「实际后果」那三条上。**而那三条恰好各自压在一个今天并不完整的系统上**——
势力播种归 P9、任务奖励发放未落地、交易一行代码都没有。本文档的主要工作不是画一棵对话树，
是把这三条后果**逐条落到今天真实存在的挂载点上**，并对够不着的部分如实标注，而不是画一张
「等 P9 就好了」的饼（ADR 0021）。

---

## 一、形状裁定：扁平节点表 + 显式跳转，不是嵌套树

### 1.1 一般游戏怎么做，三种做法各自的代价

| 做法 | 代表 | 用在这里的代价 |
|---|---|---|
| **嵌套对话树**（选项里直接嵌下一层节点） | 大量 RPG 的编辑器导出格式 | JSON5 里手写会缩进爆炸；**表达不了回环**（「还有别的事吗」回到上一层是刚需，一棵树里只能靠复制整棵子树表达）；跨文件复用一句通用台词做不到 |
| **完整状态机**（节点 + 带守卫的转移表，转移是一等公民） | 部分叙事中间件 | 转移表要独立编号、独立校验、独立本地化，作者要同时维护节点表与转移表两份真相源；本项目今天没有任何需求需要「一个转移被多个节点共享并单独命名」 |
| **扁平节点表 + 选项里写 `next: 节点 id`** | 传统 roguelike 与多数纯数据驱动的对话 | 需要一次跨表引用校验（`next` 指向的节点必须已定义）——而这套校验本仓库已经有了 |

**裁定：第三种。** 理由不是「看起来更简单」，是三条能验证的性质：

1. **与仓库既有内容类型同构**。`SkillDef.prerequisites`、`QuestNodeDef.prerequisites` 都是
   「扁平表 + 一条 `Vec<ContentIndex>` 的边」，跨表引用的 intern/查表纪律
   （`crates/ll-mod/src/content_schema.rs:46-58`：`Raw*` 里跨表引用一律是 `String`，
   intern 在 `apply_*`）**一个字不用改就能复用**。嵌套树在这条纪律下是异类——它的子节点没有
   `id`，也就没有 `ContentIndex`，进不了注册表，也进不了内容哈希。
2. **回环是真实需求，不是臆想**。「加入据点 → 好的 → 还有别的事吗 → 回到开场白」这条路径在
   丙档里必然出现（三条后果要挂在同一次会话的不同分支上）。树表达不了它，只能复制子树，而复制的
   两份迟早漂移——这正是仓库反复登记的那一类缺陷（`ContentIndex` 裸数值当判据、
   `atlas_coverage.rs` 手写地形清单、气候批次的索引平移）：**真相源之外的副本，分叉时没有任何
   东西会报错。**
3. **能被内容作者手写**。顶层是两个数组，每个元素是一个平的对象，最深一层嵌套是「选项」这一级。
   与 `mods/lostland/quests.json5` 的观感一致。

### 1.2 因此对话图**允许有环**，不能复用 `prereq_graph` 的无环校验

`SkillTable`/`QuestTable` 都在注册期跑 `validate_no_cycles`（`crates/ll-mod/src/prereq_graph.rs`），
因为那两张图表达的是「前置解锁」，环意味着「谁都学不到」。**对话图的环是合法的、且是设计意图**。

那么什么保证对话不会死循环？**每一次跳转都必须由玩家按一次键**。引擎侧不提供「条件满足就自动
推进到下一个节点」这种转移——一个节点显示出来之后，唯一的推进方式是玩家从选项里选一条。
这条规则同时把「终止性」从一个需要静态分析的性质降级成一个**结构上不可能违反**的性质，
代价是不能写「自动播放的过场」。今天没有任何需求要过场，按 YAGNI 不做。

**注册期仍然要校验的两条**（跟 quest 的 `UnregisteredPrerequisite` 同一形状）：

- 每个 `next` 指向的节点 id 必须已定义（**不是**只 intern）——用 `required_id` 那条路径。
- 每个 `DialogueDef.root` 指向的节点必须已定义。

**不校验**「每个节点都从某个根可达」：一份 mod 完全可能只提供一批被别的 mod 引用的通用节点，
判它「孤儿」会把一条正确的设计判成错误——与 `QuestCondition::KillCount.target_kind`
走 `UntypedIdSpace` 豁免是同一类判断（`crates/ll-mod/src/content_audit.rs`）。

### 1.3 谁说这句话：按 `Agent.profession` 匹配，不给 `Agent` 加对话字段

给每个 `Agent` 存一个 `dialogue: Option<ContentIndex>` 是最直觉的做法，**否决**：`Agent` 的两条
生产路径（`build_player_agent`/`build_npc_agent`）今天都不知道该往里填什么，填不了就是又一个
「声明了但没接线」的死字段，而 `scripts/ci/check_field_consumers.py` 是**阻断模式**门禁。

**改成：`DialogueDef` 自己声明它认谁。**

```
speaker: { profession: "lostland:steward" }     // 只按职业匹配
speaker: { profession: "lostland:guard", culture: "lostland:mining_hold" }  // 再收窄一档
```

两个字段今天都真的存在于每一个物化 NPC 身上：`Agent.profession` 由 `NpcProfile.profession` 赋值
（`roster.rs`），`Agent.affiliations` 里那条 `AffiliationKind::Culture` 由据点文化赋值。
**这是本文档唯一一处不需要任何新字段就能跑起来的接线。**

多条 `DialogueDef` 同时匹配时的裁决顺序，必须是确定的（约束 C5）：

1. 声明了 `culture` 的胜过只声明 `profession` 的（更具体优先）；
2. 仍然平局时，按 `ContentIndex` 升序取最小者。

`ContentIndex` 依赖 mod 装载顺序，所以第 2 条的意思是「同一套内容集内恒定」，**不是**「跨 mod 集
稳定」——与地图归并平局破法的已知代价（前一份交接第五节第 12 条）同一形状，且比它轻：那里影响
的是格子颜色，这里影响的是「装了两个都想给铁匠写台词的 mod 时，哪一个赢」。**这一条要写进
mod 文档**，不能让作者自己去猜。

---

## 二、数据放哪、长什么样

### 2.1 落点

`mods/<id>/dialogues.json5`，一个新的 `ContentFileKind`，进 `crates/ll-mod/src/content_data.rs`
的 `CONTENT_FILES`（今天 22 项）。**本体自己也走这一条路**——`mods/lostland/dialogues.json5`——
这就是规格 §10.3 / ADR 0018 的「本体即 Mod」检验：本体的声明与第三方 mod 的声明除了 `id` 里的
命名空间字符串之外没有任何结构性差异。

装载顺序上它**必须排在 `Classes`、`Cultures`、`Quests`、`Items` 之后**：`speaker` 与选项的
后果都只 `get` 不 `intern`，判据统一是 `CONTENT_FILES` 那张表自己的注释里写的那一条
（「只 get 的那一方必须排在被引用者之后」）。

### 2.2 两张表，一个文件

先例是 `crafting.json5`（配方类别 + 配方两张名册在同一个文件里）。

```json5
// mods/lostland/dialogues.json5
{
  // 会话入口：谁会说话、从哪个节点开始。
  dialogues: [
    {
      id: "lostland:steward_greeting",
      speaker: { profession: "lostland:steward" },
      root: "lostland:steward_root",
    },
  ],

  // 节点：一句话 + 若干选项。
  nodes: [
    {
      id: "lostland:steward_root",
      // 只有键，永远没有字面文案。见三节。
      text_key: "lostland:dialogue.steward.root",
      options: [
        {
          text_key: "lostland:dialogue.steward.ask_join",
          // 条件全部满足才显示这一行；空数组 = 无条件显示。
          conditions: [
            { kind: "not-affiliated", affiliation: "faction" },
          ],
          // 选中之后发生什么，见五节。空数组 = 只是句话。
          outcomes: [
            { kind: "join-settlement" },
          ],
          next: "lostland:steward_joined",
        },
        {
          text_key: "lostland:dialogue.common.farewell",
          conditions: [],
          outcomes: [],
          next: "end",      // 保留字：结束会话
        },
      ],
    },
    {
      id: "lostland:steward_joined",
      text_key: "lostland:dialogue.steward.joined",
      options: [
        {
          text_key: "lostland:dialogue.common.back",
          conditions: [],
          outcomes: [],
          next: "lostland:steward_root",   // ← 回环，合法
        },
      ],
    },
  ],
}
```

schema 侧照抄既有形状：`DialogueFile { dialogues: Vec<RawDialogue>, nodes: Vec<RawDialogueNode> }`，
全部 `Raw*` 带 `#[serde(deny_unknown_fields)]`，跨表引用在 `Raw*` 里一律是 `String`，
转换函数是 `pub fn apply_dialogues(registry, table, ..) -> Applied`。
`conditions`/`outcomes` 的元素用 `{ kind: "…", … }` 带标签的对象——与
`RawQuestCondition`（`content_schema.rs:737`）、`RawSkillEffect` 同一个写法，**不用 serde 的
`untagged`**：本 crate 里唯一一处 untagged 是 `content_expr::RawExpr`，它成立是因为三个变体的
JSON 表示天然不重叠，条件与后果不满足这个前提。

### 2.3 内容哈希与门禁的代价，如实登记

新增一类内容不是免费的。落地时必须同批做完下面每一条，少一条就是一处静默漂移：

- `content_hash.rs:816` 的 `ContentTableKind` 加变体（判别值往后接，**不挪既有值**，
  `:1780` 已写明「靠递增表达」）；
- `ContentValueTables`、`classify_index`、一个新的 `write_dialogue_fields`；
- `content_audit.rs` 里对称的那份字段清单；
- 递增 `CONTENT_HASH_ALGORITHM_VERSION`（当前值 `grep -n "pub const CONTENT_HASH_ALGORITHM_VERSION" crates/ll-mod/src/content_hash.rs` 自取，**本文档不列**）。

**不会动世界摘要与回放摘要**：对话内容不参与世界生成，也不参与任何 NPC 的既有决策。
落地时若发现两条黄金基准变了，说明有别的东西被顺手改了，应当当场停下来查，而不是重冻。

---

## 三、i18n（所有者特别点名的那一节）

对话是全项目**文本量最大**的系统。它一旦按错误的形状落地，错误会以内容的规模被复制。

### 3.1 边界：结构在 JSON5，文案在 `.ftl`，一个字面字符串都不进 JSON5

**裁定：`dialogues.json5` 里不允许出现任何用户可见的字面文本，只允许 `*_key`。**

三条理由，第一条是决定性的：

1. **门禁看不见 JSON5。** `scripts/ci/check_i18n_strings.py` 扫的是
   `crates/*/src/**/*.rs` 里含 CJK 的字符串字面量。`mods/**/*.json5` **完全不在它的视野里**，
   而且它今天还是 warn 模式。也就是说：如果允许内联文案，那么「代码中不得出现任何硬编码的
   用户可见字符串」（规格 §11.3）这条硬规则，在**项目里文本量最大的那个系统上等于不存在**。
   这不是「先这样、以后再收」——文案一旦以内容的规模写进 JSON5，回头再抽出来是一次全量迁移。
2. **一份结构、多份语言。** 文案进 JSON5 就意味着每种语言各要一份完整的对话结构文件，
   两份结构迟早不同步（少一个选项、`next` 指向不同的节点），而**结构不同步是不会被翻译流程
   发现的**——翻译者看的是文本，不是图。键在结构里、文本在 `.ftl` 里，结构只有一份，
   语言差异被压在 Fluent 那一层，天然不可能漂移。
3. **与既有全部内容类型一致。** `RawRace`/`RawClass`/`RawItem`/`CultureAttrs` 无一例外用
   `display_name_key: String` → `parse_id(.., "本地化键标识符")` → `NamespacedId`。
   注意 `parse_id` 与 `intern_id` 的区别：**本地化键只解析成 `NamespacedId`，不进注册表**，
   因此它不是 `ContentIndex`、不占内容索引号、也不参与 `ContentIndex` 的分配顺序。
   对话的 `text_key` 照办。

**建议同批新增一条门禁**：扫 `mods/**/*.json5`，命中含 CJK 的字符串字面量就报错。今天不存在
这条检查，而对话是第一个让它变得必要的系统。

### 3.2 致命缺口：mod 的 `.ftl` 装载零实现——正面处理，不绕过

> **【2026-08-30 已落地：批次 0，工作树 `wt-modftl`，计划文档
> `docs/superpowers/plans/2026-08-30-batch17-mod-localization.md`】**
> 本节以下正文原样保留（纪律：推翻要留来由，不删原文），但**两个缺口
> 都已经补上**，正文里的「现状」段落自此是历史记述，不再是现状。
>
> - `Catalog` 的桶从「语言标签」改成「**命名空间 → 语言标签**」两级；
>   `split_key`（原 `to_fluent_id`）用命名空间前缀**选桶**而不是丢弃它，
>   裸键落到 `base_namespace`，行为不变。
> - `Catalog::load_dir` **删除**——它没有命名空间入参，正是「本体特权
>   路径」的实现载体。新入口是 `Catalog::load(base_namespace, &[LocaleSource])`。
> - 新模块 `ll_mod::locale_vfs::discover_locale_dirs` 遍历
>   `mods/*/locales/`；`ll_game::locale_sources` 把**本体那一条与每个 mod
>   那一条放进同一个同构列表**，`Catalog` 无从分辨哪一条是本体。
> - `mods/example_mod/locales/{zh-CN,en}.ftl` 落地，端到端断言在
>   `crates/ll-game/tests/mod_locales.rs`。
>
> **本节没有预料到、落地时另行裁定的一条**：**语言回退**。一个 mod 只
> 提供 zh-CN 而玩家用 en 时，本节原文说「沿用现有降级……查不到就回退到
> 键名」——那意味着整屏 `mymod:item.foo.display_name`，是玩家可见的乱码。
> 落地时改成「请求语言 → `en` → 该命名空间其余语言（字典序）→ 键名」，
> 回退**不跨命名空间**，并同批新增 `Catalog::try_resolve`（精确、不回退）
> 给覆盖率门禁用——否则回退链会把已经在生效的断言弄哑。理由与代价写在
> 计划文档二节 2.4。
>
> **仍未做、不属于本批**：`mods/**/*.json5` 的 CJK 字面量门禁、
> `text_key` 的多语言覆盖率门禁（本节 3.1/3.5 建议的两条）、本地化覆盖
> 机制（mod 改写别人的译文）。

**现状精确到行**：

- 全仓库唯一的本地化装载点是 `crates/ll-game/src/lib.rs:477` 的
  `Catalog::load_dir(&paths.locales_root)`，而 `locales_root = assets_root.join("locales")`
  （`lib.rs:105`/`:75`）。**只有本体一个目录，没有任何代码遍历 `mods/*/locales/`。**
- `Catalog` 内部是 `HashMap<语言标签, FluentBundle>`（`ll-i18n/src/lib.rs:67`）——
  **一种语言一个扁平 bundle，没有命名空间这一维**。
- `to_fluent_id`（`lib.rs:225-233`）在查表前把命名空间前缀**整个剥掉**：
  `mymod:greet` 与 `lostland:greet` 剥完都是 `greet`。
- `mods/lostland/` 与 `mods/example_mod/` 下**连 `locales/` 目录都没有**——连本体自己都没走
  `mod-package-structure.md`「本地化文件」一节定的那条约定，它的 `.ftl` 在 `assets/locales/`。
  也就是说「本体即 Mod」在本地化这一项上**今天已经不成立**，只是此前没有任何东西撞到它。

于是第三方 mod 的对话今天有两个各自独立的致命问题，**修一个不修另一个没有用**：

| | 问题 | 后果 |
|---|---|---|
| ① | 它的 `.ftl` 根本没有被读过 | 每一句台词都退化成显示键名（`ll-i18n` 的「回退到键名 + warn」路径），并且每帧刷 warn |
| ② | 就算读了，剥掉命名空间之后键会跨 mod 撞 | 两个 mod 各写一条 `greeting`，后装载的那个静默覆盖前一个。**没有任何东西会报错** |

**裁定：mod 的 `.ftl` 装载是对话系统的批次 0 硬前置，不是「顺带做」。**

理由：此前每一次撞到这个缺口，都还有绕路可走——尸体命名那一批就绕过去了，
「每族一条键」改成「一条带 `$species` 参数的通用消息」（`assets/locales/zh-CN.ftl:90-94`），
用一条键覆盖了任意多个第三方种族。**对话没有这条绕路**：一个 mod 的台词内容量本来就是任意的，
不可能被压缩成本体预先写好的若干条参数化消息。「第三方 mod 能写自己的对话」与
「mod 的 `.ftl` 能被装载」是同一件事的两种说法。

**最小落地形状**（属于批次 0，不属于对话系统本身）：

- `Catalog` 的键从 `语言标签` 变成 `(命名空间, 语言标签)`；
- `resolve`/`resolve_with_args` 按 key 的命名空间前缀分流到对应的 bundle，**不再剥掉前缀之后
  丢进同一张表**；裸键（无冒号）保持现有行为，落到本体命名空间；
- 装载端遍历已装载的每个 mod 的 `<mod 目录>/locales/*.ftl`；
- **本体的 `assets/locales/` 注册成命名空间 `lostland` 的 locales**——这一步把上面那条
  「本体即 Mod 在本地化上不成立」一并补上，而不是给本体留一条特权路径；
- 缺键、缺语言、某个 mod 的 `.ftl` 语法坏掉，全部**沿用现有降级**（跳过该文件 + `warn`，
  查不到就回退到键名），**不新增任何降级机制**。

**被否决的替代方案：不改 `Catalog`，要求 mod 作者在自己的 `.ftl` 里把命名空间写进键名**
（`mymod-greet = …`）。否决理由有两条，第二条更硬：
① `mod-package-structure.md`「本地化文件：为什么键不需要再编码命名空间」一整节已经论证过，
「这个键属于哪个命名空间」由文件所在目录回答，让键再带一遍就是同一件事的两处真相源；
② 它挡不住问题 ①——mod 的 `.ftl` 还是没人读。前缀方案只解决撞键，不解决装载，而装载才是
那两条里真正无解的那一条。

### 3.3 参数化文本：能力今天就有，`narrative-system.md` 那句「必须等 `format-text`」已经过期

`knowledge/design/narrative-system.md:185` 写着「对话变量插值依赖 ADR 0019 B-2
（`format-text` 具名参数格式化），当前状态是待办……在此之前，剧本对话只能使用不含变量的静态文案」。

**这句话在今天不成立，本文档在此更正**：ADR 0019 B-2 讲的是**脚本侧**的 `format-text` API，
它随脚本系统整体拆除而消失（ADR 0028 的订正段 + ADR 0018 的订正段）。而 Rust 侧的等价能力
**早就落地并且有真实调用方**：

- `Catalog::resolve_with_args(language, key, Option<&FluentArgs>)`（`ll-i18n/src/lib.rs:167`），
  `FluentArgs` 已经从 `ll-i18n` 重新导出（`lib.rs:58`，导出理由写在那儿：此前它「公开但调不动」）；
- 真实调用点四处：`ll-game/src/menu_screen.rs:343`、`save_list.rs:109`、`settings_view.rs:75`、
  `ll-ui/src/hud/character_panel.rs:316`；
- `.ftl` 里已经在用具名参数与复数选择器：`{ $species }`（`zh-CN.ftl:94`）、
  `{ $subject }`/`{ $amount }`（`:249` 起）、`{ $sources -> …}` 选择器（`:244`）。

**结论：对话文本从第一天起就可以写 `{ $npc_name } 点了点头`，不需要等任何东西。**
「不得用字符串拼接表达变量插值」那条约束原样有效——ADR 0019 B-2 论证语序的那一段
（日语「{name}を倒した！」）与用什么语言实现无关。

### 3.4 但 NPC 今天没有名字——三条路与裁定

`{ $npc_name }` 要有值可填，而：

- `Agent` **没有 `name` 字段**；
- `naming.rs` 的 `given_name`/`surname`/`full_name` 是纯函数、已落地、有测试，
  **生产路径零调用**，唯一消费者是 `ll-sim/examples/p3_acceptance` 这个 demo；
- `CultureAttrs` 的六个字段里**没有 `naming`**（`crates/ll-world/src/culture.rs:98-207`），
  也就是说「按出生地文化选一份 `NamingRules`」这一步今天无表可查。

三条路：

| 路 | 做法 | 判断 |
|---|---|---|
| A | 给 `Agent` 加 `name: String` | **否决**。`naming-and-localization.md` 把命名列为 ADR 0009「默认派生，只存偏差」**最极端的一例**——它连偏移都不允许，因为「同一个 NPC 永远同名」是设计要求本身。存一份字符串等于把纯函数的结果落盘，还要为它写迁移、进哈希、进 remap，换来零新增能力。**改名**（`naming-and-localization.md` 四节）将来要存的也只是被改过的那一小撮「覆盖名」，不是所有人 |
| B | 第一批对话不引用 NPC 名字，用**职业显示名**代替（「守卫」「管理者」——`ClassAttrs.display_name_key` 今天就有，是本体十三条职业各自的键） | **采纳为第一批**。零前置，一行新代码都不用写在命名这一侧 |
| C | 给 `CultureAttrs` 补 `naming`，渲染期按 `(种子, EntityId, 出生地文化)` 现算 | **采纳为独立前置批次**，不与对话第一批捆绑 |

C 落地之后 B 自然升级成 `{ $npc_name }`，对话内容**一个字不用改**——因为参数插值是本地化那一层
的事，`.ftl` 里把 `{ $speaker }` 从「职业名」换成「人名」不触碰任何 JSON5。这正是三节 3.1
那条边界白送的好处。

**顺带说明音素表的 i18n 约束不归本文档管**：`naming-and-localization.md` 三节已经定死
「各语言版本表长必须相同、同一下标必须是同一个音」，并建议做成 CI 门禁。对话只是那条规则的
消费者，不重新论证。

### 3.5 中英文长度差：只给约束，不设计布局

一句结论：**同一条消息的中文与英文长度差异是双向的、且量级不小**，不能靠「按最长的那个留余量」
拍脑袋。可以直接观察 `assets/locales/` 两份文件——同一批键，`zh-CN.ftl` 485 行 / `en.ftl` 436 行，
逐条比较即可拿到真实分布，**不要用行数当结论**（注释行数不同），要逐键量。

属于本文档的三条约束（交给 `wt-uxdesign` 当输入，本文档不画布局）：

1. **对话框必须能容纳多行并且能翻页**，不能假设一句台词一行装得下。选项行同理。
2. **绝不允许按字符数截断**，也不允许在代码里拼接省略号——截断点在 CJK 与拉丁文之间语义完全
   不同，且会切断 Fluent 插值出来的参数。要省略必须由排版层（`cosmic-text`，规格 §11.3）
   在**字形层面**做，不是在字符串层面。
3. **内容作者不得被要求「数字数」**。一条台词的长度约束若只写在文档里，它就等于不存在——
   若真的需要长度上界，那必须是一条能自动检查的门禁，而不是一句劝告。

**建议同批新增的第二条门禁**：对话用到的每一个 `text_key`，在**每一种已装载语言**下都必须
存在。今天完全没有这类覆盖率检查，而对话是第一个让「某个语言少了三百条键」变成现实风险的系统。

---

## 四、选项条件：能力边界画在哪

### 4.1 边界：一张**封闭的谓词清单**，数组即合取，没有别的组合子

条件是一个数组，数组里每一项是一个 `{ kind: "…", … }` 对象，**全部满足**才显示这个选项。

第一批的封闭清单（每一条都只做「查一个今天真的存在的字段 + 一次比较」）：

| `kind` | 参数 | 读什么（全部已落地） |
|---|---|---|
| `affiliated` / `not-affiliated` | `affiliation`（五类之一）、可选 `org` | `Agent.affiliations` |
| `standing-at-least` | `affiliation`、`org`、`value` | `Affiliation.standing`（千分比） |
| `quest-completed` / `quest-not-completed` | `quest` | `ll_sim::quest::is_quest_completed` |
| `flag-set` / `flag-not-set` | `flag`（一个 `NamespacedId`） | `Agent.mod_state`（与任务进度同一套存储） |
| `has-item` | `item`、`count` | `Agent.inventory` |
| `wallet-at-least` | `value` | `Agent.wallet` |
| `is-race` | `race` | `Agent.race` |

**这七类就是全部**。没有 `or`、没有 `not`（否定由成对的 `kind` 表达，不是一个可嵌套的算子）、
没有嵌套、没有算术、没有变量、没有比较两个动态值。

### 4.2 为什么不做表达式——两个先例，一正一反

**反面先例：`content_expr.rs` 已经在仓库里，它证明了「能做」，也划出了做到什么程度才安全。**
`crates/ll-mod/src/content_expr.rs` 用嵌套 JSON5 数组表达算术表达式（伤害公式、经验曲线），
它之所以没有变成一门语言，靠的是四条同时成立的纪律：**封闭符号表**（不认识的符号一律报错）、
**装载期编译成扁平指令数组**（不是运行期解释 AST）、**指令数上限**（`MAX_FORMULA_INSTRUCTIONS`）、
**两个编译器逐条对齐、刻意不合并**（模块文档点名 ADR 0021：合并需要一层只为对称而存在的抽象）。

对话条件**不应当复用 `RawExpr`**，理由正是 ADR 0021 的判据本身：与伤害公式共享的只有**语法**
（嵌套数组），不是**算法**——伤害公式把固定的几个数值输入折成一个整数，对话条件是对世界状态的
一组**查询**。为语法相似而共用一个编译器，就要往一个今天只认识 `attack-power`/`str-mod` 的
封闭符号表里塞进 `affiliations`/`mod_state`/`inventory`，并且因为 `RawExpr` 的形状**天然支持
任意嵌套**，`["and", ["or", …], ["not", …]]` 会在第一个内容作者想要它的那天被加进来。
一旦有了布尔嵌套，紧接着就会要「比较两个动态值」，然后是算术，然后是变量——那时它已经是一个
解释器，而「不要在玩法层放解释器」正是拆掉脚本层那次裁定的全部内容。

**正面先例：`QuestCondition` 只做一档与三档，明确不做二档「受限公式」**
（`class-skill-quest-system.md` 四节，理由是「当前已知需求下没有中间层用例」）。对话条件同理，
但有一处必须说清楚的差别：**任务系统的「三档脚本回调」在今天是一个死变体**——
`mods/lostland/quests.json5` 的文件头自己写着「求值它指向的回调是尚未落地的能力……
目前只是一个携带命名空间 id 的数据标签」，而脚本系统已经整体拆除。所以对话条件**没有三档可退**，
上面那张清单就是全部能力，不是「一档，复杂的走三档」。

### 4.3 边界外的需求怎么办

三条今天就能想到会撞上的：

| 需求 | 落点 |
|---|---|
| 「白天才有这个选项」 | 加一条谓词 `time-of-day`。成本是一个 match 分支 + 一条 schema + 一条哈希字段——**清单是可增长的**，被禁止的是「组合子」，不是「谓词种类」 |
| 「NPC 心情好才肯给你」 | 今天没有心情这个量（`Traits` 纯设计、关系派生基线未落地）。**不为它预留任何东西**，等关系系统真的有生产者时再加对应谓词 |
| 「A 或 B 满足其一」 | **不加 `or`**。写成两个选项，各带自己那一条条件，指向同一个 `next` 节点。这在对话里本来就是更好的写法——两条路的措辞通常本来就该不同 |

**一条硬规则**：新增谓词必须**同批带一条真实内容用例**（本体或 `example_mod` 里真的有一句台词
在用它）。没有用例的谓词不加——这与 ADR 0021「不要为将来可能的对称性建抽象」、以及仓库对
「声明了但没接线」的长期记账是同一条纪律。

---

## 五、三条后果的接线（丙档的全部价值在这一节）

### 5.0 后果在数据里是**声明**，把声明变成 `Effect` 的是 Rust

与 `SkillEffect`（`crates/ll-sim/src/skill.rs:147`）完全同构：数据里是一个带 `kind` 的封闭枚举，
`resolve` 侧的 Rust 把它翻译成一串真正的 `Effect`。这正是 ADR 0018 订正段定下的分工——
**内容用 JSON5，行为逻辑在引擎里用 Rust**。mod 作者能声明「这个选项让玩家加入这座据点」，
不能提供一段自定义逻辑。

### 5.1 加入据点/势力——今天能加入的只有「据点」，不是「势力」

> **【2026-08-29 更新：势力播种已落地，本节的「变通」就此作废】**
> 本节以下正文原样保留（纪律：推翻要留来由，不删原文），但**第一批的
> 「加入」不再走那条变通**。
>
> **变了什么**：`OrgInstance` 现在有生产构造点了——
> `ll_world::faction::seed_factions` 把编年史的占领链折叠成
> `ll_world::faction::FactionTable`，它住在 `WorldState::factions`、
> **进存档**（所有者裁定「被占领后肯定会有变化的」）。每座活着的据点
> 恰好归一个势力，`FactionTable::faction_of(据点号)` 就是换算。
>
> **因此正确写法是**：
>
> ```rust
> Affiliation {
>     kind: AffiliationKind::Faction,
>     org: OrgRef::Instance(world.factions.faction_of(那座据点的 id)?),
>     standing: 初始值,   // 所有者裁定：加入据点给 +250，满值 1000
> }
> ```
>
> **下面三条「必须如实标注的事」各自的现状**：
>
> 1. 「把据点暂时塞进 `Faction` 这一类」——**不再需要**，指向的是真正的
>    `OrgInstance`。那笔「P9 那天变成惊喜」的迁移账**就此销账**，因为
>    还没有任何存档写过据点号形态的 `Faction` 归属（`build_player_agent`
>    今天仍写死 `Vec::new()`）。
> 2. 「`SettlementSite` 不进存档而 `Affiliation` 进存档」这条风险——
>    **正是靠「势力表进存档」解决的**。势力不再是编年史的派生物，
>    改变推演判据不会让老存档里的归属静默指向另一个势力；老存档由
>    `CURRENT_SCHEMA_VERSION` 的「版本不对就明确拒绝」兜底。本节九节
>    留给所有者的那道选择题因此关闭。
> 3. 「`standing` 没有任何常量」——**仍然成立，仍是本批之外的事**
>    （交接文档第〇之二第 5 条已裁定数值，落地属对话批次）。
>
> 「这位管理者管的是哪座据点」那一块（`Agent.home: Option<WorldId>`
> 从 `NpcProfile.home` 搬运）**同样仍然成立、仍未落地**——势力播种
> 只让势力**存在**，不做对话这一支。


**所有者裁定的落点**是 `Agent.affiliations`。现状：

- `AffiliationKind::Faction` 恒配 `OrgRef::Instance(WorldId)`（`affiliation.rs:84`）；
- 而 `OrgInstance` **全仓库零构造点**，`WorldState` 里没有 org 表，**势力播种已被所有者裁定归 P9**；
- 所以「加入一个势力」今天**没有任何东西可以加入**。

**变通（与本会话给物品归属用的是同一条）**：`SettlementSite::id` 是一个**现成的、已经在被分配
和被使用的 `WorldId`**（`settlement.rs:206`，由编年史计数器分配，与势力/家族共用同一号段）。
因此第一批的「加入」是：

```
Affiliation { kind: AffiliationKind::Faction, org: OrgRef::Instance(那座据点的 WorldId), standing: 初始值 }
```

**必须如实标注的三件事，不能含糊过去：**

1. **这是把「据点」暂时塞进 `Faction` 这一类**。据点不是势力——`society-and-affiliation.md`
   一节的定义里，`Faction` 提供的是「领土、法律、税收、兵役」。P9 势力播种落地之后，
   这条归属要么被重新指向真正的 `OrgInstance`，要么 `AffiliationKind` 需要第六个变体。
   **这条迁移账现在就要记下来**，否则它会以「玩家存档里有一堆指向据点 id 的 Faction 归属」的
   形态在 P9 那天变成惊喜。
2. **`SettlementSite` 不进存档，`Affiliation` 进存档。** 编年史是纯派生的，读档时用同一颗种子
   重跑推演，`WorldId` 因此逐位复现——今天这条成立。但 `remap_affiliations`
   （`crates/ll-content/src/remap.rs:818`）对 `OrgRef::Instance` **不做任何重映射**，
   理由是「`WorldId` 不依赖 mod 加载顺序」。这条理由对**势力**成立，对**据点 id** 只在
   「世界生成逐位不变」的前提下成立：**任何改变编年史推演的改动（地形、气候、文化、战争判据）
   都会让老存档里的这条归属静默指向另一座据点，而没有任何东西会报错**。这是一处真实的、
   已知的、必须写在这里的风险。缓解手段有两条（存档里额外记一份 `WorldId → 锚点坐标` 的
   校验、或者干脆把「玩家的归属」列进「世界生成改变即作废」的既有 mod 版本策略），
   **选哪条需要所有者裁定**，见九节。
3. **`standing` 没有任何常量、没有 clamp、没有校验**（`affiliation.rs:113-121` 只有一句
   「千分比，负值表示敌对」）。初始值取多少是一次内容数值决定，不是本文档能自己定的。

**还缺一块：这位管理者管的是哪座据点，`Agent` 今天答不出来。**
`NpcProfile.home: WorldId`（`roster.rs:600`）是这个问题的真相源，但 `build_npc_agent`
**没有把它搬进 `Agent`**。两条路：

- **按位置反查**：`world.terrain.chronicle_handle()` → `sites_touching_zone(npc 所在 zone)`
  （这条路径已经在生产代码里，见 `ll-game/src/world.rs:908-918`）。**否决**：NPC 会走动，
  离开自己那片 zone 之后这个反查要么给出错的据点、要么什么都给不出，而且**不会报错**。
  这正是「用真相源之外的东西当判据」那类缺陷的形状。
- **给 `Agent` 加 `home: Option<WorldId>`，物化时从 `NpcProfile.home` 搬运**。**采纳**。
  它不是新的真相源，是把既有真相源搬运过来；它有一个当天就成立的消费者（对话），
  因此过得了 `check_field_consumers.py`（阻断模式）；代价是一次存档主体形状变更，
  而 `scripts/ci/check_save_schema_version.py` 会强制 `CURRENT_SCHEMA_VERSION` 跟着升。
  **这条代价是实打实的**：存档主体走 postcard（non-self-describing），`#[serde(default)]`
  在那条路上是空操作，仓库已经为此犯过两次（`Agent::gender`、`GroundItemStack::placed`），
  那道门禁就是本会话为了不犯第三次才补上的（`3b92b08` 升版本、`a5d2f0e` 补门禁）。

### 5.2 接任务与发奖——缺的比想象的多

**`QuestNodeDef` 只有 `id`/`prerequisites`/`condition` 三个字段**（`quest.rs:124-139`）。
没有奖励、没有发布者、没有文案键。而任务进度只有一个状态：`Agent.mod_state` 里
`quest_progress:<id>` = `Int(1)`（`ll-sim/src/quest.rs:150-188`）。

**因此今天不存在「接取」这个概念**——任务从「不存在」直接跳到「已完成」，中间没有
「已接取、进行中」这一档。写清楚这一点很重要：对话里的「接任务」选项如果只是写一个
`accept` 标记，那个标记今天没有任何消费者会读它。

第一批能诚实交付的是这三件，**顺序不能颠倒**：

1. **`outcomes: [{ kind: "set-flag", flag: "…" }]`**——写一条 `Agent.mod_state`，
   经 `Effect::SetModState` 落地（这条路径完整落地且有验收 demo）。
   「已接取」因此是一个**对话系统自己的标志**，而不是任务系统的一个新状态。
   条件那一侧的 `flag-set` 谓词读它。**这一条第一批就能跑通，且不需要任务系统改一个字。**
2. **`outcomes: [{ kind: "complete-quest", quest: "…" }]`**——直接调既有的
   `mark_quest_completed`。这让 `QuestCondition::Script` 那个死变体第一次有了替代品：
   它的文档原话是「处理『**拜访某个 NPC 并说出特定台词**』这类无法穷举成数据的条件」
   （`quest.rs:114-115`）——**那正是对话**。三档脚本回调已随脚本系统消失，对话把这类条件
   变成了数据可表达的东西。
3. **发奖 = `outcomes: [{ kind: "give-item", item: "…", count: N }]`**——这是
   `Effect::TransferOwnership` **第一个真实调用方**。设计文档
   （`ownership-and-crime-detection.md` 四节）已经裁定赠送/购买/任务发奖**共用这一个机制**，
   本文档确认这条并原样接受它附带的硬前置：

   > 三种合法转移的 `resolve` 都**必须**校验「发起转移的一方确实是这堆物品当前的 `owner`」
   > （`Owner::Unowned` 除外）。不满足则不产出 `Effect::TransferOwnership`。
   > （`crates/ll-sim/src/effect.rs:629-641`）

   落到对话上就是：**NPC 给你东西之前，那东西必须真的在他背包里且归他所有**。
   一个 NPC 凭空变出奖励物品是**另一件事**（要先 `Effect::MergeIntoInventory` 到 NPC 身上，
   或者走一条明确的「无主物」路径），不能靠 `TransferOwnership` 假装。这条边界现在就写清楚，
   免得落地时被绕过。

**不做**：任务日志 UI、任务的 `display_name_key`、任务发布者字段。前两条属于任务系统自己的
下一批，第三条与 1.3 的「按职业匹配」重复——发布者是谁由「哪个对话挂在哪个职业上」回答。

### 5.3 交易——三块零件都在，缺的是把它们接起来

现状：`Agent.wallet: i64` 在（`agent.rs:122`，且 `Effect::AdjustWallet` 已落地
`effect.rs:204`）；物品有价格（`ItemAttrs::base_price`，`Milli` 定点整数，已进内容哈希与
内容审计，见 `content_hash.rs:1933`、`content_audit.rs:1818`）；`Effect::TransferOwnership` 在。
**缺的是一次结算把三者串起来。**

`agent-goals-and-economy.md` 三节的完整设计（行会中介、库存/需求/政策/商路四因子的本地价、
再乘一项买家归属系数）依赖行会、库存、商路——**全部属于 P9**。不要在这里实现它的任何一部分。

**最小可用形状**（明确标注为「玩家 ↔ 单个 NPC 的直接买卖」，不是经济系统）：

- 对话的一个选项声明 `outcomes: [{ kind: "open-trade" }]`，它**不产出任何 `Effect`**，
  只把 UI 推进交易界面——与 `InteractTarget::Facility` 落到「打开制作菜单」是同一形状
  （`player_action.rs:734` 起：那一支不产 `Intent`、不消耗回合）。
- 交易界面里每一次成交是一次 `Intent::Trade { partner, item, count, direction }`（**新变体**），
  `resolve` 产出三条 `Effect`：两条 `AdjustWallet`（买方减、卖方加）+ 一次库存搬运
  + 一次 `TransferOwnership`。
- **价格 = 物品基础价 × 买家归属系数**，两个因子今天都有：基础价在 `ItemDef` 里，
  归属系数读 `Affiliation.standing`（5.1 让玩家第一次真的有了一条 `standing`）。
  四因子的本地价那一层**留空**，等 P9。这不是简化版的经济系统，是**一个显式的占位公式**，
  要在代码里写明它将来会被 `agent-goals-and-economy.md` 三节的公式替换。
- **货币守恒**：NPC 的钱是有限的（`wallet` 今天恒为 `0`——`build_npc_agent` 写死
  `wallet: 0`）。**这意味着第一批 NPC 一分钱都拿不出来，玩家只能买不能卖。**
  给 NPC 一个初始钱包是一条独立的、必须同批做的事，否则「交易」这条后果落地即残废。

---

## 六、对话怎么进交互列表

交互列表的规则是所有者裁定过的，**不推翻**：按空格扫**脚下 + 相邻八格**（八向，与移动一致），
0 格有东西 → `Feedback::NothingNearby`；1 格 → 直接开那格的交互列表；2 格以上 → 先弹方向列表。
**交互列表无条件弹，一件也弹。**

**改动只有一处**：`interact_entries`（`player_action.rs:463`）今天只线性扫 `world.ground_items`
再单独 push 一行门，**从不看 `world.actors`**。给它补一段：这一格上站着的那个实体
（「每格至多站一人」是本会话刚升格的强制不变式），若不是玩家自己、且能匹配到一份
`DialogueDef`（1.3 节的匹配），就 push 一行 `InteractTarget::Talk { .. }`。

**门这一支是现成的先例**（`InteractTarget::Door`，`player_action.rs:316-339`）：它是第一个
「这一行指着的不是一件物品」的变体，为此把收敛方法从返回裸 `ContentIndex` 改成了
`item_def() -> Option<ContentIndex>`，让编译器逼每一处使用点表态。**对话这一行照走那条路**——
`Talk` 同样返回 `None`，`interact_row_text` 的名字与数量、按拾取键那条捷径在它身上自然什么都不做。

三条必须钉住的细节：

1. **顺序**（约束 C5）。`world.actors` 是 `Arena`；`interact_entries` 现有的顺序确定性论证
   （`:455-462`：`ground_items` 是 `Vec`，保序，全程线性扫描，不碰任何哈希容器）必须原样延续到
   新增的这一段。**这里的顺序是真陷阱不是形式要求**：玩家按的是「第几行」。
   实际上一格至多一人，所以最多只 push 一行——但取那一行的方式仍然不能依赖任何哈希容器的迭代顺序。
2. **`Talk` 这一行排在哪。** 建议排在**最前**：一格上同时有人和东西时，「跟他说话」几乎总是玩家
   的意图。这一条属于手感，若与 `wt-uxdesign` 的结论冲突，以那边为准。
3. **敌对目标不列对话行。** 判据用现成的 `ll_sim::ai_query::declared_hostile`
   （`ai_query.rs:323`），**不在输入层抄第二份判据**——那正是 ADR 0021 点名要拦的形状。
   注意它今天实际只有文化那一半在起作用（势力归属零生产者，第二项恒假）。

---

## 七、确定性（硬约束，写这一节之前请重读 `docs/architecture/03-invariants.md`）

### 7.1 一条分界：**会话内的位置是 UI 状态，会话产生的后果是世界状态**

这是本节唯一需要裁定的东西，其余都是它的推论。

- 「玩家现在停在哪个对话节点上」**不进 `WorldState`、不进存档、不进 `WorldState::hash`**。
  它与背包光标停在第几行、`UiMode` 栈现在有几层是同一类东西——
  `action-capability-and-input-context.md` 2.4 节已经裁定过这一类：
  「UI 导航状态不是世界状态，`resolve`/`apply` 从不读它」。代价是**中途存盘退出会丢失会话位置**，
  下次要重新开口说话；这与背包开着时退出的行为完全一致，不是新的不一致。
- 「玩家已经答应过他 / 已经加入了这座据点 / 已经拿到了那件东西」**必须进 `WorldState`**，
  走 `Intent → resolve → Effect → apply`。

### 7.2 C1：`apply` 是全局唯一写入口

每一次玩家选中一个**带 `outcomes` 的选项**，产出一个 `Intent::DialogueChoose { actor, dialogue,
node, option }`（新变体），由 `resolve` 把那一串 `outcomes` 声明翻译成 `Effect` 序列
（5.0 节的分工）。**纯导航的选项**（`outcomes` 为空，只是换个节点）不提交 `Intent`，
在 UI 层完成——它什么都没改变，提交一个恒产出空效果的 `Intent` 只会污染 `Intent` 日志。

**`resolve` 侧必须重新校验条件，不能相信 UI 传来的 `option` 序号。** UI 算出「这一行该显示」
用的是某一帧的世界快照；`resolve` 结算时世界可能已经变了（NPC 死了、物品被别人捡走了）。
条件判定的代码只写一份，UI 与 `resolve` 共用同一个函数——不各写一份，理由同六节第 3 条。

### 7.3 C3：随机必须走 `DetRng::for_entity`

对话里今天没有已知的随机需求（说服判定、随机台词都属于后续内容）。**若将来要加**，
必须走 `DetRng::for_entity(world_seed, entity_id, event_counter)`，并且**开一条独立的
stream id**——先例是 `ROSTER_GENDER_STREAM_ID`（`roster.rs`）与 `CHRONICLE_CULTURE_STREAM_ID`，
两处的文档都写明了理由：在别人那条流里插一次抽取，会把下游全部结果重抽一遍。

### 7.4 C2 / C5

- **C2**：对话不往时间轴里放任何东西。「对话消不消耗回合」若裁定为「消耗」，那也是
  `Effect::ScheduleNext` 一条既有效果，时间轴里仍然只有 `(Tick, EntityId)`。
- **C5**：三处必须用有序容器或显式排序——`DialogueDef` 的匹配裁决（1.3）、
  交互列表里 `Talk` 这一行的顺序（六节 1）、选项的显示顺序（`options` 是 JSON5 数组，
  `serde` 按书写顺序产出 `Vec`，天然保序，**不要**在中间塞进任何 `HashMap`）。

### 7.5 C4

不适用——对话是玩家在场时的前景交互，不涉及离屏世界的后台推进。

---

## 八、分批与优先级

> **【2026-08-31 落地回填：批次 1（对话内容表）已完成】**
> 计划文档 `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`，
> 工作树 `wt-dlgcontent`。落点：`crates/ll-sim/src/dialogue.rs`（十条谓词与求值）、
> `crates/ll-mod/src/dialogue.rs`（两张表、`validate_references`、`match_speaker`）、
> `crates/ll-mod/src/content_schema_dialogue.rs`（schema 与装载）、
> `mods/lostland/dialogues.json5`、`mods/example_mod/dialogues.json5`。
> `CONTENT_HASH_ALGORITHM_VERSION` 27 → 28；两条黄金基准实测未变。
>
> **本节以下正文原样保留**（纪律：推翻要留来由，不删原文），但落地时对本文档
> 有三处**偏离与补充**，逐条记在这里：
>
> 1. **本批的 schema 里没有 `outcomes` 字段。** 二节 2.2 的示例 JSON5 写了它。
>    批次 1 一条后果都不做，一个只允许空数组的字段就是一个「声明了但没接线」
>    的死字段；`deny_unknown_fields` 会让今天写 `outcomes:` 的内容当场报错。
>    批次 2 加它时加的是一个从第一天起就有真实消费者的字段。
> 2. **条件里的 `org` 参数今天只能指向内容空间的组织（实际上只有文化），且
>    `standing-at-least` 的 `org` 改成可选。** 四节 4.1 那张表把它写成必填。
>    `OrgRef::Instance(WorldId)` 是世界生成期分配的号，**内容文件里根本写不
>    出来**；不写 `org` 因此解释成「该类归属里任意一条」，`standing-at-least`
>    取该类归属 `standing` 的最大值。如实登记的缺口：今天写不出「加入了某一个
>    具体势力」这条条件。
> 3. **条件谓词与它的求值住在 `ll-sim` 而不是 `ll-mod`。** 本文档没有裁定落点。
>    判据是七节 7.2 自己那一条：「条件判定的代码只写一份，UI 与 `resolve` 共用
>    同一个函数」——`resolve` 在 `ll-sim`、UI 在 `ll-ui`/`ll-game`，唯一能被
>    两边共用的位置是 `ll-sim`。附带好处是它落在
>    `scripts/ci/check_field_consumers.py` 的决策层 glob 内。


每一批都能独立落地、独立验收。**顺序不能调换**，前置写在每一批第一行。

| 批 | 内容 | 前置 | 依赖 P9？ |
|---|---|---|---|
| **0** ✅ | **mod 的 `.ftl` 装载**（2026-08-30 已落地）：`Catalog` 加命名空间维度、遍历 `mods/*/locales/`、本体的 `assets/locales/` 注册成 `lostland` 命名空间 | 无 | 否 |
| **1** ✅ | 对话内容表（2026-08-31 已落地，计划文档 `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`）：`dialogues.json5`、两张表、进 `CONTENT_FILES` 与内容哈希、条件谓词七条、本体写一份 steward 对话、`example_mod` 写一份**带自己 `.ftl`** 的对话（这是批次 0 的验收标的） | 0 | 否 |
| **2** | 进交互列表 + 会话 UI + `Intent::DialogueChoose` + `outcomes` 里的 `set-flag`。**此时对话已经能说话、能分支、能记住玩家的选择，但还没有丙档的三条后果** | 1；UI 形状等 `wt-uxdesign` | 否 |
| **3** | **加入据点**：`Agent.home` 字段（含存档 schema 升版）、`join-settlement` 后果、`affiliated`/`standing-at-least` 两条谓词真的有东西可读 | 2 | **部分**——「加入据点」不依赖 P9；「加入**势力**」依赖 P9 的 `OrgInstance` 播种 |
| **4** | **任务**：`complete-quest` 后果、`give-item` 后果（`Effect::TransferOwnership` 的第一个调用方，含 owner 校验） | 2 | 否 |
| **5** | **交易**：NPC 初始钱包、`Intent::Trade`、占位价格公式（基础价 × 归属系数） | 3（归属系数要有 `standing` 可读） | **是**——真正的定价（库存/需求/政策/商路四因子、行会中介、商队）整体属 P9，本批只交付占位公式并在代码里写明它将来会被替换 |
| **6** | **NPC 姓名**：`CultureAttrs.naming`、渲染期现算、对话文案从「职业名」换成 `{ $npc_name }`（**只改 `.ftl`，不改任何 JSON5**） | 1 | 否 |

**两条门禁建议**（不属于任何一批的主线，但越早越省事）：`mods/**/*.json5` 的 CJK 字面量扫描
（三节 3.1）、`text_key` 的多语言覆盖率检查（三节 3.5）。

**明确不做**：对话编辑器工具、语音、多 NPC 同时在场的群体对话、对话里的随机说服判定、
NPC 主动向玩家搭话。前四条今天没有任何需求；最后一条要 NPC 的行为树能产出「走向玩家并开口」
这个动作，属于行为树自己的账。

---

## 九、需要所有者裁定的问题

**这一节是本文档最要紧的部分。** 下面每一条都是我拿不准、且拿不准会导致返工的。

### 会越拖越贵（拖到落地之后再改要动存档或内容）

1. **对话消不消耗回合？** 传统 roguelike 两种做法都有。若消耗，NPC 会在你翻选项的过程中
   继续行动（更真实，但玩家会在对话里被打）；若不消耗，对话就是一个「暂停」的窗口
   （更顺手，但一个卫兵可以被你无限次搭话来卡住他）。**这条影响 `Intent::DialogueChoose`
   要不要产出 `Effect::ScheduleNext`，落地之后再改会动回放摘要。**

2. **「加入据点」这条归属在 P9 势力播种落地时怎么迁移？**（5.1 的第 1 条）
   两个选项：(a) P9 那天写一次迁移，把指向据点 `WorldId` 的 `Faction` 归属重新指向真正的
   `OrgInstance`；(b) 现在就给 `AffiliationKind` 加第六个变体 `Settlement`，
   与 `Faction` 并存，永远不合并。我倾向 (b)——据点本来就不是势力，
   而 `society-and-affiliation.md` 一节对 `Faction` 的定义（领土、法律、税收、兵役）
   据点一条都不满足。但加一个 `AffiliationKind` 变体是一次内容语义决定，不该我定。

3. **老存档里指向据点 `WorldId` 的归属，在世界生成改变之后怎么办？**（5.1 的第 2 条）
   编年史不进存档、靠种子重跑派生，任何改动编年史的批次都会静默改变据点 `WorldId` 的分配。
   两条路：(a) 存档里额外存一份校验信息（例如那座据点的锚点坐标），对不上就丢弃这条归属并
   提示玩家；(b) 把「世界生成逻辑变更」纳入既有的 mod 版本作废策略
   （`save-and-mod-version-policy.md`：版本不对就打不开）。
   **(b) 更诚实但更粗暴**，需要裁定。

4. **`standing` 的初始值与量纲。** `Affiliation.standing` 今天只有一句「千分比，负值表示敌对」，
   **没有常量、没有 clamp、没有校验**。刚加入一个据点是多少？上限是多少？
   交易折扣按什么函数从它算出来？这几个数一旦写进内容就会有玩家存档依赖它们。

### 设计口径

5. **`AffiliationKind::Faction` 之外，第一批要不要支持「加入行会 / 宗教」？**
   两者与据点一样面临「没有实例可加入」的问题，但据点有 `SettlementSite::id` 这条变通，
   行会与宗教**连变通都没有**（`OrgInstance` 零构造点）。我的建议是第一批只做据点，
   但这意味着「和神殿祭司对话入教」这类内容在 P9 之前写不出来。

6. **NPC 的初始钱包给多少、从哪来？**（5.3 末尾）`build_npc_agent` 写死 `wallet: 0`，
   不给钱的话玩家只能买不能卖，交易这条后果落地即残废。而给钱又直接触及
   `agent-goals-and-economy.md` 四节的「货币守恒与回收汇」——凭空发钱是通胀源。
   最小方案是按职业给一个固定初始值并接受它是通胀源，代价记在 P9 的账上。

7. **对话里能不能出现玩家自定义的文本？** 文本输入通道本会话刚落地
   （`52a8a59`，`InputContext::TextEntry` + `UiMode::TextEntry`，存档命名在用）。
   「给你的据点起个名」这类需求已经在 `naming-and-localization.md` 五节的「覆盖名」里被设计过
   （原样透传、不翻译）。**问题是它算不算第一批的范围**——我倾向不算，但既然通道刚好在了，
   值得问一句。

8. **本文档要不要在 `knowledge/design/README.md` 的总索引里登记一行？**
   那份索引正文里有「二十份文档」这类会随之过期的计数，且它是一份逐条校对过的文档，
   我没有擅自改它。**如果要登记，应当是一次独立的、把计数一并核对的改动。**

### 需要更正邻接文档（不是裁定，是我在这里记一笔，等落地时一并做）

- `knowledge/design/narrative-system.md:185`/`:201`/`:214` 关于「对话变量插值必须等
  `format-text` 落地」的三处，**今天已经不成立**（三节 3.3）。那份文档整体写于脚本时代，
  三节的「文件格式：`.scm` + `register-narrative`」也已随脚本系统消失。
- `knowledge/design/ownership-and-crime-detection.md:227`/`:439` 关于
  「`Effect::TransferOwnership` 没有落地」的表述已经过期（`apply` 侧已接线），
  但「三个调用方都不存在」那一半**仍然准确**。
- `crates/ll-mod/src/quest.rs:114-115` 的 `QuestCondition::Script` 文档举的例子
  （「拜访某个 NPC 并说出特定台词」）**正是对话**。那个变体在脚本系统拆除后是死的，
  五节 2 给出的是它的替代品。

---

## 相关文档

- [社会系统：归属、文化、聚落与地图结构](society-and-affiliation.md) —— `Affiliation`/`OrgRef`/`standing`，「加入势力」的落点
- [Agent 目标、任务发布与经济](agent-goals-and-economy.md) —— 交易与定价的完整设计（P9），本文档五节 3 的占位公式将来被它替换
- [职业、技能树与任务系统](class-skill-quest-system.md) —— `QuestNodeDef` 的现状与「不做二档」的先例
- [物品归属（`Owner`）与犯罪判定系统](ownership-and-crime-detection.md) —— 四节「合法转移」，`Effect::TransferOwnership` 的共同机制与它的硬前置
- [剧本系统](narrative-system.md) —— 叙事编排层；对话是它的一个下层零件，但它整体写于脚本时代，见九节末尾的三处更正
- [命名、改名与本地化](naming-and-localization.md) —— NPC 姓名的派生机制、音素表索引对齐、覆盖名不翻译
- [mod 包结构与资产 VFS](mod-package-structure.md) —— `locales/<语言标签>.ftl` 约定与「键不需要再编码命名空间」的论证（本文档三节 3.2 的直接依据）
- [行动能力与输入上下文](action-capability-and-input-context.md) —— `UiMode` 栈与 `InputContext` 的分工，七节 1 的依据
- [0018 — 引擎层 / 玩法层的脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) —— 订正段：内容用 JSON5，行为逻辑用 Rust
- [0021 — 抽象需要共享算法，不是对称性](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) —— 四节 2 不复用 `RawExpr` 的判据
- [0028 — Steel 引擎构造期的内存破坏](../decisions/0028-steel-engine-construction-memory-corruption.md) —— 为什么对话必须是数据驱动的
- [`docs/architecture/03-invariants.md`](../../docs/architecture/03-invariants.md) —— C1–C5
