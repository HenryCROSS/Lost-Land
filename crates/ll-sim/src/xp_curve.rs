//! 经验需求曲线：`knowledge/design/level-and-experience-system.md` 三、
//! 四节定案的求值机器。
//!
//! # 为什么不复用 `FormulaOp`
//!
//! 本仓库目前没有任何 `FormulaOp`/`FormulaOperand` 落地（伤害公式仍是
//! 纯设计，见 `knowledge/design/damage-formula-mod-api.md`「落地状态」
//! 一节），但设计文档三节已经把「以后两者都会落地，不能共用同一个
//! 操作数枚举」的理由写死：`AttackPower`/`Defense`/`PenetrationFlat`
//! 这类战斗专属操作数在「经验需求」领域没有意义，反过来 `Level`/
//! `PrevRequirement` 塞进伤害公式的操作数枚举也会污染武器作者的自动
//! 补全列表。本模块因此从一开始就是与未来伤害公式**结构同构、类型
//! 独立**的姊妹类型——机器（装载期编译成扁平指令数组、运行期零脚本、
//! 全整数、除法向零截断）可以复用同一套模式，类型不行。
//!
//! # 求值语义：相邻两级的差值，不是从零累积的总量
//!
//! [`XpCurveDef`] 求的是「从等级 N 升到 N+1 需要多少经验」这个**差值
//! （delta）**，不是「N 级总共需要攒多少经验」。[`XpCurveOperand::Level`]
//! 是即将离开的那一级（求 1→2 时取 1），[`XpCurveOperand::PrevRequirement`]
//! 是上一次同一条曲线求值算出的门槛——第一次求值（1→2 级）没有「上一
//! 级」可引用，这时取 [`XpCurveDef::base_requirement`] 本身，此后每一
//! 级取上一次表达式求值的结果，见 [`eval_xp_curve`] 文档「递推链的
//! 起点」一节。
//!
//! # 为什么没有 `Pow`
//!
//! 见设计文档三节「为什么不需要 pow」：`(d N S)`/`multi-hit` 一类骰子
//! 算子的重复次数永远是编译期已知的字面常量，若给经验曲线加一个「以
//! 运行期等级为指数」的 `Pow`，会让「一条指令内部的重复次数」第一次
//! 由运行期输入决定——这与伤害公式指令集贯穿全篇的纪律正面冲突。真正
//! 的指数增长改用**递推**表达：公式本身只做一次加减乘除（`XpCurveOp`
//! 内部零循环），「逐级复利」由调用方（[`crate::apply`]）的升级循环
//! 反复调用同一条只做算术的公式自然叠加出来，见 [`eval_xp_curve`] 文档。

use ll_core::ident::ContentIndex;

/// 经验曲线定义：`base_requirement` 是递推链的起点（1→2 级门槛的种子
/// 值，见模块文档「求值语义」一节），`instructions` 是装载期由
/// `ll-mod` 的 `SteelVal → Vec<XpCurveOp>` 编译器产出的扁平指令数组
/// ——运行期只执行这个数组，不再触碰脚本引擎。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XpCurveDef {
    /// 完整命名空间标识符，例如 `lostland:warrior_xp_curve`。
    pub id: ContentIndex,
    /// 1→2 级所需经验的种子值——递推公式没有「上一级」可引用时的起点，
    /// 见模块文档「求值语义」一节。
    pub base_requirement: i64,
    /// 装载期编译出的扁平指令数组，最后一条指令的求值结果就是这一次
    /// 「从某一级升到下一级需要多少经验」的答案，见 [`eval_xp_curve`]。
    pub instructions: Vec<XpCurveOp>,
}

