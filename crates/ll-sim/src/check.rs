//! 对抗判定：`主动方 MdN + 修正  vs  被动方 MdN + 修正`。
//!
//! # 这个模块治的是什么病
//!
//! 换掉的不是「概率」这个表示法本身。对**均匀**骰，「把修正加在掷出的
//! 数上」与「把修正加在概率上」是同一个运算：
//!
//! ```text
//! P(1dN + a >= T)  =  P(1dN >= T - a)
//! ```
//!
//! 两边逐位相等，因此单纯把 `chance(500 - 400, 1000)` 改写成
//! 「d20 + 修正 对目标值」是一次**空操作**，不值得一个模块。
//!
//! 真正的病是**基数是乘法档、修正是加法量**：`guard_inspect_chance`
//! 此前用「潜行与否」在 `500‰` 与 `50‰` 之间二选一（一个 10× 的档），
//! 再从选中的那个基数上减掉一个平坦的点数。两个量不在同一把尺子上，
//! 后果是同一条被动在两个档上的效果差一个数量级：`-400‰` 从 `500‰`
//! 上减是砍掉八成，从 `50‰` 上减则直接触底被钳在 `1‰`。
//!
//! 对抗判定把那个乘法档消掉：**潜行不再换基数，它是隐蔽方的一个修正**
//! （[`CheckDice::whole_die`] 一整颗骰子的跨度）。全部影响判定的量从此
//! 只有一种形状——**加在掷出的点数上的整数点数**。
//!
//! # MdN：M 与 N 都是参数，不是写死的 20
//!
//! 项目所有者裁定「不一定是 20，可以是 N，正整数」「并且可以一次性投掷
//! M 个骰子」。[`CheckDice`] 因此携带 `count`/`sides` 两个值，本模块
//! 全部算式（包括修正上限）都由这两个值推出来，没有任何一处写死 `20`。
//!
//! `M` 是**设计旋钮**，不是副作用：
//!
//! - `M = 1` → 均匀分布，修正线性——每 `+1` 恒定值 `1/N` 的胜率。
//! - `M >= 2` → 钟形分布，**修正在势均力敌时影响大、在悬殊时影响小**。
//!   这正是「一个稍强的人对一个稍弱的人赢面明显、对一个远弱的人赢面
//!   已经封顶」这条直觉。
//!
//! 本引擎取 [`CHECK_DICE`]（`3d20`），理由见该常量文档。
//!
//! # 不允许绝对：由修正上限**证明**，不是靠钳一个下界
//!
//! 一次 `MdN` 的取值范围是 `M ..= M*N`，跨度
//! `S = M*(N-1)`（[`CheckDice::spread`]）。设主动方净修正 `a`、被动方
//! `p`，判定规则是「主动方总点数**严格大于**被动方」，于是：
//!
//! - 必定成功 <=> `M + a > M*N + p` <=> `a - p > S`
//! - 必定失败 <=> `M*N + a <= M + p` <=> `a - p <= -S`
//!
//! 两者都不可达的充分条件是 `|a - p| < S`。本模块把每一侧的净修正钳进
//! `[-L, L]`，其中
//!
//! ```text
//! L = (S - 1) / 2        （向零截断，见 CheckDice::max_modifier）
//! ```
//!
//! 于是 `|a - p| <= 2L <= S - 1 < S`，两端各留一线**由算式保证**，不
//! 需要在结果上再钳一个 `1‰` 下界——`ll_sim::rule_modifier` 里那条
//! `clamp_probability_permille` 兜底是概率模型时代的产物，本批次随它
//! 最后一个消费者一起删掉了。
//!
//! 这个上限还顺带解释了 d20 传统为什么「逼着修正保持在 ±1..±10」：
//! `1d20` 代进去恰好是 `L = (19 - 1) / 2 = 9`。上限从来不是一个绝对
//! 数字，而是**骰子跨度的一半**；`N` 可配之后必须照这个比例算，否则
//! `N = 1000` 时一条 `+400` 就能压垮一切。
//!
//! # 边界：不是求值器，不是语言
//!
//! 本模块只有「掷 M 个 N 面骰、按固定规则重掷一次、取两轮的较大/较小、
//! 加一个整数、比一次大小」这几步，**没有循环上限之外的迭代、没有
//! 递归、没有跳转**。爆骰（掷出最大面就继续掷）刻意不做——它需要无界
//! 迭代，正是二档安全性（扁平、有界、装载期全校验）不肯让出的那条线。
//!
//! # 取数纪律（C3/C5）
//!
//! 调用方用 [`ll_core::rng::DetRng::for_entity`] 构造一条流传进来，本
//! 模块按**固定程序顺序**取数：先主动方、后被动方；每一方内部先第一轮
//! 的 M 颗、再（若有优劣势）第二轮的 M 颗；每一颗骰若命中重掷面值，
//! 紧接着取一个新值。取数次数因此只依赖 `(M, 优劣势, 是否声明重掷,
//! 掷出的点数)`，不依赖任何 `HashMap` 迭代顺序、不依赖任何运行期表的
//! 排布。

