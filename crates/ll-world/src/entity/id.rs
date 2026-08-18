//! 泛用标识符：世代索引与家族号。

/// 泛用世代索引：标识任意实体存储（厚层 [`super::Arena`]、薄层
/// [`super::ThinPopulation`]）里的一个槽位。
///
/// **世代索引解决悬垂 ID**：实体死亡后槽位被复用，旧 ID 因世代号不匹配
/// 而查询失败，而不是静默指向新实体。回合制里尤其重要——时间轴队列
/// 可能残留已死实体的条目，若查询悬垂 ID 静默返回复用后的新实体，
/// 一个本该失效的旧行动会打在完全无关的对象上。
///
/// 两层各自维护自己的 `(索引, 世代)` 分配——同一个 `EntityId` 值分别喂给
/// 厚层与薄层是两次独立查询，互不相关，如同同一个整数可以是两张不同
/// 哈希表各自的 key。
///
/// # `Ord`/`serde` 派生（P3 批次 B 补齐）
///
/// 与 [`FamilyId`] 同理：`EntityId` 没有不变式——任意 `(index,
/// generation)` 对都是结构上合法的值（一个悬垂的 `EntityId` 本身就是
/// 有意义的合法状态，不是需要拒绝的非法输入），因此可以像 `FamilyId`
/// 那样直接派生，不需要 `#[serde(try_from = "…")]` 中转校验。
///
/// 新增的原因：`ll-sim` 的时间轴队列（`TimelineEntry`）与 `Intent`
/// 都需要把 `EntityId` 存进可完整序列化的结构体（存档要能把整条队列
/// /整条 Intent 流写出去，见规格 §4「确定性重放」），而 `EntityId`
/// 的字段私有、构造函数 `new` 又是 `pub(crate)`——下游 crate 唯一能
/// 拿到 `EntityId` 的方式是通过 `Arena::spawn`/`ThinPopulation::spawn`
/// 这类公开入口，无法在反序列化时凭空重建一个。派生宏在本文件内
/// 展开，可以直接访问私有字段，因此这里补上派生就是唯一可行、且不
/// 触碰任何既有方法实现的最小改动。`Ord` 用于 `BTreeMap<EntityId, _>`
/// 这类需要确定性迭代顺序的容器（规格 C4：禁止 `HashMap` 迭代顺序参与
/// 逻辑判断），比较顺序是 `(index, generation)` 字典序——一个任意但
/// 稳定的全序，不必也不需要与 [`Self::as_u64`] 的打包顺序一致。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// 构造标识符。仅供本 crate 内的存储实现使用——外部只应从
    /// `spawn` 类方法拿到 `EntityId`，不应自行拼装。
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        EntityId { index, generation }
    }

    /// 槽位下标。
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// 世代号。
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// 打包成一个稳定的 `u64`，供需要单一整数标识的场合使用（例如
    /// `ll_render::sprite::DrawOrder` 的 `entity` 排序键）。
    ///
    /// 下标占低 32 位、世代占高 32 位：两者合起来才是「同一个实体」，
    /// 只暴露下标会让不同世代的两个实体在排序上撞号。
    pub const fn as_u64(&self) -> u64 {
        (self.index as u64) | ((self.generation as u64) << 32)
    }
}

/// 家族标识：指向某个家族的稳定编号。
///
/// 与 [`EntityId`] 分开成独立类型，是因为家族不是「一个槽位里的实体」，
/// 而是姓氏、财产继承、声望连带这些机制共享的分组键——见
/// `knowledge/design/society-and-affiliation.md` 「家族与代际」一节。
/// 用不同类型而不是复用 `u32`，是为了让「把实体号错当家族号传」这种
/// 参数错位在编译期就报错，而不是运行时算出一个无意义的姓氏。
///
/// 没有不变式（任意 `u32` 都是合法的家族号，包括尚未被任何实体使用的
/// 号——「家族号存在」与「家族当前有成员」是两回事），故可以直接派生
/// `Deserialize`，不需要像 [`crate::entity::ThinPopulation`] 那样中转
/// `TryFrom` 做校验。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct FamilyId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相同下标不同世代的标识不相等() {
        // 悬垂 ID 检测的前提：世代号必须参与相等性判断。
        // Arrange
        let first = EntityId::new(3, 0);
        let second = EntityId::new(3, 1);

        // Act & Assert
        assert_ne!(first, second);
    }

    #[test]
    fn 打包的整数区分不同世代() {
        // Arrange
        let first = EntityId::new(3, 0);
        let second = EntityId::new(3, 1);

        // Act & Assert
        assert_ne!(first.as_u64(), second.as_u64());
    }
}
