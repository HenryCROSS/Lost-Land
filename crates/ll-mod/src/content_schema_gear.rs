//! 内容数据文件 schema 的**装备与规则侧**七类：资源池、伤害公式、
//! 伤害类别、武器类别、经验曲线、天赋、物品、配方。
//!
//! 形状与理由同 [`crate::content_schema`]，拆成独立模块的分界线见
//! [`crate::content_schema_world`] 模块文档。
//!
//! # 「追加指令」全部变成字段
//!
//! 脚本时代这几类内容的形状是「一条 `register-X` 定义主体 + 若干条
//! `register-X-Y` 追加声明」，例如一件武器要写四行：
//!
//! ```text
//! (register-item "examplemod:war_hammer" ... )
//! (register-item-equip-mask "examplemod:war_hammer" (list "main-hand" "off-hand"))
//! (register-item-stat-bonus "examplemod:war_hammer" "strength" 6)
//! (register-item-penetration "examplemod:war_hammer" 3 100)
//! ```
//!
//! 数据文件里它们是**同一个对象的四个字段**。这不只是好看：那套
//! 追加指令的第一个参数每次都要重复写一遍物品 id，写错了就会静默把
//! 加成挂到另一件物品上（若那件物品恰好存在），或报一条「尚未通过
//! register-item 注册」——两种都不如「这个字段属于这个对象」这条
//! 结构性保证。
//!
//! 只有一处例外：[`RawItem::tags`] 里的标签、[`RawItem::taught_recipes`]
//! 里的配方等**跨表引用**仍然是字符串，由 `apply_*` 查表解析——这正是
//! 两阶段解析的分工，见 [`crate::content_schema`] 模块文档。

use ll_core::ident::ContentIndex;
use ll_core::scaled::Milli;
use ll_sim::combat::Penetration;
use ll_sim::formula::{FormulaDef, FormulaOp};
use ll_sim::item::{BlindBoxEntry, EquipSlot, SlotMask, StatBonus, StatTarget};
use ll_sim::resource_pool::{
    CapacityFormula, RegenRule, ResourcePoolGrant, ResourcePoolShape, RestRecoveryAmount,
};
use ll_sim::rule_modifier::{RuleModifier, TypedRuleModifier};
use ll_sim::skill::SkillEffect;
use ll_sim::xp_curve::XpCurveDef;
use serde::Deserialize;

use crate::content_expr::{RawExpr, compile_damage_formula, compile_xp_curve};
use crate::content_schema::{
    Applied, RawSkillEffect, attribute_kind_from_str, intern_id, parse_id, required_id,
};
use crate::damage_category::{DamageCategoryDef, DamageCategoryError, DamageCategoryTable};
use crate::formula::{FormulaError, FormulaTable};
use crate::item::{ItemAttrs, ItemError, ItemTable};
use crate::modifier_type::{ModifierTypeDef, ModifierTypeError, ModifierTypeTable};
use crate::recipe::{RecipeAttrs, RecipeError, RecipeIngredient, RecipeTable};
use crate::registry::Registry;
use crate::resource_pool::{ResourcePoolAttrs, ResourcePoolError, ResourcePoolTable};
use crate::tag::TagTable;
use crate::trait_def::{TraitAttrs, TraitError, TraitTable};
use crate::weapon_category::{WeaponCategoryDef, WeaponCategoryError, WeaponCategoryTable};
use crate::xp_curve::{XpCurveError, XpCurveTable};

// ─────────────────────────── 资源池 ───────────────────────────

/// `resource_pools.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePoolFile {
    /// 资源池名册，按书写顺序注册。
    pub resource_pools: Vec<RawResourcePool>,
}

/// 恢复节奏——形状与理由同 [`crate::content_schema::RawResourceCost`]
/// （`kind` + 可选字段 + 手写校验，因为内部标签枚举与
/// `deny_unknown_fields` 不能共存）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRegenRule {
    /// `"none"` / `"on-turn-start"` / `"on-rest-full"` / `"on-rest-amount"`。
    pub kind: String,
    /// `on-turn-start` / `on-rest-amount` 的恢复量。
    #[serde(default)]
    pub amount: Option<u32>,
}

impl RawRegenRule {
    fn resolve(&self) -> Result<RegenRule, String> {
        let amount = || -> Result<u32, String> {
            self.amount
                .ok_or_else(|| format!("恢复节奏 kind {:?} 缺少必填字段 \"amount\"", self.kind))
        };
        let reject_amount = || -> Result<(), String> {
            if self.amount.is_some() {
                Err(format!(
                    "恢复节奏 kind {:?} 不接受字段 \"amount\"",
                    self.kind
                ))
            } else {
                Ok(())
            }
        };
        match self.kind.as_str() {
            "none" => {
                reject_amount()?;
                Ok(RegenRule::None)
            }
            "on-turn-start" => Ok(RegenRule::OnTurnStart { amount: amount()? }),
            "on-rest-full" => {
                reject_amount()?;
                Ok(RegenRule::OnRest {
                    amount: RestRecoveryAmount::Full,
                })
            }
            "on-rest-amount" => Ok(RegenRule::OnRest {
                amount: RestRecoveryAmount::Amount(amount()?),
            }),
            other => Err(format!(
                "未知的恢复节奏 kind {other:?}（只认 none/on-turn-start/on-rest-full/on-rest-amount）"
            )),
        }
    }
}

/// 资源池形状——同上。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPoolShape {
    /// `"scalar"`（标量池）/ `"tiered-slots"`（分档法术位）。
    pub kind: String,
    /// `tiered-slots` 的档位数，合法区间 1..=255。
    #[serde(default)]
    pub tier_count: Option<u32>,
}

impl RawPoolShape {
    fn resolve(&self) -> Result<ResourcePoolShape, String> {
        match self.kind.as_str() {
            "scalar" => {
                if self.tier_count.is_some() {
                    return Err(format!(
                        "资源池形状 kind {:?} 不接受字段 \"tier_count\"",
                        self.kind
                    ));
                }
                Ok(ResourcePoolShape::Scalar)
            }
            "tiered-slots" => {
                let tier_count = self.tier_count.ok_or_else(|| {
                    format!(
                        "资源池形状 kind {:?} 缺少必填字段 \"tier_count\"",
                        self.kind
                    )
                })?;
                // 0 不是合法档位数（一个没有任何档位的法术位池毫无
                // 意义），直接拒绝而不是静默钳成 1——那会掩盖笔误。
                if tier_count < 1 || tier_count > u32::from(u8::MAX) {
                    return Err(format!("法术位档位数 {tier_count} 超出合法范围（1..=255）"));
                }
                Ok(ResourcePoolShape::TieredSlots {
                    tier_count: tier_count as u8,
                })
            }
            other => Err(format!(
                "未知的资源池形状 {other:?}（只认 scalar/tiered-slots）"
            )),
        }
    }
}

/// 一条资源池声明——对应此前 `register-resource-pool` 的六个位置参数。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawResourcePool {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 池形状。
    pub shape: RawPoolShape,
    /// 恢复节奏。
    pub regen: RawRegenRule,
}

/// 把一批资源池写进注册表与资源池表。
pub fn apply_resource_pools(
    registry: &mut Registry,
    table: &mut ResourcePoolTable,
    pools: &[RawResourcePool],
) -> Applied {
    for pool in pools {
        let index = intern_id(registry, &pool.id, "资源池标识符")?;
        let display_name_key = parse_id(&pool.display_name_key, "本地化键标识符")?;
        let shape = pool.shape.resolve()?;
        let regen_rule = pool.regen.resolve()?;
        table
            .define(
                index,
                ResourcePoolAttrs {
                    display_name_key,
                    shape,
                    regen_rule,
                },
            )
            .map_err(|err: ResourcePoolError| err.to_string())?;
    }
    Ok(())
}

// ─────────────────────────── 伤害公式 ───────────────────────────

