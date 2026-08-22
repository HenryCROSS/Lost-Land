# 制作系统：一套机制、四类配方、两条前置

**落地状态**：纯设计，零实现。全代码库检索（`crates/**`/`mods/**`）确认 `RecipeDef`/`RecipeTable`/
`RecipeCategoryDef`/`register-recipe`/`Intent::Craft`/`resolve_craft` **无任何匹配**——
`food-and-cooking-system.md` 三/四/七节给出的形状至今没有一行代码。本文档要建在其上的地基
（`ItemDef`/`ItemStack`/`Agent.inventory`/`Agent.equipment`/`Effect::ConsumeInventoryItem`/
`Effect::MergeIntoInventory`/`WorldState::terrain_at`/`Agent.script_state`/`kill_progress_effects`/
`WeaponCategoryTable` 与 `DamageCategoryTable` 两张类别表）**全部已实装**，逐项核实见一节。

**冻结于** 2026-08-22，基线提交 `e81e03c`（`main` 分支，卫兵盘查批次之后）。

**并发声明**：写作时工作区有未提交的杂项（截图、`save.llsave`、`config.json5`、`.claude/`），与本文档
主题无关。本文档只新增本文件、修订 `subclass-system.md` 的两处判断、更新 `README.md` 索引，
不触碰 `crates/**`/`mods/**`/`assets/**`/`scripts/**` 任何路径。

---

## 零、项目所有者的要求与本文档的范围

> 「新增两个副职：制作物品/武器/工具的副职，和制作衣服的副职」
> 「工匠 → 工具、武器、装备（护甲）；裁缝 → 衣服」
> 「本体主要是把整个框架做好做完整，提供足够好和足够多的功能和选项」

**本文档设计的是「制作」这个动作的框架**，不是具体配方内容，也不是采矿/种植/修理/温度任何一个
相邻系统。九节会逐条说明哪些选项现在做、哪些标为将来扩展，以及每一条的理由。

**本文档修订一处既有裁定**：`food-and-cooking-system.md` 四节把「场地需求」判为 YAGNI 不做，
八节把「现在就给 `RecipeDef` 加场地需求字段」列为被否决方案。**六节推翻这条**，理由是四类制作
合并成一套机制这个决定本身产生了新的、当时不存在的需求——详见六节，不是随意翻案。

---

## 一、现状核实：逐项 grep 证实，不采信任何「XX 已经有了」

本节的每一条都在基线提交上重新检索过。**协调者转述给我的地基状态全部属实**，另有四处需要补充或
纠正的发现，标注在对应条目里。

### 已实装（可以直接建在上面）

| 能力 | 位置 | 本文档哪一节依赖它 |
|---|---|---|
| `ItemStack { def, count, durability }`、`can_merge`/`merge_stacks`/`split_stack` | `crates/ll-world/src/item.rs:84`、`:560`/`:592`/`:627` | 五节产出合并 |
| `ItemDef { stack_limit, base_weight, base_price, max_durability, equip_mask, stat_bonuses, use_effect, damage_category }` | `crates/ll-mod/src/item.rs` | 三节（食材/成品/工具全是它） |
| `StatBonus { target: StatTarget, amount }`，`StatTarget` **只有 `Attribute(AttributeKind)` 与 `Armor` 两个变体** | `crates/ll-world/src/item.rs:464`/`:505` | 四节裁缝的价值轴 |
| `EquipSlot` 22 个本体槽位 + `SlotMask(u32)`（10 位留给 mod） | `crates/ll-world/src/item.rs:208`~`:426` | 六节工具前置 |
| `Agent.inventory: Vec<ItemStack>`、`Agent.equipment: BTreeMap<EquipSlot, ItemStack>` | `crates/ll-world/src/entity/agent.rs` | 五、六节 |
| `Effect::ConsumeInventoryItem { actor, def, durability }`（**恒扣一，无 `amount` 字段**） | `crates/ll-sim/src/effect.rs:639` | 五节 |
| `Effect::MergeIntoInventory { actor, replaced, resulting: Vec<ItemStack> }`（`resulting` 溢出时两条） | `crates/ll-sim/src/effect.rs:547` | 五节 |
| `WorldState::terrain_at(pos) -> Option<TerrainKind>`，`resolve_move` 是真实调用点 | `crates/ll-world/src/state.rs:674`、`crates/ll-sim/src/resolve.rs:1629` | 六节场地前置 |
| `resolve` 侧读 `agent.equipment` 是既有能力，不是新发明 | `crates/ll-sim/src/resolve.rs:1754`/`:1816`/`:2227` | 六节工具前置 |
| 耐久归零的装备**仍占槽位但不再贡献加成** | `crates/ll-sim/src/resolve.rs:344` | 六节「坏掉的工具算不算装着」 |
| `kill_progress_effects`：完整落地的「计数 + 达标授予」 | `crates/ll-sim/src/quest.rs:229` | 八节成长挂钩 |
| `Agent.script_state` + `ScriptValue::Int(i64)`，按 `(命名空间, 键)` 隔离 | `crates/ll-world/src/script_state.rs` | 八节计数存储 |
| `WeaponCategoryTable`/`DamageCategoryTable`：两张**已落地**的开放类别注册表 | `crates/ll-mod/src/weapon_category.rs`、`damage_category.rs` | 四节类别怎么表达 |
| `BASE_ACTION_COST: u32 = 100`、`action_cost(base, speed)` | `crates/ll-sim/src/resolve.rs:88` | 五节行动开销 |

### 纯设计、零实现（不能假设它在）

- `RecipeDef`/`RecipeTable`/`RecipeRule`/`RecipeCatalog`/`Intent::Craft`/`resolve_craft`
  ——`food-and-cooking-system.md` 三、四节给出形状，代码库零匹配。
- `SubclassDef.traits`/`SubclassUnlock`/`SubclassUnlockTrigger`/`subclass-unlocks-via!`
  ——`subclass-system.md` 三、四节给出形状，代码库零匹配。**`SubclassDef` 本体已落地但是空壳**
  （只有 `id`/`display_name_key`，没有任何写入 `Agent.subclasses` 的代码路径）。
- `SkillRequirement`/`skill-requires!`/`Intent::LearnSkill`/`resolve_learn_skill` ——全部零实现。
- `Agent.satiety`/`ResourceKind::Satiety` ——零实现。本文档不依赖它。

### 四处需要补充或纠正的发现

**① `mods/lostland/` 不存在。** `mods/` 下只有 `example_mod`、`broken_syntax`、`broken_whitelist`
三个目录。「本体内容搬进 `mods/lostland/` 的 Steel 脚本」这条方向本文档全程兼容（十节的注册 API
不含任何只有 Rust 能调的东西），但在迁移真正落地之前，ADR 0018 要求的「真实 mod 脚本为证」只能
落在 `mods/example_mod/gameplay.scm`——十一节 G 组如实记录这一点，不假装迁移已经发生。

**② `Agent.script_state` 不参与存档重映射，既有击杀计数因此有一处静默的存档隐患。**
`crates/ll-content/src/remap.rs:343` 原文 `script_state: _`（穷尽解构里显式跳过），而
`kill_count_key`（`crates/ll-sim/src/quest.rs:206`）把 `ContentIndex::get()` 的**数值**直接拼进键：
`format!("kill_count:{}", kind.get())`。mod 集合变化导致索引重编号后，`kill_count:37` 会静默指向
另一种生物。对照之下 `WorldState.kill_counts`（另一份聚合统计）**有** `ContentKind::KillCount`
的重映射覆盖（`crates/ll-content/src/degrade.rs:101`）——同一件事的两条存储通道，一条被保护、
一条没有。**这是既有代码的问题，不是本文档引入的**，但八节的制作计数若照抄 `kill_count_key` 的
手法就会原样继承它，因此八节给出一个不继承的键设计，并把这条既有隐患如实上报。

**③ `buffs-and-triggers.md` 7.3 与八节表格第 5 行的记录已过期。** 两处都写着抗性卡在
「`TraitTable`/`TraitDef`/`effective_traits` 整套零实现」——核实：`crates/ll-mod/src/trait_def.rs`
与 `ll_sim::traits::resistance_multiplier_permille` 均已落地，天赋抗性早已能跑
（`crates/ll-mod/src/trait_def.rs:26` 模块文档原文：「现在有真实消费者」）。**本文档不改那份文档**
（抗性是另一个批次的事，见十四节④），只如实记录这处漂移。

**④ 物品与技能都给不了抗性，季节没有任何玩法后果。** `StatTarget` 只有 `Attribute`/`Armor`
两个变体，`SkillDef` 的字段是 `id`/`owning_class`/`prerequisites`/`cooldown_ticks`/`resource_cost`/
`effect`——没有 `rule_modifiers`。`Tick::season()` 的全部消费者是 `season_light_scale`（光照）与
HUD 的季节名展示，零玩法后果。这两条直接决定四节裁缝价值轴的写法与十五节把「保暖」排除在外。

---

## 二、烹饪/锻造/裁缝/炼金是不是同一套机制：用 ADR 0021 独立复核

