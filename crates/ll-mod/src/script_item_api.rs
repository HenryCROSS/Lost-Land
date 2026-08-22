//! 把 `register-item`/`register-item-equip-mask`/`register-item-stat-bonus`/
//! `register-item-use-effect`/`register-item-penetration` 注册进脚本
//! 引擎：mod 脚本借此定义自定义物品（箭矢、铁剑、药水……）、它们的装备
//! 占位掩码、静态属性加成、使用效果与穿透，落地
//! `knowledge/design/item-system.md`/`knowledge/design/equipment-slots.md`/
//! `knowledge/design/attribute-system.md`。
//!
//! 模式同 [`crate::script_resource_pool_api`]：扁平参数,没有为
//! `Option<i32>`（`max-durability`）或 `Milli`（`base-weight`/
//! `base-price`）发明任何新的 FFI 编码方式,理由见下面两节。
//!
//! # 为什么 `equip_mask`/`stat_bonuses`/`use_effect`/`penetration` 都走
//! 独立函数（装备栏位批次，P6 第三批；`derive_stats` 与装备属性接进
//! 战斗，P6 第四批；耐久与 `Intent::Use`，P6 第五批；武器引用与穿透
//! 接线，P6 第六批）
//!
//! `register-item` 已经是仓库里真实 mod 脚本
//! （`mods/example_mod/gameplay.scm`）在用的六参数签名——改参数个数
//! 会破坏已有脚本，与 `register-race-xp-reward`/
//! `register-trait-resource-pool` 「新增能力用新函数」同一条既有先例
//! （见 [`crate::item::ItemDef::equip_mask`] 文档）。`register-item-equip-mask`/
//! `register-item-stat-bonus`/`register-item-use-effect`/
//! `register-item-penetration` 因此都走独立函数，追加对象都是已经通过
//! `register-item` 注册过的物品。
//!
//! # `register-item-use-effect` 的参数形状照抄 `register-skill` 的
//! `effect-kind`/`effect-tag`/`effect-amount`/`effect-amount2` 四元组
//!
//! 物品使用效果复用 [`SkillEffect`]（见
//! [`ll_sim::item::ItemRule::use_effect`] 文档「为什么复用 SkillEffect」
//! 一节）——脚本层的编码没有理由另起一套，`parse_use_effect`（本模块）
//! 与 `crate::script_skill_api::parse_effect` 是对同一个目标类型
//! `SkillEffect` 的两份独立解析实现，理由同
//! `crate::script_skill_api::attribute_kind_from_str` 文档「两个模块
//! 目前都足够小，重复比引入一层间接更直接」——两个模块各自的调用点
//! 语境不同（一个在解析技能定义，一个在解析物品定义），共享一个解析
//! 函数需要先把两处的错误消息措辞、字段名字对齐,得不偿失。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_core::scaled::Milli;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::item::{ItemAttrs, ItemError, ItemTable};
use crate::registry::Registry;
use ll_sim::combat::Penetration;
use ll_sim::item::{EquipSlot, SlotMask, StatBonus, StatTarget};
use ll_sim::rule_modifier::RuleModifier;
use ll_sim::skill::{ResourceKind, SkillEffect};
use ll_world::entity::AttributeKind;

thread_local! {
    /// 当前调用窗口内，`register-item` 应该写入的物品表。
    static ACTIVE_TABLE: RefCell<Option<ItemTable>> = const { RefCell::new(None) };
}

/// 「武器」这一组的槽位——`equipment-slots.md` 槽位表里 **主手与副手
/// 同属一组**（原文分组列首行「武器」覆盖 `MAIN_HAND`/`OFF_HAND` 两行），
/// 与「头部」「躯干」等其余分组并列。`do_register_item_equip_mask` 用它
/// 判断"这件物品算不算武器"（耐久与武器槽位的组合校验，见其文档「为
/// 什么在这里校验耐久与武器槽位的组合」一节）——不是只有主手才算数,
/// 副手同样可以是「副武器」（该槽位官方说明原文：「盾、副武器、法器」）。
const WEAPON_GROUP_SLOTS: SlotMask = EquipSlot::MAIN_HAND
    .mask()
    .union(EquipSlot::OFF_HAND.mask());

/// 把 `table` 设为当前调用窗口内 `register-item` 可写入的目标。
pub fn set_active_target(table: ItemTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ItemTable`。
pub fn take_active_target() -> ItemTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-item`/`register-item-equip-mask` 注册进 `engine`。
pub fn register_item_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-item", register_item);
    engine.register_fn("register-item-equip-mask", register_item_equip_mask);
    engine.register_fn("register-item-stat-bonus", register_item_stat_bonus);
    engine.register_fn("register-item-use-effect", register_item_use_effect);
    engine.register_fn("register-item-penetration", register_item_penetration);
    engine.register_fn("register-item-damage-formula", register_item_damage_formula);
    engine.register_fn(
        "register-item-damage-category",
        register_item_damage_category,
    );
    engine.register_fn("register-item-resistance", register_item_resistance);
}

/// `(register-item id display-name-key stack-limit base-weight base-price max-durability)`。
///
/// - `id`：完整命名空间标识符字符串。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
/// - `stack-limit`：堆叠上限，必须 ≥ 1（`0` 没有意义——一堆连一个都
///   装不下的物品不该存在，直接拒绝而不是静默钳位成 1，理由同
///   `register-resource-pool` 拒绝 `tier-count == 0` 的文档）。`1`
///   表示不可堆叠。
/// - `base-weight`/`base-price`：以 `Milli` 千分之一为单位的**原始**
///   整数——`Milli(1_500)` 表示 1.5，这里的参数就是 `1500`,不是
///   "整数会被自动乘 1000"那种写法，与 `Milli` 自身文档「`Milli(1_500)`
///   表示 1.5」同一个换算关系，没有为它另外发明一层"填整数、内部
///   放大"的转换（那会让内容作者搞不清一个数字究竟是"1.5"还是
///   "填 1 会自动变 1000"，读脚本时也看不出来）。
/// - `max-durability`：耐久上限，`-1` 表示这件物品没有耐久概念
///   （`None`），`>= 0` 表示有（`Some`）——与 `register-terrain` 的
///   `opens-into` 用空串表示 `None` 是同一条"用一个该字段合法值域之外
///   的哨兵表示空"的既有约定，只是这里的字段是数值,空串哨兵不适用，
///   改用负数（耐久上限本身不该是负的，`-1` 因此是安全的哨兵）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item(
    id: String,
    display_name_key: String,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item(
                registry,
                table,
                &id,
                &display_name_key,
                stack_limit,
                base_weight,
                base_price,
                max_durability,
            )
        })
    })
}

