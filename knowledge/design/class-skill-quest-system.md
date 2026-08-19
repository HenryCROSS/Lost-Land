# 职业 / 技能树 / 副职 / 任务系统——设计基线

**冻结于** 2026-08-19。**实现阶段** P5-B（本文档是该批任务 1 的产出）。

**落地状态**：纯设计。本文档只定四个 Def 的字段形状与它们之间的关系，
不设计任何具体游戏内容（不决定「战士」「法师」这类具体职业该有什么
数值，那是内容设计，属于后续批次）。任务 2/3 落地 `ClassDef`/
`SkillDef` 两张注册表；`SubclassDef`（任务 4）与 `QuestDef`（任务 6）
在本文档定形状，实现留给各自任务。

## 为什么这份文档必须先于任何一行实现代码

规格 §15 对职业/技能树/副职/网状任务四个系统只给了一句话——不像
`coordinate-system-and-layers.md`/`identity-and-ids.md` 那样有项目所有者
多轮往返定案的冻结设计可以直接抄。若跳过这一步、让后续任务各自决定
形状，四个系统的接口会互相不一致：例如技能树若用 `Vec<SkillDef>` 表示
前置关系，任务图却用另一套图结构，读者就要学两套心智模型来理解同一
类"前置解锁"关系。本文档把这条心智模型统一下来：**技能树与任务图是
同一种结构——有向无环图，节点存唯一真相源（前置列表），"解锁了什么"
是派生视图，不是另一份存储。**

## 一、`ClassDef` 与既有 `Agent.profession` 的关系

`Agent.profession: ContentIndex` 字段在 P3 阶段就已经建好（
`crates/ll-world/src/entity/agent.rs`），`society-and-affiliation.md` 也
已经把 `AffiliationKind::Profession` 定为"恒为 `Def`"——职业本身是
**类型**不是**实例**，与 `ContentIndex` 语义完全吻合（
`identity-and-ids.md` 的类型/实例分离判据）。

**`ClassDef` 不是给 `Agent` 添加第二套"职业"概念，而是给这个既有占位
字段配一张真正的注册表。** `Agent.profession` 存的 `ContentIndex` 此后
指向 `ClassTable` 里的一条 `ClassDef`；`Affiliation { kind:
AffiliationKind::Profession, org: OrgRef::Def(index), .. }` 这条既有的
归属记录里的 `standing` 字段继续按 `society-and-affiliation.md` 的既有
设计表示熟练度/资历，与本文档新增的 `ClassDef` 静态属性是两件不冲突
的事——一个是"这个职业类型本身的静态数值"，一个是"这个具体实体在这
个职业上的动态进度"。

```rust
pub struct ClassDef {
    /// 命名空间标识符，例如 `lostland:warrior`、`yourmod:necromancer`。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键，不存字面字符串——与 `TerrainDef` 等既有
    /// 内容类型一致，UI 文案属于本地化系统的职责,不是内容注册表的。
    pub display_name_key: NamespacedId,
    /// 主属性倾向：六项主属性之一,供职业选择界面展示"这个职业更看重
    /// 什么"，以及未来（P6+）职业相关的数值加成计算使用主属性作为
    /// 输入。P5 阶段只是一个展示/分类字段，不驱动任何结算逻辑。
    pub primary_attribute: AttributeKind,
}
```

`AttributeKind` 是六项主属性（`BaseStats` 的六个字段）的枚举形式,新增
于 `crates/ll-world/src/entity/stats.rs`，与 `BaseStats` 并存——
`BaseStats` 回答"这个实体的六项数值分别是多少"，`AttributeKind` 回答
"指的是六项里的哪一项"，两者服务于不同的场景（前者是数据，后者是
对数据的引用）。

## 二、`SkillDef` 的前置关系：DAG，不是线性序列

"技能树"这个名字本身暗示分支——一个技能解锁之后可能有多条后续路径
可选，线性序列（`Vec<SkillDef>` 按顺序排列，A 之后只能是 B）表达不了
分支。本文档因此采用**有向无环图**：每个 `SkillDef` 存一份"我需要哪些
前置技能"的列表，图的"哪些技能因此被解锁"是**派生视图**，不是另一份
存储（与下面第四节任务图"单一真相源"同一条纪律）。

