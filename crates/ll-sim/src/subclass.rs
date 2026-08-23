//! 副职的授予/放弃与「使用计数」获得机制——`knowledge/design/subclass-system.md`
//! 四、五节在代码里的落点。
//!
//! # 本模块存在的理由：`Agent::subclasses` 此前没有任何写入路径
//!
//! `ll_world::entity::Agent::subclasses` 从 P5-B 起就是 `WorldState` 的
//! 一部分，`crate::resolve::resolve_craft` 的副职闸门（第③步）每次制作
//! 都要读它，`ll_content::remap::remap_subclasses` 也早就为它写好了存档
//! 重映射——**但在本模块落地之前，全代码库没有任何一处往它里面写过东
//! 西**（唯一的命中全部是测试夹具里的 `Vec::new()`）。直接后果是：
//! `08cdeb0` 落地的 `RecipeCategoryDef::required_subclasses` 闸门在结构
//! 上等价于「凡是声明了副职要求的配方类别，谁都做不了」——闸门是真
//! 代码，但门后没有任何一把钥匙。本模块补上钥匙。
//!
//! # 为什么授予必须是独立的 [`Effect`] 变体（ADR 0023）
//!
//! `Agent::subclasses` 属 `WorldState`，**不属 `Agent::script_state`**。
//! ADR 0023「脚本状态写入必须经 `apply`」的同一条纪律在这里意味着：这
//! 个写入不能塞进 [`Effect::SetScriptState`]（那一条只写 `script_state`
//! 那张表），必须是 [`Effect::GrantSubclass`]/[`Effect::RemoveSubclass`]
//! 两个独立变体，由 `crate::apply::apply` 写进 `Agent::subclasses`。
//!
//! # 唯一出口：[`grant_subclass_effects`]（ADR 0021）
//!
//! 设计文档四节点名了三条**将来都会存在**的授予路径：使用计数达标、
//! 任务节点奖励、世界生成/职业注册时写死的初始副职（NPC 唯一可行的
//! 路径——NPC 从不提交 `Intent::Craft`，使用计数对它们结构上无效）。
//! 三条路径共享的**不只是一个效果变体**，而是一整段算法：查行动者是否
//! 存在、去重、上限检查。ADR 0021 判据「有算法可共享」在这里成立（它
//! 的**反向**那半——「拦住把同一份算法复制三遍」——同样成立），因此
//! 三条路径统一走 [`grant_subclass_effects`] 这一个函数，不各写一遍。
//!
//! 本批次真正接线的只有第一条（使用计数）。另外两条各自缺的是别的东
//! 西（任务奖励缺 `QuestReward` 这个尚不存在的概念；初始副职缺
//! `Effect::SpawnActor`/职业注册时的初始状态通道），**不是缺这个出口**
//! ——它们落地时不需要再造第二个函数。
//!
//! # 计数键：**不**照抄 `crate::quest::kill_count_key`
//!
//! `kill_count_key` 把 `ContentIndex::get()` 的**数值**拼进脚本状态键，
//! 而 `crates/ll-content/src/remap.rs` 的 `remap_agent` 对 `script_state`
//! 是 `script_state: _`（明确不参与存档重映射）。两条合起来是一处真实
//! 隐患：玩家增删 mod 导致索引重编号之后，那些键会静默指向别的内容。
//! 这是击杀计数**既有**的隐患（不在本批次的修复范围内，改它要一并解决
//! 存量存档的迁移），但新造的计数不该再抄一遍——**副职的全部计数键一
//! 律用 [`ll_core::ident::NamespacedId`] 的字符串形式拼**
//! （`"craft_count:lostland:forging"`），命名空间标识符跨 mod 集合变化
//! 保持稳定，天然免疫重编号。**这条纪律对将来新增的每一种副职计数都
//! 生效，不只制作。**
//!
//! 这不需要给计数函数传一份 `Registry`：需要反查的 `NamespacedId` 由
//! [`SubclassUnlockCatalog`] 在**注册期**一次性解析好，随规则一起返回
//! ——与 [`crate::quest::QuestCatalog::kill_count_quests`] 把任务的
//! `NamespacedId` 一并带出来是同一个手法。
//!
//! # 约束核对
//!
//! - **C1**：本模块只产出 `Vec<Effect>`，一个字节的世界状态都不写。
//! - **C3**：全程零随机。
//! - **C5**：唯一的遍历对象是 [`SubclassUnlockCatalog::craft_unlocks`]
//!   返回的 `Vec`（保序），不遍历任何 `HashMap`/`HashSet`。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::entity::{Agent, EntityId};
use ll_world::script_state::{ScriptStateTarget, ScriptStateWrite, ScriptValue};
use ll_world::state::WorldState;