**结论：是同一套。协调者的判定成立，但它给出的依据（食物系统三节「成品都是普通 `ItemDef`」）
只证明了一半——那一半回答的是「食材和成品要不要建新类型」，不是「四类制作要不要建四套机制」。
本节补上真正的那一半论证。**

ADR 0021 的判据只有一条：**有没有一份算法要被多种类型真正共用？** 注意这条判据是双向的——
它既拦「为了对称而抽象」，也拦「把同一份算法复制四遍」。`compute_fov` 抽 `SightGrid` 与
`Camera`/`BoundedCamera` 不抽 trait 是同一条判据的两个方向。这里要问的是后一个方向。

### 制作的算法本体：验证 → 扣减 → 产出

不论做的是烤肉、铁剑、亚麻衬衣还是治疗药水，`resolve_craft` 要走的是同一串步骤：查配方 →
校验前置（类别闸门/场地/工具）→ 逐条校验食材数量 → 逐条产出扣减效果 → 用 `can_merge`/`merge_stacks`
把成品并进背包。这串步骤里**没有任何一步会因为「这是锻造不是烹饪」而不同**。

### 逐条检验「锻造/裁缝真的需要不一样的算法吗」

我把能想到的、最像是「锻造专属」的五条差异逐一放上判据：

1. **「铁剑有耐久，烤肉没有」** ——不成立。耐久来自 `ItemDef.max_durability`，是**成品自己**的
   属性；`ItemStack::with_durability`（`crates/ll-world/src/item.rs:111`）已经存在，`can_merge`
   的判据里本来就包含 `durability`。产出一堆成品这一步，对带耐久与不带耐久的成品是同一行代码，
   差异全部落在被查询的 `ItemDef` 数据上，不在算法里。
2. **「铁剑要在铁砧上打，烤肉在火堆上」** ——不成立。这是**同一个判定喂不同的数据**：
   `world.terrain_at(agent.pos) == recipe.required_station`。铁砧与火堆都是 `TerrainKind` 的一个取值，
   不是两种判定方式。
3. **「衣服能穿，药水能喝」** ——不成立。`equip_mask` 与 `use_effect` 都是成品 `ItemDef` 上的字段，
   `resolve_craft` 从头到尾**不读它们中的任何一个**。做出来之后怎么用，是 `resolve_equip`/
   `resolve_use_item` 的事，那两条路径早就落地且早就对所有物品一视同仁。
4. **「锻造需要工具（锤子），烹饪不需要」** ——不成立，且这条恰恰证明了统一。工具前置是
   `Option<ContentIndex>` 的有无，`None` 就是烹饪的情形。若拆成四套机制，这个 `Option` 要在四处
   各写一遍。
5. **「炼金可能失败/出品质，烹饪不会」** ——**这条如果成立就足以推翻统一，所以要认真对待。**
   但它不成立：失败与品质若真的要做（九节评估后判为将来扩展），它对「烧糊的炖菜」与「打废的
   剑坯」是同一件事，不是炼金专属；把它做成一条所有类别共用的规则才是对的，做成「只有炼金有」
   反而需要在配方上加一个「我这一类要不要走失败判定」的开关——那还是同一份算法加数据分支。

**五条全部落在「同样的算法，不同的数据」上。** 拆成 `ForgeRecipeDef`/`TailorRecipeDef`/
`AlchemyRecipeDef`/`CookRecipeDef` 四套，会把「校验食材是否齐全、按数量扣减、合并产出」这段
逻辑复制四份——ADR 0021 记录 `compute_fov` 时点名的那个代价（「此后每次修 bug 都要改四处、且容易
改漏一处」）在这里同样是真实的，不是理论上的：食材不足时的静默返回、堆叠上限溢出、耐久参与
合并判据，每一条都是容易写错且写错了测试未必发现的细节。

**因此：一套 `RecipeDef`、一个 `RecipeTable`、一个 `Intent::Craft`、一个 `resolve_craft`。
四类的差别全部落在数据上：`category`（谁能做）、`ingredients`（用什么）、`required_station`
（在哪做）、`required_tool`（拿什么做）。**

### 什么情况下这条结论会失效——留给未来的判据，不是现在的保留意见

若将来某一类制作需要一段**结构不同**的结算（例如锻造要做成「加热→锻打→淬火」的多步流程，
中间存在跨回合的半成品状态），那时候它就不再共用同一份算法，拆分才有理由。但那样的流程本身
就是 `subclass-system.md` 四节红线明确否决的「另开一个小游戏」，且引擎目前没有任何「可中断的
多回合活动」机制（五节核实）。**在那一天到来之前，统一是对的；到那一天，理由是「发现了不能共用
的算法」，不是「本来就该分开」。**

---

## 三、`RecipeDef` 与 `RecipeCategoryDef` 的最终形状

```rust
// crates/ll-mod/src/recipe.rs（新文件，列式存储照抄 item.rs 已验证的手法）
pub struct RecipeDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,

    /// 这条配方属于哪一类——指向 RecipeCategoryTable。**恒必填，不是
    /// Option**：类别是七节副职闸门与八节成长计数唯一的挂载点，一条
    /// 没有类别的配方在这两处都无处安放；且"不属于任何类别"这个语义
    /// 已经由"类别自身不设副职闸门"（RecipeCategoryDef.required_subclasses
    /// 为空）完整表达，不需要第二种表达方式。四节论证为什么类别是一个
    /// 显式字段而不是从 ingredients 推导。
    pub category: ContentIndex,

    /// 需要的食材与各自数量——恒非空，注册期拒绝空列表（同 register-item
    /// 拒绝 stack_limit 为 0 的既有纪律）。
    pub ingredients: Vec<RecipeIngredient>,

    /// 产出物品，指向 ItemDef。**不校验它是不是一件已定义的物品**——
    /// 只校验索引本身已 intern，理由同 food-and-cooking-system.md 三节：
    /// 跨表强校验会让注册顺序产生不必要的耦合。
    pub product: ContentIndex,

    /// 产出数量，恒 >= 1。
    pub product_count: u32,

    /// 必须站在哪种地形上才能制作，指向 TerrainDef。None = 随地可做。
    /// 六节论证为什么现在就加这个字段（推翻食物系统四节的 YAGNI 判断）。
    pub required_station: Option<ContentIndex>,

    /// 必须装备着哪件物品才能制作，指向 ItemDef。None = 徒手可做。
    /// 这是项目所有者点名的"工具"，也是将来采矿/种植的接入点——
    /// 六节说明为什么"工具 = 装备着的物品"而不是"背包里有就行"。
    pub required_tool: Option<ContentIndex>,
}

pub struct RecipeIngredient {
    pub item: ContentIndex,   // 指向 ItemDef
    pub count: u32,           // 恒 >= 1
}
```

```rust
// crates/ll-mod/src/recipe_category.rs（新文件，BTreeMap 存储照抄
// weapon_category.rs——类别表条目数量少、没有列式访问的性能诉求）
pub struct RecipeCategoryDef {
    /// 制作界面按类别分栏展示时的标题（"烹饪"/"锻造"/"裁缝"/"炼金"）。
    /// 与 WeaponCategoryDef 不同——那张表至今没有任何 UI 落点，所以
    /// 它没有这个字段；制作类别从设计上就是玩家会看见的分组维度。
    /// 十一节 C 组如实标注：在制作 UI 落地之前这仍然是一个待接线字段，
    /// 需要一条与 ItemDef.display_name_key 同款的门禁豁免。
    pub display_name_key: NamespacedId,

    /// 拥有其中任意一个副职即可使用本类别的配方（any-of 语义，与
    /// SkillRequirement.subclasses 同一条纪律）。**空列表 = 不设闸门，
    /// 人人可做**——这条默认值正是 food-and-cooking-system.md 五节
    /// "菜谱全部已知不设解锁门槛"裁定的直接延续，见七节。
    /// 由独立函数 recipe-category-requires-subclass! 写入，不进
    /// register-recipe-category 的位置参数。
    pub required_subclasses: Vec<ContentIndex>,
}
```

`resolve` 侧最小视图（与 `ItemRule`/`ResourcePoolRule`「只收敛真正要读的字段」同一条先例，
且 `ll-sim` 不能反向依赖 `ll-mod`，走依赖倒置——照抄 `QuestCatalog`/`SkillCatalog`）：

```rust
// crates/ll-sim/src/craft.rs（新文件）
pub struct RecipeRule {
    pub category: ContentIndex,
    pub ingredients: Vec<RecipeIngredient>,
    pub product: ContentIndex,
    pub product_count: u32,
    pub required_station: Option<ContentIndex>,
    pub required_tool: Option<ContentIndex>,
}
pub trait RecipeCatalog {
    fn recipe(&self, recipe: ContentIndex) -> Option<RecipeRule>;
    /// 类别的副职闸门——resolve_craft 只需要这一个字段，不需要
    /// display_name_key，因此不返回整个 RecipeCategoryDef。
    fn category_required_subclasses(&self, category: ContentIndex) -> Vec<ContentIndex>;
}
```

**为什么 `product` 是单个而不是 `Vec<RecipeProduct>`**：没有真实驱动（九节⑦评估了副产物），
且现在是设计阶段、表还没实现——将来若要放宽成多产出，是一次纯粹的字段加宽，没有存档迁移代价
（配方数据不进存档，十一节 D 组）。不为「以后可能要」预付复杂度。

