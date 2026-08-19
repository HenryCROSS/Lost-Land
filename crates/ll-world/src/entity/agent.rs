//! 厚层实体：被真正模拟的 `Agent`。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};

use crate::script_state::ScriptValue;
use crate::space::Space;

use super::{Affiliation, BaseStats, Goal};

/// 厚层实体：数百个，有界，被真正模拟。
///
/// 行式排布（AoS）是刻意的：数量少、按实体随机访问、一次要读它的全部
/// 字段，行式排布比列式更优——与薄层 [`crate::entity::ThinPopulation`]
/// 用列式排布不是不一致，是各自匹配访问模式（见该类型文档）。
///
/// # 可派生 `serde`（P5 批次 B 补齐，偿还两处历史债务）
///
/// 曾经不派生 `serde`：`pos` 是 [`TorusPos`]、`profession`/`race` 是
/// [`ContentIndex`]，两者当时在 `ll-core` 里都没有可直接使用的序列化
/// 实现。两处障碍现在都已解除：
///
/// 1. `TorusPos` 已在两级坐标系重写批次补上不依赖世界尺寸上下文的直接
///    `Serialize`/`Deserialize`（`ll-core/src/torus.rs`）——它本身没有
///    需要跨字段校验的不变式（任意一对 `(x, y)` 只要落在构造它的
///    `TorusSize` 环内就是合法值，反序列化只做结构转换）。
/// 2. `ContentIndex` 同样直接派生（[0015](../../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)）：
///    「结构合法」与「已注册」是两件事，前者无上下文、可以直接派生；
///    后者依赖当前会话加载了哪些 mod，是一次独立的解析，不能也不该
///    塞进 serde 的 `try_from`——`ll_core::ident` 模块文档「为什么可以
///    直接派生」一节详述了这条判断。反序列化出的 `ContentIndex` 是否
///    对应当前会话里一条真实注册的内容，由拿到注册表之后的调用方
///    （存档主体读写管线，见任务 9）显式解析，解析失败即是「缺失 mod」
///    的检测点——不是本类型自身要负责的事。
///
/// 因此 `Agent` 现在可以直接派生：全部字段要么是纯整数/字符串组合
/// （`health`/`wallet`/`luck`/`Vec<Affiliation>`/`Vec<Goal>`），要么是
/// 已经各自补齐直接派生的复合类型（`TorusPos`/`BaseStats`/
/// `ContentIndex`/`Space`）。
///
/// # 为什么 `health` 是 `Agent` 的字段，而不是 `WorldState` 的旁挂表
///
/// 生命值和 `pos`/`stats`/`wallet` 一样，是逐实体状态，天然属于
/// `Agent`——厚层行式存储本就是为「一次要读某个实体的全部字段」这种
/// 访问模式而设计的（见本类型文档开篇）。曾经有一版把它做成
/// `WorldState` 上的 `BTreeMap<EntityId, i32>` 旁挂表，图的是不改
/// `Agent` 既有布局；但这引出两个问题：
///
/// 1. 实体状态从此有两个存放地（`Agent` 本体 + 旁挂表），后续任何新增
///    字段都要面对「这个该放哪」的选择，维护者必须记住这条历史包袱。
/// 2. 旁挂表不受 [`super::Arena`] 的世代号管辖：实体被 `despawn` 之后，
///    `Arena` 里对应的槽位立刻变成 `Vacant`，但旁挂表里的条目不会跟着
///    消失，除非另外手动清理——一旦某次 `Effect` 忘了同步删除，旁挂表
///    就会攒下一批指向不存在实体的孤儿记录。这不是假设的风险：这正是
///    上一批实现里 `WorldState::health` 字段被发现的真实缺陷（该字段
///    存档可见但 `Arena` 的生死不参与序列化，往返后孤儿记录会一直
///    累积）。把生命值做成 `Agent` 的字段后，这类记录随 `Agent` 一起
///    被 `Arena::despawn` 整体收走，物理上不可能出现孤儿。
///
/// **如果日后又想把它挪回 `WorldState` 的旁挂表**——不要。以上两条
/// 理由没有过期；旁挂表能做的事，`Agent` 字段都能做，且不引入孤儿
/// 记录的风险。
///
/// # 为什么只存「当前值」，不存「上限」
///
/// 生命上限由体质（`stats.constitution`）驱动，是衍生属性（见
/// `knowledge/design/attribute-system.md` 「衍生属性绝不进存档」
/// 一节）：完整的 `derive_stats` 公式属于战斗结算批次，本批次不提前
/// 设计它。若现在就把上限存成字段，`stats` 一旦变化（升级、装备、
/// buff）就必须记得同步这个字段，否则两者不同步——这正是该原则要
/// 防的缺陷类别。因此这里只留「当前生命值」这一个存档需要的字段；
/// 上限留到 `derive_stats` 落地时按 `stats` 现算，不占字段、不需要
/// 同步。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    /// 环面世界坐标。
    pub pos: TorusPos,
    /// 六项主属性。
    pub stats: BaseStats,
    /// 下次行动的世界时刻，时间轴排序依据。
    pub next_action_at: Tick,
    /// 当前生命值。可以为零或负——是否算「死亡」是规则判断（`resolve`
    /// 的职责），`apply` 只管照 `Effect::Damage` 的数字做加减，不在这
    /// 个字段本身设下限。没有独立的「上限」字段，见本类型文档「为什么
    /// 只存当前值」一节。
    pub health: i32,

    // ↓ 以下六个字段 P3 可以留空，但字段必须现在就有——见
    // `knowledge/design/society-and-affiliation.md` 第五节与
    // `knowledge/design/agent-goals-and-economy.md` 第九节：存档格式在
    // P5 冻结，P3 加是零成本，P8 加要写迁移链。
    /// 归属列表（势力/宗教/行会/文化/家族/职业）。
    pub affiliations: Vec<Affiliation>,
    /// 钱包，最小货币单位。厚层直接存值，不像薄层那样走公式。
    pub wallet: i64,
    /// 当前主职业，指向注册表。
    pub profession: ContentIndex,
    /// 目标栈。
    pub goals: Vec<Goal>,
    /// 种族，指向注册表。
    ///
    /// 与 `profession` 同样的模式：种族是内容（mod 可以注册新种族），
    /// 因此用 [`ContentIndex`] 而不是一个封闭的本体枚举——`fireball`
    /// 那类命名冲突问题（见 `ll_core::ident` 模块文档）在种族上同样
    /// 存在，不该单开一套不经注册表的表示法。
    ///
    /// 未来将驱动的东西：基础属性的种族修正（矮人体质高、精灵敏捷高
    /// 这类刻板但好用的差异化）、种族专属的可穿装备槽位（见
    /// `knowledge/design/equipment-slots.md`）、随从招募与关系系统里
    /// 「同族/异族」这层筛选、以及部分剧情/势力对话分支的解锁条件。
    /// P3 阶段不消费这个字段，只建布局。
    pub race: ContentIndex,
    /// 当前所在的空间（任务 12：两级坐标系重写）。
    ///
    /// 默认（新生成的实体）是 `Space::Surface`——地表是唯一不需要「先
    /// 存在一个具体实例」就能站上去的空间，`Interior` 必须先经由
    /// `InteriorTable` 插入一个实例才谈得上「进入哪一个」。
    ///
    /// # 为什么不是「`pos` 之外的第二份位置真相源」
    ///
    /// `pos: TorusPos` 恒是这个实体在世界地图上的坐标，不因为进入
    /// `Interior` 而改变（设计文档「内部移动不改变世界地图坐标」，见
    /// [`crate::interior`] 模块文档）——`Interior` 内部的移动是另一个
    /// 坐标系（[`ll_core::bounded::BoundedPos`]），本批次不引入对应的
    /// 「楼层内位置」字段，见 `ll-sim::resolve` 模块文档「`Interior`
    /// 内部移动的范围边界」一节：本批次只接线进出，不接线内部漫游。
    /// `current_space` 因此只回答「这个实体此刻应该用哪一套地形/FOV/
    /// 相机」，不重复记录位置。
    ///
    /// # 唯一写入口仍是 `apply`（C1）
    ///
    /// 与 `pos` 一样，这个字段只能通过
    /// `ll_sim::apply::apply` 对 `Effect::ChangeSpace` 的响应写入,不得
    /// 在渲染/输入层直接赋值。
    pub current_space: Space,
    /// 幸运。
    ///
    /// **刻意放在 `Agent` 而非 [`BaseStats`]**：`BaseStats` 的六项主
    /// 属性统一走 `(属性 − 10) / 2` 的调整值公式（见
    /// `knowledge/design/attribute-system.md` 「六、次级属性」前的调整
    /// 值公式一节），而幸运走的是「每点 +5‰」的原始值语义（同文档
    /// 「五、幸运」一节）——两套公式形状不同。把幸运塞进 `BaseStats`
    /// 会让那个结构里出现一个不遵守自身公式的异类字段，后人照着
    /// `BaseStats` 里其余六项的调整值公式去套用幸运，必然算错。
    ///
    /// 未来将驱动的东西（均详见 `attribute-system.md` 「五、幸运」）：
    /// 暴击率（每点 +5‰）、优势掷骰（每满 20 点多掷一次取较优）、掉落
    /// 品质权重、稀有事件触发权重。P3 阶段不消费这个字段，只建布局。
    pub luck: i32,
    /// 每实体脚本状态：依附于这个具体实体的脚本存储（NPC 当前在追的
    /// 目标、某个技能的冷却计时……），键是 `(mod_namespace, key)`，见
    /// `knowledge/design/script-state-storage.md` 二、四节。
    ///
    /// # 为什么随 `Agent` 走，不开一张 `WorldState` 旁挂表
    ///
    /// 与本类型文档「为什么 `health` 是 `Agent` 的字段」一节同一条
    /// 理由：`Agent` 死亡时 `Arena::despawn` 会把整个槽位连同这个字段
    /// 一起收走，物理上不会产生孤儿记录——若做成旁挂表，`Effect::Kill`
    /// 一旦忘了同步清理，就会重演 `WorldState::health` 曾经的孤儿记录
    /// 缺陷（见该节历史记录）。设计文档六、7 节讨论的「孤儿状态」专指
    /// **mod 被卸载**这一种情况（数据仍应保留），不是「实体死亡」这
    /// 一种（数据理应随实体一起消失）——两者是完全不同的生命周期，
    /// 这里选用 `Agent` 字段而非旁挂表，保证的正是后一种「实体死亡
    /// 即回收」不需要额外代码维护。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5 禁止 `HashMap`/`HashSet` 的
    /// 迭代顺序参与逻辑判断，序列化/摘要遍历需要确定顺序，见设计文档
    /// 五、1 节。**序列化走 [`crate::script_state::serde_map`]**——理由
    /// 见该模块文档（JSON 等文本格式要求 map 键是字符串，元组键不
    /// 满足）。
    #[serde(with = "crate::script_state::serde_map")]
    pub script_state: BTreeMap<(String, String), ScriptValue>,
}

