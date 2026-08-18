//! 角色六项主属性的字段布局。
//!
//! 完整的属性系统（调整值公式、三系攻防、穿透、幸运、次级属性）冻结在
//! `knowledge/design/attribute-system.md`，实现阶段是 P3（战斗结算）与
//! P5（职业技能树）。本任务只建 P3 建 [`crate::entity::Agent`] 时必须
//! 已经存在的字段布局——具体的伤害/判定公式属于后续批次。

/// 六项主属性。全部整数，理由见 `attribute-system.md` 开篇「所有数值
/// 一律整数」。
///
/// 基础属性硬上限 30（装备与临时效果可以突破，见该文档「成长上限」
/// 一节），但那是 P5 装备系统要执行的规则，本类型自身不做范围校验——
/// 校验属于装备结算的职责，不是字段布局本身的不变式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BaseStats {
    /// 力量：物理攻击、负重上限。
    pub strength: i32,
    /// 敏捷：时间轴速度、闪避、命中。
    pub dexterity: i32,
    /// 体质：生命上限、抗性、耐力。
    pub constitution: i32,
    /// 智力：魔法攻击、法力、学习速度。
    pub intelligence: i32,
    /// 意志：精神攻防、抵抗、视野半径。
    pub willpower: i32,
    /// 魅力：招募随从、交易议价、随从士气。
    pub charisma: i32,
}

impl BaseStats {
    /// 六项主属性均取「调整值为零」的基准点（10）——`(10 − 10) / 2 = 0`，
    /// 见 `attribute-system.md` 的调整值公式。用作背景 NPC 升格
    /// （[`crate::entity::ThinPopulation::promote`]）时的默认属性：薄层
    /// 本就不追踪逐项属性，升格时给一个不偏不倚的起点，好过任意选一个
    /// 具体数值却假装它有出处。
    pub const BASELINE: BaseStats = BaseStats {
        strength: 10,
        dexterity: 10,
        constitution: 10,
        intelligence: 10,
        willpower: 10,
        charisma: 10,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 基准属性的六项均为十() {
        // Arrange & Act
        let stats = BaseStats::BASELINE;

        // Assert
        assert_eq!(
            [
                stats.strength,
                stats.dexterity,
                stats.constitution,
                stats.intelligence,
                stats.willpower,
                stats.charisma,
            ],
            [10; 6]
        );
    }

    #[test]
    fn 序列化往返后属性值不变() {
        // Arrange
        let original = BaseStats {
            strength: 14,
            dexterity: 12,
            constitution: 16,
            intelligence: 8,
            willpower: 11,
            charisma: 9,
        };

        // Act
        let json = serde_json::to_string(&original).expect("BaseStats 全字段均为整数，必可序列化");
        let decoded: BaseStats = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }
}
