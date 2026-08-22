//! 击杀产出经验值：`resolve` 侧需要的「这个生物种类值多少经验」只读
//! 接口——`knowledge/design/level-and-experience-system.md` 五节核实出
//! 的真正缺口（「没有任何地方注册『某种生物值多少经验』」），本模块是
//! 消费端的落点。
//!
//! # 为什么是 trait，不是具体类型
//!
//! 与 [`crate::skill::SkillCatalog`]/[`crate::quest::QuestCatalog`] 同
//! 一套依赖倒置手法：真正持有「种族 → 经验值」映射的一方是 `ll-mod`
//! （见 `crates/ll-mod/src/race.rs` 的 `RaceDef::xp_reward` 字段），
//! `ll-sim` 不能反过来依赖它（依赖方向 `ll-world` ← `ll-sim` ← `ll-mod`）。
//! `resolve` 因此只依赖本模块声明的接口，不知道背后是 `RaceTable` 还
//! 是别的什么存储形状。

use ll_core::ident::ContentIndex;

/// 给定一个「生物种类」（[`ll_world::entity::Agent::creature_kind`]，
/// `None` 时回退到 [`ll_world::entity::Agent::race`]——与
/// `Effect::IncrementKillCount` 现有的归并键完全同一个键空间，见
/// `crate::resolve` 模块 `append_kill_history` 文档），返回杀死这个
/// 种类的**基准经验值**。
///
/// # 基准值，不是最终值（本批次重新定义）
///
/// 这个返回值曾经就是「杀死它给多少经验」这个最终数字，`resolve`
/// 拿到后原样塞进 `Effect::GrantExperience`。项目所有者裁定
/// 「有个最低经验 1xp，然后等级差越多给越多，有个经验公式」之后，
/// 它的语义收窄成公式的**一个输入**：最终经验由
/// [`kill_experience`] 用它与击杀双方的等级差一起算出来。
///
/// 内容作者仍然只需要回答同一个问题——「这个种类本身值多少」——
/// 与武器声明伤害公式基数、不声明「这一刀最终打多少」是同一个分工：
/// 等级差是**遭遇的属性**，不是生物的属性，内容作者无从预先知道杀它
/// 的人几级。
///
/// # 为什么恒返回 `i64` 而不是 `Option<i64>`
///
/// 未注册的种类没有理由阻断整条结算链——查不到就当作「这个种类没有
/// 声明过基准值」（0）。**0 不再等于「不给经验」**：
/// [`kill_experience`] 的 [`MIN_KILL_XP`] 保底让任何击杀都至少产出
/// 1 点，见其文档。这仍然是「诚实地给出零」而不是「静默跳过」，只是
/// 零的下游含义随所有者的裁定变了。
pub trait ExperienceCatalog {
    /// 查询给定种类的击杀**基准**经验值，未注册时约定返回 0
    /// （「没有声明过基准值」，不是「不给经验」，见 trait 文档）。
    fn xp_reward_for(&self, kind: ContentIndex) -> i64;
}

/// 任何一次击杀的保底经验——项目所有者裁定原文「有个最低经验 1xp」。
///
/// # 为什么保底必须在公式的最后一步
///
/// 与 `knowledge/design/attribute-system.md` 四节「为什么下限必须夹在
/// 最后一步」记的那次教训完全同构：若把保底夹在等级差倍率**之前**
/// （先取 `base` 与 1 的较大者再乘倍率），越级往下打时倍率会把刚夹好
/// 的 1 重新乘回 0（整数除法向零截断，1 乘 10 除以 100 等于 0），
/// 「最低 1xp」这句话在它唯一本该生效的场合失效。夹在最后一步是唯一
/// 让这句话对任意输入都成立的顺序。
pub const MIN_KILL_XP: i64 = 1;

/// 每一级等级差改变的百分点——正的等级差（死者比击杀者高）加成，
/// 负的减成。
///
/// 25 这个取值不是从别处搬来的既有常量，是本批次定的：它让「越级
/// 一级」这件事的收益（加 25%）明显能感觉到，同时要连打 4 级才翻倍，
/// 不至于让「找一个高很多级的目标偷一刀」成为压倒一切的最优解。
pub const LEVEL_DIFF_PERCENT_PER_LEVEL: i64 = 25;