/// [`register_item`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_item(
    registry: &mut Registry,
    table: &mut ItemTable,
    id: &str,
    display_name_key: &str,
    stack_limit: i64,
    base_weight: i64,
    base_price: i64,
    max_durability: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    if stack_limit < 1 {
        return Err(format!("堆叠上限 {stack_limit} 非法（必须 >= 1）"));
    }
    let max_durability = match max_durability {
        -1 => None,
        value if value >= 0 => Some(value as i32),
        other => {
            return Err(format!(
                "耐久上限 {other} 非法（必须 >= 0，或用 -1 表示无耐久）"
            ));
        }
    };
    // 可堆叠物品不该有耐久——耐久落地之后，两把用过的剑几乎必然耐久
    // 不同（P6 第一批定的 can_merge 判据：def 相同且耐久相同才能合并），
    // 若一件 stack_limit > 1 的物品也携带耐久，每一份实例几乎必然各自
    // 独立成一格，堆叠机制名存实亡——这不是"可以但不建议"的边缘情形,
    // 是两条规则字面矛盾（"能堆叠"暗示"多份同质可以共存一格"，"有耐久"
    // 暗示"每份实例携带自己独立的状态"）,注册期直接拒绝，不留一个
    // 事后才会在背包 UI 上炸出来的组合,与 `register-resource-pool`
    // 拒绝矛盾配置同一条"非法组合即拒绝,不留给运行时躺平"纪律。
    if stack_limit > 1 && max_durability.is_some() {
        return Err(format!(
            "可堆叠物品（堆叠上限 {stack_limit}）不能携带耐久上限——耐久会让每份实例各自独立，与堆叠矛盾"
        ));
    }

    table
        .define(
            index,
            ItemAttrs {
                display_name_key,
                stack_limit: stack_limit as u32,
                base_weight: Milli(base_weight),
                base_price: Milli(base_price),
                max_durability,
                // 恒为空——register-item 的六参数签名不接受装备占位
                // 掩码，真正的取值由后续 register-item-equip-mask 调用
                // 写入，见模块文档「为什么 equip_mask 走独立函数」一节。
                equip_mask: SlotMask::EMPTY,
                // 恒为空列表——同上，真正的取值由后续
                // register-item-stat-bonus 调用追加写入。
                stat_bonuses: Vec::new(),
                // 恒为 None——同上，真正的取值由后续
                // register-item-use-effect 调用写入。
                use_effect: None,
                // 恒为 Penetration::NONE——同上，真正的取值由后续
                // register-item-penetration 调用写入。
                penetration: Penetration::NONE,
                // 恒为 None——同上，真正的取值由后续
                // register-item-damage-formula 调用写入。
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-equip-mask id slot-names)`——追加声明「这件物品占用
/// 哪些装备槽位」（装备栏位批次，P6 第三批），见
/// [`crate::item::ItemDef::equip_mask`] 文档「为什么不是 `register-item`
/// 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在（ADR 0017「注册期完整校验」），未注册的 `id`
///   在装载期报错，而不是静默创建一条只有占位掩码、没有其余属性的
///   半成品物品记录，与 `register-race-xp-reward` 同一条纪律。
/// - `slot-names`：`knowledge/design/equipment-slots.md` 槽位表的
///   kebab-case 名称列表（`"main-hand"`/`"off-hand"`/……22 个引擎槽位
///   之一，见 [`ll_sim::item::EquipSlot::from_name`]）——不可为空列表
///   （空列表没有意义：一件"不占用任何槽位"的物品不该调用本函数,
///   `SlotMask::EMPTY` 已经是 `register-item` 注册时的默认值）。多个
///   名称按位或合并成最终掩码——双手武器传
///   `(list "main-hand" "off-hand")`，全身板甲传七个槽位名称的列表。
///   任意一个名称不在 22 个引擎槽位表内即拒绝整次调用（不静默忽略
///   未知名称,理由同 `register-item` 拒绝非法内容标识符）。
///
/// **覆盖，不是追加**——多次调用同一个 `id` 以最后一次为准，见
/// [`crate::item::ItemTable::set_equip_mask`] 文档「覆盖，不是追加」
/// 一节。
///
/// # 为什么在这里校验耐久与武器槽位的组合（武器引用与穿透接线批次，
/// P6 第六批）
///
/// 项目所有者裁定「装备武器才有耐久，其余物品我倾向于没有」——`register-item`
/// 已经有「可堆叠物品不能携带耐久上限」这一条注册期拒绝的先例（见其
/// 文档），本函数照办同一条纪律，只是判据换成「有耐久却不占武器槽位」。
///
/// 「武器槽位」取 `equipment-slots.md` 槽位表自己的分组——**主手与副手
/// 同属「武器」这一组**（原文「副手：盾、副武器、法器」），不是只有
/// 主手才算武器：`mods/example_mod/gameplay.scm` 已注册的木盾
/// （`examplemod:wooden_shield`，只占副手）本身带耐久，若把判据收窄成
/// 「必须包含主手」会当场拒绝这份已经存在、已经通过测试的真实内容——
/// 那不是本批次要修正的缺陷，是这件物品本就该继续合法。判据因此是
/// 「掩码与 `MAIN_HAND | OFF_HAND` 有交集」，即
/// [`WEAPON_GROUP_SLOTS`]。
///
/// 与 `resolve_attack` 结算时只读主手（见其文档「武器引用」一节）不是
/// 同一个判据，两者服务不同的问题：本函数回答「这件物品的类型允不允许
/// 有耐久」（登记时定型，主手/副手皆可），`resolve_attack` 回答「这一下
/// 攻击具体用的是哪一件」（运行时只看主手，因为一次 `Intent::Attack`
/// 只产出一次伤害判定，不模拟双持连击）——一个是"这类物品算不算武器"
/// 的内容分类问题，一个是"这一下打出去用的是哪一件"的结算问题，判据
/// 范围不必相同。
///
/// 这条校验不能放进 `register-item` 本身：耐久上限（`max-durability`）
/// 是 `register-item` 六参数签名自带的既有参数，而装备占位掩码要等到
/// **后续**这条独立调用才会写入（见模块文档「为什么 `equip_mask` 都走
/// 独立函数」一节）——`register-item` 执行的那一刻，这件物品的
/// `equip_mask` 恒是默认值 `SlotMask::EMPTY`，无法据此判断"将来会不会
/// 占武器槽位"。只有到了本函数——两个事实（`max_durability` 是否为
/// `Some`、即将写入的掩码是否与武器槽位相交）第一次同时可查的这一
/// 刻——才能原子地做这个判断，因此校验放在这里，不在 `register-item`。
///
/// 一件物品若声明了耐久上限，之后把占位掩码设置成与武器槽位不相交
/// （不占主手也不占副手，包括完全不调用本函数、掩码维持默认的
/// `SlotMask::EMPTY`——`examplemod:iron_sword` 目前正是这个状态，见
/// `mods/example_mod/gameplay.scm`：只 `register-item` 未追加占位掩码，
/// 因此从未触发过本函数,不受这条校验影响），即拒绝整次调用——**反例**
/// 见 `do_register_item_equip_mask` 测试「掩码与武器槽位相交时耐久声明
/// 注册成功」：掩码包含副手（不含主手）时同样的耐久声明放行，证明这条
/// 校验拒绝的是"有耐久却不占任何武器槽位"这个组合本身，不是耐久上限
/// 这个字段恒被拒绝，也不是只有主手才算数。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_equip_mask(id: String, slot_names: Vec<String>) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-equip-mask 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_equip_mask(registry, table, &id, &slot_names)
        })
    })
}

/// [`register_item_equip_mask`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_equip_mask(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    slot_names: &[String],
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    if slot_names.is_empty() {
        return Err("装备占位掩码不能是空列表".to_string());
    }

    let mut mask = SlotMask::EMPTY;
    for name in slot_names {
        let Some(slot) = EquipSlot::from_name(name) else {
            return Err(format!("未知的装备槽位名称 {name:?}"));
        };
        mask = mask.union(slot.mask());
    }

    // 有耐久却不占任何武器槽位（主手/副手）——两条规则字面矛盾
    // （"有耐久"暗示"是一件会被使用、磨损的武器"，"不占任何武器槽位"
    // 暗示"不是武器"），注册期直接拒绝，理由见本函数文档「为什么在这里
    // 校验耐久与武器槽位的组合」一节。
    let max_durability = table
        .get(index)
        .expect("上面已经用 registry.get 确认过 id 存在，index 必然已被 define 过")
        .max_durability;
    if max_durability.is_some() && !mask.intersects(WEAPON_GROUP_SLOTS) {
        return Err(format!(
            "物品 {id:?} 声明了耐久上限（{max_durability:?}），但新的装备占位掩码不占用任何武器槽位（主手/副手）——只有武器才允许携带耐久"
        ));
    }

    table
        .set_equip_mask(index, mask)
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-stat-bonus id target amount)`——追加一条静态属性
/// 加成（P6 第四批：`derive_stats` 与装备属性接进战斗），见
/// [`crate::item::ItemDef::stat_bonuses`] 文档「为什么不是 `register-item`
/// 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，与 [`register_item_equip_mask`] 同一条 ADR 0017
///   「注册期完整校验」纪律。
/// - `target`：加成目标名——六个主属性名之一（`"strength"`/`"dexterity"`/
///   `"constitution"`/`"intelligence"`/`"willpower"`/`"charisma"`，与
///   `crate::script_skill_api::attribute_kind_from_str`/
///   `crate::script_class_api::attribute_kind_from_str` 同一份映射的
///   独立拷贝，理由同它们的文档）、`"armor"`（直接加护甲，见
///   [`ll_sim::item::StatTarget::Armor`] 文档「为什么不是只有
///   `AttributeKind` 一种取值」一节），或 `"insulation"`（保暖绝缘值，
///   温度系统批次新增，单位是十分之一摄氏度，见
///   [`ll_sim::item::StatTarget::Insulation`]）。未知名称拒绝整次调用。
/// - `amount`：增减量，可为负（诅咒装备）。
///
/// **累积，不是覆盖**——多次调用同一个 `id` 会依次追加多条加成，不是
/// 以最后一次为准，见 [`crate::item::ItemTable::add_stat_bonus`] 文档
/// 「追加，不是覆盖」一节。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_stat_bonus(id: String, target: String, amount: i64) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-stat-bonus 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_stat_bonus(registry, table, &id, &target, amount)
        })
    })
}