use crate::effect::Effect;

/// 一个角色最多能同时持有多少个副职。
///
/// # 为什么必须有上限，以及为什么是这个量级
///
/// 设计文档五节给了两条互相独立的理由，指向同一个小整数区间 2~3：
///
/// 1. **玩法**：「build 多样性来自直觉搭配 vs 错位搭配的化学反应」这条
///    价值主张的前提是副职**稀缺**。使用计数机制跑得足够久，没有上限
///    的角色理论上能集齐全部副职，那时「法师配驯兽」不再是一个需要
///    取舍的选择，只是时间问题，取舍张力消失。
/// 2. **工程**：`crate::traits::agent_trait_sources` 的返回值长度是
///    「2 + 副职数」。上限让它保持在小常数量级，是它将来能从定长数组
///    平滑长成 `Vec` 而不引入逐帧分配的前提。
///
/// 取区间上端 3 而不是 2：2 意味着「主职 + 两个副职」，第三个副职是
/// 玩家做出第一次**真正取舍**（放弃一个换一个）的位置；取 2 会让取舍
/// 来得太早，玩家还没体验过任何一个副职就要开始放弃。具体数值是内容
/// 设计参数，改它不影响本模块任何一行逻辑。
pub const MAX_SUBCLASSES: usize = 3;

/// 一条「累计在某个配方类别里制作满 `threshold` 次，就获得 `subclass`」
/// 的声明——[`SubclassUnlockCatalog`] 的查询结果。
///
/// # 为什么同时携带 `category` 与 `category_id`
///
/// `category` 用来与「这次制作的是哪个类别」比对（一次整数相等），
/// `category_id` 用来拼计数键（见模块文档「计数键」一节）。两者在
/// **注册期**一起解析好随规则带出，运行期因此既不需要 `Registry`
/// 反查，也不需要把 `ContentIndex` 的数值拼进任何持久化的键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftUnlockRule {
    /// 达标后授予哪个副职，指向 `SubclassTable`。
    pub subclass: ContentIndex,
    /// 数哪个配方类别的制作次数，指向 `RecipeCategoryTable`。
    pub category: ContentIndex,
    /// 上一字段对应的完整标识符——计数键的来源，见类型文档。
    pub category_id: NamespacedId,
    /// 需要累计多少次，恒 ≥ 1（注册期校验）。
    pub threshold: u32,
}

/// `resolve` 依赖的最小「副职获得条件来源」接口。
///
/// # 为什么只有制作这一种触发器
///
/// 设计文档四节订正后列了三个触发器变体
/// （`ItemsCrafted`/`ItemsGathered`/`RestsTaken`）。本批次**只落地
/// 制作这一种**，另外两种的判据不是「懒」而是内容前置不成立：
///
/// - `ItemsGathered(物品类别)` 指向的「物品类别」**在代码里不存在**：
///   `ll_mod::item::ItemDef` 的字段是
///   `id`/`display_name_key`/`stack_limit`/`base_weight`/`base_price`/
///   `max_durability`/`equip_mask`/`stat_bonuses`（外加抗性/穿透等追加
///   列），**没有任何一个是「类别」**。挂载点（`resolve_pick_up`/
///   `resolve_loot`）确实已落地，缺的是这个变体要指向的内容表本身——
///   造一张「物品类别表」是一整个独立批次，不是本批次顺手能做的事。
/// - `RestsTaken` 的挂载点（`resolve_rest`）已落地且不需要任何新内容
///   表，但它**当前没有任何消费者**：本批次注册的四个本体副职全部是
///   制作类（见 `mods/lostland/subclasses.json5`），求生副职不在名册里。
///   为一个没有消费者的触发器造变体是 ADR 0021 点名要避免的那种抽象。
///
/// 因此本 trait 现在只有一个方法、[`CraftUnlockRule`] 也不套一层只有
/// 一个变体的 `enum SubclassUnlockTrigger`——第二种触发器落地那天，把
/// 这个方法改成返回一个带 `enum` 的规则列表是一次机械改写，而**现在**
/// 就预先造出那个 `enum` 只会多出一处永远走不到的 `match` 分支。
pub trait SubclassUnlockCatalog {
    /// 返回全部「制作计数」类的副职获得条件，任意确定顺序。
    ///
    /// 与 [`crate::quest::QuestCatalog::kill_count_quests`] 完全同构：
    /// 返回全部规则、由 [`craft_progress_effects`] 自己按类别过滤，而
    /// 不是让目录按类别查询——规则总数是「副职数量」这个小量级，一次
    /// 线性过滤比给注册表再维护一份反向索引便宜得多。
    fn craft_unlocks(&self) -> Vec<CraftUnlockRule>;
}