---

## 四、类别怎么表达：复用已落地的类别注册表先例，不靠材料推导

### 结论：新开一张 `RecipeCategoryTable`，照抄 `register-weapon-category`/`register-damage-category` 的手法

`crates/ll-mod/src/weapon_category.rs` 与 `damage_category.rs` 是**两张已经落地**的开放类别表，
它们共同确立的模式是：**类别是一个独立的内容表**（`BTreeMap<ContentIndex, Def>` + `define` 注册期
查重 + ADR 0015「未注册返回 `None`」），引用方持有一个指向它的 `ContentIndex`
（`ItemDef.damage_category: Option<ContentIndex>`）。制作类别原样照抄，不发明新手法。

**为什么类别值得一张表，而不是一个裸的已 intern 索引**：表在注册期提供一条真实的校验——
`register-recipe` 传进来的 `category-id` 若从未 `register-recipe-category` 过，可以当场拒绝。
这能拦住 `"lostlan:cooking"` 这类拼写错误，而拼写错误若不拦，症状是「这条配方永远不出现在任何
分类里」，是最难查的一类内容 bug。这条注册期校验是**今天就成立的消费者**，不是等 UI 才有用的
承诺。

**为什么不是给 `RecipeDef` 加一个封闭枚举 `enum RecipeCategory { Cook, Forge, Tailor, Alchemy }`**：
直接违反 ADR 0018——mod 作者想加「木工」「制符」「炼器」时必须改本体 Rust 代码。两张已落地的
类别表当初正是为了避开这一点才做成开放注册表（`weapon_category.rs` 模块文档原文：「可扩展项
没有自然上限」）。

### 工匠/裁缝的分界：显式类别字段，不从 `ingredients` 反推材料

**协调者的倾向（显式类别字段，不靠推导）是对的，我独立复核后同意，并补三条它没写出来的理由：**

1. **推导需要一份不存在的算法和一个不存在的字段。** 「读每条食材的 `ItemDef`，判断它是金属还是
   布料，反推这条配方归谁」——`ItemDef` 上**没有任何材质字段**（已核实：`stack_limit`/`base_weight`/
   `base_price`/`max_durability`/`equip_mask`/`stat_bonuses`/`use_effect`/`damage_category`，没有
   材质）。要推导就得先加一个 `material` 字段、再写一段分类算法。相比之下 `category: ContentIndex`
   是一个字段、零算法。**用 ADR 0021 的判据看，推导方案是为了避免一个数据字段而发明一段算法，
   方向反了。**
2. **混合材料的配方没有定义良好的答案。** 皮甲用皮革（工匠）配麻线（裁缝）；镶铁的皮护腕同理。
   推导规则要么按「第一条食材」（任意）、要么按「数量最多的食材」（脆弱，改一下配方数量就换了
   副职）、要么按优先级表（那就是把类别信息藏进了另一张表，绕了一圈还是显式声明，只是更隐晦）。
3. **mod 作者无法预测。** 显式字段下，「这条配方归谁」由写配方的人一句话决定；推导规则下，他要
   先读懂本体的分类算法，且本体任何一次分类规则调整都会静默改变所有第三方 mod 的配方归属。

### 这条裁定不迫使系统结构改变——协调者的判断成立

护甲与衣服打同一批槽位（`BODY`/`OUTER`/`LEGS`/`BOOT_*`/`HAND_*`/`HEAD`），这确实让「按槽位划分界」
不可行。但**系统侧从来不需要划这条界**：`resolve_craft` 读的是 `RecipeDef.category`，
从不读成品的 `equip_mask`。「铁甲归工匠、亚麻袍归裁缝」是两条 `register-recipe` 调用各自填了
不同的 `category-id`，纯内容数据。系统结构一个字节都不用改。

### 裁缝的价值轴：`StatTarget::Attribute`，以及一处必须上报的结构性不对称

按现成机制，两个副职的产出可以这样区分（**内容设计示意，非最终数值**）：

| | 工匠 | 裁缝 |
|---|---|---|
| 产出 | 工具、武器、护甲 | 衣物 |
| 主要加成轴 | `StatTarget::Armor` + 力量/体质 | 敏捷/幸运（`StatTarget::Attribute`） |
| 机制成本 | 零，`StatBonus` 已实装 | 零，同左 |

**但我必须上报一处结构性不对称，它不是靠调数值能补的**：项目所有者已裁定
**只有装备武器才有耐久**。武器会磨损 → 玩家会反复回来找工匠。衣服**永远不坏**，
裁缝的每一件产出都是一次性买卖，做完这一件就再也不需要他。工匠因此天然拥有一条重复需求循环，
裁缝没有。这不是「裁缝的属性加成不够好」，是两个副职在物品经济里处的位置不同。
**能补上这个口子的三条路各自属于别的批次，本文档不设计任何一条**：（a）扩大耐久到防具/衣物
——推翻既有裁定，需要所有者定夺；（b）物品能授予抗性——见十四节④，属于跨切面的「多来源抗性」
批次；（c）温度/保暖——需要从零造温度机制，见十五节明确排除。

**抗性轴将来会补上。** 项目所有者已明确抗性会有四个来源（天赋、装备、药品、技能），
现状是只有天赋能给（一节④已核实）。那个批次落地之后，「丝袍抗火」这类产出会让裁缝
（以及炼金产的抗性药剂）获得一条工匠没有的价值轴。**本文档只记这条依赖，不设计它**：
不新增 `StatTarget` 变体，不碰抗性结算。

---

## 五、`Intent::Craft` 的形状、`resolve_craft` 的判定顺序与行动开销

### 形状（沿用 `food-and-cooking-system.md` 四节，不改）

```rust
Intent::Craft {
    actor: EntityId,
    recipe: ContentIndex,
},
```

与既有 15 个 `Intent` 变体同一条纪律：只携带裸请求，一切合法性判断留给 `resolve_craft`。
为什么不复用 `Intent::Use`，食物系统四节已论证（输入是配方索引不是物品索引、输出是多条消耗
加一条产出不是单条对单条），本节不重复。

### `resolve_craft` 的判定顺序——顺序本身是设计决定

```
1. 查 agent，查不到 → 空
2. 查 RecipeCatalog::recipe(recipe)，查不到 → 空          （ADR 0015：未注册当作没有）
3. 副职闸门：category_required_subclasses 非空 且
   agent.subclasses 与之无交集 → 空                        （七节）
4. 场地前置：required_station 为 Some(t) 且
   world.terrain_at(agent.pos) != Some(t) → 空             （六节）
5. 工具前置：required_tool 为 Some(i) 且
   agent.equipment 里没有一件"def == i 且耐久未归零"的装备 → 空  （六节）
6. 食材校验：逐条在 agent.inventory 里找 def 匹配的堆，
   任意一条 count 不够 → 空（不产出任何效果，不消耗任何食材）
7. 逐条食材产出 Effect::ConsumeInventoryItem，重复 count 次
8. 用 can_merge/merge_stacks 算出产出去向，产出 Effect::MergeIntoInventory
9. 追加 craft_progress_effects（八节），产出 Effect::SetScriptState
```

**顺序的理由**：3→4→5 三道前置排在 6 食材校验之前，是因为**前三道是「你能不能做这件事」，
第四道是「你现在够不够料」**。玩家更需要先知道「我不会锻造」而不是「你缺两块铁锭」——虽然本文档
不设计 UI，判定顺序决定了将来 UI 能拿到的失败原因的优先级，现在定下来比将来靠调用顺序意外形成
要好。

**全程静默失败，与既有纪律一致**：任何一步不满足都返回空 `Vec<Effect>`，不产出效果、不消耗
食材、不推进计数——与 `resolve_use_skill` 资源不足时静默不产出效果是同一条既有纪律。

**约束核对**：C3 不涉及（全程零随机，九节论证为什么现在不做失败判定）；C5 满足——第 5 步遍历的
`agent.equipment` 是 `BTreeMap`（有序），第 6 步遍历的 `agent.inventory` 是 `Vec`（有序），
没有任何一处依赖 `HashMap`/`HashSet` 的迭代顺序；C1/C2/C4 不涉及（不新增脚本状态跨帧持有、
不进时间轴队列、不改后台推进）。

**已知简化（继承自食物系统四节，如实重复）**：第 6 步只认第一条 `def` 匹配的堆，不跨多堆合并
计数；第 8 步若 `product_count` 大到需要三堆以上，`MergeIntoInventory.resulting` 目前的
「最多两条」语义装不下。两条都只在数量远超 `stack_limit` 时才失真，本文档的验收示例不触及，
记为已知边界。

### 行动开销：一次制作 = 一次普通行动，`action_cost(BASE_ACTION_COST, speed)`

与 `resolve_wait`/`resolve_use_item` 等既有动作走**完全相同**的计费，不新增任何常量。

