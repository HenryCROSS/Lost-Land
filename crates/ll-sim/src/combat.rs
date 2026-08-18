//! 战斗结算的纯数值公式：穿透与伤害。
//!
//! 公式冻结于 `knowledge/design/attribute-system.md` 「三、穿透属性」
//! 与「四、伤害公式」两节（规格决策 30）。本文件只实现公式本身——
//! 谁打谁、打不打得中之类的判定属于 [`crate::resolve::resolve`]，这里
//! 只提供它要调用的纯函数。

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
}