/// 空的副职获得条件目录：不知道任何获得条件。
///
/// 是不接这一路的调用方的默认实现，理由同 [`crate::quest::NoQuests`]。
/// 效果是制作永远不推进任何副职计数——注意这与「计数推进了但拿不到
/// 副职」不同：**一条计数写入都不会产生**，见 [`craft_progress_effects`]
/// 文档「没有规则就一个字节都不写」一节。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSubclassUnlocks;

impl SubclassUnlockCatalog for NoSubclassUnlocks {
    fn craft_unlocks(&self) -> Vec<CraftUnlockRule> {
        Vec::new()
    }
}

/// 制作计数的存储命名空间——理由同 `crate::quest` 的击杀计数命名空间：
/// `lostland` 是本体的既有保留命名空间，制作计数可以看成「本体额外提供
/// 的一项引擎级统计」。
///
/// **注意它与计数键里那个命名空间不是一回事**：本常量是脚本状态存储的
/// 外层隔离命名空间（引擎级统计一律落在本体名下），键字符串里那个是
/// **被计数的配方类别自己的**命名空间（`"craft_count:examplemod:cooking"`
/// 里的 `examplemod`）。第三方 mod 注册的类别，计数同样落在本体命名
/// 空间下、键里带着它自己的完整 id——与击杀计数把全部种类的计数都放在
/// `lostland` 下是同一个结构。
const CRAFT_COUNT_NAMESPACE: &str = "lostland";

/// 制作计数键前缀。
const CRAFT_COUNT_KEY_PREFIX: &str = "craft_count:";

/// 给定配方类别的完整标识符，返回它在脚本状态存储里对应的计数键。
///
/// 拼的是**完整标识符字符串**而不是 `ContentIndex` 的数值——见模块
/// 文档「计数键」一节的完整论证。
pub fn craft_count_key(category: &NamespacedId) -> String {
    format!("{CRAFT_COUNT_KEY_PREFIX}{category}")
}

/// 读取 `agent` 当前在某个配方类别上的累计制作次数，未写入过时为 0。
fn craft_count(agent: &Agent, category: &NamespacedId) -> i64 {
    match agent
        .script_state
        .get(&(CRAFT_COUNT_NAMESPACE.to_string(), craft_count_key(category)))
    {
        Some(ScriptValue::Int(n)) => *n,
        _ => 0,
    }
}

/// 三条授予路径共用的判据：现在能不能把 `subclass` 授予这个角色。
///
/// `held` 是角色**已经持有**的副职，`pending` 是同一批效果里已经决定要
/// 授予、但 `apply` 还没写下去的那些——两者都要算进上限，否则同一批
/// 结算里同时达标的两条规则会各自看到「还差一个槽位」而双双放行，把
/// 副职数写到 [`MAX_SUBCLASSES`] 之上。
fn can_grant(held: &[ContentIndex], pending: &[ContentIndex], subclass: ContentIndex) -> bool {
    if held.contains(&subclass) || pending.contains(&subclass) {
        // 去重：`Agent::subclasses` 是 `Vec` 不是集合，重复授予让
        // `contains` 判定仍然正确，但存档里会多出一份纯垃圾。
        return false;
    }
    held.len() + pending.len() < MAX_SUBCLASSES
}