```rust
pub struct SkillDef {
    /// 命名空间标识符。
    pub id: NamespacedId,
    /// 所属职业，`None` 表示通用技能——不专属任何职业,任何职业的角色
    /// 都能学（见第三节的命名空间裁定：主职与副职共享同一份技能
    /// 命名空间，"通用技能"因此是一个天然存在的类别,不需要为每个
    /// 职业各自复制一份"基础攻击"）。
    pub owning_class: Option<ContentIndex>,
    /// 前置技能，DAG 的边。空列表表示这是一个"起点"技能，不需要任何
    /// 前置即可学习。**必须无环**——注册期用拓扑排序校验，见任务 3。
    pub prerequisites: Vec<ContentIndex>,
    /// 冷却时长，游戏内 tick 数。
    pub cooldown_ticks: u32,
    /// 消耗的资源类型与数量。
    pub resource_cost: ResourceCost,
    /// 技能效果——纯数值,见第五节的边界裁定。
    pub effect: SkillEffect,
}

/// 技能消耗的资源类型与数量。
///
/// `Agent` 当前尚无法力/耐力这类"当前资源值"字段（`attribute-system.md`
/// 「六、次级属性」把"生命/法力/耐力"列为设计层面的次级属性，标注
/// "尚未落地"）——本类型只声明"哪种资源、消耗多少"这份静态数据,真正
/// 对照 `Agent` 当前资源值判断是否足够、以及是否需要给 `Agent` 补上
/// 对应字段，是任务 5 `resolve` 的职责，不在本文档与任务 2/3 的范围内。
pub enum ResourceCost {
    /// 不消耗任何资源——纯冷却限制的技能。
    None,
    /// 消耗法力。
    Mana(u32),
    /// 消耗耐力。
    Stamina(u32),
}
```

## 三、`SubclassDef` 与 `ClassDef` 的关系——命名空间裁定（P5-4）

**裁定：主职与副职共享同一份技能 `ContentIndex` 命名空间。** 技能就是
技能；"谁能学"（主职决定的可学习集合、副职决定的可学习集合、或某个
技能干脆是通用技能）是另一道闸，不是命名空间该管的事。

**理由**：

1. 若不共享（每个职业各自的技能各占一段独立命名空间），同一个技能
   若被两个职业共同拥有（例如"基础格挡"这类几乎所有近战职业都该有的
   技能），要么复制成两份定义（内容漂移风险——两份定义迟早会在数值
   上不同步）,要么发明一套跨命名空间的"技能引用"机制（复杂度只是从
   命名空间转移到了引用层，没有真正省掉）。
2. 不共享还会导致 mod 无法让副职复用主职技能——一个 mod 想设计"盗贼
   副职可以使用战士主职的部分技能作为副职技能树的一部分"这类玩法,
   若命名空间隔离,mod 完全没有公开 API 能表达这种复用,只能被迫复制
   一份技能定义。
3. 共享命名空间后，`SkillDef.owning_class: Option<ContentIndex>`
   本身已经足够表达"这个技能主要属于哪个职业"（展示/分类用途）,而
   "某个实体的主职/副职是否能学这个技能"是运行期判定（比对
   `Agent.profession`/`Agent.subclasses` 与技能的 `owning_class`，或者
   技能本身是 `owning_class: None` 的通用技能）,不需要命名空间层面的
   物理隔离来保证。

```rust
pub struct SubclassDef {
    /// 命名空间标识符。
    pub id: NamespacedId,
    /// 指向 Fluent 本地化键。
    pub display_name_key: NamespacedId,
}
```

`SubclassDef` 本身不需要一个独立的"技能命名空间"字段——正是因为裁定
为共享，副职技能与主职技能都只是 `SkillTable` 里普通的 `SkillDef`,
`owning_class` 若指向某个副职的 `ContentIndex`,或者副职本身选择复用
主职已有的技能（`owning_class` 直接指向主职），两种用法都直接可行,
不需要额外的命名空间字段承载这条关系。

