//! 存档列表屏：首页的「读取存档」不再直接读那唯一一份，而是列出全部
//! 槽位让玩家挑。
//!
//! # 它改掉的是什么
//!
//! 批次 6 的首页把「读取存档」做成了「有存档就直接读那一份」——因为当时
//! 存档确实只有一份（`GamePaths::save` 是单个文件路径）。多槽位落地之后
//! 那条路径已经不再表达玩家的意图：他点「读取存档」时想的是「读**哪**
//! 一份」。
//!
//! # 每一行长什么样
//!
//! `名字 · 时间 · 模式`。三样都是「在一屏里认出哪一份是哪一份」必需的：
//!
//! - **名字**是玩家自己起的（老存档没有，退回文件名主干）；
//! - **时间**回答「哪一份更新」，也是同名两份存档唯一的区分手段；
//! - **模式**回答「这是不是一份肉鸽档」——它决定进去之后有没有手动
//!   存档，玩家有权在读之前就知道。
//!
//! 拼接用 Fluent 的参数化消息（`screen-savelist-row`），**不在代码里拼
//! 分隔符**：不同语言的标点习惯不一样，硬编码一个 `·` 就是一处硬编码
//! 的用户可见文本。

use ll_content::mode::SaveMode;
use ll_i18n::{Catalog, FluentArgs};
use ll_platform::input::{GameKey, InputState};

use crate::menu_screen::{ScreenOutcome, ScreenState};
use crate::save_slot::{SaveSlot, format_saved_at};

/// 存档列表屏这一帧的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveListUpdate {
    /// 处理完这一帧输入之后，调用方该做什么。
    pub outcome: ScreenOutcome,
    /// 要切到哪一块屏，`None` 表示留在列表屏。
    pub next: Option<ScreenState>,
}

impl SaveListUpdate {
    fn idle() -> SaveListUpdate {
        SaveListUpdate {
            outcome: ScreenOutcome::Idle,
            next: None,
        }
    }
}

/// 处理存档列表屏这一帧的输入。
///
/// `cursor` 由调用方持有（它是 [`ScreenState::SaveList`] 的一部分），
/// 本函数就地推进它。
///
/// # 空列表时按确认什么都不做
///
/// 与首页那一行「读取存档（没有存档）」同一条纪律：**绝不**退而求其次
/// 地开一局新游戏。玩家点的是读档。
pub fn update_save_list(
    cursor: &mut usize,
    slots: &[SaveSlot],
    input: &InputState,
    pointer: crate::pointer::RowPointer,
) -> SaveListUpdate {
    if input.was_just_pressed(GameKey::Cancel) {
        return SaveListUpdate {
            outcome: ScreenOutcome::Idle,
            next: Some(ScreenState::Title),
        };
    }
    if slots.is_empty() {
        // 一份都没有：上下键无处可动，确认键无档可读。留在这块屏上，
        // 玩家按取消回首页。
        return SaveListUpdate::idle();
    }
    if input.was_just_pressed(GameKey::Down) {
        *cursor = (*cursor + 1) % slots.len();
    } else if input.was_just_pressed(GameKey::Up) {
        *cursor = (*cursor + slots.len() - 1) % slots.len();
    }
    // 指针按下把光标挪过去（不钳制越界：`row` 只可能来自这块屏自己
    // 现算的行矩形，行数与 `slots` 同源）。
    if let Some(row) = pointer.focus_row() {
        *cursor = row.min(slots.len() - 1);
    }
    if input.was_just_pressed(GameKey::Confirm) || pointer.activated() {
        return SaveListUpdate {
            outcome: ScreenOutcome::LoadSave,
            next: None,
        };
    }
    SaveListUpdate::idle()
}

/// 光标不该越界——槽位列表在玩家离开这块屏期间可能变短（例如他在游戏里
/// 存了一次又回来，或者另一个进程删了一份）。
///
/// 夹在合法范围里而不是 panic：一块屏的光标越界不该拖垮整局游戏，与本
/// 模块其余降级路径同一条纪律。
pub fn clamp_cursor(cursor: usize, slots: &[SaveSlot]) -> usize {
    if slots.is_empty() {
        0
    } else {
        cursor.min(slots.len() - 1)
    }
}

/// 列表这一帧的行文字。
pub fn save_list_row_texts(slots: &[SaveSlot], catalog: &Catalog, language: &str) -> Vec<String> {
    if slots.is_empty() {
        return vec![catalog.resolve(language, "screen-savelist-empty-row")];
    }
    slots
        .iter()
        .map(|slot| {
            let mut args = FluentArgs::new();
            args.set("name", slot.display_name());
            args.set("time", format_saved_at(slot.saved_at));
            args.set("mode", catalog.resolve(language, mode_key(slot.mode)));
            catalog.resolve_with_args(language, "screen-savelist-row", Some(&args))
        })
        .collect()
}

