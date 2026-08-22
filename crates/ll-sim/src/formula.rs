//! 伤害公式引擎：`knowledge/design/damage-formula-mod-api.md` 三节定案
//! 的求值机器——装载期把 mod 写的 s-表达式编译成本模块的扁平指令数组
//! （`ll-mod` 的 `script_damage_formula_api.rs`），运行期只执行
//! [`eval_formula`]，从此与 Steel VM 无瓜葛（该文档一节「第三处核实」）。
//!
//! # 本批次范围
//!
//! 落地 `FormulaOp`/`FormulaOperand`/编译器/求值器/骰子算子/
//! `register-damage-formula`/两层默认解析（内容自身 → 全局默认）——
//! 武器类别/伤害类别注册表、抗性、`multi-hit`、`adv`/`disadv` 不在本批
//! 次范围（任务简报「本批次范围」），留给后续批次。
//!
//! # 为什么不复用 [`crate::xp_curve::XpCurveOp`]
//!
//! 与 `xp_curve` 模块文档「为什么不复用 `FormulaOp`」一节是同一枚硬币
//! 的另一面：设计文档三节已经把「两者以后都会落地，不能共用同一个
//! 操作数枚举」的理由写死——`Level`/`PrevRequirement` 这类经验曲线专属
//! 操作数在「伤害计算」领域没有意义，反过来 `AttackPower`/`Defense`/
//! `Crit` 塞进经验曲线的操作数枚举也会污染经验曲线作者的自动补全列表。
//! 两者结构同构（都是「装载期编译成扁平指令数组、运行期零脚本、全
//! 整数、除法向零截断」）但类型独立，是姊妹类型，不是共享类型。
//!
//! # 公式只算「攻击力」，不吸收减伤链路——任务硬要求一
//!
//! `damage_after_defense`（[`crate::combat::damage_after_defense`]，含
//! 固定减 + 百分比减 + 10% 下限）**完全不改**：公式的输出是送进这条
//! 减伤链路的攻击力数值，不是最终伤害本身，减伤逻辑不被吸收进公式
//! 指令集内部。这与设计文档 v1~v3（四节两个示例，公式内部重新实现了
//! 整条减伤链路）不同——文档十八节（v4，本批次不落地的类别/分项批次）
//! 把这一点改成「每一分项的公式输出 attack-power，送进不变的
//! `damage_after_defense`」，本批次的任务书明确要求采纳这条修正后的
//! 语义，即使本批次不落地分项列表本身。`attack-power`/`defense`/
//! `pen-flat`/`pen-permille` 操作数仍然在封闭表内（mod 公式可以引用
//! 目标防御来设计"破甲"一类效果），只是它们是**公式的输入**，不是
//! "公式在内部重新算一遍减伤"的信号。
//!
//! # 骰子随机数：单条共享流，程序顺序取数（C3/C5）
//!
//! 与设计文档六节完全一致：调用方（`crate::resolve::resolve_attack`）
//! 在求值前用 [`ll_core::rng::DetRng::for_entity`] 构造**恰好一条**流，
//! [`eval_formula`] 按指令数组下标升序遍历，`Dice` 指令连续从这条流里
//! 取 `count` 个值——取数顺序完全由编译产物（指令数组顺序）决定，不
//! 依赖任何运行期状态、不触碰任何 `HashMap`/`HashSet`。
//!
//! # 本批次排除：`multi-hit`/`adv`/`disadv`
//!
//! 任务书允许对这两类算子「自行判断是否属于引擎核心」。判断：两者都
//! 是在骰子原语之上的进一步组合（多轮独立判定 / 同一个骰子摇两遍取
//! 一边），不是「一条 mod 公式能真的算出伤害」这条最小闭环的必要
//! 前提——`(d N S)` 已经能表达随机伤害，`multi-hit`/`adv`/`disadv` 是
//! 在此之上的丰富化。本批次选择先把最小闭环钉死、验证过一遍完整链路
//! 再考虑扩展，不属于本批次的引擎核心，留给后续批次。