**为什么不按配方复杂度分级（打一把剑应该比切一块肉久）**：不是不想，是**引擎没有可中断的
多回合活动机制**。核实：`crates/ll-sim` 全库没有任何「进行中的活动」状态——每个 `Intent` 都是
「提交 → 一次 `resolve` 产出全部效果 → 按 `action_cost` 推进时间轴」的一次性结算，没有
「这个动作要占用接下来 20 个 tick，期间被攻击会中断」这条路径。给 `RecipeDef` 加一个
`action_cost_multiplier` 字段倒是能表达「这次制作让时间轴前进 2000 而不是 100」，但那不是
「花了 20 回合打铁」，而是「原地消失 20 回合、期间怪物走了 20 步而你毫无察觉」——一个在传统
roguelike 里明显错误的行为。**要么做对（需要一套可中断活动机制，远超本文档范围），要么不做
（一次行动）。做成一个假的中间态最差。** 标为将来扩展，见十五节。

---

## 六、在哪制作：场地 = 地形，工具 = 装备着的物品

### 能力边界核实（先核实再设计，不发明做不到的东西）

| 想要的判定 | 能不能做 | 依据 |
|---|---|---|
| 我脚下是什么地形 | **能** | `WorldState::terrain_at(pos)`，`resolve_move` 是真实调用点 |
| 我相邻格子是什么地形 | **能** | `TorusPos` 邻居加减 + 每格一次 `terrain_at`，纯算术，零新原语 |
| 我装备栏里有没有某件物品 | **能** | `agent.equipment` 在 `resolve` 侧已被多处读取（`:1754`/`:1816`/`:2227`） |
| 我背包里有没有某件物品 | **能** | `resolve_use_item` 既有做法 |
| **我附近有没有某个实体/某个玩家放下的物件** | **不能** | 批量实体查询原语（`script-entity-handles-and-batch-queries.md`）没有接进 `resolve`；且代码库**没有任何可放置物件/构筑物系统**——`StructureKind` 是世界生成期的地图结构层，不是玩家能放下的东西 |

**因此「工作台」只有两种可行建模**，第三种（玩家亲手放下一个铁砧物件）在当前代码库里不存在
落点，本文档不设计它：

- **场地 = 一种地形**（固定在世界里，走过去用）
- **工具 = 一件装备**（随身带，装备上就能用）

这两条恰好是一对互补的设计：**场地不可携带，让城镇/据点有存在意义；工具可携带，让野外制作
成为可能。** 项目所有者要的「足够多的选项」在这里是两个 `Option<ContentIndex>` 字段加两行判定
换来的，代价极低。

### 为什么现在就加 `required_station`——推翻食物系统四节的 YAGNI 判断

`food-and-cooking-system.md` 四节把场地需求判为「机制能做，但没有真实需求驱动，YAGNI」，
八节把它列入被否决方案。**那个判断在当时是对的，现在不对了，因为二节的统一决定本身产生了
新需求**：

当四类制作合并成一套机制、共用一份 `resolve_craft` 之后，「烹饪」和「锻造」在系统里就只剩下
四个数据维度的差别（类别、食材、场地、工具）。**去掉场地这一维，锻造与烹饪的唯一差别就只剩
「食材不同 + 归不同副职管」**——那不是四类制作，那是一类制作贴了四个标签。铁砧/织机/炼金台
恰恰是让四类在玩法上真的不同的东西：它决定了你能在哪做什么，进而决定了「回城」这个行为有没有
意义。

**这不是翻案时的事后合理化，判据是可检验的**：食物系统四节写「两个验收示例都不需要它才能成立」
——那时候只有烤肉和炖菜两个示例，两个都是野外烹饪，确实不需要。本文档十三节的四个示例里有两个
（铁剑、亚麻衬衣）**需要它才说得通**：一把铁剑能在荒野徒手打出来，是一个玩家会立刻察觉不对的
设定。需求条件变了，结论跟着变。

**MVP 判定是「站在这格上」，不是「站在旁边」**：一次 `terrain_at(agent.pos)` 调用，与
`resolve_move` 完全同款。相邻判定虽然也能做（上表已核实），但它引入「多个相邻工作台算哪个」
这类不必要的问题。配套的内容纪律：**工作台地形必须是可通行的**（「锻造间地面」「灶台旁」，
而不是一个挡路的铁砧方块），否则玩家站不上去。这条纪律写给内容设计，不是系统限制。

### 工具为什么是「装备着」而不是「背包里有」

三条理由，第三条最重要：

1. **有代价。** 装备着意味着占一个槽位——拿着锤子就腾不出那只手拿盾。这是一个真实的取舍，
   「背包里有」则毫无代价，等于只是一道「你买过这个道具吗」的检查。
2. **和「工具」这个词的物理直觉一致。** 你用锤子打铁，锤子在手里。
3. **它是采矿/种植将来唯一的正确接入点。** 项目所有者要的「工具解锁动作」（镐子解锁挖矿），
   将来的 `resolve_mine` 必然要问「他手里拿着镐子吗」，而不是「他包里有镐子吗」——**这里定下
   的判定形状会被那个批次原样复用**，现在选错，将来要么跟着错，要么两套判定并存。
   **本文档不设计采矿/种植本身**，只保证这个接入点的形状是对的。

**坏掉的工具算不算装着——判定必须与既有纪律一致。** 已核实
（`crates/ll-sim/src/resolve.rs:344`）：耐久归零的装备**仍占着槽位但不再贡献属性加成**
（`item-system.md` 六节「归零 = 损坏不可用」）。工具前置因此必须用**同一个谓词**：
`def == required_tool && durability != Some(0)`。若只判 `def` 相等，会出现「锤子已经烂了但
还能打铁」——一处与既有耐久语义直接矛盾的漏洞。

**当前工具不会因为制作而磨损**，因为项目所有者已裁定「只有装备武器才有耐久」。这意味着一把
锤子可以永久使用。**这是一个需要所有者定夺的口子，本文档不擅自扩大耐久的适用范围**——只记在
十五节，与四节末尾裁缝那处不对称是同一个根因。

---

## 七、副职闸门：闸在类别上，不闸在配方上

### 结论：`RecipeCategoryDef.required_subclasses`，any-of，空列表 = 不设闸

```scheme
(recipe-category-requires-subclass! "lostland:forging" "lostland:artisan")
```

**为什么闸在类别而不是每条配方**：这正是类别存在的意义。若每条配方各自声明需要哪个副职，
「工匠能做的全部东西」就散落在几十条配方里，加一个新副职要逐条改；闸在类别上，
「工匠 = 锻造类别的访问权」是一句话，新增一条锻造配方自动继承闸门。这也让八节的成长计数有一个
天然的、数量有界的计数键（按类别计数，不是按配方计数）。

**为什么是独立函数，不塞进 `register-recipe-category` 的参数列表**：照抄 `skill-requires!`
六节的既有理由——「分类展示」与「强制闸门」是两件独立的事，混在一起会在将来某天有人想
「只分类展示、不强制」或反过来时变成一处隐藏耦合。同时也照抄
`register-item-damage-category`（`crates/ll-mod/src/script_item_api.rs:695`）这条**已落地**的
先例：给一张已注册的内容表条目追加一个可选属性，走独立函数写入同一张表，不加宽原注册函数的
位置参数列表。可对同一个类别多次调用，每次追加一个副职（any-of），不做去重校验——理由同
`subclass-grants-trait!`（重复条目在 any-of 判定里是幂等的）。

**空列表是默认，而且这条默认值就是食物系统五节裁定的延续**：`"lostland:cooking"` 类别不调用
`recipe-category-requires-subclass!`，于是人人都能做饭——`food-and-cooking-system.md` 五节
「菜谱全部已知、不设解锁门槛」原样成立，本文档没有推翻它。锻造/裁缝调用一次，于是需要对应副职。
**「有没有闸门」因此是一个纯内容决定，系统不预设立场。**

**这道闸不是「学会配方」。** 两件事必须分清：
- **类别访问权**（本节）——你是不是工匠。零新增 `Agent` 字段，读的是已落地的 `Agent.subclasses`。
- **配方解锁**（`known_recipes`，食物系统五节评估后否决）——你知不知道这张图纸。需要一个新的
  `Agent` 字段，**本文档同样不做**。十四节①说明这与「科研」方向的关系。

---

## 八、成长挂钩：`ItemsCrafted(category)`，以及对 `subclass-system.md` 的两处订正

### 订正一：工匠的成长挂钩应该是制作次数——协调者的判断成立，且比它说的更强

`subclass-system.md` 一节候选表给「工匠/锻造」的理由是：「耐久是消耗性的，推不出锻造技艺提升，
真正该挂的『修理』动作不存在」，六节据此把它排到第 8 位垫底。

**我独立复核的结论：这个理由不成立，订正是对的。** 论证：

1. 那份文档自己定的判据是「成长挂钩必须是玩家已经在做的动作」。它检查了「用装备」（因果反了）
   和「修理」（不存在），**唯独漏了「制作」这个动作本身**——而制作恰恰是工匠这个副职的定义性
   动作。这是一处遗漏，不是权衡。