/// `damage_formulas.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageFormulaFile {
    /// 伤害公式名册，按书写顺序注册。
    pub damage_formulas: Vec<RawDamageFormula>,
}

/// 一条伤害公式声明——对应此前的
/// `(register-damage-formula id (quote 表达式))`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDamageFormula {
    /// 完整命名空间标识符。
    pub id: String,
    /// 表达式，见 [`crate::content_expr`]。
    pub expr: RawExpr,
}

/// 把一批伤害公式编译并写进注册表与公式表。
pub fn apply_damage_formulas(
    registry: &mut Registry,
    table: &mut FormulaTable,
    formulas: &[RawDamageFormula],
) -> Applied {
    for formula in formulas {
        let index = intern_id(registry, &formula.id, "伤害公式标识符")?;
        let instructions = compile_damage_formula(&formula.expr)?;
        let needs_rng = instructions
            .iter()
            .any(|op| matches!(op, FormulaOp::Dice { .. }));
        table
            .define(
                index,
                FormulaDef {
                    id: index,
                    instructions,
                    needs_rng,
                },
            )
            .map_err(|err: FormulaError| err.to_string())?;
    }
    Ok(())
}

// ──────────────────── 伤害类别 / 武器类别 ────────────────────

/// `damage_categories.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageCategoryFile {
    /// 伤害类别名册，按书写顺序注册。
    pub damage_categories: Vec<RawCategory>,
}

/// `weapon_categories.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponCategoryFile {
    /// 武器类别名册，按书写顺序注册。
    pub weapon_categories: Vec<RawCategory>,
}

/// 一条伤害类别或武器类别声明——两者的字段形状逐字相同（id + 可选的
/// 默认公式），共用一个结构体；写进哪张表由调用的 `apply_*` 决定。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCategory {
    /// 完整命名空间标识符。
    pub id: String,
    /// 这个类别的默认伤害公式，**必须已注册**。整条不写表示没有默认
    /// 公式（落回全局默认），此前脚本里用空串表达同一件事。
    #[serde(default)]
    pub default_formula: Option<String>,
}

impl RawCategory {
    /// 解析可选的默认公式引用——只 get 不 intern，拼错当场报错。
    fn resolve_default_formula(&self, registry: &Registry) -> Result<Option<ContentIndex>, String> {
        match self.default_formula.as_deref() {
            None => Ok(None),
            Some(raw) => Ok(Some(required_id(registry, raw, "伤害公式")?)),
        }
    }
}

/// 把一批伤害类别写进注册表与伤害类别表。
pub fn apply_damage_categories(
    registry: &mut Registry,
    table: &mut DamageCategoryTable,
    categories: &[RawCategory],
) -> Applied {
    for category in categories {
        let default_formula = category.resolve_default_formula(registry)?;
        let index = intern_id(registry, &category.id, "伤害类别标识符")?;
        table
            .define(index, DamageCategoryDef { default_formula })
            .map_err(|err: DamageCategoryError| err.to_string())?;
    }
    Ok(())
}

/// 把一批武器类别写进注册表与武器类别表。
pub fn apply_weapon_categories(
    registry: &mut Registry,
    table: &mut WeaponCategoryTable,
    categories: &[RawCategory],
) -> Applied {
    for category in categories {
        let default_formula = category.resolve_default_formula(registry)?;
        let index = intern_id(registry, &category.id, "武器类别标识符")?;
        table
            .define(index, WeaponCategoryDef { default_formula })
            .map_err(|err: WeaponCategoryError| err.to_string())?;
    }
    Ok(())
}

// ─────────────────────────── 经验曲线 ───────────────────────────

/// `xp_curves.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XpCurveFile {
    /// 经验曲线名册，按书写顺序注册。
    pub xp_curves: Vec<RawXpCurve>,
}

/// 一条经验曲线声明——对应此前的
/// `(register-xp-curve id base-requirement (quote 表达式))`。
///
/// **绑定不在这里**：`register-class-xp-curve`／`register-race-xp-curve`
/// 那两条追加指令搬成了 `RawClass::xp_curve`／`RawRace::xp_curve` 字段
/// ——「这个职业用哪条曲线」是职业的属性，写在职业那一条里，比在曲线
/// 文件里再列一遍 id 更难写错，也省掉一整趟「先定义再绑定」的顺序约束。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawXpCurve {
    /// 完整命名空间标识符。
    pub id: String,
    /// 1→2 级所需经验的种子值。
    pub base_requirement: i64,
    /// 递推表达式，见 [`crate::content_expr`]。
    pub expr: RawExpr,
}

/// 把一批经验曲线编译并写进注册表与曲线表。
pub fn apply_xp_curves(
    registry: &mut Registry,
    table: &mut XpCurveTable,
    curves: &[RawXpCurve],
) -> Applied {
    for curve in curves {
        let index = intern_id(registry, &curve.id, "经验曲线标识符")?;
        let instructions = compile_xp_curve(&curve.expr)?;
        table
            .define(
                index,
                XpCurveDef {
                    id: index,
                    base_requirement: curve.base_requirement,
                    instructions,
                },
            )
            .map_err(|err: XpCurveError| err.to_string())?;
    }
    Ok(())
}

// ───────────────────────────── 天赋 ─────────────────────────────

/// `traits.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraitFile {
    /// 天赋名册，按书写顺序注册。
    pub traits: Vec<RawTrait>,
}

/// 一条资源池授予——对应此前的 `register-trait-resource-pool`
/// （`kind: "fixed"`）与 `register-trait-resource-pool-by-level`
/// （`kind: "by-level"`）。
///
/// 两条脚本指令合并成一个 `kind` 分派：它们写的是同一个
/// [`ResourcePoolGrant`]，差别只在容量公式。`by-level` 的多个等级断点
/// 在脚本里是**多次调用**，在这里是 `levels` 数组里的多项——
/// [`TraitTable::add_resource_pool_grant_tiered_level`] 的「同池同公式
/// 就插进已有那一条」合并语义因此原样保留，见其文档。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPoolGrant {
    /// 资源池的完整标识符，**必须已注册**。
    pub pool: String,
    /// `"fixed"` / `"by-level"`。
    pub kind: String,
    /// `fixed` 的固定容量。
    #[serde(default)]
    pub amount: Option<u32>,
    /// `by-level` 的等级断点表。
    #[serde(default)]
    pub levels: Vec<RawPoolLevel>,
}

/// `by-level` 授予的一个等级断点。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPoolLevel {
    /// 断点等级。
    pub level: u32,
    /// 这一级各档的容量，索引 0 = 第 1 档。
    pub tiers: Vec<u32>,
}