use ll_core::ident::ContentIndex;
use ll_core::rng::DetRng;
use ll_world::entity::AttributeKind;

/// 一条 [`FormulaDef::instructions`] 数组的长度上限——防止一个荒诞的
/// 大表达式（不是骰子/多轮判定那种"一条指令内部有界重复"，而是指令
/// 条数本身）拖慢装载期编译或运行期求值。与经验曲线的同名概念（设计
/// 文档在 `xp_curve` 侧没有显式冻结这个数字，但伤害公式设计文档三节
/// 「注册期完整校验」第 1 条明确给出 `64`）保持同一个数量级。
pub const MAX_FORMULA_INSTRUCTIONS: usize = 64;

/// `(d N S)` 的 `N`（骰子个数）合法范围——设计文档三节「注册期完整
/// 校验」第 6 条：上限防止一条 `Dice` 指令自己消耗过多随机抽取，下限
/// 排除没有意义的 `N=0`。
pub const DICE_COUNT_RANGE: std::ops::RangeInclusive<u32> = 1..=20;

/// `(d N S)` 的 `S`（骰子面数）合法范围——同上，`S=1`（恒定摇出 1）
/// 没有意义,大概率是笔误。
pub const DICE_SIDES_RANGE: std::ops::RangeInclusive<u32> = 2..=1000;

/// 一条完整的伤害公式定义：`instructions` 是装载期由 `ll-mod` 的
/// `SteelVal → Vec<FormulaOp>` 编译器产出的扁平指令数组——运行期只
/// 执行这个数组，不再触碰脚本引擎，见模块文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaDef {
    /// 完整命名空间标识符，例如 `lostland:default_damage_formula`。
    pub id: ContentIndex,
    /// 装载期编译出的扁平指令数组，最后一条指令的求值结果就是这条
    /// 公式产出的攻击力数值，见 [`eval_formula`]。
    pub instructions: Vec<FormulaOp>,
    /// 编译期可判定：指令数组里含 [`FormulaOp::Dice`] 时为真——供调用
    /// 方决定要不要在求值前构造随机流（当前实现里
    /// `crate::resolve::resolve_attack` 恒构造一条骰子流，这个字段
    /// 因此目前只用于诊断/未来的性能预估，不影响求值正确性：一条不含
    /// 骰子的公式即便拿到一条随机流，也不会调用 `DetRng` 的任何方法，
    /// 见模块文档「骰子随机数」一节）。
    pub needs_rng: bool,
}

/// 一条扁平指令——每条指令执行恰好一次，除 [`FormulaOp::Dice`] 内部
/// 「连续抽 `count` 次」外指令本身不含任何循环（模块文档「本批次
/// 排除」一节：`multi-hit`/`adv`/`disadv` 不在本批次指令集内）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormulaOp {
    /// 把一个操作数原样包成一条指令的结果——供表达式顶层恰好是单个
    /// 操作数时使用，理由同 [`crate::xp_curve::XpCurveOp::Ref`] 文档。
    Ref(FormulaOperand),
    /// 加法。
    Add(FormulaOperand, FormulaOperand),
    /// 减法。
    Sub(FormulaOperand, FormulaOperand),
    /// 乘法。
    Mul(FormulaOperand, FormulaOperand),
    /// 整数除法，向零截断，除数为零返回零（不 panic，防御性兜底，
    /// 理由同 [`crate::xp_curve::XpCurveOp::Div`] 文档）。
    Div(FormulaOperand, FormulaOperand),
    /// 千分比乘法：`a * b / 1000`，向零截断。
    MulPermille(FormulaOperand, FormulaOperand),
    /// 取较小值。
    Min(FormulaOperand, FormulaOperand),
    /// 取较大值。
    Max(FormulaOperand, FormulaOperand),
    /// 条件选择：两个分支的指令仍然各自在数组里、仍然会被执行到
    /// （设计文档六节「`Select` 分支：两侧都无条件求值」——扁平数组
    /// 不含真正的控制流跳转，`Select` 只是在两个已经算好的值之间挑
    /// 一个）。
    Select {
        /// 判据。
        cond: FormulaCond,
        /// 条件为真时的取值。
        if_true: FormulaOperand,
        /// 条件为假时的取值。
        if_false: FormulaOperand,
    },
    /// 骰子：掷 `count` 个 `sides` 面骰求和——`count`/`sides` 是编译期
    /// 常量（设计文档三节：`(d N S)` 的 `N`/`S` 必须是字面整数常量,
    /// 不能是子表达式）。求值时从调用方传入的 [`DetRng`] 流里**连续**
    /// 取 `count` 个 `gen_range(sides) + 1`，按取出顺序求和，见模块
    /// 文档「骰子随机数」一节。
    Dice {
        /// 骰子个数，注册期校验落在 [`DICE_COUNT_RANGE`] 内。
        count: u32,
        /// 骰子面数，注册期校验落在 [`DICE_SIDES_RANGE`] 内。
        sides: u32,
    },
}