2. 「制作动作不存在」在写那份文档时是事实，但它**无论如何都要为烹饪造出来**——项目所有者已经
   点名要食物/菜谱系统。工匠挂钩的**边际成本**因此不是「造一个新动作」，而是「在一个正在造的
   动作上加一个计数器」，与那份文档给 `ItemsPickedUp` 的评价（「不是新范式，是把已经验证过的
   模式再抄一份」）完全同级。
3. **比协调者说的更强的一点**：这个订正不只救工匠，它顺带修正了**炼金**的挂钩。那份文档给
   炼金挂的是 `ItemsPickedUp`（捡材料）——但捡草药是所有人都在做的通用动作，用它当炼金的进度
   条，等于「探索得够多就自动变成炼金术士」。`ItemsCrafted("lostland:alchemy")` 精确得多：
   **你熬过多少锅药，才是你炼金练了多久**。四个制作副职（工匠/裁缝/炼金/厨艺）因此共用同一个
   触发器变体，各自填不同的类别。

**订正后的排序**：工匠从第 8 位升到与炼金并列的第 3 梯队——**不是升到第一**。它依赖
`Intent::Craft` + `RecipeTable` + `RecipeCategoryTable` 三样都还没落地的东西，而
「测绘/制图」（`TilesExplored`，零新增存储）与「殡葬/掘尸」（复用已落地的 `kill_progress_effects`
挂载点）仍然更便宜。诚实的表述是：**工匠不再是「阻塞在一个不存在且因果倒置的机制上」，而是
「和其余三个制作副职一起，阻塞在同一个正在设计的制作系统上」**——一个投资解锁四个方向。

**一处协调者没提、但必须一并记下的隐忧**：即使成长挂钩修好了，工匠/裁缝的**长期需求**仍然
薄弱（四节末尾已论证：衣服不坏，武器坏但只有武器坏）。挂钩解决的是「怎么获得这个副职」，
不解决「获得之后它还有没有用」。后者需要耐久范围或抗性来源的裁定，不在本文档。

### 订正二：厨艺的价值不再只剩「解锁副职专属技能」

`subclass-system.md` 一节给厨艺的评语是「食物系统已裁定菜谱不设门槛，副职收益只剩技能闸，
价值比炼金弱一档」。**七节的类别闸门让这条评语部分失效**：内容设计现在可以把「野外烹饪」
「大餐」这类高阶配方放进一个**设了副职闸门的第二类别**（例如 `"lostland:gourmet"`），
而基础烹饪类别保持人人可做。厨艺因此和其余三个制作副职站在同一条起跑线上，不再天然弱一档。

**注意这不等于推翻食物系统五节**：那一节裁定的是「配方不需要逐条学会」，本文档给的是
「类别可以要求副职」——前者关于 `known_recipes`（仍然不做），后者关于 `Agent.subclasses`
（已落地）。两者不冲突，见十四节①。

### 形状：`SubclassUnlockTrigger` 第四个变体

```rust
/// 累计制作某个配方类别的成品达到 threshold 次——挂载点是新增的
/// craft_progress_effects，结构照抄 kill_progress_effects
/// （crates/ll-sim/src/quest.rs:229）。
/// ContentIndex 指向 RecipeCategoryTable，不是某一条具体配方——
/// 按类别计数让键空间保持在"类别数量"这个小量级上，而不是"配方数量"。
ItemsCrafted(ContentIndex),
```

`craft_progress_effects` 复用的既有类型，逐个点名（**全部已落地，零新类型**）：

| 复用什么 | 位置 |
|---|---|
| `Agent.script_state: BTreeMap<(String, String), ScriptValue>` | `crates/ll-world/src/script_state.rs` |
| `ScriptValue::Int(i64)` | 同上 |
| `ScriptStateWrite { target, mod_namespace, key, value }` | 同上 |
| `ScriptStateTarget::Entity(EntityId)` | 同上 |
| `Effect::SetScriptState { writes }` | `crates/ll-sim/src/effect.rs` |
| 「累加 → 达标检查 → 追加授予写入」的整体结构 | `crates/ll-sim/src/quest.rs:229` |
| ADR 0023「脚本状态写入必须过 `apply`」 | 计数写入走 `Effect`，不直接改 `Agent` |

### 计数的键：**不要**照抄 `kill_count_key`，它有一处存档隐患

一节②已核实：`kill_count_key` 把 `ContentIndex::get()` 的数值拼进键
（`format!("kill_count:{}", kind.get())`），而 `remap.rs:343` 明确不重映射 `script_state`。
mod 集合变化导致索引重编号后，这些键会静默指向别的内容。

**制作计数应当用类别的 `NamespacedId` 字符串做键**：`"craft_count:lostland:forging"`。
命名空间标识符跨 mod 集合变化保持稳定，天然免疫重编号。

**这不需要给 `craft_progress_effects` 传一份 `Registry`**——`QuestCatalog::kill_count_quests()`
已经示范了正确做法：需要反查的 `NamespacedId` 在 **catalog 构建期**一次性解析好，随规则一起
返回。照抄：

```rust
pub struct CraftUnlockRule {
    pub category: ContentIndex,      // 用于与本次制作的类别比对（整数比较）
    pub category_id: NamespacedId,   // 用于拼键（catalog 构建期已解析好）
    pub subclass: ContentIndex,
    pub threshold: u32,
}
```

只有被某条 `CraftUnlockRule` 引用到的类别才需要计数——没有规则引用的类别，计数无人读取，
不写入，键空间因此天然有界。

**既有击杀计数的同款隐患本文档不修**（不在授权范围，且属于 `ll-sim`/`ll-content`），
但如实上报给协调者，见十六节。

---

## 九、产出的变化度：现在做什么、什么标为将来扩展

项目所有者要「足够好和足够多的功能和选项」，`coding-style.md` 的 YAGNI 要求每个选项自证。
逐条评估，**判据统一是：这个选项今天有没有一个真实的需求驱动，以及它的代价落在哪**。

### 现在做（四条）

| 选项 | 代价 | 为什么值得现在做 |
|---|---|---|
| ① **配方类别** `category` | 一张 `BTreeMap` 表 + 一个必填字段 | 七节副职闸门与八节成长计数**唯一**的挂载点。没有它，项目所有者点名的两个副职无处安放——这是本次任务的直接需求，不是预留 |
| ② **场地前置** `required_station` | 一个 `Option` 字段 + 一次 `terrain_at` | 六节已论证：统一决定本身产生的需求。缺了它，四类制作在系统里只剩「食材不同」这一个差别 |
| ③ **工具前置** `required_tool` | 一个 `Option` 字段 + 一次 `equipment` 查找 | 项目所有者点名要「工具」；且是采矿/种植将来的接入点，形状现在定错将来要么跟着错要么两套并存 |
| ④ **同一成品允许多条配方** | **零**——不加任何字段，只是**不**在 `product` 上加唯一性约束 | 白拿的变化度：铁剑可以有「铁锭×2」和「废铁×3 + 木炭」两条路，粗铁匠与精铁匠的配方可以产出同一件东西。代价真的是零，只需要在 `RecipeTable::define` 里**不写**那条约束，并在文档里说明这是刻意的 |

### 将来扩展（六条，每条给出为什么不是现在）

| 选项 | 为什么不是现在 |
|---|---|
| ⑤ **制作失败（消耗材料但无产出）** | 机制上做得到（`DetRng::for_entity` 已落地，C3 合规）。但在传统 roguelike 里，一次吃掉材料、什么都不给、玩家无法通过任何决策规避的失败，是纯粹的挫败感——它没有增加任何**决策**内容，只增加了重复劳动。要让失败有意义，需要「失败率随技艺下降」这条曲线，而技艺不存在（见⑦）。**两个前提都不成立时先不做**，且不做的代价是零：加上它只是 `resolve_craft` 里多一次 `DetRng` 取样与一个分支 |
| ⑥ **产出品质分档** | `item-system.md` 提过六档品质，但 `ItemStack` **没有品质字段**（已核实：只有 `def`/`count`/`durability`）。加一个逐实例品质要动 `can_merge` 的判据、`ItemStack` 的存档形状、`WorldState::hash()`、以及全部消费 `StatBonus` 的派生路径（品质要影响加成才有意义）。**这是一个跨 `ll-world`/`ll-sim`/`ll-content` 的独立批次，不是制作系统的一个字段** |
| ⑦ **产出数量/品质随属性或技艺浮动** | **它会直接违反一条既有裁定**：`subclass-system.md` 三节「副职不给数值，只当资格闸门；给东西的是天赋」。「锻造技艺 87 点」这种数值在当前设计里**没有合法的存放位置**——要做就要先推翻那条裁定或新开一套技艺数值。不是成本问题，是与既有设计直接冲突，需要所有者定夺 |
| ⑧ **副产物 / 多产出** | 把 `product`/`product_count` 换成 `Vec<RecipeProduct>`。**纯粹的字段加宽，且因为配方数据不进存档（十一节 D 组），将来加宽零迁移代价**。没有真实驱动就先不加 |
| ⑨ **多回合制作** | 五节已论证：引擎没有可中断的多回合活动机制，做成「时间轴直接前进 2000」是一个假的中间态，比不做更差 |
| ⑩ **工具因制作而磨损** | 与所有者「只有装备武器才有耐久」的裁定直接冲突。六节末尾已标注为需所有者定夺的口子，本文档不擅自扩大耐久范围 |