use ll_core::ident::NamespacedId;
use ll_core::rng::DetRng;

/// `MdN` 的 `M`（一次判定掷几颗骰）的合法范围。
///
/// 上限与 `crate::formula::DICE_COUNT_RANGE` 同一个数量级、同一条理由
/// （一次判定不该消耗掉过多随机抽取）；下限 `1` 是「至少要掷一颗」。
pub const CHECK_DICE_COUNT_RANGE: std::ops::RangeInclusive<u32> = 1..=20;

/// `MdN` 的 `N`（骰子面数）的合法范围。
///
/// `N = 1` 被排除的理由与 `crate::formula::DICE_SIDES_RANGE` 相同，但在
/// 判定里它更严重：`N = 1` 时跨度 `S = 0`，[`CheckDice::max_modifier`]
/// 算出 `L < 0`，「不允许绝对」那条保证直接失效（双方恒定同点，主动方
/// 恒败）。这不是「大概率是笔误」，是**规则本身不成立**。
pub const CHECK_DICE_SIDES_RANGE: std::ops::RangeInclusive<u32> = 2..=1000;

/// 一次判定掷的骰子：`count` 颗 `sides` 面骰，求和。
///
/// 与 `crate::formula::FormulaOp::Dice` 是同一个概念的两次出现，刻意
/// **不共用类型**：那一个是伤害公式指令数组里的一条指令（编译产物的
/// 一部分），这一个是判定系统的量尺（引擎规则的一部分）。两者同构但
/// 服务不同的领域，理由同 `crate::formula` 模块文档「为什么不复用
/// `XpCurveOp`」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckDice {
    count: u32,
    sides: u32,
}

impl CheckDice {
    /// 构造并校验——`count`/`sides` 必须分别落在
    /// [`CHECK_DICE_COUNT_RANGE`]/[`CHECK_DICE_SIDES_RANGE`] 内。
    ///
    /// 两个区间常量在这里被逐字展开成四条比较，而不是调用
    /// `RangeInclusive::contains`：后者不是 `const fn`，而
    /// [`CHECK_DICE`] 需要在 `const` 上下文里构造。区间常量仍然是这些
    /// 数字唯一的文档出处，改动必须两处同改，由本模块测试
    /// `骰子构造拒绝一面骰与零颗骰` 逐个边界钉住。
    pub const fn new(count: u32, sides: u32) -> Option<Self> {
        if count < *CHECK_DICE_COUNT_RANGE.start() || count > *CHECK_DICE_COUNT_RANGE.end() {
            return None;
        }
        if sides < *CHECK_DICE_SIDES_RANGE.start() || sides > *CHECK_DICE_SIDES_RANGE.end() {
            return None;
        }
        Some(CheckDice { count, sides })
    }

    /// `M`：这次判定掷几颗骰。
    pub const fn count(&self) -> u32 {
        self.count
    }

    /// `N`：每颗骰几面。
    pub const fn sides(&self) -> u32 {
        self.sides
    }

    /// 掷出的和的**跨度** `S = M*(N-1)`——最大值 `M*N` 减最小值 `M`。
    ///
    /// 这是本模块全部量纲的基准：修正上限、潜行值、天赋值，都以它
    /// （或它的因子 `N-1`）为单位表达，因此换一把骰子时全部数值一起
    /// 跟着变，不会留下一个照着 d20 拟合出来的裸数字。
    pub const fn spread(&self) -> i64 {
        (self.count as i64) * (self.sides as i64 - 1)
    }