/// 天赋携带的一条规则修正——对应此前的 `register-trait-resistance` /
/// `-sneak-attack` / `-inspection-suspicion` / `-inspection-concealment`
/// 四条追加指令。形状与理由同
/// [`crate::content_schema::RawResourceCost`]。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRuleModifier {
    /// `"resistance"` / `"sneak-attack"` / `"inspection-suspicion"` /
    /// `"inspection-concealment"` / `"craft-yield"`。
    pub kind: String,
    /// 这条修正属于哪个**加值类型**，**必须已注册**
    /// （`modifier_types.json5`）。整条缺席 = 不声明类型，落进那个
    /// 共享的未分类桶，见
    /// [`ll_sim::rule_modifier::TypedRuleModifier::modifier_type`]。
    #[serde(default)]
    pub modifier_type: Option<String>,
    /// `resistance` 指向的伤害类别，**必须已注册**。
    #[serde(default)]
    pub damage_category: Option<String>,
    /// `resistance` 的**减伤点数**（flat DR）。正数抵挡、负数表示脆弱,
    /// 不钳制——减伤不封顶，保底在结算侧
    /// （[`ll_sim::rule_modifier::MINIMUM_DAMAGE_AFTER_RESISTANCE`]）。
    #[serde(default)]
    pub damage_reduction: Option<i64>,
    /// `inspection-suspicion` 从盘查触发概率上**直接减掉**的千分比
    /// 点数（越大越不起眼）。负值钳到零：一条「让别人更想搜你」的
    /// 被动不是本变体要表达的东西。
    #[serde(default)]
    pub suspicion_reduction_permille: Option<i64>,
    /// `inspection-concealment` 的千分比隐匿度，钳到 0..=1000
    /// （两端各留一线由结算侧的
    /// [`ll_sim::rule_modifier::clamp_probability_permille`] 负责）。
    #[serde(default)]
    pub conceal_permille: Option<i64>,
    /// `sneak-attack` 的每点幸运换算的千分比触发率。
    #[serde(default)]
    pub luck_chance_permille_per_point: Option<i64>,
    /// `sneak-attack` 的额外伤害。
    #[serde(default)]
    pub extra_damage: Option<i64>,
    /// `craft-yield` 指向的**配方类别**，**必须已注册**
    /// （`crafting.json5` 的 `recipe_categories`；`CONTENT_FILES` 把
    /// `Crafting` 排在 `Traits` 之前，因此这里只 get 不 intern，拼错
    /// 当场报错）。
    #[serde(default)]
    pub recipe_category: Option<String>,
    /// `craft-yield` 每次成功制作额外产出的件数。**可为负**（「手艺
    /// 生疏」这类负面天赋），与 `damage_reduction` 允许负值表示「脆弱」
    /// 是同一条先例，因此**不钳到零**；产出保底在结算侧
    /// （[`ll_sim::rule_modifier::MINIMUM_CRAFT_PRODUCT_COUNT`]）。
    #[serde(default)]
    pub bonus_product_count: Option<i64>,
}

impl RawRuleModifier {
    /// 解析成一条带加值类型的规则修正。
    ///
    /// `modifier_type` 走 [`required_modifier_type`]：**引用一个没注册
    /// 过的加值类型当场报错**，与 `register-recipe` 拒绝未注册配方类别
    /// 是同一条先例。理由见 `ll_mod::modifier_type` 模块文档——分桶会让
    /// 一个拼错的类型比正确写法**更强**（不与同类竞争、还能与别人
    /// 相加），静默接受是最坏的选项。
    fn resolve_typed(
        &self,
        registry: &Registry,
        modifier_types: &ModifierTypeTable,
    ) -> Result<TypedRuleModifier, String> {
        let modifier_type = match self.modifier_type.as_deref() {
            None => None,
            Some(raw) => Some(required_modifier_type(registry, modifier_types, raw)?),
        };
        Ok(TypedRuleModifier {
            modifier_type,
            modifier: self.resolve(registry)?,
        })
    }

    fn resolve(&self, registry: &Registry) -> Result<RuleModifier, String> {
        let reject = |present: bool, field: &str| -> Result<(), String> {
            if present {
                Err(format!(
                    "规则修正 kind {:?} 不接受字段 {field:?}",
                    self.kind
                ))
            } else {
                Ok(())
            }
        };
        let need = |value: Option<i64>, field: &str| -> Result<i64, String> {
            value.ok_or_else(|| format!("规则修正 kind {:?} 缺少必填字段 {field:?}", self.kind))
        };
        // 每个分支先把「与 kind 不搭的字段」逐条拒掉，再取自己要的
        // ——这条检查替代了 serde 内部标签枚举给不了的那一半。
        match self.kind.as_str() {
            "resistance" => {
                reject(self.conceal_permille.is_some(), "conceal_permille")?;
                reject(
                    self.suspicion_reduction_permille.is_some(),
                    "suspicion_reduction_permille",
                )?;
                reject(
                    self.luck_chance_permille_per_point.is_some(),
                    "luck_chance_permille_per_point",
                )?;
                reject(self.extra_damage.is_some(), "extra_damage")?;
                reject(self.recipe_category.is_some(), "recipe_category")?;
                reject(self.bonus_product_count.is_some(), "bonus_product_count")?;
                let raw = self.damage_category.as_deref().ok_or_else(|| {
                    format!(
                        "规则修正 kind {:?} 缺少必填字段 \"damage_category\"",
                        self.kind
                    )
                })?;
                Ok(RuleModifier::Resistance {
                    damage_category: required_id(registry, raw, "伤害类别")?,
                    // 不钳到零：负值是「脆弱」，是乘数模型里 `2000‰`
                    // 那一档在减伤模型下的对应物，见
                    // `RuleModifier::Resistance` 文档「负值 = 脆弱」。
                    damage_reduction: clamp_to_i32(need(
                        self.damage_reduction,
                        "damage_reduction",
                    )?),
                })
            }
            "sneak-attack" => {
                reject(self.damage_category.is_some(), "damage_category")?;
                reject(self.damage_reduction.is_some(), "damage_reduction")?;
                reject(
                    self.suspicion_reduction_permille.is_some(),
                    "suspicion_reduction_permille",
                )?;
                reject(self.conceal_permille.is_some(), "conceal_permille")?;
                reject(self.recipe_category.is_some(), "recipe_category")?;
                reject(self.bonus_product_count.is_some(), "bonus_product_count")?;
                Ok(RuleModifier::SneakAttack {
                    luck_chance_permille_per_point: need(
                        self.luck_chance_permille_per_point,
                        "luck_chance_permille_per_point",
                    )?
                    .max(0) as i32,
                    extra_damage: need(self.extra_damage, "extra_damage")?.max(0) as i32,
                })
            }
            "inspection-suspicion" => {
                reject(self.damage_category.is_some(), "damage_category")?;
                reject(self.damage_reduction.is_some(), "damage_reduction")?;
                reject(self.conceal_permille.is_some(), "conceal_permille")?;
                reject(
                    self.luck_chance_permille_per_point.is_some(),
                    "luck_chance_permille_per_point",
                )?;
                reject(self.extra_damage.is_some(), "extra_damage")?;
                reject(self.recipe_category.is_some(), "recipe_category")?;
                reject(self.bonus_product_count.is_some(), "bonus_product_count")?;
                Ok(RuleModifier::InspectionSuspicion {
                    suspicion_reduction_permille: need(
                        self.suspicion_reduction_permille,
                        "suspicion_reduction_permille",
                    )?
                    .max(0) as i32,
                })
            }
            "inspection-concealment" => {
                reject(self.damage_category.is_some(), "damage_category")?;
                reject(self.damage_reduction.is_some(), "damage_reduction")?;
                reject(
                    self.suspicion_reduction_permille.is_some(),
                    "suspicion_reduction_permille",
                )?;
                reject(
                    self.luck_chance_permille_per_point.is_some(),
                    "luck_chance_permille_per_point",
                )?;
                reject(self.extra_damage.is_some(), "extra_damage")?;
                reject(self.recipe_category.is_some(), "recipe_category")?;
                reject(self.bonus_product_count.is_some(), "bonus_product_count")?;
                Ok(RuleModifier::InspectionConcealment {
                    conceal_permille: need(self.conceal_permille, "conceal_permille")?
                        .clamp(0, 1000) as i32,
                })
            }
            // 制作产出加成（制作类副职奖励批次）。三个具名字段、零位置
            // 参数，与上面四条同形——mod 作者要学的新概念只有一个 kind
            // 取值。`recipe_category` 走 `required_id`「只 get 不 intern，
            // 拼错当场报错」，与同一位作者在 subclasses.json5 里写
            // `unlock.target` 时已经要写的**是同一个 id、同一套报错**,
            // 不是第二样要学的东西。
            "craft-yield" => {
                reject(self.damage_category.is_some(), "damage_category")?;
                reject(self.damage_reduction.is_some(), "damage_reduction")?;
                reject(
                    self.suspicion_reduction_permille.is_some(),
                    "suspicion_reduction_permille",
                )?;
                reject(self.conceal_permille.is_some(), "conceal_permille")?;
                reject(
                    self.luck_chance_permille_per_point.is_some(),
                    "luck_chance_permille_per_point",
                )?;
                reject(self.extra_damage.is_some(), "extra_damage")?;
                let raw = self.recipe_category.as_deref().ok_or_else(|| {
                    format!(
                        "规则修正 kind {:?} 缺少必填字段 \"recipe_category\"",
                        self.kind
                    )
                })?;
                Ok(RuleModifier::CraftYield {
                    category: required_id(registry, raw, "配方类别")?,
                    // 不钳到零：负值是「手艺生疏」，与
                    // `RuleModifier::Resistance` 的「负值 = 脆弱」同一条
                    // 先例。产出保底在结算侧，见
                    // `ll_sim::rule_modifier::MINIMUM_CRAFT_PRODUCT_COUNT`。
                    bonus_product_count: clamp_to_i32(need(
                        self.bonus_product_count,
                        "bonus_product_count",
                    )?),
                })
            }
            other => Err(format!(
                "未知的规则修正 kind {other:?}（只认 resistance/sneak-attack/\
                 inspection-suspicion/inspection-concealment/craft-yield）"
            )),
        }
    }
}