**与规格草案的关系**：本计划的实施计划文档（
`docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md`）在撰写时把
这一点列为"待裁定",给出的保守默认是"不共享"。本文档是项目所有者与
执行者在本任务实际讨论后的最终裁定（P5-4，已记入
`.superpowers/sdd/2026-08-19-p5-gameplay-systems/progress.md`「沿用的
既有裁定」一节）：**共享命名空间**,推翻了计划文档里的保守默认。后续
任务（尤其任务 4）应遵循本文档的裁定,不是计划文档里的保守默认。

`Agent` 持有主职与副职的字段形状（`profession: ContentIndex` 与
`subclasses: Vec<ContentIndex>`）留给任务 5 定夺,不在本文档展开——
本文档只需要确认"副职与主职技能不需要命名空间隔离"这一条,`Agent`
字段本身的布局与技能命名空间是否共享无关。

## 四、`QuestNodeDef` 的网状结构——单一真相源

与技能树同一条纪律：只存"我需要哪些前置任务",不存"我完成后解锁哪些
后续任务"。后者是**派生视图**，两次调用同样的已完成集合必须返回同样
的结果（纯函数),不是另一份随时间变化的存储。

这条纪律不是本文档新发明的——`Interior.anchor`/反向索引"单一真相源"
在两级坐标系重写批次已经用过同一个模式（存一个方向,另一个方向现算),
本文档延续同一条纪律,不重新发明。

```rust
pub struct QuestNodeDef {
    /// 命名空间标识符。
    pub id: NamespacedId,
    /// 前置任务节点，单一真相源。空列表表示这是一个不需要任何前置
    /// 即可开始的任务（"起点"任务）。
    pub prerequisites: Vec<ContentIndex>,
    /// 完成条件——见任务 6，一档声明式（如"击杀 N 个某类型敌人"）或
    /// 三档脚本回调，本文档不展开分级细节,交给任务 6 的简报处理
    /// （ADR 0018 的三档分级同样适用于任务系统,但本计划不做二档
    /// 受限公式,按 YAGNI 推迟——当前已知需求下,任务完成条件要么是
    /// 简单计数（一档够用）要么是复杂逻辑（需要三档）,中间的"公式"
    /// 这一档没有明确用例）。
    pub condition: QuestCondition,
}
```

`unlocked_by(table, completed) -> Vec<ContentIndex>` 是给定已完成任务
节点集合、现算出哪些后续节点因此解锁的纯函数——不是存储,任务 6 落地。

**`QuestDef` 本批只定形状，不实现**——任务 6 才落地 `QuestTable`/
`QuestCondition`/`unlocked_by`,本文档给出的是接口草案,任务 6 实现时
可以按需微调字段名,但不应该改变"前置列表是单一真相源、解锁视图是
派生"这条核心形状。

## 五、技能效果的数值边界——P6 装备接线的硬边界

**规格 §15 把 P6「物品与装备」排在 P5（技能树）之后**——本批技能效果
只能是纯数值,不能读取任何装备槽位信息。

**允许的字段**（纯数值,`SkillEffect` 的三个变体覆盖）：

```rust
pub enum SkillEffect {
    /// 造成伤害，基础值。三系攻防、穿透、10% 下限这套完整公式
    /// （`attribute-system.md` 二~四节）已经设计好，但真实防御来自
    /// 装备,装备系统（P6）还不存在——本变体现阶段只是一个基础数值,
    /// 不接入完整的伤害结算公式,那需要等 P6 装备落地之后另行接线。
    DealDamage { base: i32 },
    /// 恢复资源（法力/耐力），基础值。
    RestoreResource { base: i32 },
    /// 临时属性修正：`duration_ticks` 个 tick 内,`attribute` 项的
    /// 有效值增减 `amount`。存"到期时刻",不存"当前是否生效"——惰性
    /// 到期判定,读取时现比对世界时钟（`buffs-and-triggers.md` 一、),
    /// 具体落点是任务 5 的 `Effect::ApplyStatModifier`。
    TemporaryStatModifier {
        attribute: AttributeKind,
        amount: i32,
        duration_ticks: u32,
    },
}
```

