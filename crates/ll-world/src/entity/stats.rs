//! 角色六项主属性的字段布局。
//!
//! 完整的属性系统（调整值公式、三系攻防、穿透、幸运、次级属性）冻结在
//! `knowledge/design/attribute-system.md`，实现阶段是 P3（战斗结算）与
//! P5（职业技能树）。本任务只建 P3 建 [`crate::entity::Agent`] 时必须
//! 已经存在的字段布局——具体的伤害/判定公式属于后续批次。

use ll_core::time::Tick;

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

/// 六项主属性的枚举形式——供职业「主属性倾向」、技能「临时属性修正」
/// 等需要「指定某一项属性」而非「持有一份完整 [`BaseStats`]」的场景
/// 使用（P5-B `knowledge/design/class-skill-quest-system.md` 第一节
/// `ClassDef::primary_attribute`、第五节 `SkillEffect::TemporaryStatModifier`
/// 的落点）。
///
/// [`BaseStats`] 回答「这个实体的六项数值分别是多少」，`AttributeKind`
/// 回答「指的是六项里的哪一项」——两者服务不同的场景，并存不冲突。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AttributeKind {
    /// 力量：物理攻击、负重上限。
    Strength,
    /// 敏捷：时间轴速度、闪避、命中。
    Dexterity,
    /// 体质：生命上限、抗性、耐力。
    Constitution,
    /// 智力：魔法攻击、法力、学习速度。
    Intelligence,
    /// 意志：精神攻防、抵抗、视野半径。
    Willpower,
    /// 魅力：招募随从、交易议价、随从士气。
    Charisma,
}

/// 一条正在生效的临时属性修正——技能效果
/// （`SkillEffect::TemporaryStatModifier`，见
/// `knowledge/design/class-skill-quest-system.md` 第五节）落到具体实体
/// 上的实例状态，P5-B 任务 5 新增。
///
/// # 惰性到期判定，不存「当前是否生效」
///
/// 只存「到期时刻」与「修正量」这两个静态量，不存一个可以现算出来的
/// 布尔值——与 [`crate::entity::Agent::skill_cooldowns`] 同一条纪律
/// （见其字段文档），也是 `buffs-and-triggers.md` 一、惰性到期判定的
/// 直接落点：真正要读「这个属性当前的有效修正量」的调用方（衍生属性
/// 计算，P3/P5 之后落地）在读取的那一刻自行比对世界时钟与 `expires_at`
/// ，本类型自身不做任何判断，也不主动清理过期条目（同一条「有意留给
/// 后续阶段的缺口」，见 `Agent::skill_cooldowns` 文档）。
///
/// # 堆叠策略固定为「刷新持续时间」
///
/// `Agent::active_stat_modifiers` 按 [`AttributeKind`] 做键——同一项
/// 属性同一时刻只能有一条生效的修正，再次对同一属性施加修正（无论是
/// 同一个技能重复释放,还是另一个技能修正了同一项属性）会直接覆盖旧
/// 条目，这就是 `buffs-and-triggers.md` 五、`StackPolicy::RefreshDuration`
/// （本计划固定选用的唯一堆叠策略,见关键设计判断 4）在数据结构层面的
/// 体现，不需要额外的判断逻辑。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveStatModifier {
    /// 增减量，可为负——与技能效果 `SkillEffect::TemporaryStatModifier`
    /// 里的 `amount` 同一个数值,技能释放那一刻原样抄进来（完整形状见
    /// `knowledge/design/class-skill-quest-system.md` 第五节；本 crate
    /// 不依赖 `ll-mod`（依赖方向 `ll-world` ← `ll-sim` ← `ll-script` ←
    /// `ll-mod`，规格 §5），这里只是引用文档说明来源,不是可解析的代码
    /// 内链接）。
    pub delta: i32,
    /// 到期时刻——世界时钟达到或超过这个值时，这条修正视为已失效。
    pub expires_at: Tick,
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

    #[test]
    fn 属性种类序列化往返后不变() {
        // Arrange
        let original = AttributeKind::Willpower;

        // Act
        let json = serde_json::to_string(&original).expect("枚举变体必可序列化");
        let decoded: AttributeKind = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn 不同属性种类不相等() {
        // Arrange & Act & Assert
        assert_ne!(AttributeKind::Strength, AttributeKind::Dexterity);
    }
}
