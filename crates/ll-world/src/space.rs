//! `Space`：统一地表与离散空间的接口。
//!
//! # 为什么不能用单一 `(i, j, z)` 索引
//!
//! 一栋建筑的二楼，和它旁边空地正上方的「z = 1」，不是同一种东西：
//! 前者是一个与地表占地无关、可以任意大小的独立房间序列；后者（如果
//! 地表真的支持垂直分层）应当仍是连续地表的一部分。硬把两者塞进同一个
//! `(i, j, z)` 索引，会带出「站在空地上按上楼键该发生什么」这类没有
//! 答案的伪问题。见
//! `knowledge/design/coordinate-system-and-layers.md` 四节。
//!
//! [`Space`] 因此是一个枚举：[`Space::Surface`] 是全局连续无缝的地表，
//! [`Space::Interior`] 是各自独立、锚定在地表某一格的有界局部空间
//! （地下城、洞窟、建筑内部）。本模块只落地这个类型的形状，**不接入
//! `WorldState`**——`Space` 何时、如何成为世界状态的一部分是后续任务
//! 的范围。

use ll_core::ident::{ContentIndex, WorldId};
use ll_core::torus::TorusPos;

/// 区块坐标：与世界瓦片坐标（[`TorusPos`]）同一个类型，喂给区块粒度的
/// `TorusSize` 即得。
///
/// 不新增坐标类型，只是同一个类型在不同分辨率下的另一种叫法——区块
/// 坐标 = 世界瓦片坐标 ÷ 区块边长（整数除法），纯函数派生，不是第二个
/// 真相源。`TorusPos` 本身只是「一对被规范化的 `i32`」，不携带「这是
/// 瓦片还是区块」的标签，见设计文档三节。
pub type ZoneCoord = TorusPos;

/// 空间实例的持久标识：复用 `identity-and-ids.md` 已经定案的
/// [`WorldId`]，不发明新 ID 空间。
///
/// 建筑、地下城、洞窟实例与聚落、势力、家族是同一类东西——「世界生成器
/// （或建造玩法）造出来的、没有 mod 注册条目可供反序列化校验、数量随
/// 世界规模增长」的实例，适用 `identity-and-ids.md`「类型/实例分离」
/// 一节的判据：类型（`SpaceProfile` 的注册表条目，见 [`crate::space_profile`]）
/// 走 `ContentIndex`，实例（某一栋具体的房子、某一处具体的地下城入口）
/// 走 `WorldId`。
pub type SpaceId = WorldId;

/// 玩家/实体所处的一个具体空间：连续地表的一格区块，或一个独立的
/// 离散内部空间。
///
/// 两个变体都携带 `profile: ContentIndex`，指向 [`crate::space_profile`]
/// 注册表里的一条层属性定义——环境光基准、是否露天、温度基准等，见
/// [`crate::space_profile::SpaceProfile`]。两个变体的 `profile` 字段
/// 类型完全一致，调用方可以用同一个查表函数处理，不需要按变体分支。
///
/// # 可直接派生 `serde`（P5 批次 B，随 `Agent::current_space` 一起补齐）
///
/// `zone`/`anchor` 是 [`TorusPos`]、`id` 是 [`SpaceId`]（即 `WorldId`）、
/// `profile` 是 [`ContentIndex`]——三者都已各自补齐无上下文的直接序列化
/// 实现（见 [`crate::entity::Agent`] 模块文档「可派生 `serde`」一节），
/// 因此本枚举可以直接派生，不需要 `try_from` 中转。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Space {
    /// 露天层：连续地表的一格区块。地形来自世界尺度连续噪声场的窗口
    /// 采样，见设计文档五节。
    Surface {
        /// 区块坐标。
        zone: ZoneCoord,
        /// 预留字段。
        ///
        /// # 为什么恒为 0，为什么还留着这个字段
        ///
        /// 当前没有任何玩法要求地表本身垂直分层（悬崖多级台地、树屋
        /// 一类需求不在本次重写范围内）。留这个字段只是给「未来若真的
        /// 需要贴着地表的多层露天结构」预留一个不需要改枚举形状的
        /// 扩展口——这不是现在就要设计的功能，触发条件是出现具体玩法
        /// 需求,届时再决定 `z ≠ 0` 时地形怎么来。**当前设计下这一维
        /// 平凡地不需要稀疏索引**：只有 `z = 0` 一个取值存在，存储上
        /// 区块-层直接按区块坐标索引即可,不需要额外套一层 `z` 的稀疏
        /// 映射（这与下方 `Interior` 的存在性稀疏是两个不同的问题，
        /// 见设计文档六节「稀疏性：拆成两条」）。
        z: i8,
        /// 指向 [`crate::space_profile`] 注册表的层属性。
        profile: ContentIndex,
    },
    /// 建筑内部 / 地下城 / 洞窟：各自独立生成，与地表占地无关。地图
    /// 尺寸由内容决定，不受该空间在地表占据的格数限制，见设计文档四节
    /// 「已裁定：建筑内部不受地表占地面积限制」。
    Interior {
        /// 这个空间实例的持久标识。
        id: SpaceId,
        /// 楼层号，允许负数（地下室、地下城更深层）。
        ///
        /// # 不环绕——这条边界必须写死并解释
        ///
        /// 区块坐标（[`ZoneCoord`]）本身构成一个环面，是瓦片坐标环绕
        /// 的直接推论；`floor` 恰恰相反：从入口层出发向上/向下延伸，
        /// 有顶有底（由具体建筑/地下城的生成内容决定，不是全局统一的
        /// 上下限），走到最底层**不会**绕回最高层。这条规则反直觉，
        /// 必须写死：若「顺手」让 `floor` 也环绕，会出现「从最深的
        /// 地下城掉回天上」这类荒谬结果。见设计文档六节「环面在 i/j
        /// 上闭合，z 不闭合」。
        floor: i16,
        /// 锚点：这个空间在世界地图上显示为哪一格。
        ///
        /// # 单一真相源在这里，不在世界格子那一侧
        ///
        /// 锚点信息只存在 `Interior` 自己身上，不在世界格子那一侧另存
        /// 一份「这里有哪些空间入口」的反向列表——那样的反向索引只能是
        /// 从这个字段现算或缓存的**派生视图**，绝不能被单独编辑,否则
        /// 一旦两份数据不同步，就会重演白昼判定（ADR 0010）与
        /// `Affiliation.org`（`identity-and-ids.md`）已经付过代价的
        /// 「同一个概念被独立定义两次」缺陷。谁需要反向查询，去现算或
        /// 建一份单向更新的缓存,不要另起一份可以独立写入的存储。见
        /// 设计文档四节「锚定关系：单一真相源在哪一侧」。
        anchor: TorusPos,
        /// 指向 [`crate::space_profile`] 注册表的层属性。
        profile: ContentIndex,
    },
}