/// **三条授予路径的唯一出口**（见模块文档「唯一出口」一节）：把
/// `subclass` 授予 `actor`，两道闸门全过才产出一条
/// [`Effect::GrantSubclass`]，否则空列表。
///
/// 1. 行动者存在于世界里（不存在时静默返回空，与本 crate 全部既有
///    `resolve_*` 的同一条纪律）；
/// 2. 尚未持有这个副职（去重）；
/// 3. 持有数尚未达到 [`MAX_SUBCLASSES`]（上限）。
///
/// # 上限超出时是「拒绝」，而且**不吞掉任何东西**
///
/// 照 `crate::resolve::resolve_allocate_attribute_point` 刚落地的先例：
/// 属性到达硬上限时它拒绝加点、且**点数原样保留**，玩家可以改加别的
/// 属性；钳位式的「加了但没加上」等于凭空吞掉玩家的点数。副职这一路
/// 的对应物是**计数**——
///
/// **达到上限时被拒绝的是授予，不是计数。** [`craft_progress_effects`]
/// 恒先产出计数写入、再决定要不要授予；上限拒绝只丢掉授予那一条，计数
/// 照常累加。因此玩家在满员状态下继续制作，进度**不会白做**：一旦经
/// `Effect::RemoveSubclass` 放弃掉一个副职腾出槽位，下一次在该类别里
/// 制作时判据是 `累计次数 >= threshold`（不是 `== threshold`），当场
/// 达标、当场授予。
///
/// **这条语义有一个如实标注的边界**：腾出槽位之后必须**再制作一次**
/// 才会补发，不会在放弃的那一瞬间自动补齐全部已达标的副职。理由是
/// 「达标检查挂在动作上」这条结构本身（与击杀计数完全一致），要做成
/// 放弃时自动补发就得让 `Effect::RemoveSubclass` 的结算反过来遍历全部
/// 获得条件——那是把一条「移除」效果变成一次全表扫描，且会让「放弃
/// 副职」这个玩家动作产生玩家没有要求的副作用。
pub fn grant_subclass_effects(
    world: &WorldState,
    actor: EntityId,
    subclass: ContentIndex,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    if !can_grant(&agent.subclasses, &[], subclass) {
        return Vec::new();
    }
    vec![Effect::GrantSubclass { actor, subclass }]
}