/// 一条扁平指令——每条指令执行恰好一次，指令本身不含任何循环
/// （模块文档「为什么没有 `Pow`」一节的直接体现）。
///
/// 与伤害公式共享的算术子集：`+`/`-`/`*`/`/`/`mul-permille`/`min`/
/// `max`/`if`——设计文档三节核实过这八个算子足以表达「线性」与「递推
/// 式指数」两种截然不同的成长节奏，本次不新增任何算子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XpCurveOp {
    /// 把一个操作数原样包成一条指令的结果——供表达式的顶层恰好是单个
    /// 操作数（既不是运算也不是分支）时使用，保证「最后一条指令的
    /// 结果即整个表达式的结果」这条不变式恒成立，见 [`eval_xp_curve`]
    /// 文档「为什么需要 `Ref`」一节。
    Ref(XpCurveOperand),
    /// 加法。
    Add(XpCurveOperand, XpCurveOperand),
    /// 减法。
    Sub(XpCurveOperand, XpCurveOperand),
    /// 乘法。
    Mul(XpCurveOperand, XpCurveOperand),
    /// 整数除法，向零截断（[ADR 0002](../../../../knowledge/decisions/0002-integer-only-world-state.md)）。
    /// 除数为零时返回 0——经验门槛不允许因为一次除零而 panic 中断整场
    /// 结算，这是防御性兜底，不是设计允许的正常路径（`ll-mod` 侧
    /// 装载期无法静态排除「除数是另一条递推链算出的运行期值」这类
    /// 情形，因此运行期必须能安全处理）。
    Div(XpCurveOperand, XpCurveOperand),
    /// 千分比乘法：`a * b / 1000`，向零截断。
    MulPermille(XpCurveOperand, XpCurveOperand),
    /// 取较小值。
    Min(XpCurveOperand, XpCurveOperand),
    /// 取较大值。
    Max(XpCurveOperand, XpCurveOperand),
    /// 条件选择：`cond` 为真取 `if_true`，否则取 `if_false`。
    Select {
        /// 判据。
        cond: XpCurveCond,
        /// 条件为真时的取值。
        if_true: XpCurveOperand,
        /// 条件为假时的取值。
        if_false: XpCurveOperand,
    },
}

/// [`XpCurveOp::Select`] 的判据——六种比较，与伤害公式同一套书写风格
/// （`min`/`max`/`if` 按二元列表形式书写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpCurveCond {
    /// 小于。
    Lt(XpCurveOperand, XpCurveOperand),
    /// 小于等于。
    Le(XpCurveOperand, XpCurveOperand),
    /// 大于。
    Gt(XpCurveOperand, XpCurveOperand),
    /// 大于等于。
    Ge(XpCurveOperand, XpCurveOperand),
    /// 等于。
    Eq(XpCurveOperand, XpCurveOperand),
    /// 不等于。
    Ne(XpCurveOperand, XpCurveOperand),
}

/// 一个操作数：字面常量、对某条先前指令结果的引用，或两个运行期才
/// 存在的输入之一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XpCurveOperand {
    /// 字面整数常量。
    Const(i64),
    /// 引用 `instructions` 数组中下标为该值的指令的求值结果——装载期
    /// 编译器在递归编译一个复合子表达式后产出的引用，见 `ll-mod` 侧
    /// 编译器文档。下标必须小于当前指令自身在数组中的位置（只能引用
    /// 「已经算过」的结果），[`eval_xp_curve`] 依赖这条不变式顺序求值。
    Local(u8),
    /// 即将离开的那一级——求「从 N 级升到 N+1 级需要多少经验」时，这
    /// 个操作数就是 N，见模块文档「求值语义」一节。
    Level,
    /// 上一次同一条曲线求值算出的门槛（递推的输入）——第一次求值取
    /// [`XpCurveDef::base_requirement`]，此后取上一次表达式求值的
    /// 结果，见模块文档「求值语义」一节。
    PrevRequirement,
}