/// [`register_item_stat_bonus`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_stat_bonus(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    target: &str,
    amount: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    let target =
        stat_target_from_str(target).ok_or_else(|| format!("未知的属性加成目标 {target:?}"))?;

    table
        .add_stat_bonus(
            index,
            StatBonus {
                target,
                amount: amount as i32,
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// 加成目标名字符串 → [`StatTarget`]——七个属性名（六项主属性 + 幸运，
/// 幸运并入 `AttributeKind` 批次新增）复用
/// `crate::script_skill_api::attribute_kind_from_str` 同一份映射（各
/// 模块独立拷贝一份的既有先例，理由同其文档），额外多认识 `"armor"`
/// 与 `"insulation"` 这两个不属于 `AttributeKind` 的目标。`"luck"` 正是幸运戒指一类装备
/// （`register-item-stat-bonus id luck N`）的 authoring 入口——幸运并入
/// `AttributeKind` 批次要求装备能影响幸运，这里是决定性的一步。
fn stat_target_from_str(name: &str) -> Option<StatTarget> {
    if name == "armor" {
        return Some(StatTarget::Armor);
    }
    // 保暖绝缘值（温度系统批次）——与 `"armor"` 并列的第二个不属于
    // `AttributeKind` 的目标。`derive_stats` 对它与护甲走**同一段求和
    // 算法**（两层衣服比一层暖），不是 `ItemDef.rule_modifiers` 那条
    // tie-break 通道，见 `ll_world::item::StatTarget::Insulation` 文档。
    if name == "insulation" {
        return Some(StatTarget::Insulation);
    }
    let attribute = match name {
        "strength" => AttributeKind::Strength,
        "dexterity" => AttributeKind::Dexterity,
        "constitution" => AttributeKind::Constitution,
        "intelligence" => AttributeKind::Intelligence,
        "willpower" => AttributeKind::Willpower,
        "charisma" => AttributeKind::Charisma,
        "luck" => AttributeKind::Luck,
        _ => return None,
    };
    Some(StatTarget::Attribute(attribute))
}

/// `(register-item-use-effect id effect-kind effect-tag effect-amount effect-amount2)`
/// ——设置「使用这件物品会发生什么」（P6 第五批：耐久与 `Intent::Use`），
/// 见 [`crate::item::ItemDef::use_effect`] 文档「为什么不是 `register-item`
/// 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，与 [`register_item_equip_mask`] 同一条 ADR 0017
///   「注册期完整校验」纪律。
/// - `effect-kind`/`effect-tag`/`effect-amount`/`effect-amount2`：与
///   `register-skill` 的同名四参数完全同一套编码（见
///   `crate::script_skill_api::register_skill` 文档），因为两者的目标
///   类型都是 [`SkillEffect`]——`"deal-damage"`/`"restore-resource"`/
///   `"temporary-stat-modifier"`，`effect-tag` 按 `effect-kind` 解释,
///   `effect-amount2` 只有 `"temporary-stat-modifier"` 使用（持续 tick
///   数）。
///
/// **覆盖，不是追加**——多次调用同一个 `id` 以最后一次为准,与
/// [`crate::item::ItemTable::set_use_effect`] 文档「覆盖，不是追加」
/// 一节同一条既有语义。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_use_effect(
    id: String,
    effect_kind: String,
    effect_tag: String,
    effect_amount: i64,
    effect_amount2: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-use-effect 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_use_effect(
                registry,
                table,
                &id,
                &effect_kind,
                &effect_tag,
                effect_amount,
                effect_amount2,
            )
        })
    })
}

