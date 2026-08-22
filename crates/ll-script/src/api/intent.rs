//! 脚本返回值 → `Intent` 的解析层。
//!
//! 脚本不直接改世界：约束 C1（规格）要求 `ll_sim::apply::apply` 是唯一
//! 写入口。脚本能做的只是回答"这一回合想干什么"，宿主把这个回答解析
//! 成 [`Intent`]，再走既有的 `resolve → Effect → apply` 管线——脚本
//! 完全不接触 `Effect`/`apply`，它产出的东西跟玩家按键产出的
//! [`Intent`] 是同一种数据，走同一条管线。
//!
//! # 为什么只支持 Move/Wait/UseSkill，不支持 Attack/OpenDoor
//!
//! 与既有的 `ll_sim::intent::intent_from_input` 同样的分工：`Attack`
//! 需要知道"那个方向上到底有什么"、`OpenDoor` 需要精确的门坐标——这些
//! 判断依赖世界状态，属于 `resolve` 的职责，不是"决定意图"这一步该做
//! 的事。脚本只表达"想往哪个方向行动"，方向上具体触发攻击还是移动，
//! 由 `resolve` 现有逻辑决定，与玩家按方向键的效果完全对齐,这不是本
//! 模块偷懒省掉的功能,是刻意延续既有分工。
//!
//! 若未来需要脚本直接指定攻击目标，需要先解决"脚本如何安全持有一个
//! 不可伪造的 `EntityId` 引用"这个更大的问题——`EntityId::new` 是
//! `pub(crate)`（仅 `ll-world` 内部可见），本 crate 目前没有合法路径
//! 从脚本给的裸整数重建一个真正的 `EntityId`，硬做只能靠脚本自己拼一
//! 个假的，这正是要避免的事。留给后续任务在 `ll-world` 补一个"不可
//! 伪造的不透明句柄"机制之后再扩展。
//!
//! # `Attack`/带目标的 `UseSkill`（行为树接线批次解禁）
//!
//! [`crate::api::handle::ScriptEntityHandle`]（P5-A 脚本状态存储批次
//! 新增）解决了"脚本如何安全持有一个不可伪造的 `EntityId`"这个机制
//! 问题本身；`crate::api::actor::nearby_enemy`（行为树接线批次新增）
//! 补上了本文档曾经缺失的那一半——把"附近的敌人是谁"包成
//! `ScriptEntityHandle` 交给脚本的查询函数。有了真实、非伪造的目标
//! 来源，`parse_intent` 现在识别两种新形状：
//! - 二元素列表 `(list 'attack target-handle)` → [`Intent::Attack`]，
//!   落地 `knowledge/design/script-entity-handles-and-batch-queries.md`
//!   四节「`Intent::Attack` 的解禁」。
//! - 三元素列表 `(list 'use-skill "id" target-handle)` → 带显式目标的
//!   [`Intent::UseSkill`]；仍保留原有的二元素形状（`target` 为 `None`，
//!   技能施于自身）——两种形状都合法，不是替换关系。
//!
//! # 为什么 `parse_intent` 需要一个 `resolve_skill` 回调
//!
//! [`Intent::UseSkill::skill`] 是 [`ContentIndex`]，但脚本只能表达
//! 命名空间字符串（"这个技能叫什么名字"），字符串到索引的转换需要查
//! 当前会话的内容注册表——而 `Registry` 定义在 `ll-mod`，依赖方向
//! `ll-script` ← `ll-mod`（规格 §5）不允许本 crate 反过来依赖它。与
//! `ll-mod::script_terrain_api` 等模块解决"注册函数需要 `Registry`"
//! 问题的思路不同（那些函数本身就是脚本 FFI 注册进去的闭包，可以用
//! `thread_local!` 桥接），`parse_intent` 是一次求值完脚本之后、由宿主
//! 直接调用的普通 Rust 函数，不经过 Steel FFI——因此更简单的做法是
//! 直接让调用方（持有 `Registry` 的那一层，例如未来的行为树求值器）
//! 传入一个解析回调，本函数只在识别出 `use-skill` 形状时调用它一次，
//! 不持有、也不缓存这个回调。

use ll_core::ident::ContentIndex;
use ll_sim::intent::{Direction, Intent};
use ll_world::entity::EntityId;
use steel::rvals::SteelVal;

use crate::api::handle::ScriptEntityHandle;