/// 给定即将离开的等级与上一级门槛，对一条 [`XpCurveDef`] 求值，算出
/// 「从这一级升到下一级需要多少经验」。
///
/// # 为什么需要 `Ref`
///
/// 若 mod 作者写的表达式恰好就是单个操作数（例如一条曲线整段只写
/// `level`，不含任何运算符），装载期编译器递归下降到叶子节点后不会
/// 自然产生一条「指令」——但本函数的实现假设「最后一条指令的求值结果
/// 就是答案」，指令数组因此不能为空。[`XpCurveOp::Ref`] 就是补上这个
/// 缺口的兜底指令：编译器在顶层表达式恰好是叶子操作数时，主动包一层
/// `Ref`，让「最后一条指令即答案」这条不变式没有例外，[`eval_xp_curve`]
/// 不需要为「顶层是不是复合表达式」分两条路径。
///
/// # 递推链的起点
///
/// `prev_requirement` 参数由调用方（[`crate::apply`] 的升级循环）传入
/// ——1→2 级传 [`XpCurveDef::base_requirement`]，此后每一级传上一次
/// 本函数的返回值，本函数自己不维护任何跨调用状态（纯函数，与伤害
/// 公式求值器同一条纪律）。
pub fn eval_xp_curve(def: &XpCurveDef, level: i32, prev_requirement: i64) -> i64 {
    let mut locals: Vec<i64> = Vec::with_capacity(def.instructions.len());
    for op in &def.instructions {
        let value = match op {
            XpCurveOp::Ref(operand) => resolve_operand(*operand, level, prev_requirement, &locals),
            XpCurveOp::Add(a, b) => resolve_operand(*a, level, prev_requirement, &locals)
                .saturating_add(resolve_operand(*b, level, prev_requirement, &locals)),
            XpCurveOp::Sub(a, b) => resolve_operand(*a, level, prev_requirement, &locals)
                .saturating_sub(resolve_operand(*b, level, prev_requirement, &locals)),
            XpCurveOp::Mul(a, b) => resolve_operand(*a, level, prev_requirement, &locals)
                .saturating_mul(resolve_operand(*b, level, prev_requirement, &locals)),
            XpCurveOp::Div(a, b) => {
                let divisor = resolve_operand(*b, level, prev_requirement, &locals);
                if divisor == 0 {
                    0
                } else {
                    resolve_operand(*a, level, prev_requirement, &locals) / divisor
                }
            }
            XpCurveOp::MulPermille(a, b) => {
                let a = resolve_operand(*a, level, prev_requirement, &locals);
                let b = resolve_operand(*b, level, prev_requirement, &locals);
                a.saturating_mul(b) / 1000
            }
            XpCurveOp::Min(a, b) => resolve_operand(*a, level, prev_requirement, &locals)
                .min(resolve_operand(*b, level, prev_requirement, &locals)),
            XpCurveOp::Max(a, b) => resolve_operand(*a, level, prev_requirement, &locals)
                .max(resolve_operand(*b, level, prev_requirement, &locals)),
            XpCurveOp::Select {
                cond,
                if_true,
                if_false,
            } => {
                if eval_cond(*cond, level, prev_requirement, &locals) {
                    resolve_operand(*if_true, level, prev_requirement, &locals)
                } else {
                    resolve_operand(*if_false, level, prev_requirement, &locals)
                }
            }
        };
        locals.push(value);
    }
    locals.last().copied().unwrap_or(0)
}

fn eval_cond(cond: XpCurveCond, level: i32, prev_requirement: i64, locals: &[i64]) -> bool {
    match cond {
        XpCurveCond::Lt(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                < resolve_operand(b, level, prev_requirement, locals)
        }
        XpCurveCond::Le(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                <= resolve_operand(b, level, prev_requirement, locals)
        }
        XpCurveCond::Gt(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                > resolve_operand(b, level, prev_requirement, locals)
        }
        XpCurveCond::Ge(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                >= resolve_operand(b, level, prev_requirement, locals)
        }
        XpCurveCond::Eq(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                == resolve_operand(b, level, prev_requirement, locals)
        }
        XpCurveCond::Ne(a, b) => {
            resolve_operand(a, level, prev_requirement, locals)
                != resolve_operand(b, level, prev_requirement, locals)
        }
    }
}

fn resolve_operand(
    operand: XpCurveOperand,
    level: i32,
    prev_requirement: i64,
    locals: &[i64],
) -> i64 {
    match operand {
        XpCurveOperand::Const(value) => value,
        XpCurveOperand::Local(index) => locals.get(index as usize).copied().unwrap_or(0),
        XpCurveOperand::Level => i64::from(level),
        XpCurveOperand::PrevRequirement => prev_requirement,
    }
}