/// [`FormulaOp::Select`] 的判据——六种比较，与经验曲线/`XpCurveCond`
/// 同一套书写风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaCond {
    /// 小于。
    Lt(FormulaOperand, FormulaOperand),
    /// 小于等于。
    Le(FormulaOperand, FormulaOperand),
    /// 大于。
    Gt(FormulaOperand, FormulaOperand),
    /// 大于等于。
    Ge(FormulaOperand, FormulaOperand),
    /// 等于。
    Eq(FormulaOperand, FormulaOperand),
    /// 不等于。
    Ne(FormulaOperand, FormulaOperand),
}

/// 一个操作数：字面常量、对某条先前指令结果的引用，或战斗结算专属的
/// 运行期输入（设计文档三节「操作数」表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaOperand {
    /// 字面整数常量。
    Const(i64),
    /// 引用 `instructions` 数组中下标为该值的指令的求值结果，下标
    /// 必须小于当前指令自身的位置，理由同
    /// [`crate::xp_curve::XpCurveOperand::Local`] 文档。
    Local(u8),
    /// 攻击方按伤害系别选好的攻击力——公式的**输入**，不是公式要
    /// 重新算的东西，见模块文档「公式只算『攻击力』」一节。
    AttackPower,
    /// 防御方按伤害系别选好的防御力。
    Defense,
    /// 固定穿透。
    PenetrationFlat,
    /// 千分比穿透。
    PenetrationPermille,
    /// 攻击方对应主属性的调整值（`(属性 − 10) / 2`，见
    /// [`attribute_modifier`]）——`ll-mod` 侧编译器把 `str-mod`/
    /// `dex-mod`/`con-mod`/`int-mod`/`wis-mod`/`cha-mod` 六个符号映射
    /// 到这个操作数的六个 [`AttributeKind`] 取值（`wis-mod` 映射到
    /// [`AttributeKind::Willpower`]——本项目没有独立的「感知/意志」
    /// 二分，`Willpower` 是六项主属性里承担 D&D「感知」概念的那一项,
    /// 见 `AttributeKind::Willpower` 文档「意志：精神攻防、抵抗、视野
    /// 半径」）。
    AttributeModifier(AttributeKind),
    /// 本次攻击是否暴击，`0`/`1`，调用方预先判定好后喂进来（设计
    /// 文档六节「`crit` 本身不消耗公式内部的随机流」）。
    Crit,
}

/// 给定攻击方按 `(属性 − 10) / 2` 换算出的调整值——`attribute-system.md`
/// 「调整值 = (属性 − 10) / 2」，整数除法向零截断（[ADR 0002](../../../../knowledge/decisions/0002-integer-only-world-state.md)），
/// 与本仓库既有公式同一条惯例（`Rust` 的 `/` 对 `i64` 本身就是向零
/// 截断，不需要额外处理）。
pub fn attribute_modifier(raw_attribute: i32) -> i64 {
    (i64::from(raw_attribute) - 10) / 2
}

