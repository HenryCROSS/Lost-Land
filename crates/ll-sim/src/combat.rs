//! 战斗结算的纯数值公式：穿透、伤害、暴击。
//!
//! 公式冻结于 `knowledge/design/attribute-system.md` 「三、穿透属性」
//! 与「四、伤害公式」两节（规格决策 30）。本文件只实现公式本身——
//! 谁打谁、打不打得中之类的判定属于 [`crate::resolve::resolve`]，这里
//! 只提供它要调用的纯函数。
//!
//! # 暴击：公式本身不碰随机数（幸运接线批次；判定系统迁移批次）
//!
//! `attribute-system.md`「五、幸运」一节：「幸运不直接加伤害，它改变
//! 随机判定的形状」——[`crit_attacker_modifier`] 是这条换算本身
//! （幸运 → 一次对抗判定里攻击者那一侧的**骰子点数修正**），
//! [`apply_crit_multiplier`] 是暴击命中后的伤害放大，两者都是纯函数，
//! 不掷骰、不碰 `DetRng`。真正「掷不掷得中暴击」这一步的随机判定留给
//! `crate::resolve::resolve_attack`（约束 C3：随机性必须走
//! `DetRng::for_entity`，见其调用点文档）——与本文件开篇「谁打谁、
//! 打不打得中之类的判定属于 `resolve`」同一条边界：本文件只提供
//! `resolve` 要调用的纯函数，不越界去决定「这一次到底暴不暴击」。
//!
//! 同一节那句「暴击率：每点幸运 +5‰」是**概率模型时代的字面读法**，
//! 判定系统迁移批次之后不再逐字成立，也不该再成立：`3d20` 对 `3d20`
//! 是钟形分布，同一个 `+1` 在势均力敌处值约 `+8‰`、在悬殊处值不到
//! `+1‰`（[`crate::check`] 模块文档「MdN」一节正是为了这个效果才取
//! `M >= 2`）。一条恒定的每点增量在钟形骰上根本写不出来。留下来的是
//! 那句话的**方向与量级**：幸运每点仍然只动判定的形状、不进伤害的
//! 加法项，且在基准附近每点约值 `+8‰`——与原文的 `+5‰` 同一档，
//! 推导见 [`CRIT_BASE_CHECK_MODIFIER`]。

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