/// 等级差倍率的下限（百分比）——碾压低级目标时倍率不会跌到零，
/// 基准值高的稀有生物即便被高等级角色秒杀也仍按基准值的一成结算，
/// 而不是被一刀切成保底的 1 点。
///
/// 下限存在的意义是让**基准值本身**在碾压区间仍然有区分度：没有它
/// 的话，等级差足够大时所有目标都收敛到 [`MIN_KILL_XP`]，内容作者
/// 声明的基准值在游戏中后期就再也不起作用了。
pub const LEVEL_DIFF_PERCENT_FLOOR: i64 = 10;

/// 等级差倍率的上限（百分比）——越级挑战的收益封顶在四倍。
///
/// 上限存在的意义是防一条真实的漏洞：等级差没有上限时，一次侥幸
/// 击杀（陷阱、地形、友军补刀记在自己头上）一个高出几十级的目标，
/// 会一次性把角色推过好几级——那不是「越级挑战有回报」，那是一台
/// 随机跳级机器。
pub const LEVEL_DIFF_PERCENT_CEILING: i64 = 400;

/// 一次击杀最终产出多少经验——项目所有者裁定的那条「经验公式」。
///
/// ```text
/// 倍率(百分比) = clamp(100 + 25 × (死者等级 − 击杀者等级), 10, 400)
/// 经验         = max(1, 基准值 × 倍率 / 100)
/// ```
///
/// # 等级差的方向：杀比自己高的给得多
///
/// 所有者的原话是「等级差越多给越多」，字面上既可以读成「差的绝对
/// 值越大给越多」，也可以读成「死者比自己高得越多给越多」。本函数
/// 取后者，理由不是惯例而是前半句本身：同一句裁定里还写着「有个最低
/// 经验 1xp」——保底之所以需要存在，只可能是因为**存在一档给得极少
/// 的击杀**；若按绝对值读，碾压弱小目标反而给得多，保底就没有任何
/// 会触发的场合，那半句话会变成空文。两半句话只有在「杀高级的多、
/// 杀低级的少」这个读法下才同时有意义。
///
/// # 全整数，无 `f32` 中间值
///
/// [ADR 0020](../../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)：
/// 经验流进世界状态，是乙区。倍率用**百分比整数**表达（不是 0.25
/// 一类浮点比例），除以 100 那一步是整数除法向零截断，与
/// [`crate::xp_curve::XpCurveOp::MulPermille`] 同一条纪律。
///
/// # 为什么是一个 Rust 函数，不是一条可注册的 `XpCurveDef`
///
/// [`crate::xp_curve`] 那台机器求的是「升到下一级要多少经验」，它的
/// 两个运行期输入是 `Level` 与 `PrevRequirement`——**没有「死者等级」
/// 这个操作数**，也不该有（把战斗遭遇的量塞进经验需求曲线的操作数
/// 枚举，正是该模块文档「机器可以复用，类型不能」一节拒绝过的那种
/// 污染）。给击杀经验另开第三套指令集需要一个真实的、内容作者提出
/// 过的可配置需求来支撑，当前没有（[ADR 0021](../../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)：
/// 抽象的理由是有算法可共享，不是对称）。内容作者能调的旋钮是
/// `xp_reward` 这个基准值，那已经是每个种类各自独立的一个数。
///
/// # 溢出
///
/// `base_reward` 来自 mod 声明，可以是任意 `i64`；乘法用
/// [`i64::saturating_mul`]，退化输入不会 panic 也不会绕回负数。
/// 负的基准值先夹到 0（内容作者写负数是笔误，不是「杀了要倒扣经验」
/// 这个从未被设计过的机制），随后由 [`MIN_KILL_XP`] 兜到 1。
pub fn kill_experience(base_reward: i64, killer_level: i32, victim_level: i32) -> i64 {
    let level_diff = i64::from(victim_level) - i64::from(killer_level);
    let percent = (100 + LEVEL_DIFF_PERCENT_PER_LEVEL.saturating_mul(level_diff))
        .clamp(LEVEL_DIFF_PERCENT_FLOOR, LEVEL_DIFF_PERCENT_CEILING);
    let base = base_reward.max(0);
    let scaled = base.saturating_mul(percent) / 100;
    scaled.max(MIN_KILL_XP)
}

/// 空经验目录：任何种类查询恒返回 0，不产出任何经验。
///
/// 与 [`crate::skill::NoSkills`]/[`crate::quest::NoQuests`] 同一个模式
/// ——调用方没有接好内容注册表时的保底实现，见两者文档。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExperience;