/// [`eval_formula`] 求值一条公式所需的全部运行期输入——由调用方
/// （`crate::resolve::resolve_attack`）在求值前一次性准备好,公式
/// 内部只读取,不反过来驱动任何副作用。
#[derive(Debug, Clone, Copy)]
pub struct FormulaInputs {
    /// [`FormulaOperand::AttackPower`]。
    pub attack_power: i64,
    /// [`FormulaOperand::Defense`]。
    pub defense: i64,
    /// [`FormulaOperand::PenetrationFlat`]。
    pub pen_flat: i64,
    /// [`FormulaOperand::PenetrationPermille`]。
    pub pen_permille: i64,
    /// 六项主属性的调整值，按 [`AttributeKind`] 的判别值下标（`Luck`
    /// 那一档不会被任何合法编译产物引用到——本项目封闭操作数表没有
    /// `luck-mod` 符号，见模块文档「操作数」一节，这里仍然留一个槽位
    /// 只是让下标运算不需要特判，不代表 `luck-mod` 是受支持的符号）。
    pub attribute_modifiers: [i64; 7],
    /// [`FormulaOperand::Crit`]。
    pub crit: bool,
}

impl FormulaInputs {
    /// 从攻击方六项主属性的原始值（不是调整值）与其余标量输入构造——
    /// `raw_attributes` 按 [`AttributeKind`] 判别值下标，调用方通常从
    /// `crate::resolve::DerivedStats::attribute` 逐项取值后传入。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attack_power: i64,
        defense: i64,
        pen_flat: i64,
        pen_permille: i64,
        raw_attributes: [i32; 7],
        crit: bool,
    ) -> Self {
        let mut attribute_modifiers = [0i64; 7];
        for (idx, raw) in raw_attributes.iter().enumerate() {
            attribute_modifiers[idx] = attribute_modifier(*raw);
        }
        FormulaInputs {
            attack_power,
            defense,
            pen_flat,
            pen_permille,
            attribute_modifiers,
            crit,
        }
    }
}

/// 对一条 [`FormulaDef`] 求值，算出送进
/// [`crate::combat::damage_after_defense`] 的攻击力数值。
///
/// `rng` 恒由调用方传入（约束 C3：随机性必须走
/// [`DetRng::for_entity`]）——不含 [`FormulaOp::Dice`] 的公式不会调用
/// `rng` 的任何方法，传入一条未被使用的流没有可观测的副作用，见
/// [`FormulaDef::needs_rng`] 文档。
///
/// 按指令数组下标升序遍历（约束 C5：不依赖任何 `HashMap`/`HashSet`
/// 迭代顺序），每条指令的运行期成本是编译期已知的常量（`Dice` 内部
/// 「连续抽 `count` 次」的 `count` 是注册期校验过的编译期常量），全程
/// 使用饱和/防御性运算,不 panic。
pub fn eval_formula(def: &FormulaDef, inputs: &FormulaInputs, rng: &mut DetRng) -> i64 {
    let mut locals: Vec<i64> = Vec::with_capacity(def.instructions.len());
    for op in &def.instructions {
        let value = match op {
            FormulaOp::Ref(operand) => resolve_operand(*operand, inputs, &locals),
            FormulaOp::Add(a, b) => resolve_operand(*a, inputs, &locals)
                .saturating_add(resolve_operand(*b, inputs, &locals)),
            FormulaOp::Sub(a, b) => resolve_operand(*a, inputs, &locals)
                .saturating_sub(resolve_operand(*b, inputs, &locals)),
            FormulaOp::Mul(a, b) => resolve_operand(*a, inputs, &locals)
                .saturating_mul(resolve_operand(*b, inputs, &locals)),
            FormulaOp::Div(a, b) => {
                let divisor = resolve_operand(*b, inputs, &locals);
                if divisor == 0 {
                    0
                } else {
                    resolve_operand(*a, inputs, &locals) / divisor
                }
            }
            FormulaOp::MulPermille(a, b) => {
                let a = resolve_operand(*a, inputs, &locals);
                let b = resolve_operand(*b, inputs, &locals);
                a.saturating_mul(b) / 1000
            }
            FormulaOp::Min(a, b) => {
                resolve_operand(*a, inputs, &locals).min(resolve_operand(*b, inputs, &locals))
            }
            FormulaOp::Max(a, b) => {
                resolve_operand(*a, inputs, &locals).max(resolve_operand(*b, inputs, &locals))
            }
            FormulaOp::Select {
                cond,
                if_true,
                if_false,
            } => {
                if eval_cond(*cond, inputs, &locals) {
                    resolve_operand(*if_true, inputs, &locals)
                } else {
                    resolve_operand(*if_false, inputs, &locals)
                }
            }
            FormulaOp::Dice { count, sides } => {
                let mut sum: i64 = 0;
                for _ in 0..*count {
                    // gen_range 取 [0, sides)，骰子面值是 1..=sides。
                    sum = sum.saturating_add(rng.gen_range(u64::from(*sides)) as i64 + 1);
                }
                sum
            }
        };
        locals.push(value);
    }
    locals.last().copied().unwrap_or(0)
}