/// 一条天赋声明——对应此前的 `(register-trait id 显示名键 授予技能)`
/// 外加四条 `register-trait-*` 追加指令。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTrait {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 授予的技能（intern，允许技能表后到），缺省无。
    #[serde(default)]
    pub granted_skills: Vec<String>,
    /// 授予的资源池容量，缺省无。
    #[serde(default)]
    pub resource_pools: Vec<RawPoolGrant>,
    /// 携带的规则修正，缺省无。
    #[serde(default)]
    pub rule_modifiers: Vec<RawRuleModifier>,
}

/// 把一批天赋写进注册表与天赋表。
///
/// `stat_modifiers` 恒为空列表：脚本时代没有任何一条注册函数能写它
/// （`register-trait` 不接受、也没有 `register-trait-stat-modifier`），
/// 本模块**不顺手补上**——本批次只做等价迁移，新增能力是后续批次的事。
pub fn apply_traits(
    registry: &mut Registry,
    table: &mut TraitTable,
    modifier_types: &ModifierTypeTable,
    traits: &[RawTrait],
) -> Applied {
    for trait_def in traits {
        let index = intern_id(registry, &trait_def.id, "天赋标识符")?;
        let display_name_key = parse_id(&trait_def.display_name_key, "本地化键标识符")?;
        let mut granted_skills = Vec::with_capacity(trait_def.granted_skills.len());
        for raw in &trait_def.granted_skills {
            granted_skills.push(intern_id(registry, raw, "技能标识符")?);
        }
        table
            .define(
                index,
                TraitAttrs {
                    display_name_key,
                    granted_skills,
                    stat_modifiers: Vec::new(),
                    rule_modifiers: Vec::new(),
                    granted_resource_pools: Vec::new(),
                },
            )
            .map_err(|err: TraitError| err.to_string())?;

        for grant in &trait_def.resource_pools {
            apply_pool_grant(registry, table, index, grant)?;
        }
        for modifier in &trait_def.rule_modifiers {
            let resolved = modifier.resolve_typed(registry, modifier_types)?;
            table
                .add_rule_modifier(index, resolved)
                .map_err(|err: TraitError| err.to_string())?;
        }
    }
    Ok(())
}

/// 把一条资源池授予写进天赋表。
fn apply_pool_grant(
    registry: &Registry,
    table: &mut TraitTable,
    trait_index: ContentIndex,
    grant: &RawPoolGrant,
) -> Applied {
    let pool = required_id(registry, &grant.pool, "资源池")?;
    match grant.kind.as_str() {
        "fixed" => {
            if !grant.levels.is_empty() {
                return Err(format!(
                    "资源池授予 kind {:?} 不接受字段 \"levels\"",
                    grant.kind
                ));
            }
            let amount = grant.amount.ok_or_else(|| {
                format!("资源池授予 kind {:?} 缺少必填字段 \"amount\"", grant.kind)
            })?;
            table
                .add_resource_pool_grant(
                    trait_index,
                    ResourcePoolGrant {
                        pool,
                        capacity: CapacityFormula::Fixed(amount),
                    },
                )
                .map_err(|err: TraitError| err.to_string())
        }
        "by-level" => {
            if grant.amount.is_some() {
                return Err(format!(
                    "资源池授予 kind {:?} 不接受字段 \"amount\"",
                    grant.kind
                ));
            }
            if grant.levels.is_empty() {
                return Err(format!(
                    "资源池授予 kind {:?} 的 \"levels\" 不能是空列表",
                    grant.kind
                ));
            }
            // 逐个断点调用，与脚本时代「一条 register-trait-resource-pool-by-level
            // 一个断点」逐字相同——合并成同一条 ByLevel 授予的逻辑在
            // TraitTable 里，不在这里重写一遍。
            for level in &grant.levels {
                table
                    .add_resource_pool_grant_tiered_level(
                        trait_index,
                        pool,
                        level.level,
                        level.tiers.clone(),
                    )
                    .map_err(|err: TraitError| err.to_string())?;
            }
            Ok(())
        }
        other => Err(format!(
            "未知的资源池授予 kind {other:?}（只认 fixed/by-level）"
        )),
    }
}

// ───────────────────────────── 物品 ─────────────────────────────

/// `items.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFile {
    /// 物品名册，按书写顺序注册。
    pub items: Vec<RawItem>,
}

/// 一条静态属性加成——对应此前的
/// `(register-item-stat-bonus id target amount)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStatBonus {
    /// 加成目标：七个属性名之一，或 `"armor"` / `"insulation"`。
    pub target: String,
    /// 增减量，可为负（诅咒装备）。
    pub amount: i32,
}

/// 穿透——对应此前的 `(register-item-penetration id flat permille)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPenetration {
    /// 固定穿透值。
    pub flat: i32,
    /// 千分比穿透（`1000` = 100%）。
    pub permille: i32,
}

/// 物品携带的一条抗性——对应此前的
/// `(register-item-resistance id damage-category multiplier-permille)`。
///
/// 物品当前只能携带抗性这一种规则修正（脚本时代同样如此：
/// `register-item-resistance` 是唯一一条写 `ItemAttrs::rule_modifiers`
/// 的指令），因此这里是一个专门的结构体而不是天赋那种 `kind` 分派——
/// 没有第二种取值可分派。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawItemResistance {
    /// 伤害类别的完整标识符，**必须已注册**。
    pub damage_category: String,
    /// **减伤点数**（flat DR）：正数抵挡、负数表示脆弱，不钳制。语义与
    /// [`RawRuleModifier::damage_reduction`] 逐字相同。
    pub damage_reduction: i64,
    /// 这条抗性属于哪个**加值类型**，**必须已注册**；缺席 = 未分类，
    /// 语义与 [`RawRuleModifier::modifier_type`] 逐字相同。
    #[serde(default)]
    pub modifier_type: Option<String>,
}