impl Agent {
    /// 生成/升格新实体时的占位起始生命值。
    ///
    /// 真正的生命上限应由 `stats.constitution` 经 `derive_stats` 算出
    /// （见本类型文档「为什么只存当前值」一节），但那个公式属于战斗
    /// 结算批次，本批次不提前设计。新实体总要有一个非零起点——不然
    /// 刚生成就是「零血」，第一下判定就会触发死亡——因此这里给一个
    /// 明确标记为占位的常量，供 [`crate::entity::ThinPopulation::promote`]
    /// 等构造路径统一使用，公式落地后只需替换这一处。
    pub const STARTING_HEALTH: i32 = 100;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{AffiliationKind, OrgRef};
    use crate::space::Space;
    use ll_core::ident::{Interner, NamespacedId, WorldId};

    /// 构造一个全字段都非默认值的 `Agent`——包括非空的 `affiliations`/
    /// `goals`，确保往返测试真的会经过 `Vec` 分支，而不是恰好因为空
    /// `Vec` 序列化恒等于自身而掩盖真正的编解码缺陷。
    fn fully_populated_agent() -> Agent {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:blacksmith").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:dwarf").expect("合法标识符"));
        let culture =
            interner.intern(NamespacedId::parse("lostland:mountain").expect("合法标识符"));
        let goal_kind = interner.intern(NamespacedId::parse("lostland:trade").expect("合法标识符"));
        let mut world_id_counter = 7u32;
        let zone = ll_core::torus::TorusSize::new(48, 32)
            .expect("48x32 是合法尺寸")
            .wrap(3, 5);

        Agent {
            pos: ll_core::torus::TorusSize::new(64, 64)
                .expect("64x64 是合法尺寸")
                .wrap(10, 20),
            stats: BaseStats {
                strength: 14,
                dexterity: 12,
                constitution: 16,
                intelligence: 8,
                willpower: 11,
                charisma: 9,
            },
            next_action_at: Tick(42),
            health: 77,
            affiliations: vec![Affiliation {
                kind: AffiliationKind::Culture,
                org: OrgRef::Def(culture),
                standing: 250,
            }],
            wallet: 12345,
            profession,
            goals: vec![Goal {
                kind: goal_kind,
                params: vec![1, -2, 3],
                progress: 500,
                priority: 3,
            }],
            race,
            current_space: Space::Interior {
                id: WorldId::next(&mut world_id_counter),
                floor: -2,
                anchor: zone,
                profile: ContentIndex::default(),
            },
            luck: 9,
            script_state: BTreeMap::from([(
                ("lostland".to_string(), "cooldown".to_string()),
                ScriptValue::Int(5),
            )]),
        }
    }

    #[test]
    fn agent序列化往返后全部字段逐一相等() {
        // Arrange
        let original = fully_populated_agent();

        // Act
        let encoded = serde_json::to_string(&original).expect("全部字段均已可派生序列化");
        let decoded: Agent = serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert：Agent 派生了 PartialEq，逐字段比较（含 current_space）
        // 由这一个断言覆盖，不需要逐个字段单独断言。
        assert_eq!(decoded, original);
    }
}