/// [`register_item_use_effect`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_use_effect(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    effect_kind: &str,
    effect_tag: &str,
    effect_amount: i64,
    effect_amount2: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    let effect = parse_use_effect(effect_kind, effect_tag, effect_amount, effect_amount2)?;

    table
        .set_use_effect(index, effect)
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `effect-kind`/`effect-tag`/`effect-amount`/`effect-amount2` →
/// [`SkillEffect`]——与 `crate::script_skill_api::parse_effect` 是对
/// 同一个目标类型的独立解析实现，见模块文档「`register-item-use-effect`
/// 的参数形状」一节。
fn parse_use_effect(
    kind: &str,
    tag: &str,
    amount: i64,
    amount2: i64,
) -> Result<SkillEffect, String> {
    match kind {
        "deal-damage" => Ok(SkillEffect::DealDamage {
            base: amount as i32,
        }),
        "restore-resource" => {
            let resource = match tag {
                "mana" => ResourceKind::Mana,
                "stamina" => ResourceKind::Stamina,
                _ => return Err(format!("未知的资源种类 {tag:?}")),
            };
            Ok(SkillEffect::RestoreResource {
                resource,
                base: amount as i32,
            })
        }
        "temporary-stat-modifier" => {
            let attribute = use_effect_attribute_kind_from_str(tag)
                .ok_or_else(|| format!("未知的主属性名 {tag:?}"))?;
            Ok(SkillEffect::TemporaryStatModifier {
                attribute,
                amount: amount as i32,
                duration_ticks: amount2.max(0) as u32,
            })
        }
        _ => Err(format!("未知的物品使用效果种类 {kind:?}")),
    }
}

/// 属性名字符串 → [`AttributeKind`]，[`parse_use_effect`] 的
/// `"temporary-stat-modifier"` 分支专用——与
/// `crate::script_skill_api::attribute_kind_from_str`/
/// `stat_target_from_str`（本模块，多认识 `"armor"`）是第三份独立
/// 拷贝，理由同模块文档「`register-item-use-effect` 的参数形状」一节：
/// 不借用 `stat_target_from_str` 是因为它的返回类型（`StatTarget`，
/// 多一个 `Armor` 分支）与这里要的 `AttributeKind` 不同，硬借用需要
/// 再过滤掉 `Armor` 这一支，比六行重复更绕。
fn use_effect_attribute_kind_from_str(name: &str) -> Option<AttributeKind> {
    Some(match name {
        "strength" => AttributeKind::Strength,
        "dexterity" => AttributeKind::Dexterity,
        "constitution" => AttributeKind::Constitution,
        "intelligence" => AttributeKind::Intelligence,
        "willpower" => AttributeKind::Willpower,
        "charisma" => AttributeKind::Charisma,
        "luck" => AttributeKind::Luck,
        _ => return None,
    })
}

/// `(register-item-penetration id flat permille)`——设置这件物品的穿透
/// （武器引用与穿透接线批次，P6 第六批），见
/// [`crate::item::ItemDef::penetration`] 文档「为什么不是 `register-item`
/// 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，与 [`register_item_equip_mask`] 同一条 ADR 0017
///   「注册期完整校验」纪律。
/// - `flat`：固定穿透值,对应 [`Penetration::flat`]。
/// - `permille`：千分比穿透,对应 [`Penetration::permille`]（`1000`
///   表示 100%）。
///
/// 本函数不要求 `id` 已经占用武器槽位——与 `register-item-stat-bonus`
/// 同一条既有纪律（一件装备的属性加成/穿透只在它真的被装备时才会被
/// `derive_stats`/`resolve_attack` 读到，见两者文档，注册期不重复这层
/// "装备了才生效"的判断）。
///
/// **覆盖，不是追加**——与 [`register_item_equip_mask`]/
/// [`register_item_use_effect`] 同一种"单值覆盖"语义：一件武器只有一份
/// 穿透（不像 `stat_bonuses` 是可以累积的列表，见
/// [`crate::item::ItemDef::penetration`] 文档「为什么现在也收进来了」
/// 一节），多次调用同一个 `id` 以最后一次为准。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_penetration(id: String, flat: i64, permille: i64) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-penetration 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_penetration(registry, table, &id, flat, permille)
        })
    })
}