**一句话概括这一节的取舍**：现在做的四条，三条是所有者点名需求的直接落点、一条是零代价；
推迟的六条里，两条与既有裁定冲突（⑦⑩）、一条缺前提（⑤）、一条是独立批次（⑥）、
两条是零迁移代价的将来加宽（⑧⑨）。**没有一条是「以后可能用得上所以先留着」。**

---

## 十、mod 注册 API：一档，六个函数

**全部一档**（ADR 0016/0017：声明式，注册期物化，运行期查表，零脚本回调）。档位判据三步走：
有自由度（mod 能声明任意配方/类别/闸门）；自由度落在纯数据上（食材表、阈值、索引，运行期只做
整数比较与 `BTreeMap` 查找）；调用频率——制作结算是常规玩法路径，与既有物品/技能注册同一量级。
**配方是纯静态声明，没有任何运行期才能决定的成分，没有任何理由走二档或三档。**

**ADR 0020 核对**：六个函数的参数只有字符串（标识符）与 `i64`（数量/阈值），
**没有任何浮点**。乙区（流进世界状态）的量只有 `count`/`product_count`/`threshold`，全是整数，
不需要 `Milli` 量化。

```scheme
;; ① 类别（新内容表）
(register-recipe-category "lostland:forging" "lostland:recipe_category_forging_display_name")

;; ② 类别的副职闸门——独立函数，可多次调用追加（any-of），不调用即人人可做
(recipe-category-requires-subclass! "lostland:forging" "lostland:artisan")

;; ③ 配方本体
(register-recipe "lostland:iron_sword_recipe"           ; id
                 "lostland:iron_sword_recipe_display_name"
                 "lostland:forging"                     ; category-id
                 (list (recipe-ingredient "lostland:iron_ingot" 2)
                       (recipe-ingredient "lostland:leather_strip" 1))
                 "lostland:iron_sword"                  ; product-id
                 1)                                     ; product-count

;; ④ 食材构造器（沿用 food-and-cooking-system.md 七节）
(recipe-ingredient item-id count)

;; ⑤⑥ 两条可选前置——独立函数，不加宽 register-recipe 的位置参数
(recipe-requires-station! "lostland:iron_sword_recipe" "lostland:forge_floor")
(recipe-requires-tool!    "lostland:iron_sword_recipe" "lostland:smithing_hammer")
```

**为什么 `ingredients` 内联而 `station`/`tool` 走独立函数**——两条不同的既有先例，各自适用：

- `ingredients` **内联**：照 `register-trait` 的四个 `(list ...)` 参数。食材表在注册那一刻就是
  完整的、必填的，且是配方的定义性内容。`food-and-cooking-system.md` 七节已论证，不重复。
- `station`/`tool` **独立函数**：照 `register-item-damage-category`（已落地）。两者都是
  **可选**属性，大多数配方两个都不填——把两个可选参数塞进位置列表，会逼每一条普通配方都传两个
  空串哨兵。`register-damage-category` 确实用了空串哨兵，但那是**一个**可选参数；两个就该拆。

### 注册期校验（全部在 `define` 里，失败即拒绝，ADR 0017）

- `ingredients` 恒非空；每条 `count` 与 `product_count` 恒 `>= 1`。
- `category-id` **必须已经 `register-recipe-category` 注册过**——这是类别值得一张表的直接理由
  （四节），拦拼写错误。
- `id` 不重复（`DuplicateDefinition`，同 `ItemTable`/`WeaponCategoryTable` 既有模式）。
- `recipe-category-requires-subclass!` 的两个参数都必须已注册（跨表存在性校验，同
  `subclass-grants-trait!`）。
- `product-id`/食材 `item-id`/`station`/`tool` **只校验索引已 intern，不跨表校验是不是一件真物品/
  真地形**——理由同 `food-and-cooking-system.md` 三节：跨表强校验会让注册顺序产生不必要的耦合，
  与 `TraitTable`/`ItemTable` 目前互相不做这类校验一致。
- **`product` 不做唯一性校验**（九节④，刻意）。

---

## 十一、新内容类型的完整接线清单

两张新内容表（`RecipeTable`/`RecipeCategoryTable`）必须走完下列全部环节。
**标 ★ 的是协调者交付清单里没有点到、但我核实后确认必须做的。**

### A. 内容表与注册（`ll-mod`）

1. `crates/ll-mod/src/recipe.rs`——`RecipeDef`/`RecipeAttrs`/`RecipeTable`/`RecipeError`，
   列式存储照抄 `item.rs`（`Vec<...>` 每字段一列 + `defined: Vec<bool>`）。
2. `crates/ll-mod/src/recipe_category.rs`——`RecipeCategoryDef`/`RecipeCategoryTable`/
   `RecipeCategoryError`，`BTreeMap` 存储照抄 `weapon_category.rs`。
3. `crates/ll-mod/src/script_recipe_api.rs` / `script_recipe_category_api.rs`——
   `thread_local! { ACTIVE_TABLE }` + `set_active_target`/`take_active_target` 成对手法，
   照抄 `script_weapon_category_api.rs`。
4. ★ **`crates/ll-mod/src/pipeline.rs` 装载管线接线**——两张表要有各自的写入目标字段与
   set/take 调用点，照 `pipeline.rs:162`/`:166`（`register-weapon-category`/
   `register-damage-category` 的写入目标）既有先例。**漏掉这一步的症状是注册函数在「没有活跃表
   的窗口内被调用」而全部报错**，不是静默失败，但清单上必须有。
5. `crates/ll-mod/src/lib.rs` 模块声明；`register_*_api` 接进引擎装配点。

### B. 内容值哈希（ADR 0022 / 0027）

6. `ContentTableKind` 新增 `Recipe`/`RecipeCategory` 两个变体——该枚举的 `match` **不带通配分支**
   （编译期防线一）。
7. `ContentValueTables` 新增 `recipe`/`recipe_category` 两个字段——`classify_index` 对 `*tables`
   做**穷尽解构**（不带 `..`，编译期防线二）。
8. ★ **`classify_index` 两条新分支**——协调者说的「穷尽 match」具体落在这个函数
   （`content_hash.rs:379`）。
9. `write_recipe_fields`/`write_recipe_category_fields`——**ADR 0027：覆盖字段值，不只 id 集合**。
   具体注意两处：`ingredients` 是变长 `Vec`，照既有处理 `Vec` 的手法先写长度再逐条写
   （否则 `[(A,1),(B,2)]` 与 `[(A,1),(B,2),(C,3)]` 可能撞哈希）；
   `required_station`/`required_tool` 是 `Option<ContentIndex>`，照
   `content_hash.rs:998` 处理 `max_durability: Option<i32>` 的 `match` 手法，
   **`None` 与 `Some` 必须写入不同的判别字节**。
10. `CONTENT_HASH_ALGORITHM_VERSION`：**6 → 7**，并在该常量上方的版本沿革注释里追加一条
    （既有注释在 `content_hash.rs:225`/`:235` 已为第 13/14/15 张表各记了一条）。
11. ★ **`crates/ll-game/src/content.rs:535` 的 `Opaque` 覆盖率回归测试**——该测试断言
    「被判定成 `ContentTableKind::Opaque` 的 id 集合恰好等于已知例外集合，不多不少」。
    新表条目若没被 `classify_index` 认领会全部落进 `Opaque` 从而让它变红。**这是第 8 步的
    运行期防线，与编译期两道防线互补，清单上不能只写「穷尽 match」。**

### C. 字段消费者门禁（`scripts/ci/check_field_consumers.py`）

12. `TARGET_TYPES` 新增两行：`("crates/ll-mod/src/recipe.rs", "struct", "RecipeDef")` 与
    `("crates/ll-mod/src/recipe_category.rs", "struct", "RecipeCategoryDef")`。
13. `EXEMPTIONS` 新增 `RecipeDef.display_name_key` 与 `RecipeCategoryDef.display_name_key`
    ——理由照抄既有的 `ItemDef.display_name_key` 条目（「指向 Fluent 本地化键，UI 展示用，
    不是玩法数值」）。
14. ★ **两道门禁之间的互校会强制第 12 步**——`check_content_hash_gate_cross_coverage()`
    （同文件 `:398`）把内容值哈希门禁当权威来源：`ContentTableKind` 一旦多出一个变体，
    而 `TARGET_TYPES` 没跟上，CI **立刻变红**。含义是 **B6 与 C12 必须在同一个提交里**，
    不能分批。这条互校是 `4b4d202` 引入的，协调者的清单里没有点到。

### D. 存档

15. **`crates/ll-content/src/remap.rs`：核实为无操作，且这是有依据的结论，不是遗漏。**
    MVP 下**没有任何 `WorldState` 字段持有指向这两张表的 `ContentIndex`**——配方不进背包
    （`ItemStack.def` 指 `ItemTable`）、不进装备、不进地面堆，`Agent.known_recipes` 不存在
    （七节裁定不做）。`remap.rs` 的穷尽解构因此不需要新分支，
    `crates/ll-content/src/degrade.rs` 的 `ContentKind` 也不需要新变体。
    **将来若做 `known_recipes`，那时才需要一个 `ContentKind::Recipe`，处理方式照抄
    `ContentKind::Skill`/`Subclass`（无条件丢弃并警告）。**
