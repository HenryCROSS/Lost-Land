//! 把 `register-animation-clip` 注册进脚本引擎：mod 脚本借此定义自定义
//! 动画剪辑。模式同 [`crate::script_terrain_api`]/
//! [`crate::script_race_api`]，`frames` 的 FFI 处理同
//! [`crate::script_quest_api`] 的 `prerequisites`——`Vec<String>` 有
//! steel-core 现成的 `FromSteelVal`，脚本传一个字符串列表
//! `(list "hero_walk_0" "hero_walk_1")` 即可，不需要额外的自定义类型。
//!
//! `exit-grace-frames` 是否暴露给脚本的结论见 [`crate::clip`] 模块
//! 文档「`exit_grace_frames` 暴露给脚本：结论与理由」一节——本模块
//! 直接把它做成第五个参数，不留一个「以后再加」的缺口。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_render::anim::Clip;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::clip::{ClipError, ClipTable};
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-animation-clip` 应该写入的剪辑表。
    static ACTIVE_TABLE: RefCell<Option<ClipTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-animation-clip` 可写入的
/// 目标。
pub fn set_active_target(table: ClipTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `ClipTable`。
pub fn take_active_target() -> ClipTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-animation-clip` 注册进 `engine`。
pub fn register_clip_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-animation-clip", register_animation_clip);
}

/// `(register-animation-clip id frames frames-per-step looping? exit-grace-frames)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"examplemod:slime_squish"`。
/// - `frames`：依序播放的图集条目名列表，`(list "a" "b")`；空列表在
///   注册期就会被拒绝（见 [`crate::clip::ClipError::EmptyFrames`]）。
/// - `frames-per-step`：每一帧停留的游戏帧数，非负整数，钳位到 `u32`；
///   为零时的降级行为见 `ll_render::anim::Playback::current_frame`
///   文档。
/// - `looping?`：布尔，播完最后一帧后是否从头循环。
/// - `exit-grace-frames`：非负整数，钳位到 `u32`——只有交给
///   `AnimStateMachine::trigger`/`update` 管理的触发式状态动画才会
///   读它，原地循环/电平驱动的剪辑填零即可（见
///   `ll_render::anim::Clip::exit_grace_frames` 文档）。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_animation_clip(
    id: String,
    frames: Vec<String>,
    frames_per_step: i64,
    looping: bool,
    exit_grace_frames: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-animation-clip 在没有活跃剪辑表的窗口内被调用".to_string());
            };
            do_register_animation_clip(
                registry,
                table,
                &id,
                frames,
                frames_per_step.max(0) as u32,
                looping,
                exit_grace_frames.max(0) as u32,
            )
        })
    })
}

/// [`register_animation_clip`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_animation_clip(
    registry: &mut Registry,
    table: &mut ClipTable,
    id: &str,
    frames: Vec<String>,
    frames_per_step: u32,
    looping: bool,
    exit_grace_frames: u32,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    table
        .define(
            index,
            Clip {
                frames,
                frames_per_step,
                looping,
                exit_grace_frames,
            },
        )
        .map(|()| true)
        .map_err(|err: ClipError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法剪辑声明注册成功并写入剪辑表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClipTable::new();

        // Act
        let result = do_register_animation_clip(
            &mut registry,
            &mut table,
            "examplemod:slime_squish",
            vec!["slime_0".to_string(), "slime_1".to_string()],
            6,
            true,
            0,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("examplemod:slime_squish").unwrap())
            .expect("刚注册的内容应能查到索引");
        let clip = table.get(index).expect("刚注册的剪辑应能查到");
        assert_eq!(
            clip.frames,
            vec!["slime_0".to_string(), "slime_1".to_string()]
        );
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = ClipTable::new();

        // Act
        let result = do_register_animation_clip(
            &mut registry,
            &mut table,
            "Not Valid",
            vec!["a".to_string()],
            4,
            true,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 空帧列表返回clip表的校验错误() {
        // Arrange：ADR 0017「注册期完整校验」——空帧列表由
        // ClipTable::define 拒绝，本测试确认脚本入口没有绕过这条校验。
        let mut registry = Registry::new();
        let mut table = ClipTable::new();

        // Act
        let result = do_register_animation_clip(
            &mut registry,
            &mut table,
            "examplemod:broken",
            Vec::new(),
            4,
            true,
            0,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_animation_clip() {
        // 端到端验证：这是本模块真正要交付的能力——脚本里写
        // (register-animation-clip ...)，不需要脚本作者知道 Rust 侧的
        // Registry/ClipTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_clip_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ClipTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-animation-clip "examplemod:slime_squish" (list "slime_0" "slime_1") 6 #t 0)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("examplemod:slime_squish").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.get(index).unwrap().frames_per_step, 6);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误，宿主必须优雅报错。
        let mut engine = ScriptEngine::new();
        register_clip_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(ClipTable::new());

        // Act
        let result = engine
            .load_source(r#"(register-animation-clip "Not Valid" (list "a") 4 #t 0)"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：即便脚本出错，接线契约仍要求成对调用，否则下一个
        // 测试用例会因为 ACTIVE_TABLE/ACTIVE_REGISTRY 里残留旧值而互相
        // 污染（thread_local 在同一测试线程内跨用例存活）。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
