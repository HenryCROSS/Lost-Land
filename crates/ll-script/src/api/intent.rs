//! 脚本返回值 → `Intent` 的解析层。
//!
//! 脚本不直接改世界：约束 C1（规格）要求 `ll_sim::apply::apply` 是唯一
//! 写入口。脚本能做的只是回答"这一回合想干什么"，宿主把这个回答解析
//! 成 [`Intent`]，再走既有的 `resolve → Effect → apply` 管线——脚本
//! 完全不接触 `Effect`/`apply`，它产出的东西跟玩家按键产出的
//! [`Intent`] 是同一种数据，走同一条管线。
//!
//! # 为什么只支持 Move/Wait，不支持 Attack/OpenDoor
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

use ll_sim::intent::{Direction, Intent};
use ll_world::entity::EntityId;
use steel::rvals::SteelVal;

/// 把脚本返回值解析成 [`Intent`]。
///
/// `actor` 由宿主提供——调用脚本时宿主已经知道在为哪个实体请求意图，
/// 不从脚本返回值里读：脚本没有任何合法路径能构造出一个 `EntityId`。
///
/// 识别两种形状：
/// - 符号 `'wait` → [`Intent::Wait`]
/// - 二元素列表 `(list 'move 'north)`（方向名见 [`direction_from_symbol`]）
///   → [`Intent::Move`]
///
/// 其余形状（包括脚本产出的任何不认识的符号/结构）返回 `None`——宿主
/// 应把 `None` 当作"这一回合什么都不做"处理。脚本产出一个我们不认识
/// 的值，属于脚本行为不符合预期而不是脚本报错（`call_raw` 本身没有
/// `Err`），同样必须能降级，不能因为一个意外的返回形状让宿主决策逻辑
/// panic。
pub fn parse_intent(actor: EntityId, value: &SteelVal) -> Option<Intent> {
    match value {
        SteelVal::SymbolV(sym) if sym.as_str() == "wait" => Some(Intent::Wait { actor }),
        SteelVal::ListV(list) => {
            let mut iter = list.iter();
            let action = symbol_str(iter.next()?)?;
            if action != "move" {
                return None;
            }
            let dir = direction_from_symbol(symbol_str(iter.next()?)?)?;
            if iter.next().is_some() {
                // 多余的元素——不是我们认识的形状，宁可拒绝也不猜测。
                return None;
            }
            Some(Intent::Move { actor, dir })
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
        let intent = parse_intent(actor, &value);

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
        let intent = parse_intent(actor, &value);

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
        let intent = parse_intent(actor, &value);

        // Assert
        assert_eq!(intent, None);
    }

    #[test]
    fn 数字返回值解析为空而非崩溃() {
        // Arrange：脚本返回了一个我们完全不认识形状的值。
        let actor = some_actor();
        let value = SteelVal::IntV(42);

        // Act
        let intent = parse_intent(actor, &value);

        // Assert
        assert_eq!(intent, None);
    }
}