/// [`register_item_penetration`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_penetration(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    flat: i64,
    permille: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };

    table
        .set_penetration(
            index,
            Penetration {
                flat: flat as i32,
                permille: permille as i32,
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-damage-formula id formula-id)`——设置这件物品显式
/// 声明的伤害公式（伤害公式引擎批次新增），见
/// [`crate::item::ItemDef::damage_formula`] 文档「为什么不是
/// `register-item` 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，与 [`register_item_penetration`] 同一条 ADR
///   0017「注册期完整校验」纪律。
/// - `formula-id`：已经通过 `register-damage-formula`
///   （`crate::script_damage_formula_api`）注册过的完整命名空间标识符
///   字符串——与 `formula-id` 未注册即拒绝同一条纪律（本函数不
///   `intern`，只 `get`，理由同 `crate::script_xp_curve_api::resolve_registered_id`
///   文档），不允许静默创建一个指向不存在公式的悬空引用。
///
/// **覆盖，不是追加**——与 [`register_item_penetration`] 同一种"单值
/// 覆盖"语义：一件武器只有一份显式公式引用，多次调用同一个 `id` 以
/// 最后一次为准。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_damage_formula(id: String, formula_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-item-damage-formula 在没有活跃物品表的窗口内被调用".to_string(),
                );
            };
            do_register_item_damage_formula(registry, table, &id, &formula_id)
        })
    })
}

/// [`register_item_damage_formula`] 的纯函数核心，方便单元测试不必
/// 绕过 `thread_local!`。
fn do_register_item_damage_formula(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    formula_id: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    let parsed_formula_id = NamespacedId::parse(formula_id)
        .map_err(|err| format!("非法内容标识符 {formula_id:?}：{err}"))?;
    let Some(formula_index) = registry.get(&parsed_formula_id) else {
        return Err(format!(
            "伤害公式 {formula_id:?} 尚未通过 register-damage-formula 注册"
        ));
    };

    table
        .set_damage_formula(index, formula_index)
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-damage-category id category-id)`——设置这件物品显式
/// 声明的伤害类别（伤害类别/抗性接线批次新增），见
/// [`crate::item::ItemDef::damage_category`] 文档「为什么不是
/// `register-item` 的参数」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，与 [`register_item_damage_formula`] 同一条 ADR
///   0017「注册期完整校验」纪律。
/// - `category-id`：已经通过 `register-damage-category`
///   （`crate::script_damage_category_api`）注册过的完整命名空间标识符
///   字符串——与 `formula-id` 未注册即拒绝同一条纪律（本函数不
///   `intern`，只 `get`），不允许静默创建一个指向不存在类别的悬空引用。
///
/// **覆盖，不是追加**——与 [`register_item_damage_formula`] 同一种"单值
/// 覆盖"语义：一件武器只有一份显式伤害类别引用，多次调用同一个 `id`
/// 以最后一次为准。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_damage_category(id: String, category_id: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err(
                    "register-item-damage-category 在没有活跃物品表的窗口内被调用".to_string(),
                );
            };
            do_register_item_damage_category(registry, table, &id, &category_id)
        })
    })
}

/// [`register_item_damage_category`] 的纯函数核心，方便单元测试不必
/// 绕过 `thread_local!`。
fn do_register_item_damage_category(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    category_id: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    let parsed_category_id = NamespacedId::parse(category_id)
        .map_err(|err| format!("非法内容标识符 {category_id:?}：{err}"))?;
    let Some(category_index) = registry.get(&parsed_category_id) else {
        return Err(format!(
            "伤害类别 {category_id:?} 尚未通过 register-damage-category 注册"
        ));
    };

    table
        .set_damage_category(index, category_index)
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

/// `(register-item-resistance id damage-category-id multiplier-permille)`
/// ——追加声明「这件物品携带对某个伤害类别的抗性」（抗性多来源聚合
/// 批次新增），落地项目所有者对抗性来源的裁定「抗性肯定会来自天赋，
/// 以及装备，还有各种药品，或者技能」里**装备**这一路。与
/// [`register_item_damage_category`] 同一个「新增能力用新函数」模式：
/// 不改 `register-item` 已有的六参数签名。
///
/// # 与 `register-trait-resistance` 的关系：同一条规则，换一路来源
///
/// 参数形状、校验纪律、负值处理、追加语义全部与
/// `crate::script_trait_api::register_trait_resistance` 逐条对齐——两者
/// 写进的是**同一个** [`RuleModifier::Resistance`] 载荷，最终被**同一个**
/// 聚合点（[`ll_sim::rule_modifier::resistance_multiplier_permille`]）
/// 按同一条 tie-break 规则消费。差别只有「写进哪张表」（物品表 vs
/// 天赋表）这一处，这正是 ADR 0021 说的「算法可共享才抽象」在这里成立
/// 的原因，见该聚合点模块文档「ADR 0021 复核」一节。
///
/// - `id`：已经通过 `register-item` 注册过的完整命名空间标识符字符串
///   ——目标必须已存在，同 [`register_item_damage_category`] 一条 ADR
///   0017「注册期完整校验」纪律。
/// - `damage-category-id`：已经通过 `register-damage-category`
///   （`crate::script_damage_category_api`）注册过的完整命名空间标识符
///   字符串——不允许静默创建一个指向不存在类别的悬空抗性声明。
/// - `multiplier-permille`：千分比乘数（`0`=免疫，`500`=半伤，
///   `2000`=双倍）。负值钳到零而不是拒绝整次调用，理由同
///   `register-trait-resistance` 文档同一段。
///
/// **追加，不是覆盖**——一件装备可以同时声明对多个伤害类别的抗性，见
/// [`crate::item::ItemTable::add_rule_modifier`] 文档。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_item_resistance(
    id: String,
    damage_category_id: String,
    multiplier_permille: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-item-resistance 在没有活跃物品表的窗口内被调用".to_string());
            };
            do_register_item_resistance(
                registry,
                table,
                &id,
                &damage_category_id,
                multiplier_permille,
            )
        })
    })
}