impl ExperienceCatalog for NoExperience {
    fn xp_reward_for(&self, _kind: ContentIndex) -> i64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未声明基准值的种类被杀死仍然产出保底一点经验() {
        // 项目所有者裁定「有个最低经验 1xp」——基准值为零（未注册的
        // 种类、或本体三族这种曾经刻意不声明的内容）不再等于零经验。
        // Arrange & Act
        let reward = kill_experience(0, 5, 5);

        // Assert
        assert_eq!(reward, MIN_KILL_XP);
    }

    #[test]
    fn 同级击杀恰好产出基准值() {
        // 等级差为零时倍率是 100%，公式退化成「基准值本身」——这条
        // 钉住「基准值」这个新语义的锚点：没有等级差时，内容作者写的
        // 数字就是玩家拿到的数字。
        // Arrange & Act
        let reward = kill_experience(40, 7, 7);

        // Assert
        assert_eq!(reward, 40);
    }

    #[test]
    fn 杀死高于自己等级的目标比同级给得多() {
        // Arrange
        let same_level = kill_experience(40, 7, 7);

        // Act：死者高两级，倍率 100 + 25×2 = 150%
        let higher = kill_experience(40, 7, 9);

        // Assert
        assert!(higher > same_level);
        assert_eq!(higher, 60);
    }

    #[test]
    fn 杀死低于自己等级的目标比同级给得少() {
        // Arrange
        let same_level = kill_experience(40, 9, 9);

        // Act：死者低两级，倍率 100 − 25×2 = 50%
        let lower = kill_experience(40, 9, 7);

        // Assert
        assert!(lower < same_level);
        assert_eq!(lower, 20);
    }

    #[test]
    fn 等级差越大给得越多是单调的() {
        // 「等级差越多给越多」不是只在某两个取值之间成立的巧合——
        // 逐级递增地核一遍单调性，这条断言才真的钉住了那句裁定。
        // Arrange
        let mut previous = kill_experience(100, 10, 1);

        // Act & Assert
        for victim_level in 2..=20 {
            let current = kill_experience(100, 10, victim_level);
            assert!(
                current >= previous,
                "死者等级 {victim_level} 的经验 {current} 不应低于上一级的 {previous}"
            );
            previous = current;
        }
    }

    #[test]
    fn 碾压低级目标时倍率夹在下限而不是跌到零() {
        // 下限存在的意义：基准值高的目标在碾压区间仍然与基准值低的
        // 目标有区分度，不是全部收敛到保底 1 点，见
        // LEVEL_DIFF_PERCENT_FLOOR 文档。
        // Arrange & Act：等级差 −50，远超把倍率压到下限所需的差距。
        let rare = kill_experience(1000, 60, 10);
        let common = kill_experience(10, 60, 10);

        // Assert：1000 × 10 / 100 = 100；10 × 10 / 100 = 1。
        assert_eq!(rare, 100);
        assert_eq!(common, MIN_KILL_XP);
        assert!(rare > common);
    }

    #[test]
    fn 越级挑战的倍率封顶在上限() {
        // Arrange & Act：等级差 100 与等级差 12 都已经达到或超过封顶
        // 所需的 12 级（100 + 25×12 = 400），两者必须给出同一个数。
        let absurd_gap = kill_experience(10, 1, 101);
        let exactly_at_ceiling = kill_experience(10, 1, 13);

        // Assert：10 × 400 / 100 = 40。
        assert_eq!(absurd_gap, 40);
        assert_eq!(exactly_at_ceiling, 40);
    }

    #[test]
    fn 保底夹在最后一步而不是最前面() {
        // 红线测试：若把保底写在乘倍率之前，本例会先算出 max(1, 0)
        // = 1，再乘 10 除以 100 得 0，返回 0 而不是 1——正是
        // MIN_KILL_XP 文档「为什么保底必须在公式的最后一步」描述的
        // 那个失效模式。
        // Arrange & Act
        let reward = kill_experience(0, 99, 1);

        // Assert
        assert_eq!(reward, MIN_KILL_XP);
    }

    #[test]
    fn 负的基准值被当成零而不是倒扣经验() {
        // Arrange & Act
        let reward = kill_experience(-500, 5, 5);

        // Assert
        assert_eq!(reward, MIN_KILL_XP);
    }

    #[test]
    fn 极端基准值不会溢出panic也不会绕回负数() {
        // Arrange & Act
        let reward = kill_experience(i64::MAX, 1, 99);

        // Assert
        assert!(reward > 0);
    }

    #[test]
    fn 空经验目录对任意种类查询恒返回零() {
        // Arrange
        let catalog = NoExperience;

        // Act
        let reward = catalog.xp_reward_for(ContentIndex::default());

        // Assert
        assert_eq!(reward, 0);
    }
}