/// 把脚本返回值解析成 [`Intent`]。
///
/// `actor` 由宿主提供——调用脚本时宿主已经知道在为哪个实体请求意图，
/// 不从脚本返回值里读：脚本没有任何合法路径能构造出一个 `EntityId`。
/// `resolve_skill` 由宿主提供，把技能命名空间字符串解析成
/// [`ContentIndex`]，理由见模块文档「为什么 `parse_intent` 需要一个
/// `resolve_skill` 回调」一节。
///
/// 识别六种形状：
/// - 符号 `'wait` → [`Intent::Wait`]
/// - 二元素列表 `(list 'move 'north)`（方向名见 [`direction_from_symbol`]）
///   → [`Intent::Move`]
/// - 二元素列表 `(list 'attack target-handle)`（`target-handle` 是
///   [`ScriptEntityHandle`]，例如 `nearby-enemy` 的返回值）→
///   [`Intent::Attack`]。
/// - 二元素列表 `(list 'inspect target-handle)`（卫兵职业接线批次；
///   `target-handle` 通常来自 `nearby-actor-in-view`）→
///   [`Intent::Inspect`]，形状与 `attack` 完全同构。
/// - 二元素列表 `(list 'use-skill "lostland:strike")`（技能命名空间
///   标识符字符串，经 `resolve_skill` 解析）→ [`Intent::UseSkill`]，
///   `target` 为 `None`（技能施于自身）。
/// - 三元素列表 `(list 'use-skill "lostland:strike" target-handle)`
///   ——同上，但 `target` 为 `Some(target-handle 解出的 EntityId)`。
///
/// 两种情形下 `resolve_skill` 返回 `None`（字符串不合法，或当前会话
/// 没有注册这个技能）时，本函数整体返回 `None`——与"脚本产出一个不
/// 认识的形状"同等对待，不是单独的错误路径。
///
/// 其余形状（包括脚本产出的任何不认识的符号/结构）返回 `None`——宿主
/// 应把 `None` 当作"这一回合什么都不做"处理。脚本产出一个我们不认识
/// 的值，属于脚本行为不符合预期而不是脚本报错（`call_raw` 本身没有
/// `Err`），同样必须能降级，不能因为一个意外的返回形状让宿主决策逻辑
/// panic。
pub fn parse_intent(
    actor: EntityId,
    value: &SteelVal,
    resolve_skill: &dyn Fn(&str) -> Option<ContentIndex>,
) -> Option<Intent> {
    match value {
        SteelVal::SymbolV(sym) if sym.as_str() == "wait" => Some(Intent::Wait { actor }),
        SteelVal::ListV(list) => {
            let mut iter = list.iter();
            let action = symbol_str(iter.next()?)?;
            match action {
                "move" => {
                    let dir = direction_from_symbol(symbol_str(iter.next()?)?)?;
                    if iter.next().is_some() {
                        // 多余的元素——不是我们认识的形状，宁可拒绝也不猜测。
                        return None;
                    }
                    Some(Intent::Move { actor, dir })
                }
                "attack" => {
                    let target = entity_handle(iter.next()?)?;
                    if iter.next().is_some() {
                        return None;
                    }
                    Some(Intent::Attack { actor, target })
                }
                "inspect" => {
                    // 卫兵职业接线批次——形状与 "attack" 完全同构（一个
                    // 目标句柄，无第三个元素），理由同 `Intent::Attack`
                    // 那一支：`Intent::Inspect` 同样只有 actor/target 两
                    // 个 `EntityId` 字段。
                    let target = entity_handle(iter.next()?)?;
                    if iter.next().is_some() {
                        return None;
                    }
                    Some(Intent::Inspect { actor, target })
                }
                "use-skill" => {
                    let skill_id = string_str(iter.next()?)?;
                    let target = match iter.next() {
                        Some(value) => Some(entity_handle(value)?),
                        None => None,
                    };
                    if iter.next().is_some() {
                        return None;
                    }
                    let skill = resolve_skill(skill_id)?;
                    Some(Intent::UseSkill {
                        actor,
                        skill,
                        target,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// 从一个 `SteelVal` 里取出 [`ScriptEntityHandle`] 并解包成
/// [`EntityId`]——句柄不合法（脚本没有能力伪造出这样一个值，见
/// `crate::api::handle` 模块文档「防伪造的三层论证」，因此这里唯一
/// 会失败的情形是脚本传了一个完全不相干的值，比如整数）时返回
/// `None`，与本函数其余分支同一条「宁可拒绝也不猜测」的纪律。
fn entity_handle(value: &SteelVal) -> Option<EntityId> {
    use steel::rvals::FromSteelVal;
    ScriptEntityHandle::from_steelval(value)
        .ok()
        .map(|handle| handle.entity_id())
}

fn symbol_str(value: &SteelVal) -> Option<&str> {
    match value {
        SteelVal::SymbolV(s) => Some(s.as_str()),
        _ => None,
    }
}

/// 从一个 `SteelVal` 里取出字符串字面量——只认 `SteelVal::StringV`
/// （不像 `symbol_str` 那样接受符号）：技能标识符含 `:` 分隔命名空间
/// 与路径，用字符串字面量书写比依赖符号语法在不同 Scheme 方言下是否
/// 接受 `:` 更可预期，脚本作者写 `"lostland:strike"` 即可。
fn string_str(value: &SteelVal) -> Option<&str> {
    match value {
        SteelVal::StringV(s) => Some(s.as_str()),
        _ => None,
    }
}

/// 方向名 → [`Direction`]。八个方向名字直接对应
/// [`Direction`] 的八个变体，命名沿用 `world-` 系列查询函数已经建立的
/// kebab-case 惯例。
fn direction_from_symbol(name: &str) -> Option<Direction> {
    Some(match name {
        "north" => Direction::North,
        "south" => Direction::South,
        "east" => Direction::East,
        "west" => Direction::West,
        "north-east" => Direction::NorthEast,
        "south-east" => Direction::SouthEast,
        "south-west" => Direction::SouthWest,
        "north-west" => Direction::NorthWest,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::ScriptEngine;
    use ll_world::entity::Arena;
    use steel::rvals::FromSteelVal;

    fn some_actor() -> EntityId {
        let mut arena: Arena<()> = Arena::new();
        arena.spawn(())
    }

    #[test]
    fn 符号wait解析为等待意图() {
        // Arrange
        let actor = some_actor();
        let value = SteelVal::SymbolV("wait".into());

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, Some(Intent::Wait { actor }));
    }

    #[test]
    fn 列表move加方向解析为移动意图() {
        // Arrange
        let actor = some_actor();
        let mut engine = ScriptEngine::new();
        engine
            .load_source("(define (probe) (list 'move 'north))".to_string())
            .unwrap();
        let value = engine.call_raw("probe", Vec::new()).unwrap();

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(
            intent,
            Some(Intent::Move {
                actor,
                dir: Direction::North
            })
        );
    }

    #[test]
    fn 不认识的符号解析为空() {
        // Arrange
        let actor = some_actor();
        let value = SteelVal::SymbolV("dance".into());

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, None);
    }

    #[test]
    fn 数字返回值解析为空而非崩溃() {
        // Arrange：脚本返回了一个我们完全不认识形状的值。
        let actor = some_actor();
        let value = SteelVal::IntV(42);

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, None);
    }

    /// 现造一个真实的 [`ContentIndex`]——测试不能直接构造私有字段的
    /// 元组结构体，走 `Interner::intern` 是唯一合法路径（见
    /// `ll_core::ident` 模块文档）。
    fn some_skill_index() -> ContentIndex {
        let mut interner = ll_core::ident::Interner::new();
        interner.intern(ll_core::ident::NamespacedId::parse("lostland:strike").expect("合法标识符"))
    }

    #[test]
    fn 列表use_skill加合法标识符解析为使用技能意图() {
        // Arrange
        let actor = some_actor();
        let skill = some_skill_index();
        let mut engine = ScriptEngine::new();
        engine
            .load_source(r#"(define (probe) (list 'use-skill "lostland:strike"))"#.to_string())
            .unwrap();
        let value = engine.call_raw("probe", Vec::new()).unwrap();

        // Act
        let intent = parse_intent(actor, &value, &|id| {
            (id == "lostland:strike").then_some(skill)
        });

        // Assert
        assert_eq!(
            intent,
            Some(Intent::UseSkill {
                actor,
                skill,
                target: None,
            })
        );
    }

    #[test]
    fn resolve_skill返回none时use_skill解析为空() {
        // Arrange：脚本引用的技能字符串，宿主的注册表当前查不到——
        // 与"缺失 mod"同一条降级纪律（ADR 0015），不是脚本的错。
        let actor = some_actor();
        let mut engine = ScriptEngine::new();
        engine
            .load_source(r#"(define (probe) (list 'use-skill "yourmod:unknown"))"#.to_string())
            .unwrap();
        let value = engine.call_raw("probe", Vec::new()).unwrap();

        // Act
        let intent = parse_intent(actor, &value, &|_| None);

        // Assert
        assert_eq!(intent, None);
    }

    #[test]
    fn use_skill缺少技能标识符参数解析为空() {
        // Arrange：形状不完整——脚本作者笔误，宁可拒绝也不猜测。
        let actor = some_actor();
        let mut engine = ScriptEngine::new();
        engine
            .load_source("(define (probe) (list 'use-skill))".to_string())
            .unwrap();
        let value = engine.call_raw("probe", Vec::new()).unwrap();

        // Act
        let intent = parse_intent(actor, &value, &|_| None);

        // Assert
        assert_eq!(intent, None);
    }

    /// 现造一个真实的目标句柄——`nearby-enemy` 之类查询函数在生产
    /// 路径上产出的正是这种值，这里直接用 `ScriptEntityHandle` 自己的
    /// `IntoSteelVal` 构造，不需要真的跑一遍查询函数。
    fn some_target_handle() -> SteelVal {
        use steel::rvals::IntoSteelVal;
        ScriptEntityHandle::new(some_actor())
            .into_steelval()
            .expect("Custom 类型转换恒成功")
    }

    #[test]
    fn 列表attack加合法句柄解析为攻击意图() {
        // Arrange
        let actor = some_actor();
        let target_handle = some_target_handle();
        let target = ScriptEntityHandle::from_steelval(&target_handle)
            .expect("刚构造的句柄恒能转换回去")
            .entity_id();
        let value = SteelVal::ListV(
            [SteelVal::SymbolV("attack".into()), target_handle]
                .into_iter()
                .collect(),
        );

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, Some(Intent::Attack { actor, target }));
    }

    #[test]
    fn attack参数不是合法句柄时解析为空() {
        // Arrange：脚本没有任何合法路径构造出一个句柄——这里模拟的是
        // 「脚本传了个完全不相干的值」这种防御性场景，而不是「脚本
        // 伪造了句柄」（后者结构上不可能，见句柄防伪造论证）。
        let actor = some_actor();
        let value = SteelVal::ListV(
            [SteelVal::SymbolV("attack".into()), SteelVal::IntV(999)]
                .into_iter()
                .collect(),
        );

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, None);
    }

    #[test]
    fn 列表inspect加合法句柄解析为盘查意图() {
        // 卫兵职业接线批次——形状与 attack 完全同构，见模块文档。
        // Arrange
        let actor = some_actor();
        let target_handle = some_target_handle();
        let target = ScriptEntityHandle::from_steelval(&target_handle)
            .expect("刚构造的句柄恒能转换回去")
            .entity_id();
        let value = SteelVal::ListV(
            [SteelVal::SymbolV("inspect".into()), target_handle]
                .into_iter()
                .collect(),
        );

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, Some(Intent::Inspect { actor, target }));
    }

    #[test]
    fn inspect参数不是合法句柄时解析为空() {
        // Arrange
        let actor = some_actor();
        let value = SteelVal::ListV(
            [SteelVal::SymbolV("inspect".into()), SteelVal::IntV(999)]
                .into_iter()
                .collect(),
        );

        // Act
        let intent = parse_intent(actor, &value, &|_: &str| None);

        // Assert
        assert_eq!(intent, None);
    }

    #[test]
    fn 三元素列表use_skill加句柄解析为带目标的使用技能意图() {
        // Arrange
        let actor = some_actor();
        let skill = some_skill_index();
        let target_handle = some_target_handle();
        let target = ScriptEntityHandle::from_steelval(&target_handle)
            .expect("刚构造的句柄恒能转换回去")
            .entity_id();
        let value = SteelVal::ListV(
            [
                SteelVal::SymbolV("use-skill".into()),
                SteelVal::StringV("lostland:strike".into()),
                target_handle,
            ]
            .into_iter()
            .collect(),
        );

        // Act
        let intent = parse_intent(actor, &value, &|id| {
            (id == "lostland:strike").then_some(skill)
        });

        // Assert
        assert_eq!(
            intent,
            Some(Intent::UseSkill {
                actor,
                skill,
                target: Some(target),
            })
        );
    }
}