16. ★ **但制作计数是一处真实的存档缺口**——见一节②与八节：`remap.rs:343` 不重映射
    `script_state`。**八节的键设计（用 `NamespacedId` 字符串而非索引数值）是为了不继承这个
    缺口**，实现时必须照做，否则等于新写一个已知有 bug 的东西。
17. 存档 schema 版本/迁移链：**不需要动**——没有新增任何世界状态字段，`WorldState::hash()`
    也不需要改（配方数据全在内容层，靠内容哈希覆盖，不靠世界状态哈希）。

### E. i18n

18. `assets/locales/zh-CN.ftl` 与 `en.ftl` 各新增两组键：`recipe-*-display_name`、
    `recipe_category-*-display_name`（键名沿用既有 `race-*`/`class-*`/`subclass-*`/
    `equip_slot-*` 的形状；`ll_i18n::to_fluent_id` 会把 `_key` 里的点号换成连字符，
    下划线原样保留）。
19. ★ **两个 `.ftl` 文件头部的「键的来源」注释清单要各加两行**——`zh-CN.ftl` 头部有一份
    按字段分组的覆盖核对表（`race-*-display_name  ll-mod RaceAttrs::display_name_key` 一类），
    漏掉它不会让 CI 变红，但会让那份核对表从此不可信。
20. 内容若搬进 `mods/lostland/`，键改放 `mods/lostland/locales/*.ftl`——按
    `mod-package-structure.md` 五节，本地化文件按 mod 目录隔离，键里**不重复命名空间**
    （Fluent 标识符不允许冒号）。

### F. `ll-sim` 决策层

21. ★ **`RecipeCatalog`/`RecipeRule` 依赖倒置**——trait 定义在 `ll-sim`，
    由 `ll-mod` 实现。`ll-sim` **不能**反向依赖 `ll-mod`，这是既有架构约束
    （`crates/ll-sim/src/quest.rs` 模块文档完整记录了 `QuestCatalog` 为此存在的理由）。
    协调者的清单里没有这一条，但它是「新内容类型要被结算读到」的必经环节。
22. `Intent::Craft` 变体 + `resolve_craft`（五节）。
23. `craft_progress_effects` + `CraftUnlockRule`（八节）。
24. `SubclassUnlockTrigger::ItemsCrafted` ——**依赖 `subclass-system.md` 的 `SubclassUnlock`
    本身落地**，那部分至今零实现。制作系统可以先落地而不接这一条，副职进度后补。

### G. ADR 0018 的真实 mod 脚本证据

25. 需要一个真实 mod 脚本注册配方。**`mods/lostland/` 不存在**（一节①已核实），
    在迁移落地前证据只能落在 `mods/example_mod/gameplay.scm`——它已经有
    `register-item-use-effect` 的先例可以紧挨着放。
    **迁移落地后，本体配方应当成为「按新流程加一个内容类型」的第一个验证案例**：
    十节六个函数没有任何一个需要 Rust 侧特权，全部可从 mod 脚本调用。

### H. 输入层——★ 本设计最大的未接线缺口，必须如实标注

26. **`Intent::Craft` 目前没有任何产出者。** 配方全部注册好、`resolve_craft` 全部写好之后，
    玩家**仍然没有任何方式提交一次制作**——没有制作界面，`action-capability-and-input-context.md`
    的 `UiMode` 模式栈是纯设计零实现。这与 `Intent::LearnSkill` 的处境完全相同
    （`subclass-system.md` 二节已核实：「学习技能」这个动作在真实玩法路径里还没有落点）。
    **务实的落地顺序是：先把 22~23 与一个最小的占位提交路径（例如验收 demo 里直接构造
    `Intent::Craft`）接通，UI 另批。** 不标注这一条，会出现「全部接线做完但玩家玩不到」的
    验收落差。

---

## 十二、被否决的方案

- **锻造/裁缝/炼金各建一套配方类型与结算**——二节用 ADR 0021 逐条检验五种「看起来专属」的差异，
  全部落在「同样的算法、不同的数据」上；拆开会把验证/扣减/产出复制四份。
- **`RecipeDef.category` 用封闭 Rust 枚举**——违反 ADR 0018，mod 加「木工」要改本体代码；
  两张已落地的类别表当初正是为避开这一点才做成开放注册表。
- **从 `ingredients` 的材质反推配方类别**——四节：需要一个不存在的 `ItemDef.material` 字段
  加一段新算法（为避免一个数据字段而发明一段算法，方向反了）、混合材料无定义良好的答案、
  mod 作者无法预测。
- **副职闸门闸在每条配方上**——七节：几十条配方各自声明，新增副职要逐条改；且会让八节的
  计数键空间从「类别数」膨胀到「配方数」。
- **把副职闸门塞进 `register-recipe-category` 的参数列表**——七节：照 `skill-requires!` 六节，
  分类展示与强制闸门是两件独立的事。
- **把 `station`/`tool` 塞进 `register-recipe` 的位置参数**——十节：两个可选参数会逼每条普通
  配方传两个空串哨兵；`register-item-damage-category` 是更贴切的先例。
- **工具判定成「背包里有」**——六节：没有代价（不占槽位）、与「工具」的物理直觉不符、
  且会让将来 `resolve_mine` 要么跟着错要么两套判定并存。
- **工具判定只比 `def` 不看耐久**——六节：会出现「锤子烂了还能打铁」，与
  `resolve.rs:344`「耐久归零 = 不可用」的既有语义直接矛盾。
- **制作计数照抄 `kill_count_key` 的索引数值键**——八节/一节②：`script_state` 不参与存档
  重映射，会继承一处已知的静默失真。
- **给 `RecipeDef` 加 `action_cost_multiplier` 表达「打铁很久」**——五节：没有可中断的多回合
  活动机制，做出来是「原地消失 20 回合」这个明显错误的行为。
- **现在做失败判定/品质分档/技艺浮动**——九节⑤⑥⑦各自的理由（缺前提、跨 crate 独立批次、
  与「副职不给数值」的既有裁定直接冲突）。
- **给 `StatTarget` 加抗性变体让衣服抗火**——项目所有者已裁定抗性有四个来源（天赋/装备/药品/
  技能），是一个要动 `TraitTable`/`ItemDef`/`SkillDef`/buff 存储/聚合点的跨切面批次，
  单独派工。本文档只记依赖，见十四节④。
- **温度/保暖系统**——一节④核实季节零玩法后果，做保暖等于从零造温度机制，远超制作系统范围。

---

## 十三、四个验收示例（每类一个，覆盖全部字段组合）

数值均为示意，非最终曲线（同 `resource-pools-and-rest.md` 十一节先例）。

```scheme
;; ── 类别与闸门 ───────────────────────────────────────────
(register-recipe-category "lostland:cooking"  "lostland:recipe_category_cooking_display_name")
(register-recipe-category "lostland:forging"  "lostland:recipe_category_forging_display_name")
(register-recipe-category "lostland:tailoring" "lostland:recipe_category_tailoring_display_name")
(register-recipe-category "lostland:alchemy"  "lostland:recipe_category_alchemy_display_name")

;; 烹饪刻意不设闸门——food-and-cooking-system.md 五节「不设解锁门槛」原样成立
(recipe-category-requires-subclass! "lostland:forging"   "lostland:artisan")
(recipe-category-requires-subclass! "lostland:tailoring" "lostland:tailor")
(recipe-category-requires-subclass! "lostland:alchemy"   "lostland:herbalist")

;; ① 烹饪：无类别闸、无场地、无工具——完全等价于食物系统九节的烤肉示例，
;;    证明本文档的三个新字段全部可选、不破坏那份文档已定形的东西
(register-recipe "lostland:roast_meat_recipe" "lostland:roast_meat_recipe_display_name"
                 "lostland:cooking"
                 (list (recipe-ingredient "lostland:raw_meat" 1))
                 "lostland:roast_meat" 1)

;; ② 锻造：有类别闸 + 场地 + 工具——三条前置全开的最复杂路径
(register-recipe "lostland:iron_sword_recipe" "lostland:iron_sword_recipe_display_name"
                 "lostland:forging"
                 (list (recipe-ingredient "lostland:iron_ingot" 2)
                       (recipe-ingredient "lostland:leather_strip" 1))
                 "lostland:iron_sword" 1)
(recipe-requires-station! "lostland:iron_sword_recipe" "lostland:forge_floor")
(recipe-requires-tool!    "lostland:iron_sword_recipe" "lostland:smithing_hammer")

;; ③ 裁缝：有类别闸 + 工具，无场地——针线随身带，野外就能缝；
;;    成品走 StatTarget::Attribute 加敏捷（四节的裁缝价值轴）
(register-recipe "lostland:linen_shirt_recipe" "lostland:linen_shirt_recipe_display_name"
                 "lostland:tailoring"
                 (list (recipe-ingredient "lostland:linen_cloth" 3))
                 "lostland:linen_shirt" 1)
(recipe-requires-tool! "lostland:linen_shirt_recipe" "lostland:sewing_needle")

;; ④ 炼金：有类别闸 + 场地，无工具；product-count 为 2，覆盖"一次产出多份"
(register-recipe "lostland:healing_potion_recipe" "lostland:healing_potion_recipe_display_name"
                 "lostland:alchemy"
                 (list (recipe-ingredient "lostland:red_herb" 2)
                       (recipe-ingredient "lostland:spring_water" 1))
                 "lostland:healing_potion" 2)
(recipe-requires-station! "lostland:healing_potion_recipe" "lostland:alchemy_bench")

;; ⑤ 九节④的白拿变化度：同一件铁剑的第二条配方，用废铁而不是铁锭。
;;    register-recipe 刻意不校验 product 唯一性，这条因此合法。
(register-recipe "lostland:iron_sword_from_scrap" "lostland:iron_sword_from_scrap_display_name"
                 "lostland:forging"
                 (list (recipe-ingredient "lostland:scrap_iron" 3)
                       (recipe-ingredient "lostland:charcoal" 1))
                 "lostland:iron_sword" 1)
(recipe-requires-station! "lostland:iron_sword_from_scrap" "lostland:forge_floor")
(recipe-requires-tool!    "lostland:iron_sword_from_scrap" "lostland:smithing_hammer")
```

