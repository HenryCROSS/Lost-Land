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
#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    /// 环面世界坐标。
    pub pos: TorusPos,
    /// 六项主属性。
    pub stats: BaseStats,
    /// 下次行动的世界时刻，时间轴排序依据。
    pub next_action_at: Tick,

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