/// [`crate::apply`] 升级循环需要的最小只读接口：给定一个实体的职业与
/// 种族，返回它应该使用的经验曲线。
///
/// # 为什么不是 `Option`
///
/// 与 [`crate::skill::SkillCatalog::skill`] 不同——「这个技能存不存在」
/// 是一个合法的查询结果（不存在就静默跳过），但「升级该用哪条曲线」
/// 不允许查不到：没有曲线就没有办法算出下一级门槛，升级循环会卡死。
/// 真正的「未显式绑定」退化（设计文档八节「未绑定的职业/种族退回
/// `lostland:default_xp_curve`」）是**实现方的职责**，不是本 trait
/// 调用方需要处理的分支——`ll-mod` 的具体实现在找不到绑定时自己回退
/// 到默认曲线，本 trait 因此可以恒返回一个具体值，调用方不需要为
/// 「没有曲线」写任何兜底代码。
///
/// # 为什么按值返回，不是按引用
///
/// 升级是低频事件（一场战斗里一个实体最多触发几次，不是逐 tick 路径），
/// `XpCurveDef` 也不大（`Vec<XpCurveOp>` 通常只有几条到十几条指令）
/// ——按值返回换来的是调用方不需要纠结生命周期，与
/// `SkillCatalog::skill` 返回 `Option<SkillRule>`（`SkillRule` 本身是
/// `Copy`）同一个「低频调用不必为性能纠结所有权」的取舍，只是这里的
/// 值类型带一个 `Vec` 所以是 `Clone` 不是 `Copy`。
pub trait XpCurveCatalog {
    /// 查询给定职业/种族组合应使用的经验曲线。
    fn curve_for(&self, profession: ContentIndex, race: ContentIndex) -> XpCurveDef;
}

/// 保底经验曲线：任何职业/种族组合恒返回同一条「每级固定 100 点经验」
/// 的平曲线。
///
/// 与 [`crate::skill::NoSkills`]/[`crate::quest::NoQuests`] 同一个模式
/// ——调用方没有接好内容注册表（例如尚未装载任何 mod、或只想测试升级
/// 循环本身而不关心具体数值）时的保底实现；[`crate::apply::apply`]
/// （不接收曲线目录参数的既有入口）内部就用这个默认曲线，真正想让
/// mod 声明的曲线生效的调用方应改用
/// [`crate::apply::apply_with_xp_curves`]，传入一个真正实现了
/// [`XpCurveCatalog`] 的目录。
#[derive(Debug, Clone, Copy)]
pub struct FlatXpCurve {
    /// 每一级固定需要的经验量。
    pub amount: i64,
}

impl FlatXpCurve {
    /// 默认档位：每级固定 100 点，与
    /// [`ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL`] 同一个量级
    /// ——两者都是占位值，不是平衡设计。
    pub const DEFAULT: FlatXpCurve = FlatXpCurve { amount: 100 };
}