/// 一条物品声明——对应此前 `register-item` 的六个位置参数外加九条
/// `register-item-*` 追加指令。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawItem {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 堆叠上限，恒 ≥ 1。`1` 表示不可堆叠。
    pub stack_limit: u32,
    /// 基础重量（千分之一单位）。
    pub base_weight: i64,
    /// 基础价格（千分之一单位）。
    pub base_price: i64,
    /// 耐久上限。整条不写表示这件物品没有耐久，此前脚本里用 `-1`
    /// 表达同一件事——`-1` 是个需要读文档才知道的哨兵值，数据文件里
    /// 换成「字段缺席」。
    #[serde(default)]
    pub max_durability: Option<i32>,
    /// 占用的装备槽位名，缺省表示不可装备。非空时每个名字都必须是
    /// 已知槽位。
    #[serde(default)]
    pub equip_slots: Vec<String>,
    /// 静态属性加成，缺省无。
    #[serde(default)]
    pub stat_bonuses: Vec<RawStatBonus>,
    /// 使用效果，缺省无。与技能效果同一套编码，见
    /// [`crate::content_schema::RawSkillEffect`]。
    #[serde(default)]
    pub use_effect: Option<RawSkillEffect>,
    /// 穿透，缺省无穿透。
    #[serde(default)]
    pub penetration: Option<RawPenetration>,
    /// 显式声明的伤害公式，**必须已注册**，缺省无。
    #[serde(default)]
    pub damage_formula: Option<String>,
    /// 显式声明的伤害类别，**必须已注册**，缺省无。
    #[serde(default)]
    pub damage_category: Option<String>,
    /// 携带的抗性，缺省无。
    #[serde(default)]
    pub resistances: Vec<RawItemResistance>,
    /// 标签，**必须是已注册的标签**（不只是「这个 id 被 intern 过」），
    /// 缺省无。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 读这件物品能学会的配方，**必须是已注册的配方**，缺省无。
    #[serde(default)]
    pub taught_recipes: Vec<String>,
    /// 这件物品要不要先鉴定才认得，缺省 `false`（一眼认得）。见
    /// [`crate::item::ItemDef::requires_identification`]。
    #[serde(default)]
    pub requires_identification: bool,
    /// 鉴定或研读这件物品一次给多少经验，缺省 `0`。恒非负。见
    /// [`crate::item::ItemDef::study_experience`]。
    #[serde(default)]
    pub study_experience: i64,
    /// 盲盒产出池，缺省空（这件物品不是盲盒）。非空时这件物品**必须
    /// 同时**声明 `requires_identification: true`（否则没有任何动作能
    /// 打开它），注册期硬校验。见
    /// [`crate::item::ItemDef::blind_box_pool`]。
    #[serde(default)]
    pub blind_box_pool: Vec<RawBlindBoxEntry>,
}

/// 盲盒产出池里的一条候选——对应 [`ll_sim::item::BlindBoxEntry`]。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBlindBoxEntry {
    /// 产出物品的完整标识符，**必须是已注册的物品**。
    pub item: String,
    /// 一次产出几个，恒 ≥ 1。
    pub count: u32,
    /// 相对权重，恒 ≥ 1。
    pub weight: u32,
}

/// 把一批物品写进注册表与物品表。
///
/// `tags` 需要标签表、`taught_recipes` 需要配方表——两者都只读，用来
/// 回答「这个已注册的 id 真的是一个标签／一条配方吗」。只查
/// [`Registry`] 等于没查：它对任何已注册内容都返回 `Some`，而这条校验
/// 拦的正是 `"examplemod:roast_meat_recipie"` 这类拼写错误，它的症状是
/// **这本书静默什么都不教**。
pub fn apply_items(
    registry: &mut Registry,
    table: &mut ItemTable,
    tags: &TagTable,
    recipes: &RecipeTable,
    modifier_types: &ModifierTypeTable,
    items: &[RawItem],
) -> Applied {
    for item in items {
        define_one_item(registry, table, item)?;
        let index = required_id(registry, &item.id, "物品")?;
        apply_item_extras(registry, table, tags, recipes, modifier_types, index, item)?;
    }
    Ok(())
}

/// 写入物品主体（`register-item` 那六个参数对应的部分）。
fn define_one_item(registry: &mut Registry, table: &mut ItemTable, item: &RawItem) -> Applied {
    let index = intern_id(registry, &item.id, "物品标识符")?;
    let display_name_key = parse_id(&item.display_name_key, "本地化键标识符")?;
    if item.stack_limit < 1 {
        return Err(format!("堆叠上限 {} 非法（必须 >= 1）", item.stack_limit));
    }
    // 可堆叠物品不该有耐久：两条规则字面矛盾（"能堆叠"暗示多份同质
    // 可以共存一格，"有耐久"暗示每份实例携带自己独立的状态），注册期
    // 直接拒绝，与 `register-item` 当时逐字相同。
    if item.stack_limit > 1 && item.max_durability.is_some() {
        return Err(format!(
            "可堆叠物品（堆叠上限 {}）不能携带耐久上限——耐久会让每份实例各自独立，与堆叠矛盾",
            item.stack_limit
        ));
    }
    if let Some(durability) = item.max_durability
        && durability < 0
    {
        return Err(format!("耐久上限 {durability} 非法（必须 >= 0）"));
    }
    // 研究经验恒非负，与 `RaceDef::xp_reward` 的注册期校验逐字同一条
    // 先例：负经验没有任何设计含义，只会是笔误。
    if item.study_experience < 0 {
        return Err(format!(
            "研究经验 {} 非法（必须 >= 0）",
            item.study_experience
        ));
    }
    // 盲盒必须同时需要鉴定——否则这个盒子声明了一池产出却没有任何动作
    // 能打开它，症状是「这件东西什么都不做」，正是注册期该拦下的那类
    // 静默内容缺陷（ADR 0017）。
    if !item.blind_box_pool.is_empty() && !item.requires_identification {
        return Err(
            "声明了 blind_box_pool 的物品必须同时 requires_identification: true——否则没有任何动作能打开这个盒子"
                .to_string(),
        );
    }

    table
        .define(
            index,
            ItemAttrs {
                display_name_key,
                stack_limit: item.stack_limit,
                base_weight: Milli(item.base_weight),
                base_price: Milli(item.base_price),
                max_durability: item.max_durability,
                // 以下全部先留空，与 `register-item` 一样——真正的取值
                // 由 `apply_item_extras` 走各自的 `set_*`/`add_*` 写入，
                // 保住那些方法自带的注册期校验（重复标签、重复配方……）。
                equip_mask: SlotMask::EMPTY,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                tags: Vec::new(),
                taught_recipes: Vec::new(),
                blind_box_pool: Vec::new(),
                // 这两条**不**留空：它们没有任何跨表引用要校验，因此
                // 不需要一条单独的 `set_*` 入口，见
                // `crate::item::ItemAttrs::requires_identification`。
                requires_identification: item.requires_identification,
                study_experience: item.study_experience,
            },
        )
        .map_err(|err: ItemError| err.to_string())
}