/// 某个模式在界面上叫什么（的 Fluent 键）。
///
/// **「曾经从肉鸽降级而来」不单独显示一档**：玩家关心的是「我现在能不能
/// 手动存档」，而不是这个世界的履历。降级标记仍然永久留在存档头里
/// （`ll_content::mode` 模块文档），只是不占列表那一行的宽度。
pub fn mode_key(mode: SaveMode) -> &'static str {
    match mode {
        SaveMode::Permadeath => "screen-savelist-mode-roguelike",
        SaveMode::FreeSave { .. } => "screen-savelist-mode-normal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_slot::SlotId;
    use std::path::PathBuf;

    fn 槽位(stem: &str, name: &str, saved_at: i64, mode: SaveMode) -> SaveSlot {
        SaveSlot {
            id: SlotId::from_name(stem),
            path: PathBuf::from(format!("saves/{stem}.llsave")),
            save_name: name.to_string(),
            character_name: "旅人".to_string(),
            saved_at,
            mode,
        }
    }

    fn 按下(keys: &[GameKey]) -> InputState {
        let mut input = InputState::new();
        for key in keys {
            input.press(*key);
        }
        input
    }

    #[test]
    fn 上下键在列表里循环() {
        // Arrange
        let slots = vec![
            槽位("a", "甲", 3, SaveMode::fresh_free_save()),
            槽位("b", "乙", 2, SaveMode::Permadeath),
            槽位("c", "丙", 1, SaveMode::fresh_free_save()),
        ];
        let mut cursor = 0;

        // Act & Assert
        update_save_list(
            &mut cursor,
            &slots,
            &按下(&[GameKey::Down]),
            crate::pointer::RowPointer::Idle,
        );
        assert_eq!(cursor, 1);
        update_save_list(
            &mut cursor,
            &slots,
            &按下(&[GameKey::Up]),
            crate::pointer::RowPointer::Idle,
        );
        assert_eq!(cursor, 0);
        // 从第一行往上走绕到最后一行。
        update_save_list(
            &mut cursor,
            &slots,
            &按下(&[GameKey::Up]),
            crate::pointer::RowPointer::Idle,
        );
        assert_eq!(cursor, 2);
    }

    #[test]
    fn 确认产出读档意图() {
        // Arrange
        let slots = vec![槽位("a", "甲", 3, SaveMode::fresh_free_save())];
        let mut cursor = 0;

        // Act
        let update = update_save_list(
            &mut cursor,
            &slots,
            &按下(&[GameKey::Confirm]),
            crate::pointer::RowPointer::Idle,
        );

        // Assert
        assert_eq!(update.outcome, ScreenOutcome::LoadSave);
    }

    #[test]
    fn 空列表按确认绝不悄悄开一局新游戏() {
        // Arrange
        let mut cursor = 0;

        // Act
        let update = update_save_list(
            &mut cursor,
            &[],
            &按下(&[GameKey::Confirm]),
            crate::pointer::RowPointer::Idle,
        );

        // Assert
        assert_eq!(update.outcome, ScreenOutcome::Idle);
        assert_eq!(update.next, None);
    }

    #[test]
    fn 取消回首页() {
        // Arrange
        let slots = vec![槽位("a", "甲", 3, SaveMode::fresh_free_save())];
        let mut cursor = 0;

        // Act
        let update = update_save_list(
            &mut cursor,
            &slots,
            &按下(&[GameKey::Cancel]),
            crate::pointer::RowPointer::Idle,
        );

        // Assert
        assert_eq!(update.next, Some(ScreenState::Title));
    }

    #[test]
    fn 列表变短时光标被夹回合法范围() {
        // Arrange：玩家离开这块屏期间列表变短了。
        let slots = vec![槽位("a", "甲", 3, SaveMode::fresh_free_save())];

        // Act & Assert
        assert_eq!(clamp_cursor(7, &slots), 0);
        assert_eq!(clamp_cursor(7, &[]), 0);
        assert_eq!(clamp_cursor(0, &slots), 0);
    }

    #[test]
    fn 两种模式的标签键不一样() {
        // 玩家有权在读档之前就知道这是不是一份肉鸽档。
        assert_ne!(
            mode_key(SaveMode::Permadeath),
            mode_key(SaveMode::fresh_free_save())
        );
        // 降级过的普通档与从一开始就是普通档的显示成同一档——玩家关心的
        // 是「现在能不能手动存档」。
        assert_eq!(
            mode_key(SaveMode::Permadeath.downgrade().expect("必然可降级")),
            mode_key(SaveMode::fresh_free_save())
        );
    }
}