fn eval_cond(cond: FormulaCond, inputs: &FormulaInputs, locals: &[i64]) -> bool {
    match cond {
        FormulaCond::Lt(a, b) => {
            resolve_operand(a, inputs, locals) < resolve_operand(b, inputs, locals)
        }
        FormulaCond::Le(a, b) => {
            resolve_operand(a, inputs, locals) <= resolve_operand(b, inputs, locals)
        }
        FormulaCond::Gt(a, b) => {
            resolve_operand(a, inputs, locals) > resolve_operand(b, inputs, locals)
        }
        FormulaCond::Ge(a, b) => {
            resolve_operand(a, inputs, locals) >= resolve_operand(b, inputs, locals)
        }
        FormulaCond::Eq(a, b) => {
            resolve_operand(a, inputs, locals) == resolve_operand(b, inputs, locals)
        }
        FormulaCond::Ne(a, b) => {
            resolve_operand(a, inputs, locals) != resolve_operand(b, inputs, locals)
        }
    }
}

fn resolve_operand(operand: FormulaOperand, inputs: &FormulaInputs, locals: &[i64]) -> i64 {
    match operand {
        FormulaOperand::Const(value) => value,
        FormulaOperand::Local(index) => locals.get(index as usize).copied().unwrap_or(0),
        FormulaOperand::AttackPower => inputs.attack_power,
        FormulaOperand::Defense => inputs.defense,
        FormulaOperand::PenetrationFlat => inputs.pen_flat,
        FormulaOperand::PenetrationPermille => inputs.pen_permille,
        FormulaOperand::AttributeModifier(kind) => inputs.attribute_modifiers[kind as usize],
        FormulaOperand::Crit => i64::from(inputs.crit),
    }
}

/// 全局默认伤害公式的指令数组本身——单独导出成函数，供 `ll-mod` 的
/// `base_damage_formula::register_base_damage_formula` 与本模块的
/// [`NoFormulas`] 共用同一份定义，不在两处各写一份可能漂移的字面量。
///
/// 逐行复现 `resolve_attack` 接入公式引擎之前的既有行为——
/// `attack_power = attacker_derived.attribute(AttributeKind::Strength)`
/// ——这条公式只是把这个已经算好的值原样交回去（`Ref(AttackPower)`），
/// 见任务硬要求二「全局默认公式必须逐行复现现在的行为」与
/// `crate::resolve` 模块「行为等价」测试。
pub fn default_attack_power_instructions() -> Vec<FormulaOp> {
    vec![FormulaOp::Ref(FormulaOperand::AttackPower)]
}

