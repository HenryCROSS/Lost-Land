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
/// # 现在派生 `serde`（势力播种批次）——这推翻本段的旧文字
///
/// 这里原本写着「不派生 `serde`：`def` 里的 [`ContentIndex`] 依赖 mod
/// 加载顺序、不可持久化」。**按纪律重写而不是删掉**：那条理由在今天只
/// 剩一半。[`ContentIndex`] 早已补齐了无上下文的直接
/// `Serialize`/`Deserialize`（`crate::entity::Affiliation` 的
/// [`crate::entity::OrgRef::Def`] 就在派生），「这个索引当前是否已注册」
/// 的校验留给拿到注册表之后的调用方——ADR 0015 与 0011 分工的既有落点。
///
/// 项目所有者已裁定（2026-08-29）：**「`OrgInstance` 进入存档，因为被
/// 占领后肯定会有变化的。」** 势力播种（[`crate::faction`]）因此把它
/// 放进 [`crate::state::WorldState::factions`]，随存档主体的 `postcard`
/// 一起走。
///
/// **仍然成立的那一半，如实记账**：播种出来的势力 `def`/`authored`
/// **恒为 `None`**（纯生成），因此今天存档里不存在任何一个真实的
/// `ContentIndex`。等 mod 真的定义具体势力那天（
/// `knowledge/design/identity-and-ids.md` 四），`def` 会开始携带真实
/// 索引，那时 `ll_content::remap` 必须为它补一条重映射——否则换一批
/// mod 装载顺序之后它会静默指向另一个模板。**这笔账现在就记在这里。**
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