**明确排除的字段**（任何"读取装备槽位"的字段）：

- 不允许任何指向 `ItemDef`/装备实例的字段或引用。
- 不允许"武器攻击力""护甲值"这类需要先读装备栏才能算出的输入——
  `DealDamage.base` 是技能自身声明的固定基础值,不是"武器伤害 ×
  倍率"这类公式。
- 不允许触发完整的三系攻防结算（`attribute-system.md` 二~四节的伤害
  公式）——那套公式的"防御"来自装备,装备系统不存在,技能效果目前只能
  产出一个裸的伤害数值,由后续任务（若已落地）决定这个数值如何应用到
  目标的生命值,不在本批次展开完整结算管线。

**这条边界必须写进代码文档注释**（`crates/ll-mod/src/skill.rs` 的
`SkillEffect`/`ResourceCost` 定义处),不能只停留在本设计文档——避免
后来者在实现任务 5（`Intent::UseSkill`/`resolve`）时试图提前接装备,
那会造出一堆将来 P6 装备落地时需要返工的接口。

## 与既有架构的接线点（供任务 2/3/4/5/6 对照）

- `ClassDef`/`SkillDef` 走内容注册表模式（`ClassTable`/`SkillTable`,
  与 `TerrainTable`/`SpaceProfileTable` 同一套"私有字段 + `define` 注册
  期校验 + `materialize_base_*` 本体注册入口 + `*_fixture` 测试夹具"
  形状,见 `crates/ll-world/src/terrain.rs`/`crates/ll-world/src/space_profile.rs`
  的模块文档）,物化为按 `ContentIndex` 索引的平铺列（ADR 0016/0017）。
- 与地形/层属性不同,`ClassDef`/`SkillDef` 不依赖任何"世界空间"概念,
  因此定义本身直接落在 `ll-mod`（`crates/ll-mod/src/class.rs`/
  `crates/ll-mod/src/skill.rs`）,不像地形那样需要在 `ll-world` 定义、
  `ll-mod` 只做一层 `Registry::intern` 的薄封装——`ll-mod` 本身就是
  可以直接持有 `Registry` 的那一层,不需要为了保持 `ll-world` 不反向
  依赖 `ll-mod` 而拆成两处。
- 「本体即 Mod」检验（ADR 0016）：本体注册的职业/技能与假想 mod
  注册的职业/技能必须走完全相同的 `ClassTable::define`/
  `SkillTable::define` 公开函数——任务 2/3 各自必须有一条测试直接
  验证这一点,不能只是"看起来像"。
- 技能树的"已解锁节点集合"与"冷却到期时刻"归 `Agent` 的直接字段
  （引擎层,每回合结算都要读的高频状态）；任务进度归脚本状态存储
  （玩法层,mod 高度可扩展的状态）。这条判断记入实施计划的"关键设计
  判断 2",本文档不重复展开,只在此确认与本文档的四个 Def 形状不
  冲突——`ClassDef`/`SkillDef`/`SubclassDef`/`QuestNodeDef` 都是
  **类型**（走 `ContentIndex`,注册表存储),"解锁了哪些节点""冷却还剩
  多久""完成了哪些任务"都是**实例状态**（`Agent` 字段或脚本状态
  存储),两类状态不混在同一张表里。

## 相关文档

- [角色属性系统](attribute-system.md) —— 六项主属性、三系攻防、装备
  接线边界（本文档第五节的依据）
- [社会与归属](society-and-affiliation.md) —— `AffiliationKind::Profession`
  既有形状（本文档第一节的依据）
- [身份与标识符](identity-and-ids.md) —— 类型/实例分离判据
- [增益与触发器](buffs-and-triggers.md) —— 惰性到期判定（本文档第五节
  `TemporaryStatModifier` 的机制来源,本批只借用这一条,不实现完整
  `TriggerDef`/`StackPolicy`）
- [P5-B 实施计划](../../docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md)