**四个示例覆盖的组合**：无前置（①）、三条全开（②）、有工具无场地（③）、有场地无工具（④）、
同成品多配方（⑤）、`product_count > 1`（④）。**没有任何一个示例需要本文档之外的新能力。**

---

## 十四、与既有设计文档的冲突（指出，不代所有者决定）

### ① 「菜谱全部已知」vs「科研」——一处真实冲突，需要所有者裁定

`food-and-cooking-system.md` 五节明确裁定：**「MVP 阶段，任何角色只要凑齐食材、提交
`Intent::Craft`，就能做出对应菜谱——不需要『学会』这一步」**，并把 `known_recipes` 字段
列为将来扩展。项目所有者后来提到的「科研」方向，本质就是给配方加解锁门槛——**两条直接冲突。**

**本文档没有解决这个冲突，也没有回避它，而是刻意把两件事拆开了**：

| | 是什么 | 本文档的立场 |
|---|---|---|
| **类别访问权**（七节） | 你是不是工匠 | **做**。读已落地的 `Agent.subclasses`，零新增字段 |
| **配方解锁**（`known_recipes`） | 你知不知道这张图纸 | **不做**。需要新 `Agent` 字段 + 新 `Effect` + 存档 `ContentKind::Recipe` |

**这个拆分意味着**：即使将来做「科研」，它是在 `known_recipes` 那条线上加东西，
**不需要回头改本文档定形的任何部分**——`resolve_craft` 那时多一句
「若配方声明需要解锁，检查 `agent.known_recipes.contains(recipe)`」即可（食物系统五节已给出
这条演进路径）。**但「科研」与食物系统五节裁定的冲突本身仍然存在，需要项目所有者决定是
推翻五节、还是把科研限定在非食物类别上。本文档不代为决定。**

### ② `subclass-system.md` 一节候选表「工匠/锻造」行 + 六节排序第 8 位

八节已完整论证，**本次一并修订该文档**（见十七节改动清单）。

### ③ `subclass-system.md` 一节候选表「厨艺」行

「副职收益只剩技能闸，价值比炼金弱一档」——七节的类别闸门让这条部分失效，**一并修订**。

### ④ `buffs-and-triggers.md` 7.3 与八节表格第 5 行已过期——**不改，只上报**

两处都写着抗性卡在「`TraitTable` 整套零实现」。核实：`TraitDef`/`RuleModifier::Resistance`/
`ll_sim::traits::resistance_multiplier_permille` **均已落地**
（`crates/ll-mod/src/trait_def.rs:26` 模块文档原文：「现在有真实消费者」）。
抗性现在卡的是**别的东西**：物品给不了（`StatTarget` 只有 `Attribute`/`Armor`）、技能给不了
（`SkillDef` 无 `rule_modifiers`）、药品的限时抗性会撞上 `active_stat_modifiers` 按属性做键的
存储限制。**那是「多来源抗性聚合」批次的事，不归本文档**——本文档只在四节记一条依赖：
制作系统产出的衣物/药品将来要能授予抗性，补上之后裁缝与炼金会更有区分度。

### ⑤ `food-and-cooking-system.md` 四节/八节的场地需求裁定——**本文档明确推翻**

那份文档四节判「YAGNI 不做」、八节把「现在就加场地需求字段」列为被否决方案。
**六节推翻了这条**，理由是四类制作统一这个决定本身产生了当时不存在的需求。
**这是一次有意的翻案，不是疏忽**——如实记在这里，供将来读那份文档的人对照。
那份文档的其余全部结论（食材/成品不建新类型、`Intent::Craft` 不复用 `Intent::Use`、
零新增 `Effect`、`ConsumeInventoryItem` 不加 `amount`、菜谱不设解锁门槛）**本文档全部沿用，
一条未改**。

### ⑥ `README.md`「落地状态速览」记录过期

`subclass-system.md` 零节已经指出该速览把 `SubclassDef`/`QuestNodeDef` 记成「纯设计」是过期的。
本文档写作时该速览仍未更正。**本文档只更新自己那一行索引，不越权改写速览。**

---

## 十五、将来扩展（明确不在本文档范围）

- **采矿 / 种植 / 采集**——六节的 `required_tool` 是它们将来的接入点（「拿着镐子才能挖」），
  但这三个动作本身各自需要 `Intent::Mine`/`Intent::Plant` 与对应的世界状态改动，不在本文档。
- **修理**——`subclass-system.md` 一节核实过它不存在。它与制作共用「消耗材料」这一半，
  但产出端是「恢复某件已有装备的 `durability`」而不是「产出新物品」，**是一份不同的算法**
  （ADR 0021 判据：不该硬塞进 `resolve_craft`），值得单独的 `Intent::Repair`。
- **耐久扩大到防具/衣物**——六节末尾与四节末尾两处不对称的共同根因，需所有者裁定。
- **物品/药品/技能授予抗性**——十四节④，跨切面批次。
- **温度 / 保暖**——一节④核实季节零玩法后果（只影响光照），做保暖等于从零造温度机制。
  **衣服目前确实缺了「保暖」这层意义**，如实记在这里。
- **多回合可中断制作**（九节⑨）、**副产物**（⑧）、**失败判定**（⑤）、**品质分档**（⑥）、
  **技艺浮动**（⑦）——各自理由见九节。
- **配方解锁 / 科研**——十四节①，与食物系统五节的冲突待所有者裁定。
- **食材腐败 / 保质期**——食物系统十节已标为将来扩展，本文档沿用。

---

## 相关文档

- `knowledge/design/food-and-cooking-system.md`——`RecipeDef`/`RecipeIngredient`/`Intent::Craft`/
  `resolve_craft` 的原始形状，本文档三、五节直接沿用并只在三处加宽（`category`/`required_station`/
  `required_tool`）；十四节⑤记录本文档对其场地需求裁定的明确推翻
- `knowledge/design/subclass-system.md`——`SubclassUnlock`/`SubclassUnlockTrigger` 的原始形状，
  本文档八节新增第四个变体并订正其一节候选表两行与六节排序
- `knowledge/design/skill-learn-requirements.md`——`skill-requires!` 独立注册的先例，七节副职闸门
  为什么不塞进 `register-recipe-category` 的直接依据
- `knowledge/design/damage-formula-mod-api.md` 十七、二十一节——`register-weapon-category`/
  `register-damage-category` 两张开放类别表的原始设计，四节类别表手法的来源
- `knowledge/design/item-system.md`——`ItemDef`/耐久「归零 = 损坏不可用」，六节工具判定谓词的依据
- `knowledge/design/mod-package-structure.md` 五节——本地化文件按 mod 目录隔离，十一节 E 组依据
- `knowledge/design/buffs-and-triggers.md` 7.3 / 八节表格——十四节④指出其记录已过期
- `crates/ll-mod/src/weapon_category.rs` / `damage_category.rs` / `script_weapon_category_api.rs`
  ——四节与十一节 A 组照抄的落地先例
- `crates/ll-sim/src/quest.rs:229` `kill_progress_effects`——八节成长挂钩的直接复制来源，
  以及一节②记录的 `kill_count_key` 存档隐患所在
- `crates/ll-content/src/remap.rs:343`——`script_state: _` 不重映射，一节②/十一节 D 组的依据
- `crates/ll-mod/src/content_hash.rs`——十一节 B 组全部环节；
  `crates/ll-game/src/content.rs:535` `Opaque` 覆盖率回归测试
- `scripts/ci/check_field_consumers.py`——十一节 C 组；`check_content_hash_gate_cross_coverage()`
  是两道门禁的互校
- `knowledge/decisions/0016`/`0017`——十节档位判据；`0018`——四节反对封闭枚举、十一节 G 组；
  `0020`——十节浮点分区核对；`0021`——二节统一判定的判据来源；
  `0022`/`0027`——十一节 B 组哈希完整性
