//! 组织实例：势力、宗教、行会、家族等在世界生成期间被创造出来的具体
//! 个体，与它所属的 Def（若有）分开。
//!
//! 完整判据冻结在 `knowledge/design/identity-and-ids.md` 二：**mod 定义
//! 「种类」，世界生成造「个体」。** 「铁匠」这个职业类型全世界只有一份
//! 定义、走 [`ContentIndex`]；「卡拉克第三王朝」这个具体势力是世界生成
//! 器造出来的个体、数量随世界规模增长、走 [`crate::entity`] 之外定义的
//! [`ll_core::ident::WorldId`]（详见该文档「三、`WorldId` 的两条规则」）。

use ll_core::ident::{ContentIndex, NamespacedId, WorldId};

/// 一个组织实例——势力、宗教、行会、家族等在世界生成期间被创造出来的
/// 具体个体。
///
/// 不派生 `serde`：`def` 里的 [`ContentIndex`] 依赖 mod 加载顺序、
/// `ll_core::ident` 模块文档明确写着不可持久化，理由同
/// [`crate::entity::Goal::kind`]。真正持久化需要先把 `def` 解析回
/// [`NamespacedId`] 字符串再重新登记，属于内容注册表的存档格式，不在
/// 本任务范围内。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgInstance {
    /// 一切实例都有：这是它在存档、事件日志、脚本查询里被引用的方式。
    /// 永不复用——王朝覆灭后历史事件仍要能解析回它。
    pub id: WorldId,
    /// 源自哪个 mod 模板；纯生成（没有对应模板，例如自然形成的王国）
    /// 则为 `None`。
    pub def: Option<ContentIndex>,
    /// 只有 mod 直接命名定义的具体实例才有——例如 mod 直接定义「铁血
    /// 兄弟会」这个特定势力，而不是一种势力类型，见
    /// `knowledge/design/identity-and-ids.md` 「四、mod 可以定义具体
    /// 势力」。区别于 `def`：`def` 指向「造出这个实例所依据的模板」，
    /// `authored` 指向「mod 为这个具体实例本身起的名字」，两者可以
    /// 同时为 `Some`，也可以一个有一个没有。
    pub authored: Option<NamespacedId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_id(counter: &mut u32) -> WorldId {
        WorldId::next(counter)
    }

    #[test]
    fn 纯生成实例的authored字段为空() {
        // 世界生成器自己造出的势力，没有对应的 mod 具名定义。
        // Arrange
        let mut counter = 0u32;
        let id = dummy_id(&mut counter);

        // Act
        let org = OrgInstance {
            id,
            def: None,
            authored: None,
        };

        // Assert
        assert_eq!(org.authored, None);
    }

    #[test]
    fn mod命名实例的authored字段区分于纯生成实例() {
        // mod 直接命名定义的具体势力（例如「铁血兄弟会」）在世界生成时
        // 播种进世界，authored 字段记录它源自哪个具名 mod 定义。
        // Arrange
        let mut counter = 0u32;
        let id = dummy_id(&mut counter);
        let named = NamespacedId::parse("lostland:iron_blood_brotherhood").expect("合法标识符");

        // Act
        let org = OrgInstance {
            id,
            def: None,
            authored: Some(named.clone()),
        };

        // Assert
        assert_eq!(org.authored, Some(named));
    }
}