    /// 一侧净修正的绝对值上限 `L = (S - 1) / 2`。
    ///
    /// 见模块文档「不允许绝对」一节的推导：`2L <= S - 1 < S` 是「必定
    /// 成功」与「必定失败」同时不可达的充分条件，而 `(S-1)/2` 是满足
    /// 它的**最大**整数——再大一格（`S/2`，`S` 为偶数时）就会让
    /// `a - p = -S` 变得可写出来，那正是必定失败。这一条由本模块的
    /// `修正上限再放宽一格就会出现绝对结果` 测试反向钉死。
    ///
    /// 代入几组：`1d20 -> 9`（复现 d20 传统的 ±1..±10 量级）、
    /// `3d20 -> 28`、`1d2 -> 0`（一颗硬币容不下任何修正，算式如实地
    /// 这么说，不假装还有空间）。
    pub const fn max_modifier(&self) -> i64 {
        (self.spread() - 1) / 2
    }

    /// **一整颗骰子的跨度** `N - 1`——本引擎给「主动潜行」这类强效果
    /// 定价用的单位。
    ///
    /// 用它而不是一个裸数字，是为了让数值随 `N` 一起缩放：改 `N` 时
    /// 「潜行值一整颗骰子」这句话仍然成立，不需要重新拟合。
    pub const fn whole_die(&self) -> i64 {
        self.sides as i64 - 1
    }

    /// **半颗骰子的跨度** `(N - 1) / 2`——给「天生不起眼」这类被动
    /// 天赋定价用的单位：一条被动不该与「此刻真的藏起来了」等价，取
    /// 主动效果的一半是唯一有内在依据的档位。
    pub const fn half_die(&self) -> i64 {
        self.whole_die() / 2
    }

    /// 把一个净修正钳进 `[-L, L]`——[`max_modifier`](Self::max_modifier)。
    ///
    /// 这是「不允许绝对」的**运行期**执行点：装载期只校验单条声明
    /// （见 `ll_mod::content_schema_gear::RawRuleModifier`），而属性
    /// 调整值、装备加成、多条天赋跨加值类型相加之后的**总和**是装载期
    /// 看不见的，必须在这里再兜一次。
    pub const fn clamp_modifier(&self, modifier: i64) -> i64 {
        let limit = self.max_modifier();
        if modifier > limit {
            limit
        } else if modifier < -limit {
            -limit
        } else {
            modifier
        }
    }

    /// 掷一轮 `MdN` 求和；`reroll_on` 命中时那一颗**立即**重掷一次，
    /// 取新值（不再检查新值是否又命中，见
    /// [`crate::rule_modifier::RuleModifier::RerollOnce`]「重抽一次」）。
    fn roll_once(&self, rng: &mut DetRng, reroll_on: Option<i32>) -> i64 {
        let mut sum: i64 = 0;
        for _ in 0..self.count {
            // gen_range 取 [0, sides)，骰子面值是 1..=sides，与
            // `crate::formula::eval_formula` 的 `Dice` 指令同一套换算。
            let mut face = rng.gen_range(u64::from(self.sides)) as i64 + 1;
            if reroll_on.is_some_and(|value| i64::from(value) == face) {
                face = rng.gen_range(u64::from(self.sides)) as i64 + 1;
            }
            sum = sum.saturating_add(face);
        }
        sum
    }

    /// 按 `side.bias` 掷这一侧的骰：正常掷一轮；优势/劣势掷**两轮**取
    /// 较大/较小（`["max", MdN, MdN]` / `["min", ...]`，与伤害公式求值
    /// 器上已验证过的写法同一个形状）。
    fn roll(&self, rng: &mut DetRng, side: &CheckSide) -> i64 {
        let first = self.roll_once(rng, side.reroll_on);
        match side.bias {
            RollBias::Normal => first,
            RollBias::Advantage => first.max(self.roll_once(rng, side.reroll_on)),
            RollBias::Disadvantage => first.min(self.roll_once(rng, side.reroll_on)),
        }
    }
}