/// [`register_item_resistance`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_item_resistance(
    registry: &Registry,
    table: &mut ItemTable,
    id: &str,
    damage_category_id: &str,
    multiplier_permille: i64,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let Some(index) = registry.get(&parsed_id) else {
        return Err(format!("物品 {id:?} 尚未通过 register-item 注册"));
    };
    let parsed_category_id = NamespacedId::parse(damage_category_id)
        .map_err(|err| format!("非法内容标识符 {damage_category_id:?}：{err}"))?;
    let Some(category_index) = registry.get(&parsed_category_id) else {
        return Err(format!(
            "伤害类别 {damage_category_id:?} 尚未通过 register-damage-category 注册"
        ));
    };

    table
        .add_rule_modifier(
            index,
            RuleModifier::Resistance {
                damage_category: category_index,
                multiplier_permille: multiplier_permille.max(0) as i32,
            },
        )
        .map(|()| true)
        .map_err(|err: ItemError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法可堆叠物品声明注册成功并写入物品表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 99);
        assert_eq!(view.base_price, Milli(2000));
        assert_eq!(view.max_durability, None);
    }

    #[test]
    fn 合法不可堆叠物品声明携带耐久上限() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:iron_sword",
            "yourmod:item.iron_sword",
            1,
            3000,
            50000,
            100,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:iron_sword").unwrap())
            .expect("刚注册的内容应能查到索引");
        let view = table.get(index).expect("刚注册的物品应能查到属性");
        assert_eq!(view.stack_limit, 1);
        assert_eq!(view.max_durability, Some(100));
    }

    #[test]
    fn 堆叠上限为零时返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            0,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 耐久上限小于负一时返回错误而不panic() {
        // Arrange：-2 不是合法的"无耐久"哨兵（只有 -1 是）。
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:x",
            "yourmod:item.x",
            1,
            0,
            0,
            -2,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 可堆叠物品携带耐久上限时返回错误而不panic() {
        // Arrange：堆叠上限 99（可堆叠）却同时声明了耐久上限——两条
        // 规则字面矛盾，见 do_register_item「可堆叠物品不该有耐久」
        // 一节。
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:cursed_arrow",
            "yourmod:item.cursed_arrow",
            99,
            50,
            2000,
            10,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 不可堆叠物品携带耐久上限时注册成功() {
        // 反例：stack_limit == 1 与耐久上限不矛盾，证明上一条测试拒绝
        // 的确实是"可堆叠 + 有耐久"这个组合本身，不是耐久上限这个参数
        // 恒被拒绝。
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:cursed_dagger",
            "yourmod:item.cursed_dagger",
            1,
            50,
            2000,
            10,
        );

        // Assert
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "InvalidNamespace:foo",
            "yourmod:item.foo",
            1,
            0,
            0,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 重复定义同一个物品索引返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        )
        .expect("首次注册应当成功");

        // Act
        let result = do_register_item(
            &mut registry,
            &mut table,
            "yourmod:arrow",
            "yourmod:item.arrow",
            99,
            50,
            2000,
            -1,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-item "yourmod:arrow" "yourmod:item.arrow" 99 50 2000 -1)"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:arrow").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().stack_limit, 99);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：堆叠上限为零——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());

        // Act
        let result = engine
            .load_source(r#"(register-item "yourmod:x" "yourmod:item.x" 0 0 0 -1)"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_trait_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    /// 建一张已经注册过一件不可堆叠武器（模拟大斧）的物品表 + 对应的
    /// registry——`register-item-equip-mask` 的测试共用这份前置状态。
    fn registry_and_table_with_great_axe() -> (Registry, ItemTable) {
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:great_axe",
            "yourmod:item.great_axe",
            1,
            5000,
            8000,
            120,
        )
        .expect("大斧注册应当成功");
        (registry, table)
    }

    #[test]
    fn 多个槽位名称按位或合并成最终掩码() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["main-hand".to_string(), "off-hand".to_string()],
        );

        // Assert
        assert_eq!(result, Ok(true));
        let expected = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        assert_eq!(table.get(index).unwrap().equip_mask, expected);
    }

    #[test]
    fn 有耐久的物品占位掩码不占任何武器槽位时返回错误() {
        // 大斧声明了耐久上限（120），若把占位掩码设置成不占用任何武器
        // 槽位（这里选头顶，随便一个非武器分组的槽位即可），两条规则
        // 字面矛盾——见 `do_register_item_equip_mask` 文档「为什么在
        // 这里校验耐久与武器槽位的组合」一节。
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["head".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 有耐久的物品占位掩码只占副手时注册成功() {
        // 反例，与上一条测试成对：占位掩码只包含副手（不包含主手）时
        // 同样的耐久声明必须放行——证明这条校验拒绝的是"不占任何武器
        // 槽位"这个组合本身，不是耐久上限恒被拒绝，也不是只有主手才
        // 算数（`equipment-slots.md` 槽位表「武器」这一组同时覆盖主手
        // 与副手，见 `WEAPON_GROUP_SLOTS` 文档）。
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["off-hand".to_string()],
        );

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().equip_mask,
            EquipSlot::OFF_HAND.mask()
        );
    }

    #[test]
    fn 未注册的物品id追加装备掩码返回错误() {
        // Arrange
        let registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:never_registered",
            &["head".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未知槽位名称返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result = do_register_item_equip_mask(
            &registry,
            &mut table,
            "yourmod:great_axe",
            &["tail".to_string()],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 空槽位名称列表返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result = do_register_item_equip_mask(&registry, &mut table, "yourmod:great_axe", &[]);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_equip_mask() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-equip-mask "yourmod:great_axe" (list "main-hand" "off-hand"))"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        let expected = EquipSlot::MAIN_HAND
            .mask()
            .union(EquipSlot::OFF_HAND.mask());
        assert_eq!(table.get(index).unwrap().equip_mask, expected);
    }

    #[test]
    fn 追加的力量加成能被真正查到() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result =
            do_register_item_stat_bonus(&registry, &mut table, "yourmod:great_axe", "strength", 5);

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().stat_bonuses,
            &[StatBonus {
                target: StatTarget::Attribute(AttributeKind::Strength),
                amount: 5,
            }]
        );
    }

    #[test]
    fn 追加的护甲加成目标不是主属性而是armor() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result =
            do_register_item_stat_bonus(&registry, &mut table, "yourmod:great_axe", "armor", 8);

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().stat_bonuses,
            &[StatBonus {
                target: StatTarget::Armor,
                amount: 8,
            }]
        );
    }

    #[test]
    fn 连续两次调用追加而非覆盖此前的加成() {
        // Arrange：先加力量,再加护甲——两条加成必须都留在列表里,不是
        // 第二次调用把第一次的结果顶替掉,见 add_stat_bonus 文档「追加,
        // 不是覆盖」一节。
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        do_register_item_stat_bonus(&registry, &mut table, "yourmod:great_axe", "strength", 5)
            .expect("第一次追加应当成功");

        // Act
        do_register_item_stat_bonus(&registry, &mut table, "yourmod:great_axe", "armor", 8)
            .expect("第二次追加应当成功");

        // Assert
        assert_eq!(
            table.get(index).unwrap().stat_bonuses,
            &[
                StatBonus {
                    target: StatTarget::Attribute(AttributeKind::Strength),
                    amount: 5,
                },
                StatBonus {
                    target: StatTarget::Armor,
                    amount: 8,
                },
            ]
        );
    }

    #[test]
    fn 未知的加成目标名称返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();

        // Act
        let result =
            do_register_item_stat_bonus(&registry, &mut table, "yourmod:great_axe", "swagger", 5);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未注册的物品id追加属性加成返回错误() {
        // Arrange
        let registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item_stat_bonus(
            &registry,
            &mut table,
            "yourmod:never_registered",
            "strength",
            5,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_stat_bonus() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-stat-bonus "yourmod:great_axe" "strength" 5)"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().stat_bonuses,
            &[StatBonus {
                target: StatTarget::Attribute(AttributeKind::Strength),
                amount: 5,
            }]
        );
    }

    /// 建一张已经注册过一件可堆叠消耗品（模拟治疗药水）的物品表 +
    /// 对应的 registry——`register-item-use-effect` 的测试共用这份
    /// 前置状态。
    fn registry_and_table_with_healing_potion() -> (Registry, ItemTable) {
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        do_register_item(
            &mut registry,
            &mut table,
            "yourmod:healing_potion",
            "yourmod:item.healing_potion",
            10,
            200,
            500,
            -1,
        )
        .expect("治疗药水注册应当成功");
        (registry, table)
    }

    #[test]
    fn 恢复资源效果注册后能被真正查到() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_healing_potion();
        let index = registry
            .get(&NamespacedId::parse("yourmod:healing_potion").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "restore-resource",
            "mana",
            30,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().use_effect,
            Some(SkillEffect::RestoreResource {
                resource: ResourceKind::Mana,
                base: 30,
            })
        );
    }

    #[test]
    fn 造成伤害效果注册后能被真正查到() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_healing_potion();
        let index = registry
            .get(&NamespacedId::parse("yourmod:healing_potion").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "deal-damage",
            "",
            15,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().use_effect,
            Some(SkillEffect::DealDamage { base: 15 })
        );
    }

    #[test]
    fn 临时属性修正效果携带持续tick数() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_healing_potion();
        let index = registry
            .get(&NamespacedId::parse("yourmod:healing_potion").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result = do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "temporary-stat-modifier",
            "strength",
            5,
            50,
        );

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().use_effect,
            Some(SkillEffect::TemporaryStatModifier {
                attribute: AttributeKind::Strength,
                amount: 5,
                duration_ticks: 50,
            })
        );
    }

    #[test]
    fn 连续两次调用使用效果覆盖而非追加() {
        // 与 register-item-equip-mask「覆盖，不是追加」同一条语义——第
        // 二次调用的结果应当完全顶替第一次，不是两条效果并存（`use_effect`
        // 是 `Option<SkillEffect>`，类型上就不能装两条）。
        // Arrange
        let (registry, mut table) = registry_and_table_with_healing_potion();
        let index = registry
            .get(&NamespacedId::parse("yourmod:healing_potion").unwrap())
            .expect("刚注册的内容应能查到索引");
        do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "deal-damage",
            "",
            15,
            0,
        )
        .expect("第一次注册应当成功");

        // Act
        do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "restore-resource",
            "stamina",
            20,
            0,
        )
        .expect("第二次注册应当成功");

        // Assert
        assert_eq!(
            table.get(index).unwrap().use_effect,
            Some(SkillEffect::RestoreResource {
                resource: ResourceKind::Stamina,
                base: 20,
            })
        );
    }

    #[test]
    fn 未知的使用效果种类返回错误而不panic() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_healing_potion();

        // Act
        let result = do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:healing_potion",
            "cast-a-spell",
            "",
            0,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 未注册的物品id设置使用效果返回错误() {
        // Arrange
        let registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result = do_register_item_use_effect(
            &registry,
            &mut table,
            "yourmod:never_registered",
            "deal-damage",
            "",
            15,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_use_effect() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:healing_potion" "yourmod:item.healing_potion" 10 200 500 -1)"#
                    .to_string(),
            )
            .expect("治疗药水基础注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-use-effect "yourmod:healing_potion" "restore-resource" "mana" 30 0)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:healing_potion").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().use_effect,
            Some(SkillEffect::RestoreResource {
                resource: ResourceKind::Mana,
                base: 30,
            })
        );
    }

    #[test]
    fn 追加的穿透能被真正查到() {
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");

        // Act
        let result =
            do_register_item_penetration(&registry, &mut table, "yourmod:great_axe", 4, 200);

        // Assert
        assert_eq!(result, Ok(true));
        assert_eq!(
            table.get(index).unwrap().penetration,
            Penetration {
                flat: 4,
                permille: 200,
            }
        );
    }

    #[test]
    fn 连续两次调用穿透覆盖而非累加() {
        // 与 register-item-equip-mask「覆盖，不是追加」同一条语义——第
        // 二次调用的结果应当完全顶替第一次，不是两条穿透并存（一件
        // 武器只有一份 `Penetration`，见 `ItemDef::penetration` 文档）。
        // Arrange
        let (registry, mut table) = registry_and_table_with_great_axe();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        do_register_item_penetration(&registry, &mut table, "yourmod:great_axe", 4, 200)
            .expect("第一次注册应当成功");

        // Act
        do_register_item_penetration(&registry, &mut table, "yourmod:great_axe", 1, 50)
            .expect("第二次注册应当成功");

        // Assert
        assert_eq!(
            table.get(index).unwrap().penetration,
            Penetration {
                flat: 1,
                permille: 50,
            }
        );
    }

    #[test]
    fn 未注册的物品id设置穿透返回错误() {
        // Arrange
        let registry = Registry::new();
        let mut table = ItemTable::new();

        // Act
        let result =
            do_register_item_penetration(&registry, &mut table, "yourmod:never_registered", 1, 0);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_penetration() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");

        // Act
        let result = engine
            .load_source(r#"(register-item-penetration "yourmod:great_axe" 4 200)"#.to_string());

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.get(index).unwrap().penetration,
            Penetration {
                flat: 4,
                permille: 200,
            }
        );
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_item_damage_category() {
        // Arrange：先注册一个真实的伤害类别,再用它设置武器的显式引用。
        use crate::damage_category::DamageCategoryTable;
        use crate::script_damage_category_api::{
            register_damage_category_api, set_active_target as set_active_category_target,
            take_active_target as take_active_category_target,
        };
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        register_damage_category_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        set_active_category_target(DamageCategoryTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");
        engine
            .load_source(r#"(register-damage-category "yourmod:fire" "")"#.to_string())
            .expect("伤害类别注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-damage-category "yourmod:great_axe" "yourmod:fire")"#.to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let _ = take_active_category_target();
        let item_index = registry
            .get(&NamespacedId::parse("yourmod:great_axe").unwrap())
            .expect("刚注册的物品应能查到索引");
        let category_index = registry
            .get(&NamespacedId::parse("yourmod:fire").unwrap())
            .expect("刚注册的伤害类别应能查到索引");
        assert_eq!(
            table.get(item_index).unwrap().damage_category,
            Some(category_index)
        );
    }

    #[test]
    fn 伤害类别未注册时register_item_damage_category失败而不panic() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_item_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ItemTable::new());
        engine
            .load_source(
                r#"(register-item "yourmod:great_axe" "yourmod:item.great_axe" 1 5000 8000 120)"#
                    .to_string(),
            )
            .expect("大斧基础注册应当成功");

        // Act
        let result = engine.load_source(
            r#"(register-item-damage-category "yourmod:great_axe" "yourmod:never_registered")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
    #[test]
    fn 物品抗性声明写进物品表并按追加语义累积() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item_index = registry.intern(NamespacedId::parse("yourmod:ward_amulet").unwrap());
        let fire = registry.intern(NamespacedId::parse("yourmod:fire").unwrap());
        let cold = registry.intern(NamespacedId::parse("yourmod:cold").unwrap());
        table
            .define(
                item_index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("yourmod:item.ward_amulet").unwrap(),
                    stack_limit: 1,
                    base_weight: Milli::from_whole(1),
                    base_price: Milli::from_whole(10),
                    max_durability: None,
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            )
            .expect("首次定义必成功");

        // Act
        let first = do_register_item_resistance(
            &registry,
            &mut table,
            "yourmod:ward_amulet",
            "yourmod:fire",
            500,
        );
        let second = do_register_item_resistance(
            &registry,
            &mut table,
            "yourmod:ward_amulet",
            "yourmod:cold",
            0,
        );

        // Assert：两条各自独立存在，第二条不覆盖第一条。
        assert_eq!(first, Ok(true));
        assert_eq!(second, Ok(true));
        let view = table.get(item_index).expect("已定义");
        assert_eq!(
            view.rule_modifiers,
            &[
                RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                },
                RuleModifier::Resistance {
                    damage_category: cold,
                    multiplier_permille: 0,
                },
            ]
        );
    }

    #[test]
    fn 物品抗性声明的负乘数钳到零而不是拒绝整次调用() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item_index = registry.intern(NamespacedId::parse("yourmod:cursed_ring").unwrap());
        let fire = registry.intern(NamespacedId::parse("yourmod:fire").unwrap());
        table
            .define(
                item_index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("yourmod:item.cursed_ring").unwrap(),
                    stack_limit: 1,
                    base_weight: Milli::from_whole(1),
                    base_price: Milli::from_whole(10),
                    max_durability: None,
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            )
            .expect("首次定义必成功");

        // Act
        let result = do_register_item_resistance(
            &registry,
            &mut table,
            "yourmod:cursed_ring",
            "yourmod:fire",
            -300,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let view = table.get(item_index).expect("已定义");
        assert_eq!(
            view.rule_modifiers,
            &[RuleModifier::Resistance {
                damage_category: fire,
                multiplier_permille: 0,
            }]
        );
    }

    #[test]
    fn 物品抗性引用未注册的伤害类别被拒绝() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        let item_index = registry.intern(NamespacedId::parse("yourmod:ward_amulet").unwrap());
        table
            .define(
                item_index,
                ItemAttrs {
                    display_name_key: NamespacedId::parse("yourmod:item.ward_amulet").unwrap(),
                    stack_limit: 1,
                    base_weight: Milli::from_whole(1),
                    base_price: Milli::from_whole(10),
                    max_durability: None,
                    equip_mask: SlotMask::EMPTY,
                    stat_bonuses: Vec::new(),
                    use_effect: None,
                    penetration: Penetration::NONE,
                    damage_formula: None,
                    damage_category: None,
                    rule_modifiers: Vec::new(),
                },
            )
            .expect("首次定义必成功");

        // Act
        let result = do_register_item_resistance(
            &registry,
            &mut table,
            "yourmod:ward_amulet",
            "yourmod:nonexistent",
            500,
        );

        // Assert：ADR 0017 注册期完整校验——悬空引用当场拒绝，不静默创建。
        assert!(result.is_err());
        assert!(
            table
                .get(item_index)
                .expect("已定义")
                .rule_modifiers
                .is_empty()
        );
    }

    #[test]
    fn 给未注册的物品声明抗性被拒绝() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ItemTable::new();
        registry.intern(NamespacedId::parse("yourmod:fire").unwrap());

        // Act
        let result = do_register_item_resistance(
            &registry,
            &mut table,
            "yourmod:never_registered",
            "yourmod:fire",
            500,
        );

        // Assert
        assert!(result.is_err());
    }
}