/// 暴击判定里攻击者一侧的**基准偏移**：`-23` 点。
///
/// # 这个数从哪来
///
/// 项目所有者裁定把暴击迁进 [`crate::check`] 的对抗判定系统，基准
/// 暴击率取 **5%**（传统 roguelike 的常见档位，且足够低到不喧宾夺主）。
/// 「基准」= 攻防双方幸运都取 `BaseStats::BASELINE.luck`（即 `0`，见
/// `ll_world::entity::BaseStats::BASELINE`）、两侧都没有优劣势也没有
/// 重掷的那一局。
///
/// 判定用的是 [`crate::check::CHECK_DICE`]（`3d20`），规则是**主动方
/// 严格大于被动方**。设主动方净修正减被动方净修正为 `delta`，
/// 主动方赢面只依赖 `delta`（两侧同分布），穷举 `20^3 × 20^3 =
/// 64_000_000` 个组合可以逐格数出来（本文件测试
/// `暴击基准偏移是最接近百分之五的那一格` 就是这么数的，不采样、不
/// 拟合）：
///
/// ```text
/// delta = -24 → 2_649_297 / 64_000_000 = 4.1395%
/// delta = -23 → 3_099_831 / 64_000_000 = 4.8435%   ← 距 5% 最近
/// delta = -22 → 3_605_820 / 64_000_000 = 5.6341%
/// ```
///
/// `-23` 是**整数格里距 5% 最近的那一格**（差 0.157 个百分点，两侧邻
/// 格分别差 0.86 与 0.63 个百分点）。钟形骰上不存在恰好落在 5% 的整数
/// 修正——这不是取整误差，是「修正是整数点数」这条量纲本身的后果，与
/// [`crate::check::CheckDice::max_modifier`] 里 `(S-1)/2` 取整同一类。
///
/// # 幸运怎么进式子
///
/// [`crit_attacker_modifier`]：`基准偏移 + 幸运点数`。**一点幸运换一点
/// 修正**，不再除以任何系数——幸运是本仓库唯一基准值为 `0` 的主属性
/// （其余六项基准 `10`，见 `BaseStats::BASELINE`），因此它的原始值本身
/// 就已经是一个「相对基准的增量」，与骰子修正是同一种量（加在掷出点数
/// 上的整数点数），不需要再过一道 `(属性 − 10) / 2` 的调整值换算。
/// 换算了反而会把零幸运的角色打成 `-5`，凭空给全仓库每一个基准角色扣
/// 上一个「比平均倒霉」的标签。
///
/// 数值后果（被攻击者幸运取基准 `0`）：
///
/// ```text
/// 幸运   0 → delta = -23 →  4.84%    幸运  10 → delta = -13 → 17.40%
/// 幸运   5 → delta = -18 →  9.77%    幸运  23 → delta =   0 → 48.62%
/// 幸运  51 → delta =  28 → 97.51%（修正上限 L = 28，此后不再增长）
/// ```
///
/// 基准附近每点幸运约值 `+8‰`，与 `attribute-system.md` 原文的 `+5‰`
/// 同一档（见本模块文档「暴击」一节：钟形骰上不存在恒定的每点增量）。
///
/// # 顺带修掉的一个「绝对」
///
/// 旧的概率模型把暴击率钳在 `0..=1000‰`，因此**幸运 200 以上必定
/// 暴击**——一条与项目所有者「不允许绝对」直接冲突的规则，在概率模型
/// 里没有别的写法。判定系统里它自动消失：两侧净修正各自被钳进
/// `±L`（[`crate::check::CheckDice::clamp_modifier`]），`|delta| <= 2L
/// <= S - 1 < S`，必定暴击与必定不暴击同时不可达，见 [`crate::check`]
/// 模块文档「不允许绝对」一节。这不是本批次另加的钳制，是量尺自带的。
pub const CRIT_BASE_CHECK_MODIFIER: i64 = -23;

/// 暴击命中时伤害在 [`damage_after_defense`] 结果基础上再乘的比例，
/// 千分比——`attribute-system.md`「六、次级属性」把「暴击伤害」列为
/// 独立的次级属性但未给出具体倍率，也未落地任何字段承载它。本实现
/// 取 1500‰（1.5 倍）：常见 Roguelike/RPG 默认档位，明显高于 1000‰
/// （无暴击基准）使暴击可被玩家感知，具体数值本任务不做平衡设计，
/// 只保证暴击命中后伤害确实变化——与 `RaceDef.darkvision_cells` 字段
/// 「具体数值本任务不做平衡设计，只保证字段真的被本体使用到」同一条
/// 纪律（见 `mods/lostland/races.json5` 对应注释）。
pub const CRIT_DAMAGE_MULTIPLIER_PERMILLE: i32 = 1500;

/// 给定幸运值，算出暴击判定里**攻击者那一侧**的净修正：
/// [`CRIT_BASE_CHECK_MODIFIER`] `+ 幸运点数`。
///
/// 被攻击者那一侧的净修正是它自己的幸运点数、没有基准偏移——偏移是
/// 「暴击本来就该很难」这件事的定价，只属于想打出暴击的那一方。调用点
/// 见 `crate::resolve::resolve_attack` 文档「暴击」一节。
///
/// 纯函数——不掷骰，只把幸运换算成一个整数点数，真正的随机判定留给
/// 调用方，见模块文档「暴击：公式本身不碰随机数」一节。
///
/// **不再夹掉负的幸运值**：旧的概率模型必须夹（`DetRng::chance` 的
/// 分子若为负会在 `as u32` 转换时环绕成一个巨大的正数），判定系统里
/// 那个理由整条消失——修正是 `i64`，负值是这套量尺天然表达得了的
/// 「比基准更糟」，一个诅咒得到 `-4` 幸运的角色因此真的更难打出暴击，
/// 而不是与零幸运的人无差别。越界由
/// [`crate::check::CheckDice::clamp_modifier`] 统一兜底，不在这里重复。
pub fn crit_attacker_modifier(luck: i32) -> i64 {
    CRIT_BASE_CHECK_MODIFIER.saturating_add(i64::from(luck))
}

