//! 战斗结算的纯数值公式：穿透、伤害、暴击。
//!
//! 公式冻结于 `knowledge/design/attribute-system.md` 「三、穿透属性」
//! 与「四、伤害公式」两节（规格决策 30）。本文件只实现公式本身——
//! 谁打谁、打不打得中之类的判定属于 [`crate::resolve::resolve`]，这里
//! 只提供它要调用的纯函数。
//!
//! # 暴击：公式本身不碰随机数（幸运接线批次）
//!
//! `attribute-system.md`「五、幸运」一节：「幸运不直接加伤害，它改变
//! 随机判定的形状……暴击率：每点幸运 +5‰」——[`crit_chance_permille`]
//! 是这条换算本身（幸运 → 千分比暴击率），[`apply_crit_multiplier`]
//! 是暴击命中后的伤害放大，两者都是纯函数，不掷骰、不碰 `DetRng`。
//! 真正「掷不掷得中暴击」这一步的随机判定留给
//! `crate::resolve::resolve_attack`（约束 C3：随机性必须走
//! `DetRng::for_entity`，见其调用点文档）——与本文件开篇「谁打谁、
//! 打不打得中之类的判定属于 `resolve`」同一条边界：本文件只提供
//! `resolve` 要调用的纯函数，不越界去决定「这一次到底暴不暴击」。

/// 穿透：固定值与千分比两个分量。
///
/// 见 `knowledge/design/attribute-system.md` 「三、穿透属性」：四种
/// 穿透（破甲/破魔/破意/破盾）字段形状相同，故用同一个类型表示，具体
/// 是哪一种由调用方的上下文决定，这个类型本身不区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Penetration {
    /// 固定穿透值：从防御里先减去的部分。
    pub flat: i32,
    /// 千分比穿透：固定值减完后，再按这个比例削减剩余防御。
    pub permille: i32,
}

impl Penetration {
    /// 无穿透——两个分量均为零。
    pub const NONE: Penetration = Penetration {
        flat: 0,
        permille: 0,
    };
}

/// 千分比运算的分母：`1000` 表示 `100%`。见
/// `knowledge/design/attribute-system.md` 开篇「所有数值一律整数，
/// 百分比一律用千分比」。
const PERMILLE_SCALE: i64 = 1000;

/// 伤害下限：最终伤害不得低于基础伤害的这个千分比。
///
/// 见 `attribute-system.md` 「四、伤害公式」：「10% 下限保证不会出现
/// 『完全打不动』的死局——那是单机 Roguelike 里最劝退的局面之一，
/// 玩家会直接删档」。
const DAMAGE_FLOOR_PERMILLE: i64 = 100;

/// 按防御与穿透算出最终伤害。
///
/// 公式（`attribute-system.md` 「四、伤害公式」，规格决策 30）：
///
/// ```text
/// 有效防御 = max(0, (防御 − 穿透.flat) × (1000 − 穿透.permille) / 1000)
/// 减后伤害 = max(基础伤害 × 100 / 1000, 基础伤害 − 有效防御)   // 下限 10%
/// 最终伤害 = 减后伤害 × 1000 / (1000 + 有效防御)
/// ```
///
/// # 为什么本函数把「下限」放在百分比减免**之后**而不是文档字面顺序
///
/// 设计文档按「减后伤害」「最终伤害」两行分列，字面顺序是先夹 10% 下限
/// 再乘 `1000 / (1000 + 有效防御)` 这个比例项。但字面顺序会让下限形同
/// 虚设：一旦有效防御把中间值压到下限（比如防御极高），后续那个比例项
/// 仍会继续按有效防御的大小把这个已经夹好的下限值进一步缩小——防御越
/// 高，缩得越狠，`有效防御` 足够大时最终值会被整数除法直接压到零，
/// 与文档紧接着那句「10% 下限保证不会出现『完全打不动』的死局」直接
/// 矛盾（该矛盾只在有效防御为零时才不出现，也就是下限从未真正起作用
/// 的唯一情形）。
///
/// 本实现改为：先把「基础伤害 − 有效防御」按比例项做完百分比减免，
/// **最后**才与 10% 下限取较大值。两种顺序在下限不生效的区间（有效
/// 防御不足以把伤害压过下限）结果完全一致——比例项除法对单调的输入
/// 保持单调，讨论顺序无关；只有在下限本该生效的区间，两种顺序才分道
/// 扬镳，而这里选的顺序是唯一让「10% 下限」这句话对任意防御值都成立
/// 的顺序。
pub fn damage_after_defense(attack: i32, defense: i32, pen: Penetration) -> i32 {
    let attack = i64::from(attack);
    let defense = i64::from(defense);
    let flat = i64::from(pen.flat);
    let permille = i64::from(pen.permille);

    let effective_defense =
        ((defense - flat) * (PERMILLE_SCALE - permille) / PERMILLE_SCALE).max(0);
    let mitigated =
        (attack - effective_defense) * PERMILLE_SCALE / (PERMILLE_SCALE + effective_defense);
    let floor = attack * DAMAGE_FLOOR_PERMILLE / PERMILLE_SCALE;

    mitigated.max(floor) as i32
}