/// 伤害公式目录——`resolve_attack` 需要知道的最小「给我一个可能存在
/// 的显式公式引用，还我一条真的能求值的公式」接口，与
/// [`crate::item::ItemCatalog`]/[`crate::xp_curve::XpCurveCatalog`]
/// 同一套依赖倒置手法：真正的 `FormulaTable` 定义在下游的 `ll-mod`。
///
/// # 为什么不是 `Option<FormulaDef>`
///
/// 与 [`crate::xp_curve::XpCurveCatalog::curve_for`] 「为什么不是
/// `Option`」同一条理由：「这一下攻击该用哪条公式」不允许查不到——
/// 找不到就没有办法算出攻击力，战斗结算会卡死。真正的「未显式指定
/// 公式」退化（两层下探：内容自身声明的公式 → 全局默认公式）是
/// **实现方的职责**（`ll-mod` 的具体实现在找不到显式引用/显式引用
/// 未注册时自己回退到默认公式），本 trait 因此可以恒返回一个具体值。
pub trait DamageFormulaCatalog {
    /// `explicit` 是内容自身声明的公式引用（例如武器的
    /// `damage_formula` 字段）；`None` 或该引用未注册时，实现方应当
    /// 回退到全局默认公式，不应该产出一个「查不到」的结果。
    fn formula_for(&self, explicit: Option<ContentIndex>) -> FormulaDef;
}

/// 空公式目录：任何查询恒返回 [`default_attack_power_instructions`]
/// 构造的公式——理由同 [`crate::item::NoItems`]：调用方没有接好真正
/// 的 `FormulaTable`（多数只测试移动/开门这类不涉及内容注册表的既有
/// 测试场景，或未接入公式引擎的旧版 `resolve_attack` 入口）时的保底
/// 实现，与 [`crate::xp_curve::FlatXpCurve`] 同一个模式。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFormulas;

impl DamageFormulaCatalog for NoFormulas {
    fn formula_for(&self, _explicit: Option<ContentIndex>) -> FormulaDef {
        FormulaDef {
            id: ContentIndex::default(),
            instructions: default_attack_power_instructions(),
            needs_rng: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(attack_power: i64) -> FormulaInputs {
        FormulaInputs::new(attack_power, 0, 0, 0, [0; 7], false)
    }

    fn no_rng() -> DetRng {
        DetRng::for_entity(1, 2, 3)
    }

    #[test]
    fn 全局默认公式原样返回攻击力输入() {
        // Arrange：默认公式必须逐行复现"攻击力=有效力量"这条既有行为
        // （任务硬要求二），验证公式求值结果与输入的 attack_power 完全
        // 相等，不经过任何变换。
        let def = FormulaDef {
            id: ContentIndex::default(),
            instructions: default_attack_power_instructions(),
            needs_rng: false,
        };

        // Act
        let result = eval_formula(&def, &inputs(37), &mut no_rng());

        // Assert
        assert_eq!(result, 37);
    }

    #[test]
    fn 加减乘除与千分比运算按程序顺序求值() {
        // Arrange：(max 1 (- (+ attack-power str-mod) (mul-permille defense 500)))
        let mut in_ = inputs(20);
        in_.attribute_modifiers[AttributeKind::Strength as usize] = 3;
        in_.defense = 10;
        let def = FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![
                FormulaOp::Add(
                    FormulaOperand::AttackPower,
                    FormulaOperand::AttributeModifier(AttributeKind::Strength),
                ),
                FormulaOp::MulPermille(FormulaOperand::Defense, FormulaOperand::Const(500)),
                FormulaOp::Sub(FormulaOperand::Local(0), FormulaOperand::Local(1)),
                FormulaOp::Max(FormulaOperand::Local(2), FormulaOperand::Const(1)),
            ],
            needs_rng: false,
        };

        // Act
        let result = eval_formula(&def, &in_, &mut no_rng());

        // Assert：(20+3) - (10*500/1000=5) = 18，大于下限 1。
        assert_eq!(result, 18);
    }

    #[test]
    fn 除数为零时返回零而不panic() {
        // Arrange
        let def = FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![FormulaOp::Div(
                FormulaOperand::Const(100),
                FormulaOperand::Const(0),
            )],
            needs_rng: false,
        };

        // Act
        let result = eval_formula(&def, &inputs(0), &mut no_rng());

        // Assert
        assert_eq!(result, 0);
    }

