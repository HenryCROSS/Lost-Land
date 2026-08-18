//! 厚层实体：被真正模拟的 `Agent`。

use ll_core::ident::ContentIndex;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;

use super::{Affiliation, BaseStats, Goal};

/// 厚层实体：数百个，有界，被真正模拟。
///
/// 行式排布（AoS）是刻意的：数量少、按实体随机访问、一次要读它的全部
/// 字段，行式排布比列式更优——与薄层 [`crate::entity::ThinPopulation`]
/// 用列式排布不是不一致，是各自匹配访问模式（见该类型文档）。
///
/// 不派生 `serde`：`pos` 是 [`TorusPos`]、`profession` 是
/// [`ContentIndex`]，两者在 `ll-core` 里都没有（也不该有，见各自字段
/// 文档）可直接使用的序列化实现——`TorusPos` 的唯一构造路径是
/// `TorusSize::wrap`，脱离世界尺寸上下文无法在反序列化时重新校验其
/// 「恒被规范化」的不变式；`ContentIndex` 则被 `ll_core::ident` 模块
/// 文档明确标记为不可持久化。厚层的存档格式需要世界尺寸与内容注册表
/// 两项额外上下文，属于后续批次（`apply`/`resolve` 落地、存档格式在
/// P5 冻结时）要解决的问题，本任务只建字段布局。
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
#[derive(Debug, Clone, PartialEq)]
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

    // ↓ 以下四个字段 P3 可以留空，但字段必须现在就有——见
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