/// 制作结算的副职进度：`actor` 刚刚成功完成了一次 `category` 类别的
/// 制作之后，应该产出的效果——累计次数 +1，以及任何因此达标、且通过
/// [`can_grant`] 两道闸门的副职授予。
///
/// 结构逐条照抄 [`crate::quest::kill_progress_effects`]（那是本仓库
/// 已经跑通一次的「计数 + 达标授予」实现），只把「达标时标记任务完成」
/// 换成「达标时授予副职」。
///
/// # 没有规则就一个字节都不写
///
/// 与击杀计数的一处**刻意差异**：`kill_progress_effects` 对每一次击杀
/// 都无条件写一条计数，本函数只在**存在至少一条指向这个类别的获得
/// 条件**时才写。两条理由：
///
/// 1. 计数键的 `NamespacedId` 来自规则本身（见模块文档「计数键」
///    一节）——没有规则就没有 id，写不出键，也不该为了写一条没人读的
///    计数而反过来要求调用方传一份 `Registry`。
/// 2. `Agent::script_state` 进存档。给一个谁都不关心的类别逐次写计数，
///    是往每个玩家的存档里堆永远不会被读到的字节。
///
/// **代价如实标注**：mod 作者事后才给某个类别加上获得条件时，玩家在
/// 那之前的制作次数不算数，进度从零开始。这是可接受的——内容变更本来
/// 就不追溯（同一条道理见 `remap_subclasses` 对解析不到的索引直接丢弃），
/// 且「装了新 mod 之后进度从零开始」比「装了新 mod 之后凭空拿到一个
/// 副职」更符合玩家预期。
///
/// # 同一批里可能授予多个副职
///
/// 两个副职声明同一个类别、阈值不同（例如 10 次给「学徒工匠」、50 次
/// 给「大师工匠」）是合法内容。因此 `pending` 随循环增长，上限判据看
/// 的是「已持有 + 本批待授予」的总数，见 [`can_grant`] 文档。
pub fn craft_progress_effects(
    world: &WorldState,
    actor: EntityId,
    category: ContentIndex,
    unlocks: &dyn SubclassUnlockCatalog,
) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    let rules: Vec<CraftUnlockRule> = unlocks
        .craft_unlocks()
        .into_iter()
        .filter(|rule| rule.category == category)
        .collect();
    let Some(first) = rules.first() else {
        return Vec::new();
    };

    // 全部命中规则的 `category_id` 必然相同（它们按 `category` 过滤
    // 出来，而注册期保证同一个 `ContentIndex` 只对应一个标识符），
    // 因此计数键取第一条即可，且这一批只写**一条**计数。
    let new_count = craft_count(agent, &first.category_id) + 1;
    let mut effects = vec![Effect::SetScriptState {
        writes: vec![ScriptStateWrite {
            target: ScriptStateTarget::Entity(actor),
            mod_namespace: CRAFT_COUNT_NAMESPACE.to_string(),
            key: craft_count_key(&first.category_id),
            value: ScriptValue::Int(new_count),
        }],
    }];

    let mut pending: Vec<ContentIndex> = Vec::new();
    for rule in &rules {
        if new_count >= i64::from(rule.threshold)
            && can_grant(&agent.subclasses, &pending, rule.subclass)
        {
            pending.push(rule.subclass);
            effects.push(Effect::GrantSubclass {
                actor,
                subclass: rule.subclass,
            });
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::Interner;

    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    #[test]
    fn 计数键用完整标识符字符串而不是索引数值() {
        // 这一条守的是模块文档「计数键」那节的纪律本身：键里出现的是
        // 命名空间标识符，换 mod 集合导致索引重编号时它不会漂移。
        // Arrange
        let category = NamespacedId::parse("lostland:forging").expect("合法");

        // Act
        let key = craft_count_key(&category);

        // Assert
        assert_eq!(key, "craft_count:lostland:forging");
    }

    #[test]
    fn 不同类别产出不同的计数键() {
        // Arrange
        let forging = NamespacedId::parse("lostland:forging").expect("合法");
        let cooking = NamespacedId::parse("lostland:cooking").expect("合法");

        // Act & Assert
        assert_ne!(craft_count_key(&forging), craft_count_key(&cooking));
    }

    #[test]
    fn 已持有的副职不会被重复授予() {
        // Arrange
        let mut interner = Interner::new();
        let artisan = index(&mut interner, "lostland:artisan");

        // Act & Assert
        assert!(!can_grant(&[artisan], &[], artisan));
    }

    #[test]
    fn 同一批里已决定授予的副职不会被第二条规则再授予一次() {
        // Arrange
        let mut interner = Interner::new();
        let artisan = index(&mut interner, "lostland:artisan");

        // Act & Assert
        assert!(!can_grant(&[], &[artisan], artisan));
    }

    #[test]
    fn 达到上限后拒绝授予新副职() {
        // Arrange：恰好装满。
        let mut interner = Interner::new();
        let held: Vec<ContentIndex> = (0..MAX_SUBCLASSES)
            .map(|n| index(&mut interner, &format!("lostland:held_{n}")))
            .collect();
        let another = index(&mut interner, "lostland:another");

        // Act & Assert
        assert!(!can_grant(&held, &[], another));
    }

    #[test]
    fn 本批待授予的副职同样占用上限槽位() {
        // 若上限只看 `held`，同一批里同时达标的两条规则会双双放行，
        // 把副职数写到 MAX_SUBCLASSES 之上。
        // Arrange：已持有 MAX-1 个，本批已决定再授予 1 个。
        let mut interner = Interner::new();
        let held: Vec<ContentIndex> = (0..MAX_SUBCLASSES - 1)
            .map(|n| index(&mut interner, &format!("lostland:held_{n}")))
            .collect();
        let pending = vec![index(&mut interner, "lostland:pending")];
        let another = index(&mut interner, "lostland:another");

        // Act & Assert
        assert!(!can_grant(&held, &pending, another));
    }

    #[test]
    fn 空目录的获得条件恒为空列表() {
        // Arrange & Act & Assert
        assert!(NoSubclassUnlocks.craft_unlocks().is_empty());
    }
}