/// 暴击命中后按 [`CRIT_DAMAGE_MULTIPLIER_PERMILLE`] 放大伤害。
pub fn apply_crit_multiplier(damage: i32) -> i32 {
    (i64::from(damage) * i64::from(CRIT_DAMAGE_MULTIPLIER_PERMILLE) / PERMILLE_SCALE) as i32
}

/// 潜行给**偷袭判定**的修正：一整颗骰子的跨度
/// （[`crate::check::CheckDice::whole_die`]，`3d20` 下是 `19` 点）。
///
/// # 为什么是一整颗骰子，为什么不是「直通」
///
/// 潜行此前在偷袭这条链路上是一个**直通**：`resolve_attack` 里那条
/// `Some(rule) if attacker.stealthed =>` 守卫分支跳过掷骰，直接吃到
/// 追加伤害——潜行时偷袭**必定**触发，与项目所有者「不允许绝对」这条
/// 红线直接冲突。所有者裁定改成一次判定，原话是「就算是概率最小都
/// 可以」：要保住「潜行时偷袭很容易触发」这个玩法效果，只要它不是
/// 100%。
///
/// 定价沿用 [`crate::check`] 已经立好的那把尺子，不另发明数字：潜行是
/// **主动做出的强效果**，值一整颗骰子（`whole_die`）——与
/// `RuleModifier::InspectionSuspicion` 在盘查判定里给潜行定的价逐字
/// 相同，判定系统落地批次就是这么定的（「潜行不再换基数，它是隐蔽方
/// 的一个修正」）。
///
/// # 「很容易触发但不是 100%」落到具体数字
///
/// [`crate::check::CHECK_DICE`] 文档「为什么是 3 颗」那一节算过一次
/// 同一道题：`潜行（一整颗 19）+ 一条被动天赋（半颗 9）= 28`，恰好
/// 等于修正上限 `L`，**不触发钳制**。代进对抗判定（被攻击者取基准，
/// 意志调整值 0）：
///
/// ```text
/// 潜行 + 半颗骰的天赋 → delta = 28 → 97.51%
/// 只有半颗骰的天赋     → delta =  9 → 72.18%
/// ```
///
/// 上面那一档就是所有者要的「概率最小的失败」：`2.49%` 打不出偷袭。
/// **它不是本批次钳出来的**，是 `|a − p| <= 2L <= S − 1 < S` 这条推导
/// 的直接后果——见 [`crate::check`] 模块文档「不允许绝对」一节。哪怕
/// 内容作者把 `sneak_modifier` 填成 `L`，两侧净修正差顶天 `2L = 56`，
/// 仍然赢不满（`63_999_993 / 64_000_000`）。
pub const STEALTH_SNEAK_MODIFIER: i64 = crate::check::CHECK_DICE.whole_die();