/// 引擎全部对抗判定共用的那把骰子：**3d20**。
///
/// # 为什么只有一把，不是每种判定各一把
///
/// 「判定」是同一件事。不同判定之间的区别应当落在**修正**上（谁强
/// 谁弱、什么条件加多少），不该落在**量尺**上——两种判定用不同的骰子
/// 时，同一个 `+9` 在两处的含义就不同了，而内容作者看到的只是同一个
/// 数字。一把尺子是本模块能给出的最强的可比性保证。
///
/// 类型本身仍然是参数化的（[`CheckDice`] 携带 `M`/`N`，全部算式由它们
/// 推出），因此「换一把骰子」是改一个常量，不是改机制。
///
/// # 为什么是引擎常量而不是内容声明
///
/// 与它并肩的那些数——盘查的基础意愿、暴击的基准偏移、暴击伤害
/// 倍率、潜行给偷袭的那一整颗骰子——今天全部是引擎常量，没有一个走
/// 内容声明。把骰子单独做成
/// 内容可配，只会造出「一个内容可配的 `N` 紧挨着一堆写死的基数」这种
/// 半截形状。内容负责声明的是**修正**（`RuleModifier`，已经有完整
/// 通道），引擎负责量尺。真要把量尺也交出去，那是一次独立的内容
/// schema 批次，见提交信息里留给项目所有者的那一条。
///
/// # 为什么是 3 颗，为什么是 20 面
///
/// - **20 面**：沿用本仓库既有的 d20 语汇（属性调整值 `(属性 - 10) / 2`
///   的值域 `-5..=+10` 正是照着 d20 的量级设计的，见
///   `crate::formula::attribute_modifier` 与 `attribute-system.md`）。
///   换成别的面数，那套调整值就要跟着重定标。
/// - **3 颗**：`M >= 2` 才有钟形分布（模块文档「MdN」一节）。取到 3 是
///   为了给修正留出足够的量程——`L = (3*19 - 1) / 2 = 28`，而
///   「潜行（一整颗骰子 19）+ 一条被动天赋（半颗骰子 9）」正好是 28，
///   **不触发钳制**。`M = 2` 时 `L = 18`，同样两条叠起来就已经越界被
///   截掉，天赋会被潜行吃掉大半——那是旧模型「乘法档吃掉加法量」的
///   同一个病换了个地方犯。
pub const CHECK_DICE: CheckDice = match CheckDice::new(3, 20) {
    Some(dice) => dice,
    // 3d20 恒落在两个合法区间内；这一支不可达，写出来只是因为 `const`
    // 上下文里 `Option::expect` 不可用。
    None => panic!("3d20 恒合法"),
};

/// 掷骰的偏向——优势/劣势的落点。
///
/// 只有三档，且**优势与劣势互相抵消**（同时存在 → [`RollBias::Normal`]，
/// 见 [`crate::rule_modifier::check_roll_bias`]）：不叠加、不计数，
/// 与 D&D 5e 同一条规则。理由不是致敬，是它让结果与「有几条来源声明了
/// 优势」无关，因而与聚合顺序无关（约束 C5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RollBias {
    /// 掷一轮。
    #[default]
    Normal,
    /// 掷两轮取较大。
    Advantage,
    /// 掷两轮取较小。
    Disadvantage,
}

/// 一次判定里**一侧**的全部输入：净修正 + 掷骰偏向 + 重掷面值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckSide {
    /// 这一侧加在掷出点数上的整数点数。**构造时不钳制**，钳制发生在
    /// [`opposed_check`] 里（用那次判定的骰子算上限），理由见
    /// [`CheckDice::clamp_modifier`]。
    pub modifier: i64,
    /// 优势/劣势。
    pub bias: RollBias,
    /// 掷出这个面值时重掷一次；`None` = 不重掷。
    pub reroll_on: Option<i32>,
}

impl CheckSide {
    /// 只有修正、没有优劣势也没有重掷的一侧——最常见的构造。
    pub const fn plain(modifier: i64) -> Self {
        CheckSide {
            modifier,
            bias: RollBias::Normal,
            reroll_on: None,
        }
    }
}

/// 一次对抗判定的完整结果——两侧各自的总点数。
///
/// 返回结构体而不是裸 `bool`：调用点写日志/测试时需要看到两个总点数，
/// 而重新掷一次是不可能的（流已经往前走了）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckOutcome {
    /// 主动方总点数（已含钳制后的修正）。
    pub active_total: i64,
    /// 被动方总点数（已含钳制后的修正）。
    pub passive_total: i64,
}