impl XpCurveCatalog for FlatXpCurve {
    fn curve_for(&self, _profession: ContentIndex, _race: ContentIndex) -> XpCurveDef {
        XpCurveDef {
            id: ContentIndex::default(),
            base_requirement: self.amount,
            instructions: vec![XpCurveOp::Ref(XpCurveOperand::Const(self.amount))],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 战士曲线（设计文档四节示例一）：`100 + 40 * level`——纯等级
    /// 函数，完全不读 `prev-requirement`。装载器产出的指令数组按
    /// 「先算子表达式再算加法」的顺序排列：指令 0 = `40 * level`，
    /// 指令 1（最终结果）= `100 + local(0)`。
    fn warrior_curve() -> XpCurveDef {
        XpCurveDef {
            id: ContentIndex::default(),
            base_requirement: 140,
            instructions: vec![
                XpCurveOp::Mul(XpCurveOperand::Const(40), XpCurveOperand::Level),
                XpCurveOp::Add(XpCurveOperand::Const(100), XpCurveOperand::Local(0)),
            ],
        }
    }

    /// 法师曲线（设计文档四节示例二）：
    /// `max(prev_requirement + 20, prev_requirement * 1.18)`——递推式，
    /// 早期由加法分支主导、后期由千分比乘法分支主导。指令 0 = `+20`
    /// 分支，指令 1 = `mul-permille` 分支，指令 2（最终结果）=
    /// `max(local(0), local(1))`。
    fn mage_curve() -> XpCurveDef {
        XpCurveDef {
            id: ContentIndex::default(),
            base_requirement: 80,
            instructions: vec![
                XpCurveOp::Add(XpCurveOperand::PrevRequirement, XpCurveOperand::Const(20)),
                XpCurveOp::MulPermille(
                    XpCurveOperand::PrevRequirement,
                    XpCurveOperand::Const(1180),
                ),
                XpCurveOp::Max(XpCurveOperand::Local(0), XpCurveOperand::Local(1)),
            ],
        }
    }

    #[test]
    fn 战士曲线一升二级门槛与设计文档手算表一致() {
        // Arrange
        let curve = warrior_curve();

        // Act
        let requirement = eval_xp_curve(&curve, 1, curve.base_requirement);

        // Assert：设计文档四节手算表「1→2」行。
        assert_eq!(requirement, 140);
    }

    #[test]
    fn 战士曲线九升十级门槛与设计文档手算表一致() {
        // Arrange
        let curve = warrior_curve();

        // Act：战士曲线不读 prev_requirement，传入任意占位值。
        let requirement = eval_xp_curve(&curve, 9, 0);

        // Assert：设计文档四节手算表「9→10」行。
        assert_eq!(requirement, 460);
    }

    #[test]
    fn 法师曲线一升二级门槛与设计文档手算表一致() {
        // Arrange
        let curve = mage_curve();

        // Act
        let requirement = eval_xp_curve(&curve, 1, curve.base_requirement);

        // Assert：设计文档四节手算表「1→2」行——加法分支（100）大于
        // 千分比乘法分支（94），取较大值。
        assert_eq!(requirement, 100);
    }

    #[test]
    fn 法师曲线四升五级门槛与设计文档手算表一致() {
        // Arrange：设计文档手算表「3→4」行算出的门槛是 141，供本次
        // 「4→5」求值当 prev_requirement 输入。
        let curve = mage_curve();

        // Act
        let requirement = eval_xp_curve(&curve, 4, 141);

        // Assert：设计文档四节手算表「4→5」行——千分比乘法分支（166）
        // 反超加法分支（161），交叉点已过。
        assert_eq!(requirement, 166);
    }

    #[test]
    fn 法师曲线十五升十六级门槛超过战士同一级门槛() {
        // 设计文档四节论证②：即便法师起点（种子值 80）远低于战士
        // （140），复利曲线终将反超线性曲线——这里直接验证两条真实
        // 注册数据在同一级的门槛确实不同,且法师反超。
        // Arrange：设计文档手算表「14→15」行算出的门槛是 855。
        let mage = mage_curve();
        let warrior = warrior_curve();

        // Act
        let mage_requirement = eval_xp_curve(&mage, 15, 855);
        let warrior_requirement = eval_xp_curve(&warrior, 15, 0);

        // Assert：法师（1008）超过战士（700）。
        assert_eq!(mage_requirement, 1008);
        assert_eq!(warrior_requirement, 700);
        assert!(mage_requirement > warrior_requirement);
    }

    #[test]
    fn 除数为零时不panic而是返回零() {
        // Arrange：一条刻意除以零的畸形曲线——防御性兜底，不是设计
        // 允许的正常路径,见 XpCurveOp::Div 文档。
        let curve = XpCurveDef {
            id: ContentIndex::default(),
            base_requirement: 0,
            instructions: vec![XpCurveOp::Div(
                XpCurveOperand::Const(100),
                XpCurveOperand::Const(0),
            )],
        };

        // Act
        let requirement = eval_xp_curve(&curve, 1, 0);

        // Assert
        assert_eq!(requirement, 0);
    }

    #[test]
    fn 平曲线对任意职业种族组合恒返回同一个固定门槛() {
        // Arrange
        let flat = FlatXpCurve::DEFAULT;

        // Act
        let curve_a = flat.curve_for(ContentIndex::default(), ContentIndex::default());
        let requirement = eval_xp_curve(&curve_a, 7, 999);

        // Assert
        assert_eq!(requirement, 100);
    }
}