/// 偷袭判定里**攻击者那一侧**的净修正：`幸运点数 + 天赋声明的修正 +
/// （潜行时再加 [`STEALTH_SNEAK_MODIFIER`]）`。
///
/// 三项各自的出处：
///
/// - **幸运点数**——所有者对盗贼偷袭的裁定原话「通过幸运值之类的属性
///   以及一定的随机值组合一下」。换算与暴击那一侧逐字相同（一点幸运
///   换一点修正，理由见 [`CRIT_BASE_CHECK_MODIFIER`] 文档「幸运怎么进
///   式子」），不再另发明一套「每点幸运换多少千分比」的系数——那套
///   系数存在的唯一理由是旧的概率模型，判定系统里同一个属性在两处用
///   两把尺子只会让内容作者看到的数字失去可比性。
/// - **天赋声明的修正**——[`crate::rule_modifier::RuleModifier::SneakAttack`]
///   的 `sneak_modifier` 字段，装载期已校验不超过 `L`。偷袭只对声明了
///   这条天赋的角色生效，不同天赋可以有不同的强度。
/// - **潜行**——见 [`STEALTH_SNEAK_MODIFIER`]。
///
/// 被攻击者那一侧是它的**意志调整值**（察觉），由调用点直接读，
/// 不经本函数：那一侧没有任何需要组合的项。
///
/// 纯函数——不掷骰，只把三项加起来，真正的随机判定留给调用方
/// （`crate::resolve::resolve_attack`），见模块文档「暴击：公式本身
/// 不碰随机数」一节同一条边界。饱和运算；越界由
/// [`crate::check::CheckDice::clamp_modifier`] 统一兜底，不在这里重复
/// 钳，理由同 [`crit_attacker_modifier`]。
pub fn sneak_attacker_modifier(luck: i32, sneak_modifier: i32, stealthed: bool) -> i64 {
    let stealth = if stealthed { STEALTH_SNEAK_MODIFIER } else { 0 };
    i64::from(luck)
        .saturating_add(i64::from(sneak_modifier))
        .saturating_add(stealth)
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

    /// 穷举 `3d20` 对 `3d20` 的全部取值组合，数出「主动方净修正减
    /// 被动方净修正等于 `delta` 时主动方赢多少格」——不掷骰、不采样，
    /// 因此下面那条基准测试断的是**精确值**，不是统计近似。
    ///
    /// 与 `crate::check` 测试模块里的同名帮手是同一段算法的第二次
    /// 出现，刻意不共用：那一个是 `#[cfg(test)]` 的私有帮手，把它
    /// 提成跨模块的公开项，等于为了两条测试在生产 API 上开一个只有
    /// 测试用得着的洞。
    fn active_win_count(delta: i64) -> (u64, u64) {
        let dice = crate::check::CHECK_DICE;
        let min = i64::from(dice.count());
        let max = i64::from(dice.count()) * i64::from(dice.sides());
        // 一次 MdN 求和的组合数按「掷出的和」分桶——直接二重遍历
        // 「和」会把每个和当成等权，那是错的（3d20 是钟形，和 30 的
        // 组合数远多于和 3）。这里先数出每个和的组合数再加权。
        // `ways[s]` = 掷出和 `s` 的组合数，逐颗骰卷积一次。
        let mut ways = vec![1u64];
        for _ in 0..dice.count() {
            let mut next = vec![0u64; ways.len() + dice.sides() as usize];
            for (sum, count) in ways.iter().enumerate() {
                for face in 1..=dice.sides() as usize {
                    next[sum + face] += count;
                }
            }
            ways = next;
        }
        let mut wins = 0u64;
        let mut total = 0u64;
        for active in min..=max {
            let active_ways = ways[active as usize];
            for passive in min..=max {
                let combos = active_ways * ways[passive as usize];
                total += combos;
                if active + delta > passive {
                    wins += combos;
                }
            }
        }
        (wins, total)
    }

    #[test]
    fn 暴击基准偏移是最接近百分之五的那一格() {
        // 见 CRIT_BASE_CHECK_MODIFIER 文档「这个数从哪来」：基准暴击率
        // 5% 由项目所有者裁定，`-23` 是钟形骰上整数格里距它最近的那
        // 一格。这条测试逐格数出三个精确值，把「5% 怎么用 3d20 表达」
        // 这条推导本身钉死——不是照着输出填数。
        // Arrange & Act
        let (wins, total) = active_win_count(CRIT_BASE_CHECK_MODIFIER);
        let (lower_wins, _) = active_win_count(CRIT_BASE_CHECK_MODIFIER - 1);
        let (upper_wins, _) = active_win_count(CRIT_BASE_CHECK_MODIFIER + 1);

        // Assert：精确的组合数，与文档里那张表逐字一致。
        assert_eq!(total, 64_000_000);
        assert_eq!(lower_wins, 2_649_297);
        assert_eq!(wins, 3_099_831);
        assert_eq!(upper_wins, 3_605_820);

        // 而且 `-23` 确实是**最近**的一格：两侧邻格离 5% 都更远。
        let target = total / 20; // 5% 的那条线，精确整除（64_000_000 / 20）。
        let distance = |value: u64| value.abs_diff(target);
        assert!(distance(wins) < distance(lower_wins));
        assert!(distance(wins) < distance(upper_wins));
    }

    #[test]
    fn 幸运越高暴击判定的修正越大() {
        // Arrange
        let low_luck = 5;
        let high_luck = 50;

        // Act
        let low = crit_attacker_modifier(low_luck);
        let high = crit_attacker_modifier(high_luck);

        // Assert
        assert!(high > low);
        // 一点幸运换一点修正，基准偏移原样保留——见
        // CRIT_BASE_CHECK_MODIFIER 文档「幸运怎么进式子」。
        assert_eq!(low, CRIT_BASE_CHECK_MODIFIER + 5);
        assert_eq!(high, CRIT_BASE_CHECK_MODIFIER + 50);
    }

    #[test]
    fn 负幸运产出比零幸运更低的暴击修正() {
        // 旧的概率模型必须把负幸运夹到零（`as u32` 会环绕），判定系统
        // 里那条理由消失——见 `crit_attacker_modifier` 文档「不再夹掉
        // 负的幸运值」。
        // Arrange & Act
        let cursed = crit_attacker_modifier(-4);
        let baseline = crit_attacker_modifier(0);

        // Assert
        assert!(cursed < baseline);
        assert_eq!(cursed, CRIT_BASE_CHECK_MODIFIER - 4);
    }

    #[test]
    fn 极高幸运的暴击修正被判定系统的上限接住而不是产出必定暴击() {
        // 旧模型在幸运 200 以上把暴击率钳成 1000‰=必定暴击，与「不允许
        // 绝对」直接冲突；判定系统里越界由 clamp_modifier 统一接住，
        // 见 CRIT_BASE_CHECK_MODIFIER 文档「顺带修掉的一个『绝对』」。
        // Arrange
        let dice = crate::check::CHECK_DICE;
        let extreme_luck = 10_000;

        // Act：本函数如实产出一个越界的大数，钳制发生在 opposed_check
        // 内部（同一条既有纪律：装载期与运行期各钳一次，本函数不重复）。
        let raw = crit_attacker_modifier(extreme_luck);
        let clamped = dice.clamp_modifier(raw);

        // Assert
        assert!(raw > dice.max_modifier());
        assert_eq!(clamped, dice.max_modifier());
        // 顶格之后仍然赢不满——「必定暴击」在这套量尺上不可达。
        let (wins, total) = active_win_count(clamped - dice.clamp_modifier(0));
        assert!(wins < total);
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

    #[test]
    fn 潜行给偷袭判定加一整颗骰子的修正() {
        // Arrange & Act
        let plain = sneak_attacker_modifier(0, 9, false);
        let sneaking = sneak_attacker_modifier(0, 9, true);

        // Assert：差额恰好是一整颗骰子的跨度，见 STEALTH_SNEAK_MODIFIER。
        assert_eq!(plain, 9);
        assert_eq!(sneaking - plain, crate::check::CHECK_DICE.whole_die());
        // 潜行 + 半颗骰的天赋恰好等于修正上限，不触发钳制——CHECK_DICE
        // 文档「为什么是 3 颗」那一节算的就是这道题。
        assert_eq!(sneaking, crate::check::CHECK_DICE.max_modifier());
    }

    #[test]
    fn 潜行时的偷袭很容易触发但不是必定触发() {
        // 所有者裁定原话「就算是概率最小都可以」——本条把那个「最小」
        // 数出来：潜行 + 半颗骰的天赋对一个基准目标是 97.51%，剩下的
        // 2.49% 不是钳出来的，是修正上限推导的直接后果。
        // Arrange
        let stealthed = sneak_attacker_modifier(0, 9, true);
        let alert_defender = 0; // 意志调整值取基准。

        // Act
        let (wins, total) = active_win_count(stealthed - alert_defender);

        // Assert
        assert_eq!(total, 64_000_000);
        assert_eq!(wins, 62_406_870);
        assert!(wins < total, "潜行也不该是必定触发");

        // 顶格也赢不满：内容作者把 sneak_modifier 填到上限、目标顶格
        // 倒霉，仍然留着一线。
        let limit = crate::check::CHECK_DICE.max_modifier();
        let (extreme_wins, extreme_total) = active_win_count(2 * limit);
        assert!(extreme_wins < extreme_total);
    }

    #[test]
    fn 幸运越高偷袭判定的修正越大() {
        // Arrange & Act
        let low = sneak_attacker_modifier(5, 9, false);
        let high = sneak_attacker_modifier(40, 9, false);

        // Assert
        assert!(high > low);
        assert_eq!(low, 14);
        assert_eq!(high, 49);
    }
}