/// 每点幸运贡献的暴击率加成，千分比——`attribute-system.md`「五、
/// 幸运」一节原文「暴击率：每点幸运 +5‰」，字面量直接冻结这个系数。
///
/// 没有独立的「基础暴击率」常量：设计文档只论证了幸运的**增量**效应，
/// 没有给出零幸运时的基准暴击率该是多少。取隐含基础值为零而不是自行
/// 编造一个非零基准，还有一个额外的好处——本仓库全部现存测试夹具
/// （`spawn_agent`/`test_world` 等）里的 `BaseStats::BASELINE.luck`
/// （幸运并入 `AttributeKind` 批次后的存放位置，曾经是 `Agent.luck`）
/// 恒为 `0`（见 `crates/ll-sim/tests/*.rs`/本文件下方 `resolve.rs` 测试模块的
/// 同名字面量），零幸运 ⟺ 零暴击率是唯一能保证「暴击接线批次不改变
/// 任何一条既有确定性测试的期望伤害/黄金基准哈希」的选择：非零基础
/// 暴击率会让这些原本假定「伤害必然等于 `damage_after_defense` 原始
/// 结果」的测试在某些随机流上偶然变成暴击，从确定性通过变成依赖具体
/// `(世界种子, 实体, 事件计数)` 组合是否撞上暴击的隐性赌博。
pub const LUCK_CRIT_BONUS_PERMILLE: i32 = 5;

/// 暴击命中时伤害在 [`damage_after_defense`] 结果基础上再乘的比例，
/// 千分比——`attribute-system.md`「六、次级属性」把「暴击伤害」列为
/// 独立的次级属性但未给出具体倍率，也未落地任何字段承载它。本实现
/// 取 1500‰（1.5 倍）：常见 Roguelike/RPG 默认档位，明显高于 1000‰
/// （无暴击基准）使暴击可被玩家感知，具体数值本任务不做平衡设计，
/// 只保证暴击命中后伤害确实变化——与 `RaceDef.darkvision_floor` 字段
/// 「具体数值本任务不做平衡设计，只保证字段真的被本体使用到」同一条
/// 纪律（见 `ll_mod::race::materialize_base_races` 对应注释）。
pub const CRIT_DAMAGE_MULTIPLIER_PERMILLE: i32 = 1500;

/// 给定幸运值，算出这次攻击的暴击率（千分比，夹在 `0..=1000`）。
///
/// 纯函数——不掷骰，只把幸运换算成一个概率分母，真正的随机判定留给
/// 调用方（`crate::resolve::resolve_attack`），见模块文档「暴击：
/// 公式本身不碰随机数」一节。负的幸运值（当前没有任何来源会产出，
/// 但类型上是 `i32`，理论可能来自诅咒一类未来效果）夹到零，不产出
/// 负的暴击率——`DetRng::chance` 的分子若为负会在 `as u32` 转换时
/// 环绕成一个巨大的正数，那是比「零暴击率」危险得多的隐性缺陷。
pub fn crit_chance_permille(luck: i32) -> i32 {
    (luck.max(0) * LUCK_CRIT_BONUS_PERMILLE).clamp(0, PERMILLE_SCALE as i32)
}