impl CheckOutcome {
    /// 主动方是否赢下这次判定——**严格大于**才算赢。
    ///
    /// 平局归被动方：被动方是「维持现状」的那一方（东西藏着、人没被
    /// 拦下），主动方要改变现状就必须真的赢过去，而不是打平。这条
    /// 规则同时让「不允许绝对」的推导只需要处理一个方向的严格不等式，
    /// 见模块文档。
    pub const fn active_wins(&self) -> bool {
        self.active_total > self.passive_total
    }
}

/// 一次对抗判定：**主动方 MdN + 修正 vs 被动方 MdN + 修正**。
///
/// 两侧的修正各自过一遍 [`CheckDice::clamp_modifier`]，因此无论调用方
/// 传进来什么（属性调整值 + 装备 + 多条天赋相加，全都可能越界），
/// 「必定成功」与「必定失败」都不可达——见模块文档「不允许绝对」。
///
/// 取数顺序：**先主动方、后被动方**，见模块文档「取数纪律」。
pub fn opposed_check(
    dice: &CheckDice,
    active: &CheckSide,
    passive: &CheckSide,
    rng: &mut DetRng,
) -> CheckOutcome {
    let active_total = dice
        .roll(rng, active)
        .saturating_add(dice.clamp_modifier(active.modifier));
    let passive_total = dice
        .roll(rng, passive)
        .saturating_add(dice.clamp_modifier(passive.modifier));
    CheckOutcome {
        active_total,
        passive_total,
    }
}

/// 一类判定的标识——[`crate::rule_modifier::RuleModifier::Advantage`] 与
/// [`Disadvantage`](crate::rule_modifier::RuleModifier::Disadvantage) 的
/// `check_context` 指向的就是它。
///
/// 存成两截 `&'static str` 而不是 `NamespacedId`：后者的构造函数不是
/// `const`（要做字符集校验），而这些是引擎自己的常量，不需要在运行期
/// 反复解析、更不需要为了比较一次而分配一个 `String`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckContext {
    /// 命名空间，例如 `lostland`。
    pub namespace: &'static str,
    /// 路径，例如 `inspection`。
    pub path: &'static str,
}

impl CheckContext {
    /// 内容侧写的那个标识符是不是指本判定。
    pub fn matches(&self, id: &NamespacedId) -> bool {
        id.namespace() == self.namespace && id.path() == self.path
    }
}

/// 盘查判定：卫兵（察觉）主动，被盘查者（隐蔽）被动。
///
/// 消费点在 `ll_mod::native_behavior` 的卫兵行为树。
pub const INSPECTION_CHECK: CheckContext = CheckContext {
    namespace: "lostland",
    path: "inspection",
};

/// 藏匿判定：搜身的人（察觉）主动，被搜的人（隐蔽）被动，**每一件
/// 物品各判一次**。
///
/// 消费点在 `crate::resolve::resolve_inspect`。
pub const CONCEALMENT_CHECK: CheckContext = CheckContext {
    namespace: "lostland",
    path: "concealment",
};

/// 暴击判定：攻击者（想打在要害上）主动，被攻击者（想不被打在要害
/// 上）被动，**每一下攻击各判一次**。
///
/// 消费点在 `crate::resolve::resolve_attack`。基准偏移与幸运怎么进
/// 式子见 `crate::combat::CRIT_BASE_CHECK_MODIFIER`。
pub const CRITICAL_CHECK: CheckContext = CheckContext {
    namespace: "lostland",
    path: "critical",
};

