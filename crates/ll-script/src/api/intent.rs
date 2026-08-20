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
//! # `UseSkill` 的 `target` 恒为 `None`（同一个"更大的问题"，仍未解决）
//!
//! [`crate::api::handle::ScriptEntityHandle`]（P5-A 脚本状态存储批次
//! 新增）已经解决了"脚本如何安全持有一个不可伪造的 `EntityId`"这个
//! 机制问题本身——但截至本次改动，还没有任何脚本可调用的查询函数会
//! 把"附近的敌人是谁"这类信息包成 `ScriptEntityHandle` 交给脚本。没有
//! 这样一个查询源，`parse_intent` 即便认识某种"目标"语法，脚本也无法
//! 提供一个真实、非伪造的目标——因此本函数解析出的 [`Intent::UseSkill`]
//! 固定把 `target` 设为 `None`（技能施于自身，见
//! `ll_sim::intent::Intent::UseSkill::target` 文档"未显式给出目标的
//! 技能施于自身"的既定语义），不是遗漏，是如实反映当前脚本层查不到
//! 目标这个事实。若未来给脚本补上"最近的敌人"一类返回
//! `ScriptEntityHandle` 的查询函数，这里可以再扩展一种带目标的
//! `use-skill` 列表形状。
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

/// 把脚本返回值解析成 [`Intent`]。
///
/// `actor` 由宿主提供——调用脚本时宿主已经知道在为哪个实体请求意图，
/// 不从脚本返回值里读：脚本没有任何合法路径能构造出一个 `EntityId`。
/// `resolve_skill` 由宿主提供，把技能命名空间字符串解析成
/// [`ContentIndex`]，理由见模块文档「为什么 `parse_intent` 需要一个
/// `resolve_skill` 回调」一节。
///
/// 识别三种形状：
/// - 符号 `'wait` → [`Intent::Wait`]
/// - 二元素列表 `(list 'move 'north)`（方向名见 [`direction_from_symbol`]）
///   → [`Intent::Move`]
/// - 二元素列表 `(list 'use-skill "lostland:strike")`（技能命名空间
///   标识符字符串，经 `resolve_skill` 解析）→ [`Intent::UseSkill`]，
///   `target` 恒为 `None`（见模块文档同名一节）；`resolve_skill` 返回
///   `None`（字符串不合法，或当前会话没有注册这个技能）时，本函数
///   整体返回 `None`——与"脚本产出一个不认识的形状"同等对待，不是
///   单独的错误路径。
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
                "use-skill" => {
                    let skill_id = string_str(iter.next()?)?;
                    if iter.next().is_some() {
                        return None;
                    }
                    let skill = resolve_skill(skill_id)?;
                    Some(Intent::UseSkill {
                        actor,
                        skill,
                        target: None,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
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
}
