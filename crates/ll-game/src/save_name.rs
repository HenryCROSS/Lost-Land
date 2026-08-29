//! 给存档起名字：一行文本输入 + 那块屏的状态机。
//!
//! # 先确认一件事：仓库今天没有任何文本输入控件
//!
//! `ll_ui::screen::ScreenData` 是「标题 + 若干行 + 一句提示」的只读面板，
//! `ll_ui::widget` 下也没有输入框。唯一能拿到**裸物理键**的通道是
//! [`ll_platform::input::InputState::last_physical_key`]——设置屏的键位
//! 捕获模式在用它（绕过绑定表读原始键）。本模块复用同一条通道。
//!
//! # 做到多简单（以及做不到什么）
//!
//! 所有者要的是「一个输入名字的地方让存档可标识」。够用的最小形状：
//!
//! - 一行输入，只接受 `a-z`/`0-9`/空格/`-`/`_`；
//! - 退格删一个字符；
//! - 确认键提交，取消键退回上一块屏；
//! - 长度上限 [`MAX_SAVE_NAME_CHARS`]（24 字符），够认出「哪一份是哪一
//!   份」，又不至于把那块面板撑破；
//! - 光标是一个尾随的 `_`。
//!
//! **没有**：插入点移动、选区、剪贴板、撤销。它们各自都要一套编辑状态，
//! 而存档名是玩家一辈子只为这个世界打一次的东西。
//!
//! ## 打不了中文，这一点如实说明
//!
//! 没有 IME。做 IME 要接 winit 的 `Ime` 事件、维护预编辑串、还要一套
//! 能画任意字形的文本栈——那是独立的一整批。玩家什么都不打就确认时退回
//! 一个默认名（Fluent 键 `screen-savename-default`），**绝不写出一份
//! 没有名字的存档**。
//!
//! ## 不读修饰键，因此全部按小写记
//!
//! `last_physical_key` 这一层拿不到 `Shift` 的状态（它返回的是一个
//! [`KeyCode`]，不带修饰键）。要拿到大小写就得改平台层的事件通道，
//! 那超出本批范围。存档名因此全是小写——这是本批为了不动平台层做的
//! 最保守取舍，单列在计划文档第八节。

use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::KeyCode;

use crate::menu_screen::ScreenState;
use crate::save_slot::MAX_SAVE_NAME_CHARS;

/// 屏上表示输入光标的那个字符。
const CURSOR_GLYPH: char = '_';

/// 一行文本输入。
///
/// **刻意不叫 `TextInput`**：它不是一个通用控件，是一行只认识若干个
/// 字符的输入框，见模块文档「做到多简单」。真要做通用控件，那是
/// `ll-ui` 的事，不是本体二进制的事。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameField {
    text: String,
}

impl NameField {
    /// 建一个空输入框。
    pub fn new() -> NameField {
        NameField::default()
    }

    /// 当前已经输入的内容。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 屏上该显示的那一行：内容后面跟一个光标。
    pub fn display(&self) -> String {
        format!("{}{CURSOR_GLYPH}", self.text)
    }

    /// 内容是不是空的（决定确认时要不要退回默认名）。
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// 吃掉一个物理键。返回真表示内容变了。
    ///
    /// 不认识的键**静默忽略**——玩家在这块屏上按 F5 不该有任何后果，
    /// 更不该被当成一个字符插进名字里。
    pub fn accept(&mut self, key: KeyCode) -> bool {
        if key == KeyCode::Backspace {
            return self.text.pop().is_some();
        }
        let Some(ch) = typed_char(key) else {
            return false;
        };
        // 长度上限按**字符**算而不是字节：本模块只接受 ASCII，两者今天
        // 相等，但按字符算的话将来真接上 IME 时这一行不用改。
        if self.text.chars().count() >= MAX_SAVE_NAME_CHARS {
            return false;
        }
        self.text.push(ch);
        true
    }
}

/// 一个物理键对应哪个可输入字符；不可输入返回 `None`。
///
/// 白名单而不是「除了这些控制键之外都算字符」：后者要求本函数认识
/// winit 那一整套 `KeyCode`（两百多个变体，还会随版本增加），漏掉一个
/// 就会有奇怪的字符钻进文件名。白名单漏掉一个只是「那个键打不出来」。
fn typed_char(key: KeyCode) -> Option<char> {
    let ch = match key {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::Digit0 | KeyCode::Numpad0 => '0',
        KeyCode::Digit1 | KeyCode::Numpad1 => '1',
        KeyCode::Digit2 | KeyCode::Numpad2 => '2',
        KeyCode::Digit3 | KeyCode::Numpad3 => '3',
        KeyCode::Digit4 | KeyCode::Numpad4 => '4',
        KeyCode::Digit5 | KeyCode::Numpad5 => '5',
        KeyCode::Digit6 | KeyCode::Numpad6 => '6',
        KeyCode::Digit7 | KeyCode::Numpad7 => '7',
        KeyCode::Digit8 | KeyCode::Numpad8 => '8',
        KeyCode::Digit9 | KeyCode::Numpad9 => '9',
        KeyCode::Space => ' ',
        KeyCode::Minus => '-',
        _ => return None,
    };
    Some(ch)
}