/// 写入物品的九类追加声明。
fn apply_item_extras(
    registry: &Registry,
    table: &mut ItemTable,
    tags: &TagTable,
    recipes: &RecipeTable,
    modifier_types: &ModifierTypeTable,
    index: ContentIndex,
    item: &RawItem,
) -> Applied {
    let to_err = |err: ItemError| err.to_string();

    if !item.equip_slots.is_empty() {
        let mut mask = SlotMask::EMPTY;
        for name in &item.equip_slots {
            let slot =
                EquipSlot::from_name(name).ok_or_else(|| format!("未知的装备槽位名称 {name:?}"))?;
            mask = mask.union(slot.mask());
        }
        table.set_equip_mask(index, mask).map_err(to_err)?;
    }

    for bonus in &item.stat_bonuses {
        let target = stat_target_from_str(&bonus.target)
            .ok_or_else(|| format!("未知的属性加成目标 {:?}", bonus.target))?;
        table
            .add_stat_bonus(
                index,
                StatBonus {
                    target,
                    amount: bonus.amount,
                },
            )
            .map_err(to_err)?;
    }

    if let Some(effect) = &item.use_effect {
        let resolved: SkillEffect = effect.resolve()?;
        table.set_use_effect(index, resolved).map_err(to_err)?;
    }

    if let Some(penetration) = &item.penetration {
        table
            .set_penetration(
                index,
                Penetration {
                    flat: penetration.flat,
                    permille: penetration.permille,
                },
            )
            .map_err(to_err)?;
    }

    if let Some(raw) = &item.damage_formula {
        let formula = required_id(registry, raw, "伤害公式")?;
        table.set_damage_formula(index, formula).map_err(to_err)?;
    }

    if let Some(raw) = &item.damage_category {
        let category = required_id(registry, raw, "伤害类别")?;
        table.set_damage_category(index, category).map_err(to_err)?;
    }

    for resistance in &item.resistances {
        let category = required_id(registry, &resistance.damage_category, "伤害类别")?;
        let modifier_type = match resistance.modifier_type.as_deref() {
            None => None,
            Some(raw) => Some(required_modifier_type(registry, modifier_types, raw)?),
        };
        table
            .add_rule_modifier(
                index,
                TypedRuleModifier {
                    modifier_type,
                    modifier: RuleModifier::Resistance {
                        damage_category: category,
                        damage_reduction: clamp_to_i32(resistance.damage_reduction),
                    },
                },
            )
            .map_err(to_err)?;
    }

    for raw in &item.tags {
        let tag_index = required_id(registry, raw, "标签")?;
        let tag_def = tags.get(tag_index).ok_or_else(|| {
            format!("{raw:?} 是一个已注册的内容标识符，但它不是标签（没有登记在标签表里）")
        })?;
        table
            .add_tag(index, tag_index, tag_def.wear)
            .map_err(to_err)?;
    }

    for raw in &item.taught_recipes {
        let recipe_index = required_id(registry, raw, "配方")?;
        if !recipes.is_defined(recipe_index) {
            return Err(format!(
                "{raw:?} 是一个已注册的内容标识符，但它不是配方（没有登记在配方表里）"
            ));
        }
        table
            .add_taught_recipe(index, recipe_index)
            .map_err(to_err)?;
    }

    // 盲盒产出池——跨表校验与上面 `taught_recipes` 那一段逐行同构：
    // 候选必须是一件真的登记在物品表里的物品，拼错一个 id 的症状是
    // 「这个盒子静默少一档产出」。
    for raw in &item.blind_box_pool {
        let product = required_id(registry, &raw.item, "物品")?;
        if !table.is_defined(product) {
            return Err(format!(
                "{:?} 是一个已注册的内容标识符，但它不是物品（没有登记在物品表里）",
                raw.item
            ));
        }
        // 一个盲盒不能开出它自己——效果顺序的硬约束，不是风格偏好：
        // `resolve_identify` 产出的 `ConsumeInventoryItem` 与
        // `MergeIntoInventory` 按同一对 `(def, durability)` 定位同一堆，
        // 而后者的结果是在**消耗之前**的背包上算出来的，自产出的盒子会
        // 让这两条效果互相抵消，症状是「开了个盒子，什么都没发生」。
        // 拦在注册期，结算侧就不需要一条只为一种病态内容存在的分支。
        if product == index {
            return Err(format!(
                "盲盒 {:?} 把自己列进了产出池——开出自己没有意义，且会让消耗与产出两条效果互相抵消",
                item.id
            ));
        }
        table
            .add_blind_box_entry(
                index,
                BlindBoxEntry {
                    item: product,
                    count: raw.count,
                    weight: raw.weight,
                },
            )
            .map_err(to_err)?;
    }

    Ok(())
}

/// `modifier_types.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierTypeFile {
    /// 加值类型名册，按书写顺序注册。
    pub modifier_types: Vec<RawModifierType>,
}

/// 一条加值类型声明——只有 id。
///
/// 与 `weapon_categories.json5` 那一条同一种形状（也只有 id 加一个
/// 可选字段），理由见 `crate::modifier_type` 模块文档「为什么
/// [`crate::modifier_type::ModifierTypeDef`] 一个字段都没有」一节：
/// 加值类型是纯身份。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawModifierType {
    /// 完整命名空间标识符。
    pub id: String,
}

/// 把一批加值类型写进注册表与加值类型表。
pub fn apply_modifier_types(
    registry: &mut Registry,
    table: &mut ModifierTypeTable,
    modifier_types: &[RawModifierType],
) -> Applied {
    for raw in modifier_types {
        let index = intern_id(registry, &raw.id, "加值类型标识符")?;
        table
            .define(index, ModifierTypeDef {})
            .map_err(|err: ModifierTypeError| err.to_string())?;
    }
    Ok(())
}

/// 把一个 `i64` 声明值收进 `i32`——饱和，不回绕。
///
/// 内容文件里的数值一律先反序列化成 `i64`（与本模块其余
/// `Option<i64>` 字段同一条既有惯例：json5 的整数可以写得比 `i32` 大,
/// 反序列化阶段不该因此失败），再在这里收窄。饱和而不是 `as i32` 直接
/// 截断：一条写成 `10_000_000_000` 的减伤点数应当是「大得挡住一切」,
/// 不该回绕成一个负数（那会静默变成「脆弱」，方向完全相反）。
fn clamp_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// 解析一个**必须已注册**的加值类型标识符。
///
/// 两级校验，缺一不可：
///
/// 1. [`required_id`]——这个 id 在 [`Registry`] 里出现过吗。
/// 2. [`ModifierTypeTable::is_defined`]——它**真的是一个加值类型**吗,
///    而不是恰好同名的某条天赋/物品。
///
/// 第二级不能省：只查 [`Registry`] 等于没查（它对任何已注册内容都返回
/// `Some`），而这条校验拦的正是 `"lostland:enhancment"` 这类拼写错误。
/// 与 [`RawItem::tags`]/[`RawItem::taught_recipes`] 的「是已注册 id，
/// 但它是不是标签/配方」是同一条既有纪律。
fn required_modifier_type(
    registry: &Registry,
    modifier_types: &ModifierTypeTable,
    raw: &str,
) -> Result<ContentIndex, String> {
    let index = required_id(registry, raw, "加值类型")?;
    if !modifier_types.is_defined(index) {
        return Err(format!(
            "{raw:?} 是一个已注册的内容标识符，但它不是加值类型（没有登记在加值类型表里）"
        ));
    }
    Ok(index)
}

/// 加成目标名 → [`StatTarget`]：七个属性名，外加 `"armor"` 与
/// `"insulation"` 这两个不属于 [`ll_world::entity::AttributeKind`] 的目标。
fn stat_target_from_str(name: &str) -> Option<StatTarget> {
    match name {
        "armor" => Some(StatTarget::Armor),
        "insulation" => Some(StatTarget::Insulation),
        other => attribute_kind_from_str(other)
            .ok()
            .map(StatTarget::Attribute),
    }
}

// ───────────────────────────── 配方 ─────────────────────────────

/// 一条食材声明。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawIngredient {
    /// 物品的完整标识符（intern，允许物品表后到）。
    pub item: String,
    /// 需要几件。
    pub count: u32,
}

/// 一条配方声明——对应此前 `register-recipe` 的七个位置参数外加
/// `recipe-requires-station!` / `-tool!` / `-discovery!` 三条追加指令。
///
/// 食材从「两个平行列表」（`(list "a" "b")` + `(list 1 2)`）换成一个
/// 对象数组：平行列表长度不一致会报错，但**长度一致而顺序错位**不会
/// ——症状是配方悄悄要 2 份肉 1 份草，正是本批次要消灭的失败模式。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRecipe {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 所属配方类别，**必须已注册**。
    pub category: String,
    /// 食材表，恒非空。
    pub ingredients: Vec<RawIngredient>,
    /// 产出物品（intern）。
    pub product: String,
    /// 产出数量。
    pub product_count: u32,
    /// 需要的制作场地（地形，intern），缺省无。
    #[serde(default)]
    pub required_station: Option<String>,
    /// 需要的工具（物品，intern），缺省无。
    #[serde(default)]
    pub required_tool: Option<String>,
    /// 这条配方是否需要先被"发现"才能制作，缺省否。
    #[serde(default)]
    pub requires_discovery: bool,
}