/// 暴击命中后按 [`CRIT_DAMAGE_MULTIPLIER_PERMILLE`] 放大伤害。
pub fn apply_crit_multiplier(damage: i32) -> i32 {
    (i64::from(damage) * i64::from(CRIT_DAMAGE_MULTIPLIER_PERMILLE) / PERMILLE_SCALE) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 防御极高时伤害仍不低于攻击力的一成() {
        // Arrange：防御刻意取到攻击力的千倍以上，逼近整数除法会把
        // 「减后伤害 × 比例项」压到零的那个区间。
        let attack = 100;
        let defense = 1_000_000;

        // Act
        let damage = damage_after_defense(attack, defense, Penetration::NONE);

        // Assert
        assert!(damage >= attack / 10);
    }

    #[test]
    fn 无穿透无防御时伤害等于攻击力() {
        // Arrange & Act
        let damage = damage_after_defense(50, 0, Penetration::NONE);

        // Assert
        assert_eq!(damage, 50);
    }

    #[test]
    fn 固定穿透对低防御目标收益更高() {
        // 「收益」定义为：加上这份穿透后，比不穿透多打出的伤害。
        // 固定穿透先从防御里扣一个常数，防御本就低的目标被扣掉的
        // 是其防御的更大比例，故收益应更高。
        // Arrange
        let attack = 1000;
        let low_defense = 100;
        let high_defense = 400;
        let pen = Penetration {
            flat: 50,
            permille: 0,
        };

        // Act
        let benefit_low = damage_after_defense(attack, low_defense, pen)
            - damage_after_defense(attack, low_defense, Penetration::NONE);
        let benefit_high = damage_after_defense(attack, high_defense, pen)
            - damage_after_defense(attack, high_defense, Penetration::NONE);

        // Assert
        assert!(benefit_low > benefit_high);
    }

    #[test]
    fn 千分比穿透对高防御目标收益更高() {
        // 千分比穿透按比例削减防御，防御的绝对值越大，被削掉的绝对
        // 点数越多，故收益应随防御升高而变大——与固定穿透的规律相反。
        // Arrange
        let attack = 1000;
        let low_defense = 100;
        let high_defense = 400;
        let pen = Penetration {
            flat: 0,
            permille: 250,
        };

        // Act
        let benefit_low = damage_after_defense(attack, low_defense, pen)
            - damage_after_defense(attack, low_defense, Penetration::NONE);
        let benefit_high = damage_after_defense(attack, high_defense, pen)
            - damage_after_defense(attack, high_defense, Penetration::NONE);

        // Assert
        assert!(benefit_high > benefit_low);
    }

    #[test]
    fn 零幸运暴击率为零() {
        // 见 crit_chance_permille 文档「没有独立的『基础暴击率』常量」
        // 一节：零幸运必须精确产出零暴击率，这是保证既有确定性测试
        // 不受影响的前提。
        // Arrange & Act
        let chance = crit_chance_permille(0);

        // Assert
        assert_eq!(chance, 0);
    }

    #[test]
    fn 幸运越高暴击率越高() {
        // Arrange
        let low_luck = 5;
        let high_luck = 50;

        // Act
        let low_chance = crit_chance_permille(low_luck);
        let high_chance = crit_chance_permille(high_luck);

        // Assert
        assert!(high_chance > low_chance);
    }

    #[test]
    fn 暴击率不超过千分之一千() {
        // 幸运极高时公式的裸乘积会远超 1000，必须夹住,否则
        // DetRng::chance 会把「必定暴击」误判成「必定不暴击」以外的
        // 未定义比例。
        // Arrange
        let extreme_luck = 10_000;

        // Act
        let chance = crit_chance_permille(extreme_luck);

        // Assert
        assert_eq!(chance, PERMILLE_SCALE as i32);
    }

    #[test]
    fn 暴击伤害高于原始伤害() {
        // Arrange
        let damage = 200;

        // Act
        let crit_damage = apply_crit_multiplier(damage);

        // Assert
        assert!(crit_damage > damage);
    }
}