impl Space {
    /// 取出该空间的层属性索引，两个变体走同一条路径。
    ///
    /// 这正是两个变体都携带同名同类型 `profile` 字段的意义——调用方
    /// （光照、FOV 等消费者）不需要先判断「这是 `Surface` 还是
    /// `Interior`」才能查层属性。
    pub const fn profile(&self) -> ContentIndex {
        match self {
            Space::Surface { profile, .. } => *profile,
            Space::Interior { profile, .. } => *profile,
        }
    }

    /// 便捷构造：`z` 恒为 0 的地表空间（见 [`Space::Surface`] 文档
    /// 「为什么恒为 0」）。
    ///
    /// 供 [`crate::entity::Agent::current_space`] 的默认值、以及任务 12
    /// 结算 `Intent::ExitSpace` 时重新构造地表空间使用——两处都不需要
    /// 关心 `z` 这个当前恒为零的预留维度，写这个构造函数避免每处调用
    /// 都重复拼一遍 `z: 0`。
    pub const fn surface(zone: ZoneCoord, profile: ContentIndex) -> Space {
        Space::Surface {
            zone,
            z: 0,
            profile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;

    fn zone() -> ZoneCoord {
        TorusSize::new(48, 32)
            .expect("48x32 是合法的区块尺寸")
            .wrap(3, 5)
    }

    #[test]
    fn surface与interior变体的profile字段类型一致可以用同一个查表函数() {
        // 用同一个 profile 索引构造两个变体，验证 Space::profile 这个
        // 统一访问入口对两者都返回同样的值——调用方不需要分支处理。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let profile =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:surface").expect("合法"));
        let surface = Space::Surface {
            zone: zone(),
            z: 0,
            profile,
        };
        let mut world_id_counter = 0u32;
        let interior = Space::Interior {
            id: WorldId::next(&mut world_id_counter),
            floor: 1,
            anchor: zone(),
            profile,
        };

        // Act
        let surface_profile = surface.profile();
        let interior_profile = interior.profile();

        // Assert
        assert_eq!(surface_profile, interior_profile);
    }

    #[test]
    fn interior的floor允许负数() {
        // 地下室、地下城更深层都是负楼层号——这是有效输入，不是需要
        // 拒绝的边界情形。
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let profile =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("合法"));
        let mut world_id_counter = 0u32;

        // Act
        let interior = Space::Interior {
            id: WorldId::next(&mut world_id_counter),
            floor: -3,
            anchor: zone(),
            profile,
        };

        // Assert
        assert!(matches!(interior, Space::Interior { floor: -3, .. }));
    }
}