/// 把一批配方写进注册表与配方表。
pub fn apply_recipes(
    registry: &mut Registry,
    table: &mut RecipeTable,
    recipes: &[RawRecipe],
) -> Applied {
    for recipe in recipes {
        let to_err = |err: RecipeError| err.to_string();
        let category = required_id(registry, &recipe.category, "配方类别")?;
        let mut ingredients = Vec::with_capacity(recipe.ingredients.len());
        for ingredient in &recipe.ingredients {
            ingredients.push(RecipeIngredient {
                item: intern_id(registry, &ingredient.item, "食材标识符")?,
                count: ingredient.count,
            });
        }
        let product = intern_id(registry, &recipe.product, "成品标识符")?;
        let display_name_key = parse_id(&recipe.display_name_key, "本地化键标识符")?;
        let index = intern_id(registry, &recipe.id, "配方标识符")?;

        table
            .define(
                index,
                RecipeAttrs {
                    display_name_key,
                    category,
                    ingredients,
                    product,
                    product_count: recipe.product_count,
                },
            )
            .map_err(to_err)?;

        if let Some(raw) = &recipe.required_station {
            let station = intern_id(registry, raw, "场地标识符")?;
            table.set_required_station(index, station).map_err(to_err)?;
        }
        if let Some(raw) = &recipe.required_tool {
            let tool = intern_id(registry, raw, "工具标识符")?;
            table.set_required_tool(index, tool).map_err(to_err)?;
        }
        if recipe.requires_discovery {
            table.set_requires_discovery(index).map_err(to_err)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    /// 造一件除指定字段外全部取默认值的物品声明——三条鉴定相关注册期
    /// 校验共用，避免每条测试各抄一遍十六个字段。
    fn raw_item(id: &str, requires_identification: bool, study_experience: i64) -> RawItem {
        RawItem {
            id: id.to_string(),
            display_name_key: format!("{id}.name"),
            stack_limit: 1,
            base_weight: 50,
            base_price: 2000,
            max_durability: None,
            equip_slots: Vec::new(),
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: None,
            damage_formula: None,
            damage_category: None,
            resistances: Vec::new(),
            tags: Vec::new(),
            taught_recipes: Vec::new(),
            requires_identification,
            study_experience,
            blind_box_pool: Vec::new(),
        }
    }

    #[test]
    fn 负数研究经验被拒绝() {
        // ADR 0017「注册期完整校验」——负经验没有任何设计含义，只会是
        // 笔误，与 `RaceDef::xp_reward < 0` 同一条既有先例。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item = raw_item("m:relic", true, -1);

        // Act
        let result = define_one_item(&mut registry, &mut table, &item);

        // Assert
        assert!(result.is_err(), "负数研究经验必须当场报错");
    }

    #[test]
    fn 声明了盲盒池却不需要鉴定的物品被拒绝() {
        // 没有任何动作能打开这样一个盒子，症状是「这件东西什么都不做」
        // ——正是注册期该拦下的那类静默内容缺陷。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let prize = raw_item("m:prize", false, 0);
        define_one_item(&mut registry, &mut table, &prize).expect("普通物品应当注册成功");
        let mut boxed = raw_item("m:box", false, 0);
        boxed.blind_box_pool = vec![RawBlindBoxEntry {
            item: "m:prize".to_string(),
            count: 1,
            weight: 1,
        }];

        // Act
        let result = define_one_item(&mut registry, &mut table, &boxed);

        // Assert
        assert!(
            result.is_err(),
            "声明了 blind_box_pool 却没有 requires_identification 必须当场报错"
        );
    }

    #[test]
    fn 把自己列进产出池的盲盒被拒绝() {
        // 效果顺序的硬约束——见 apply_item_extras 里那段注释与
        // `ll_sim::resolve::resolve_identify` 文档「一个盲盒不能开出它
        // 自己」一节。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let tags = TagTable::new();
        let recipes = RecipeTable::new();
        let mut boxed = raw_item("m:box", true, 5);
        boxed.blind_box_pool = vec![RawBlindBoxEntry {
            item: "m:box".to_string(),
            count: 1,
            weight: 1,
        }];
        define_one_item(&mut registry, &mut table, &boxed).expect("主体声明本身合法");
        let index = registry
            .get(&NamespacedId::parse("m:box").expect("合法标识符"))
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = apply_item_extras(
            &registry,
            &mut table,
            &tags,
            &recipes,
            &ModifierTypeTable::new(),
            index,
            &boxed,
        );

        // Assert
        assert!(result.is_err(), "自产出的盲盒必须当场报错");
    }

    #[test]
    fn 权重为零的盲盒候选被拒绝() {
        // 「写了等于没写」的候选永远抽不中，与其静默吞掉不如当场报错。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let tags = TagTable::new();
        let recipes = RecipeTable::new();
        let prize = raw_item("m:prize", false, 0);
        define_one_item(&mut registry, &mut table, &prize).expect("普通物品应当注册成功");
        let mut boxed = raw_item("m:box", true, 5);
        boxed.blind_box_pool = vec![RawBlindBoxEntry {
            item: "m:prize".to_string(),
            count: 1,
            weight: 0,
        }];
        define_one_item(&mut registry, &mut table, &boxed).expect("主体声明本身合法");
        let index = registry
            .get(&NamespacedId::parse("m:box").expect("合法标识符"))
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = apply_item_extras(
            &registry,
            &mut table,
            &tags,
            &recipes,
            &ModifierTypeTable::new(),
            index,
            &boxed,
        );

        // Assert
        assert!(result.is_err(), "权重为零的候选必须当场报错");
    }

    #[test]
    fn 物品的max_durability缺席表示没有耐久() {
        // 此前脚本里用 -1 这个哨兵值，数据文件换成字段缺席——本测试
        // 钉住换法没有把语义弄丢。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item = RawItem {
            id: "m:arrow".to_string(),
            display_name_key: "m:arrow.name".to_string(),
            stack_limit: 99,
            base_weight: 50,
            base_price: 2000,
            max_durability: None,
            equip_slots: Vec::new(),
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: None,
            damage_formula: None,
            damage_category: None,
            resistances: Vec::new(),
            tags: Vec::new(),
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
        };

        // Act
        define_one_item(&mut registry, &mut table, &item).expect("合法声明应当注册成功");

        // Assert
        let index = registry
            .get(&NamespacedId::parse("m:arrow").expect("合法标识符"))
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).expect("已注册").max_durability, None);
    }

    #[test]
    fn 可堆叠物品带耐久上限被拒绝() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item = RawItem {
            id: "m:arrow".to_string(),
            display_name_key: "m:arrow.name".to_string(),
            stack_limit: 99,
            base_weight: 50,
            base_price: 2000,
            max_durability: Some(10),
            equip_slots: Vec::new(),
            stat_bonuses: Vec::new(),
            use_effect: None,
            penetration: None,
            damage_formula: None,
            damage_category: None,
            resistances: Vec::new(),
            tags: Vec::new(),
            taught_recipes: Vec::new(),
            requires_identification: false,
            study_experience: 0,
            blind_box_pool: Vec::new(),
        };

        // Act
        let result = define_one_item(&mut registry, &mut table, &item);

        // Assert
        assert!(result.is_err_and(|err| err.contains("与堆叠矛盾")));
    }

    #[test]
    fn 食材是对象数组顺序错位不再可能() {
        // 平行列表时代 (list "a" "b") + (list 1 2) 顺序错位不报错；
        // 对象数组把数量绑在食材自己身上，结构上错不了。
        // Arrange & Act
        let file: RawRecipe = json5::from_str(
            r#"{ id: "m:stew", display_name_key: "m:stew.name", category: "m:cooking",
                 ingredients: [ { item: "m:meat", count: 1 }, { item: "m:herb", count: 2 } ],
                 product: "m:stew_item", product_count: 1 }"#,
        )
        .expect("合法配方");

        // Assert
        assert_eq!(file.ingredients[1].item, "m:herb");
        assert_eq!(file.ingredients[1].count, 2);
    }

    /// 造一条只填了 `kind` 的规则修正，其余字段由各测试自己覆盖。
    fn bare_modifier(kind: &str) -> RawRuleModifier {
        RawRuleModifier {
            kind: kind.to_string(),
            modifier_type: None,
            damage_category: None,
            damage_reduction: None,
            suspicion_reduction_permille: None,
            conceal_permille: None,
            luck_chance_permille_per_point: None,
            extra_damage: None,
            recipe_category: None,
            bonus_product_count: None,
        }
    }

    #[test]
    fn 规则修正填了与kind不搭的字段报错() {
        // Arrange：潜行偷袭却填了伤害类别。
        let modifier = RawRuleModifier {
            damage_category: Some("m:acid".to_string()),
            luck_chance_permille_per_point: Some(20),
            extra_damage: Some(15),
            ..bare_modifier("sneak-attack")
        };

        // Act
        let result = modifier.resolve(&Registry::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("damage_category")));
    }

    #[test]
    fn 抗性填了盘查那一条的字段同样报错() {
        // 新字段 `suspicion_reduction_permille` 也进了逐分支的拒字段
        // 清单——加值类型批次把两个乘数字段拆成两个同名不同义的点数
        // 字段，拒字段清单必须跟着拆，否则「填错分支」这一类内容错误会
        // 从此静默通过。
        // Arrange
        let modifier = RawRuleModifier {
            damage_category: Some("m:acid".to_string()),
            damage_reduction: Some(3),
            suspicion_reduction_permille: Some(400),
            ..bare_modifier("resistance")
        };

        // Act
        let result = modifier.resolve(&Registry::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("suspicion_reduction_permille")));
    }

    #[test]
    fn 引用未注册的加值类型当场报错() {
        // 项目所有者「引用未注册的类型要当场报错」那条裁定的直接落点,
        // 也是 `ll_mod::modifier_type` 模块文档点名的那个失效模式：一个
        // 拼错的类型会**自成一个桶**，于是比正确写法更强（不与同类竞争、
        // 还能与别人相加）——静默接受是最坏的选项。
        // Arrange：类型 id 连 intern 都没有过。
        let mut registry = Registry::new();
        let acid = registry.intern(parse_id("m:acid", "伤害类别").unwrap());
        let _ = acid;
        let modifier = RawRuleModifier {
            damage_category: Some("m:acid".to_string()),
            damage_reduction: Some(3),
            modifier_type: Some("m:enhancement".to_string()),
            ..bare_modifier("resistance")
        };

        // Act
        let result = modifier.resolve_typed(&registry, &ModifierTypeTable::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("加值类型")));
    }

    #[test]
    fn 已注册但不是加值类型的标识符同样被拒() {
        // 只查 Registry 等于没查——它对任何已注册内容都返回 Some。这条
        // 反例证明 `required_modifier_type` 真的查了加值类型表本身,与
        // `RawItem::tags`「是已注册 id,但它是不是标签」是同一条纪律。
        // Arrange：`m:acid` 已注册（作为伤害类别），但没进加值类型表。
        let mut registry = Registry::new();
        registry.intern(parse_id("m:acid", "伤害类别").unwrap());
        let modifier = RawRuleModifier {
            damage_category: Some("m:acid".to_string()),
            damage_reduction: Some(3),
            modifier_type: Some("m:acid".to_string()),
            ..bare_modifier("resistance")
        };

        // Act
        let result = modifier.resolve_typed(&registry, &ModifierTypeTable::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("不是加值类型")));
    }

    #[test]
    fn 不声明加值类型时解析出的类型为none() {
        // 默认值那条承诺在解析层的落点：整条字段缺席 = `None` = 那个
        // 共享的未分类桶，见
        // `ll_sim::rule_modifier::TypedRuleModifier::modifier_type` 文档。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(parse_id("m:acid", "伤害类别").unwrap());
        let modifier = RawRuleModifier {
            damage_category: Some("m:acid".to_string()),
            damage_reduction: Some(3),
            ..bare_modifier("resistance")
        };

        // Act
        let resolved = modifier
            .resolve_typed(&registry, &ModifierTypeTable::new())
            .expect("不声明类型的解析恒成功");

        // Assert
        assert_eq!(resolved.modifier_type, None);
    }

    // ── craft-yield（制作类副职奖励批次）───────────────────────────

    #[test]
    fn 制作产出加成解析出配方类别与件数() {
        // Arrange：配方类别先注册（CONTENT_FILES 保证 Crafting 排在
        // Traits 之前，因此这里只 get 不 intern 是安全的）。
        let mut registry = Registry::new();
        let forging = registry.intern(parse_id("m:forging", "配方类别").unwrap());
        let modifier = RawRuleModifier {
            recipe_category: Some("m:forging".to_string()),
            bonus_product_count: Some(1),
            ..bare_modifier("craft-yield")
        };

        // Act
        let resolved = modifier.resolve(&registry).expect("合法声明");

        // Assert
        assert_eq!(
            resolved,
            RuleModifier::CraftYield {
                category: forging,
                bonus_product_count: 1,
            }
        );
    }

    #[test]
    fn 制作产出加成允许为负不被钳到零() {
        // 项目所有者裁定「允许为负，做成负面天赋」——照 `Resistance`
        // 允许「脆弱」的先例。保底在结算侧
        // （`ll_sim::rule_modifier::MINIMUM_CRAFT_PRODUCT_COUNT`），
        // 不在这里，若哪天有人在解析层补一个 `.max(0)`，本条会变红。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(parse_id("m:forging", "配方类别").unwrap());
        let modifier = RawRuleModifier {
            recipe_category: Some("m:forging".to_string()),
            bonus_product_count: Some(-2),
            ..bare_modifier("craft-yield")
        };

        // Act
        let resolved = modifier.resolve(&registry).expect("负值同样合法");

        // Assert
        assert!(matches!(
            resolved,
            RuleModifier::CraftYield {
                bonus_product_count: -2,
                ..
            }
        ));
    }

    #[test]
    fn 制作产出加成引用未注册的配方类别当场报错() {
        // 「只 get 不 intern，拼错当场报错」——与 subclasses.json5 的
        // unlock.target 同一套报错，不是第二样要学的东西。
        // Arrange：类别 id 连 intern 都没有过。
        let modifier = RawRuleModifier {
            recipe_category: Some("m:frging".to_string()),
            bonus_product_count: Some(1),
            ..bare_modifier("craft-yield")
        };

        // Act
        let result = modifier.resolve(&Registry::new());

        // Assert
        assert!(result.is_err_and(|err| err.contains("配方类别")));
    }

    #[test]
    fn 制作产出加成填了与kind不搭的字段报错() {
        // Arrange：制作加成却填了伤害类别。
        let mut registry = Registry::new();
        registry.intern(parse_id("m:forging", "配方类别").unwrap());
        let modifier = RawRuleModifier {
            recipe_category: Some("m:forging".to_string()),
            bonus_product_count: Some(1),
            damage_category: Some("m:acid".to_string()),
            ..bare_modifier("craft-yield")
        };

        // Act
        let result = modifier.resolve(&registry);

        // Assert
        assert!(result.is_err_and(|err| err.contains("damage_category")));
    }

    #[test]
    fn 其余四个kind都不接受制作产出加成的两个字段() {
        // 上一条的对偶：新增字段必须被既有四个分支逐条拒掉，否则
        // 「填了但没生效」会静默通过——这正是每个分支先 reject 再取值
        // 那套写法要防的东西。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(parse_id("m:acid", "伤害类别").unwrap());

        // Act & Assert
        for kind in [
            "resistance",
            "sneak-attack",
            "inspection-suspicion",
            "inspection-concealment",
        ] {
            let modifier = RawRuleModifier {
                recipe_category: Some("m:forging".to_string()),
                ..bare_modifier(kind)
            };
            assert!(
                modifier
                    .resolve(&registry)
                    .is_err_and(|err| err.contains("recipe_category")),
                "{kind} 必须拒绝 recipe_category"
            );
            let modifier = RawRuleModifier {
                bonus_product_count: Some(1),
                ..bare_modifier(kind)
            };
            assert!(
                modifier
                    .resolve(&registry)
                    .is_err_and(|err| err.contains("bonus_product_count")),
                "{kind} 必须拒绝 bonus_product_count"
            );
        }
    }
}