/// 偷袭判定：攻击者（隐蔽，想打在对方没防备的地方）主动，被攻击者
/// （察觉）被动，**每一下攻击各判一次**，且只对声明了
/// [`crate::rule_modifier::RuleModifier::SneakAttack`] 的攻击者发生。
///
/// 与 [`CONCEALMENT_CHECK`] 是同一对角色的**攻守互换**：那边隐蔽方在
/// 被动位（藏东西的人只要不输就藏住了），这边隐蔽方在主动位（偷袭的
/// 人要改变现状，必须真的赢过去）。两边的「察觉 = 意志调整值」是同一条
/// 所有者裁定。
///
/// 消费点在 `crate::resolve::resolve_attack`。潜行怎么进式子见
/// `crate::combat::STEALTH_SNEAK_MODIFIER`。
pub const SNEAK_ATTACK_CHECK: CheckContext = CheckContext {
    namespace: "lostland",
    path: "sneak-attack",
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 穷举一次判定的全部 `(主动掷值, 被动掷值)` 组合，数出主动方赢
    /// 多少格——不掷骰，直接在取值范围上算，因此是精确的边界判断，
    /// 不是采样。注意这里数的是**取值组合**而不是概率，够用：本文件
    /// 两条测试只问「有没有出现 0 或全中」，不问具体概率。
    fn active_win_count(dice: &CheckDice, delta: i64) -> (u64, u64) {
        let min = i64::from(dice.count());
        let max = i64::from(dice.count()) * i64::from(dice.sides());
        let mut wins = 0u64;
        let mut total = 0u64;
        for active in min..=max {
            for passive in min..=max {
                total += 1;
                if active + delta > passive {
                    wins += 1;
                }
            }
        }
        (wins, total)
    }

    #[test]
    fn 修正上限保证两端各留一线() {
        // Arrange：遍历一批合法骰子，覆盖 M=1（均匀）与 M>=2（钟形）。
        for (count, sides) in [(1u32, 2u32), (1, 20), (2, 6), (3, 20), (20, 100)] {
            let dice = CheckDice::new(count, sides).expect("取值均在合法区间内");
            let limit = dice.max_modifier();

            // Act：最极端的两个净差——主动方顶格 vs 被动方顶格。
            for delta in [2 * limit, -2 * limit] {
                let (wins, total) = active_win_count(&dice, delta);

                // Assert：既没有必胜（wins == total），也没有必败（wins == 0）。
                assert!(
                    wins > 0 && wins < total,
                    "{count}d{sides} 在净修正差 {delta} 上出现了绝对结果（{wins}/{total}）"
                );
            }
        }
    }

    #[test]
    fn 修正上限再放宽一格就会出现绝对结果() {
        // 这条与上一条成对：证明 (S-1)/2 不是随手取的保守值，而是
        // **最大**的安全值——再大一格立刻破功。
        // Arrange
        let dice = CheckDice::new(1, 20).expect("1d20 合法");
        let too_much = dice.max_modifier() + 1;

        // Act
        let (wins, total) = active_win_count(&dice, -2 * too_much);

        // Assert：净差 -20 时主动方必败。
        assert_eq!(wins, 0, "净修正差 {} 应当必败", -2 * too_much);
        assert!(total > 0);
    }

    #[test]
    fn d20的修正上限复现传统的正负十量级() {
        // Arrange & Act
        let limit = CheckDice::new(1, 20).expect("1d20 合法").max_modifier();

        // Assert：d20 之所以「逼着修正保持在 ±1..±10」，出处就是这个数。
        assert_eq!(limit, 9);
    }

    #[test]
    fn 本引擎那把骰子的三个量纲() {
        // Arrange & Act & Assert：潜行（一整颗）+ 一条被动（半颗）
        // 恰好等于上限，不触发钳制——见 CHECK_DICE 文档「为什么是 3 颗」。
        assert_eq!(CHECK_DICE.spread(), 57);
        assert_eq!(CHECK_DICE.max_modifier(), 28);
        assert_eq!(CHECK_DICE.whole_die(), 19);
        assert_eq!(CHECK_DICE.half_die(), 9);
        assert_eq!(CHECK_DICE.whole_die() + CHECK_DICE.half_die(), 28);
    }

    #[test]
    fn 超出上限的修正被钳回上限() {
        // Arrange & Act & Assert
        assert_eq!(CHECK_DICE.clamp_modifier(10_000), 28);
        assert_eq!(CHECK_DICE.clamp_modifier(-10_000), -28);
        assert_eq!(CHECK_DICE.clamp_modifier(7), 7);
    }

    #[test]
    fn 骰子构造拒绝一面骰与零颗骰() {
        // 一面骰的跨度是 0，「不允许绝对」在它上面根本不成立，见
        // CHECK_DICE_SIDES_RANGE 文档。
        assert!(CheckDice::new(1, 1).is_none());
        assert!(CheckDice::new(0, 20).is_none());
        assert!(CheckDice::new(21, 20).is_none());
        assert!(CheckDice::new(1, 1001).is_none());
        assert!(CheckDice::new(1, 2).is_some());
        assert!(CheckDice::new(20, 1000).is_some());
    }

    #[test]
    fn 优势掷两轮取较大劣势取较小() {
        // Arrange：同一颗种子的三条流；优势与劣势各消耗 2M 个随机数，
        // 正常消耗 M 个——这条测试同时钉死取数次数。
        let dice = CheckDice::new(1, 20).expect("1d20 合法");
        let mut normal_rng = DetRng::for_entity(7, 1, 0);
        let mut advantage_rng = DetRng::for_entity(7, 1, 0);
        let mut disadvantage_rng = DetRng::for_entity(7, 1, 0);

        // Act
        let first = dice.roll(&mut normal_rng, &CheckSide::plain(0));
        let second = dice.roll(&mut normal_rng, &CheckSide::plain(0));
        let advantage = dice.roll(
            &mut advantage_rng,
            &CheckSide {
                modifier: 0,
                bias: RollBias::Advantage,
                reroll_on: None,
            },
        );
        let disadvantage = dice.roll(
            &mut disadvantage_rng,
            &CheckSide {
                modifier: 0,
                bias: RollBias::Disadvantage,
                reroll_on: None,
            },
        );

        // Assert：优势/劣势就是同一条流上前两轮的 max/min。
        assert_eq!(advantage, first.max(second));
        assert_eq!(disadvantage, first.min(second));
    }

    #[test]
    fn 重掷只在命中面值时多取一个数() {
        // Arrange：先掷两轮拿到流上前两个值，再用同一颗种子声明
        // 「掷出第一个值就重掷」，结果必然是流上的第二个值。
        let dice = CheckDice::new(1, 20).expect("1d20 合法");
        let mut probe = DetRng::for_entity(11, 2, 0);
        let first = dice.roll_once(&mut probe, None);
        let next = dice.roll_once(&mut probe, None);

        let mut rerolled = DetRng::for_entity(11, 2, 0);

        // Act
        let value = dice.roll_once(&mut rerolled, Some(first as i32));

        // Assert：命中 → 取到流上的第二个数，而不是第一个。
        assert_eq!(value, next);

        // 而声明一个掷不出来的面值时，一个数都不多取。
        let mut untouched = DetRng::for_entity(11, 2, 0);
        assert_eq!(dice.roll_once(&mut untouched, Some(1_000)), first);
    }

    #[test]
    fn 对抗判定先掷主动方后掷被动方() {
        // Arrange：手工按「主动方 M 颗、被动方 M 颗」的顺序复现一遍，
        // 逐位对上——这条测试钉死的是取数顺序，不是某个具体点数。
        let dice = CheckDice::new(2, 6).expect("2d6 合法");
        let mut expected_rng = DetRng::for_entity(3, 5, 9);
        let expected_active = dice.roll_once(&mut expected_rng, None) + 4;
        let expected_passive = dice.roll_once(&mut expected_rng, None) - 4;

        let mut rng = DetRng::for_entity(3, 5, 9);

        // Act
        let outcome = opposed_check(&dice, &CheckSide::plain(4), &CheckSide::plain(-4), &mut rng);

        // Assert
        assert_eq!(outcome.active_total, expected_active);
        assert_eq!(outcome.passive_total, expected_passive);
    }

    #[test]
    fn 平局归被动方() {
        // Arrange & Act & Assert
        assert!(
            !CheckOutcome {
                active_total: 30,
                passive_total: 30
            }
            .active_wins()
        );
        assert!(
            CheckOutcome {
                active_total: 31,
                passive_total: 30
            }
            .active_wins()
        );
    }

    #[test]
    fn 判定种类标识符按命名空间与路径逐字比对() {
        // Arrange
        let inspection = NamespacedId::parse("lostland:inspection").expect("合法标识符");
        let other = NamespacedId::parse("lostland:inspections").expect("合法标识符");

        // Act & Assert
        assert!(INSPECTION_CHECK.matches(&inspection));
        assert!(!INSPECTION_CHECK.matches(&other));
        assert!(!CONCEALMENT_CHECK.matches(&inspection));
    }
}