    #[test]
    fn select两个分支都会被求值但只有命中分支的值生效() {
        // Arrange：(if (= crit 1) (d 2 12) (d 1 12))——两个分支各自是
        // 一条独立的 Dice 指令，指令 0/1 都会被执行（消耗随机流），
        // 指令 2 的 Select 只是在两个已经算好的值之间挑一个。
        let mut in_ = inputs(0);
        in_.crit = true;
        let def = FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![
                FormulaOp::Dice {
                    count: 2,
                    sides: 12,
                },
                FormulaOp::Dice {
                    count: 1,
                    sides: 12,
                },
                FormulaOp::Select {
                    cond: FormulaCond::Eq(FormulaOperand::Crit, FormulaOperand::Const(1)),
                    if_true: FormulaOperand::Local(0),
                    if_false: FormulaOperand::Local(1),
                },
            ],
            needs_rng: true,
        };

        // Act：暴击分支（指令 0，2d12）的结果落在 2..=24。
        let result = eval_formula(&def, &in_, &mut DetRng::for_entity(1, 2, 3));

        // Assert
        assert!((2..=24).contains(&result));
    }

    #[test]
    fn 同一种子同一公式同一输入两次求值结果相同() {
        // 约束 C3/C5 的直接验收：确定性不依赖任何全局状态，只依赖
        // (世界种子, 实体, 事件计数) 三元组。
        // Arrange
        let def = FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![FormulaOp::Dice { count: 3, sides: 8 }],
            needs_rng: true,
        };

        // Act
        let first = eval_formula(&def, &inputs(0), &mut DetRng::for_entity(42, 7, 3));
        let second = eval_formula(&def, &inputs(0), &mut DetRng::for_entity(42, 7, 3));

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 多个骰子指令按数组下标顺序连续取数() {
        // 验证"谁先取谁后取"完全由指令数组顺序决定：把同一条流分别
        // 喂给"先掷 1d20 再掷 1d20"与"直接掷 1d20 两次"，两者应当消费
        // 出完全相同的两个数字，证明 Dice 指令内部与指令之间共用同一
        // 条按程序顺序推进的流,不存在乱序/并行取数。
        // Arrange
        let two_instructions = FormulaDef {
            id: ContentIndex::default(),
            instructions: vec![
                FormulaOp::Dice {
                    count: 1,
                    sides: 20,
                },
                FormulaOp::Dice {
                    count: 1,
                    sides: 20,
                },
            ],
            needs_rng: true,
        };
        let mut rng_a = DetRng::for_entity(9, 9, 9);
        let mut rng_b = DetRng::for_entity(9, 9, 9);

        // Act：分别对两条独立指令各自求值一次，与用同一条 rng_b 连续
        // 摇两次 1d20 的结果逐一比对。
        let a0 = eval_formula(
            &FormulaDef {
                instructions: vec![two_instructions.instructions[0].clone()],
                ..two_instructions.clone()
            },
            &inputs(0),
            &mut rng_a,
        );
        let a1 = eval_formula(
            &FormulaDef {
                instructions: vec![two_instructions.instructions[1].clone()],
                ..two_instructions.clone()
            },
            &inputs(0),
            &mut rng_a,
        );
        let expected0 = rng_b.gen_range(20) as i64 + 1;
        let expected1 = rng_b.gen_range(20) as i64 + 1;

        // Assert
        assert_eq!(a0, expected0);
        assert_eq!(a1, expected1);
    }

    #[test]
    fn 属性调整值按属性系统公式向零截断() {
        // Arrange & Act & Assert：(16-10)/2=3；(9-10)/2 在 Rust 里向零
        // 截断为 0（不是向下取整的 -1）。
        assert_eq!(attribute_modifier(16), 3);
        assert_eq!(attribute_modifier(9), 0);
        assert_eq!(attribute_modifier(10), 0);
    }

    #[test]
    fn 空目录查询恒返回全局默认公式() {
        // Arrange
        let catalog = NoFormulas;

        // Act
        let def = catalog.formula_for(Some(ContentIndex::default()));

        // Assert
        assert_eq!(eval_formula(&def, &inputs(55), &mut no_rng()), 55);
    }
}