/// 命名屏这一帧的产出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveNameUpdate {
    /// 要切到哪一块屏，`None` 表示留在命名屏。
    pub next: Option<ScreenState>,
    /// 玩家按了确认——调用方应当拿 [`NameField::text`] 去开槽位并进世界。
    pub confirmed: bool,
}

impl SaveNameUpdate {
    fn idle() -> SaveNameUpdate {
        SaveNameUpdate {
            next: None,
            confirmed: false,
        }
    }
}

/// 处理命名屏这一帧的输入。
///
/// # 为什么读的是物理键而不是 [`GameKey`]
///
/// 与设置屏的捕获模式同一条理由（`crate::menu_screen::update_settings`
/// 文档）：这块屏要的是「玩家按下的那个字母」，而绑定表回答的是
/// 「玩家按下的那个**动作**」。字母 W 在绑定表里是「向上走」。
///
/// 确认与取消仍然走 `GameKey`——它们是动作，玩家在设置屏里能改。
pub fn update_save_name(field: &mut NameField, input: &InputState) -> SaveNameUpdate {
    if input.was_just_pressed(GameKey::Cancel) {
        // 退回选出生地屏：玩家可能只是想再挑一次地方。**不丢弃已经
        // 打好的名字**——草稿还留着这个 `NameField`。
        return SaveNameUpdate {
            next: Some(ScreenState::SpawnPick),
            confirmed: false,
        };
    }
    if input.was_just_pressed(GameKey::Confirm) {
        return SaveNameUpdate {
            next: None,
            confirmed: true,
        };
    }
    let Some(key) = input.last_physical_key() else {
        return SaveNameUpdate::idle();
    };
    field.accept(key);
    SaveNameUpdate::idle()
}

/// 命名屏这一帧的两行文字：提示 + 正在输入的名字。
pub fn save_name_row_texts(
    field: &NameField,
    catalog: &ll_i18n::Catalog,
    language: &str,
) -> Vec<String> {
    vec![
        catalog.resolve(language, "screen-savename-prompt"),
        field.display(),
    ]
}

/// 玩家什么都没打时用哪个名字——**绝不写出一份没有名字的存档**。
pub fn resolved_name(field: &NameField, catalog: &ll_i18n::Catalog, language: &str) -> String {
    if field.is_empty() {
        catalog.resolve(language, "screen-savename-default")
    } else {
        field.text().trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字母数字与空格连字符打得出来() {
        // Arrange
        let mut field = NameField::new();

        // Act
        for key in [
            KeyCode::KeyM,
            KeyCode::KeyY,
            KeyCode::Space,
            KeyCode::KeyW,
            KeyCode::Minus,
            KeyCode::Digit1,
        ] {
            field.accept(key);
        }

        // Assert
        assert_eq!(field.text(), "my w-1");
    }

    #[test]
    fn 退格删一个字符空框上退格什么都不做() {
        // Arrange
        let mut field = NameField::new();
        field.accept(KeyCode::KeyA);
        field.accept(KeyCode::KeyB);

        // Act
        let 删掉了 = field.accept(KeyCode::Backspace);
        let 再删 = field.accept(KeyCode::Backspace);
        let 空框上再删 = field.accept(KeyCode::Backspace);

        // Assert
        assert!(删掉了 && 再删);
        assert!(!空框上再删, "空框上退格不该报告「内容变了」");
        assert_eq!(field.text(), "");
    }

    #[test]
    fn 不认识的键静默忽略() {
        // 玩家在这块屏上按 F5 不该有任何后果，更不该有奇怪的字符钻进
        // 文件名——白名单而不是黑名单，见 `typed_char` 文档。
        // Arrange
        let mut field = NameField::new();

        // Act
        let 变了 = field.accept(KeyCode::F5)
            || field.accept(KeyCode::Tab)
            || field.accept(KeyCode::ShiftLeft);

        // Assert
        assert!(!变了);
        assert_eq!(field.text(), "");
    }

    #[test]
    fn 长度到上限之后再打不进去() {
        // Arrange
        let mut field = NameField::new();

        // Act：打 MAX + 5 个字符。
        for _ in 0..MAX_SAVE_NAME_CHARS + 5 {
            field.accept(KeyCode::KeyA);
        }

        // Assert
        assert_eq!(field.text().chars().count(), MAX_SAVE_NAME_CHARS);
    }

    #[test]
    fn 显示的那一行带一个尾随光标() {
        // Arrange
        let mut field = NameField::new();
        field.accept(KeyCode::KeyA);

        // Act & Assert
        assert_eq!(field.display(), "a_");
        assert_eq!(NameField::new().display(), "_", "空框也要看得见光标");
    }
}
