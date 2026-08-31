//! 给存档起名字：一行文本输入 + 那块屏的状态机。
//!
//! # 它消费的是**文本通道**，不是物理键
//!
//! 本模块上一版是拿 [`ll_platform::input::InputState::last_physical_key`]
//! 硬拼的——一张 `KeyCode::KeyA -> 'a'` 的白名单表，因此**只能输入小写
//! ASCII**：打不了中文、打不了大写、打不了标点。
//!
//! 根因不是那张表写得不好，是**它在回答错误的问题**：`KeyCode` 是
//! 「玩家按下了键盘上哪个位置」，而输入框要的是「玩家实际输入了什么
//! 文字」。中文输入法一次上屏的「你好」根本没有对应的物理键。
//!
//! 现在走 [`ll_platform::text_input`]：编辑序列
//! （[`TextEdit::Insert`]/[`TextEdit::Backspace`]）+ 预编辑串。
//! 那条通道同时接了输入法上屏与直接键入两条来源，所以中文、大写、
//! 标点全部打得出来。
//!
//! **物理键通道没有被删掉**，也不该删：键位重绑必须按物理键（玩家要
//! 绑的是「那个键」，不是「那个键打出的字」）。两条通道并存，分工见
//! `ll_platform::text_input` 模块文档开头那张表。
//!
//! # 显示名与文件名是两样东西
//!
//! 本模块产出的是**显示名**——玩家原样输入的串，中文/大写/标点全留，
//! 存进存档头（`ll_content::header::SaveHeader::save_name`）。
//!
//! **文件名是另一回事**：`crate::save_slot::SlotId::from_name` 把它过滤
//! 成 ASCII 白名单主干，而那张白名单**兼作路径穿越闸门**（`/`、`\`、
//! `:`、`.` 全不在名单里，`../../etc/passwd` 落不出目标目录）。为了让
//! 存档名支持中文去放宽它，等于用一个显示问题换一个安全漏洞——而显示
//! 问题根本不需要动它：显示名走存档头，从来不经过文件系统。
//!
//! 副作用如实记录：一个纯中文名过滤后为空，文件名退回兜底主干
//! `save`/`save-2`/`save-3`。玩家在列表里看到的仍然是他起的中文名。
//!
//! # 做到多简单（以及做不到什么）
//!
//! - 一行输入，接受**任何可打印字符**（控制字符在平台层就被挡掉了）；
//! - 退格删一个**字符**（不是一个字节——一个汉字是 3 字节）；
//! - 长度上限 [`MAX_SAVE_NAME_CHARS`] **按字符数**算；
//! - 正在拼写的串显示在已上屏文本后面（见 [`NameField::display`]）；
//! - 确认键提交，取消键退回上一块屏；光标是一个尾随的 `_`。
//!
//! **没有**：插入点移动、选区、剪贴板、撤销。它们各自都要一套编辑
//! 状态，而存档名是玩家一辈子只为这个世界打一次的东西。

use ll_platform::input::{GameKey, InputState};
use ll_platform::text_input::TextEdit;

use crate::menu_screen::ScreenState;
use crate::save_slot::MAX_SAVE_NAME_CHARS;

/// 屏上表示输入光标的那个字符。
const CURSOR_GLYPH: char = '_';

/// 一行文本输入。
///
/// **刻意不叫 `TextInput`**：那个名字已经被平台层那条通道占了
/// （[`ll_platform::text_input::TextInput`]），而这里是它的一个
/// **消费者**——一个只有「追加 / 退格 / 显示」三种操作的单行输入框。
/// 真要做通用控件，那是 `ll-ui` 的事，不是本体二进制的事。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameField {
    /// 已经上屏的内容。
    text: String,
    /// 正在拼写、尚未上屏的串——**必须显示出来**，见
    /// [`NameField::display`]。
    preedit: String,
}

impl NameField {
    /// 建一个空输入框。
    pub fn new() -> NameField {
        NameField::default()
    }

    /// 当前**已上屏**的内容。预编辑串不在内——它还不是名字的一部分。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 屏上该显示的那一行：已上屏文本 + 正在拼的串 + 光标。
    ///
    /// # 这一条是中文能不能真的用起来的分界线
    ///
    /// 候选窗（列着「你/尼/泥/逆」的那个小窗）由操作系统绘制。但**正在
    /// 拼的那串拼音必须出现在输入框里**——玩家看不见自己打了什么，这个
    /// 功能就等于没做。
    ///
    /// 例：已上屏「我的」、正在拼 `shijie` ⇒ `我的shijie_`；输入法一
    /// 上屏，下一帧变成 `我的世界_`。
    ///
    /// **预编辑串与已上屏文本没有视觉区分**（不加下划线/反色）：
    /// `ll_ui::screen::ScreenData` 的一行就是一个 `String`，没有富文本，
    /// 做区分要给整条渲染链加「一行里的一段」这个概念。先让玩家看得见，
    /// 再谈好不好看——看不见是功能缺失，没有下划线只是不够漂亮。
    pub fn display(&self) -> String {
        format!("{}{}{CURSOR_GLYPH}", self.text, self.preedit)
    }

    /// 内容是不是空的（决定确认时要不要退回默认名）。
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// 正在拼写的串是不是非空——非空表示玩家还在跟输入法打交道。
    pub fn is_composing(&self) -> bool {
        !self.preedit.is_empty()
    }

    /// 更新正在拼写的串（覆盖式）。
    pub fn set_preedit(&mut self, preedit: &str) {
        if self.preedit != preedit {
            self.preedit.clear();
            self.preedit.push_str(preedit);
        }
    }

    /// 应用一次编辑。返回真表示内容变了。
    ///
    /// **按顺序逐条应用**：一帧之内玩家完全可能「打 abc → 退格 →
    /// 打 d」，顺序错了结果就错。平台层把顺序存在
    /// [`TextEdit`] 序列里，这里照着重放。
    pub fn apply(&mut self, edit: &TextEdit) -> bool {
        match edit {
            // `String::pop` 弹的就是一个 `char`——按字符退是它自带的
            // 语义，不是这里额外维护的。按字节退（`truncate(len - 1)`）
            // 会把一个汉字切成非法 UTF-8。
            TextEdit::Backspace => self.text.pop().is_some(),
            TextEdit::Insert(inserted) => self.insert(inserted),
        }
    }

    /// 追加一段上屏文本，超出长度上限的部分丢弃。
    ///
    /// 上限按**字符数**而不是字节数：一个汉字是 3 个 UTF-8 字节，按
    /// 字节算的话 24 字节只装得下 8 个汉字。
    fn insert(&mut self, inserted: &str) -> bool {
        let mut changed = false;
        for ch in inserted.chars() {
            if self.text.chars().count() >= MAX_SAVE_NAME_CHARS {
                break;
            }
            self.text.push(ch);
            changed = true;
        }
        changed
    }
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
/// # 三条通道各管各的
///
/// - **文字**走 [`InputState::text_edits`]/[`InputState::preedit`]；
/// - **确认与取消**走 [`GameKey`]——它们是动作，玩家在设置屏里能改。
///   这块屏处于 `InputContext::TextEntry` 上下文，那张表里只有这两条，
///   所以 WASD 在这里解析不出任何东西（打字不会让角色走起来）。
/// - **物理键**（`last_physical_key`）在这块屏上**完全不用**——它是
///   键位重绑那条路的东西，见模块文档。
///
/// # 正在拼字时按确认不提交
///
/// 玩家敲回车多半是在跟输入法选词。输入法通常自己就把回车吃掉了，但
/// 平台之间不一致；加这条守卫最坏是「玩家得多按一次回车」，不加则是
/// 「选个词就莫名其妙进了世界」。
pub fn update_save_name(
    field: &mut NameField,
    input: &InputState,
    origin: crate::menu_screen::SpawnOrigin,
) -> SaveNameUpdate {
    if input.was_just_pressed(GameKey::Cancel) {
        // 退回选出生地屏：玩家可能只是想再挑一次地方。**不丢弃已经
        // 打好的名字**——草稿还留着这个 `NameField`。
        // **把来处原样带回去**：玩家在选点屏上再按一次取消时，要回到
        // 当初进选点屏的那一块屏，而不是命名屏自己编一个（规格 N5
        // 判据 3）。
        return SaveNameUpdate {
            next: Some(ScreenState::SpawnPick { origin }),
            confirmed: false,
        };
    }
    for edit in input.text_edits() {
        field.apply(edit);
    }
    field.set_preedit(input.preedit());
    if input.was_just_pressed(GameKey::Confirm) && !field.is_composing() {
        return SaveNameUpdate {
            next: None,
            confirmed: true,
        };
    }
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

    fn 插入(text: &str) -> TextEdit {
        TextEdit::Insert(text.to_string())
    }

    /// 造一个「处于文本输入态」的输入状态。
    fn 文本输入态() -> InputState {
        let mut input = InputState::new();
        input.text_input_mut().set_active(true);
        input
    }

    #[test]
    fn 中文打得出来() {
        // 本批存在的全部理由。上一版这条断言写不出来——那张
        // `KeyCode -> char` 白名单表里没有任何汉字。
        // Arrange
        let mut field = NameField::new();

        // Act：输入法一次上屏一个词。
        field.apply(&插入("我的"));
        field.apply(&插入("世界"));

        // Assert
        assert_eq!(field.text(), "我的世界");
    }

    #[test]
    fn 大写与标点也打得出来() {
        // 上一版拿不到修饰键状态，所有字符一律记小写，标点更是整个
        // 打不出来。
        // Arrange
        let mut field = NameField::new();

        // Act
        field.apply(&插入("Hello, 世界！"));

        // Assert
        assert_eq!(field.text(), "Hello, 世界！");
    }

    #[test]
    fn 退格按字符退而不是按字节退() {
        // 一个汉字是 3 个 UTF-8 字节。按字节退会切出非法 UTF-8
        // （在 Rust 里直接 panic）。
        // Arrange
        let mut field = NameField::new();
        field.apply(&插入("你好"));

        // Act
        let 删掉了 = field.apply(&TextEdit::Backspace);

        // Assert
        assert!(删掉了);
        assert_eq!(field.text(), "你");
        assert_eq!(field.text().chars().count(), 1);
    }

    #[test]
    fn 空框上退格什么都不做() {
        // Arrange
        let mut field = NameField::new();

        // Act
        let 变了 = field.apply(&TextEdit::Backspace);

        // Assert
        assert!(!变了, "空框上退格不该报告「内容变了」");
        assert_eq!(field.text(), "");
    }

    #[test]
    fn 长度上限按字符数算而不是按字节数() {
        // 按字节算的话，24 字节只装得下 8 个汉字。
        // Arrange
        let mut field = NameField::new();
        let 一长串汉字: String = "汉".repeat(MAX_SAVE_NAME_CHARS + 5);

        // Act
        field.apply(&插入(&一长串汉字));

        // Assert
        assert_eq!(field.text().chars().count(), MAX_SAVE_NAME_CHARS);
        assert!(
            field.text().len() > MAX_SAVE_NAME_CHARS,
            "字节数应当远大于字符数，否则这条测试没在测汉字"
        );
    }

    #[test]
    fn 一帧里的编辑按发生顺序重放() {
        // 一帧之内「打 abc → 退格 → 打 d」是完全可能的。把这一帧的
        // 输入压成「本帧文本 + 退格次数」两个标量就表达不了顺序，合成
        // 回去只能猜。
        //
        // **刻意走 `update_save_name` 而不是直接调 `apply`**：顺序这件
        // 事的落点在「怎么消费那条队列」，直接调 `apply` 测不到它。
        // Arrange
        let mut input = 文本输入态();
        input.text_input_mut().push_committed("abc");
        input.text_input_mut().push_backspace();
        input.text_input_mut().push_committed("d");
        input.text_input_mut().push_committed("你好");
        input.text_input_mut().push_backspace();
        let mut field = NameField::new();

        // Act
        update_save_name(
            &mut field,
            &input,
            crate::menu_screen::SpawnOrigin::WorldSetup,
        );

        // Assert
        assert_eq!(field.text(), "abd你");
    }

    #[test]
    fn 正在拼的串显示在已上屏文本后面() {
        // 候选窗由操作系统画，但正在拼的拼音必须出现在输入框里——
        // 玩家看不见自己打了什么，这个功能就等于没做。
        // Arrange
        let mut field = NameField::new();
        field.apply(&插入("我的"));

        // Act
        field.set_preedit("shijie");

        // Assert
        assert_eq!(field.display(), "我的shijie_");
        assert_eq!(field.text(), "我的", "拼到一半的串还不是名字的一部分");

        // Act：上屏
        field.set_preedit("");
        field.apply(&插入("世界"));

        // Assert
        assert_eq!(field.display(), "我的世界_");
    }

    #[test]
    fn 正在拼的串不占长度上限() {
        // 否则玩家打到第 24 个字时会突然连拼音都打不出来。
        // Arrange
        let mut field = NameField::new();
        field.apply(&插入(&"a".repeat(MAX_SAVE_NAME_CHARS)));

        // Act
        field.set_preedit("shijie");

        // Assert
        assert!(field.display().contains("shijie"));
        assert!(field.is_composing());
    }

    #[test]
    fn 正在拼字时按确认不提交() {
        // 玩家敲回车多半是在跟输入法选词。不加这条守卫，选个词就
        // 莫名其妙进了世界。
        // Arrange
        let mut input = 文本输入态();
        input.text_input_mut().set_preedit("shijie");
        input.press(GameKey::Confirm);
        let mut field = NameField::new();

        // Act
        let update = update_save_name(
            &mut field,
            &input,
            crate::menu_screen::SpawnOrigin::WorldSetup,
        );

        // Assert
        assert!(!update.confirmed);
        assert_eq!(field.display(), "shijie_", "但拼到一半的串照样要显示");
    }

    #[test]
    fn 拼完上屏之后按确认才提交() {
        // 上一条的对照组：证明确认这条路本身没被堵死。
        // Arrange
        let mut input = 文本输入态();
        input.text_input_mut().push_committed("世界");
        input.press(GameKey::Confirm);
        let mut field = NameField::new();

        // Act
        let update = update_save_name(
            &mut field,
            &input,
            crate::menu_screen::SpawnOrigin::WorldSetup,
        );

        // Assert
        assert!(update.confirmed);
        assert_eq!(field.text(), "世界");
    }

    #[test]
    fn 这块屏一个字都不从物理键通道读() {
        // 物理键通道是键位重绑的东西。它若还被这块屏消费着，重绑
        // 与打字就会互相干扰，且非 QWERTY 布局下打出的字会是错的。
        // Arrange
        let mut input = 文本输入态();
        input.record_physical_key(ll_platform::keybind::KeyCode::KeyQ);
        let mut field = NameField::new();

        // Act
        update_save_name(
            &mut field,
            &input,
            crate::menu_screen::SpawnOrigin::WorldSetup,
        );

        // Assert
        assert_eq!(field.text(), "", "物理键不该变成字符");
    }

    #[test]
    fn 显示的那一行带一个尾随光标() {
        // Arrange & Act & Assert
        assert_eq!(NameField::new().display(), "_", "空框也要看得见光标");
    }

    #[test]
    fn 按取消退回选点屏时把来处原样带回去() {
        // **规格 N5 判据 3**：玩家从命名屏退回选点屏、再按一次取消，要
        // 回到当初进选点屏时的那一块屏——命名屏不能自己编一个来处。
        // 这块屏只有一个入口（选点屏确认），来处对它而言纯粹是过路件。
        //
        // 反例验证（已实跑）：把 `next` 改回写死的
        // `ScreenState::SpawnPick { origin: SpawnOrigin::WorldSetup }`，
        // 本条的第二段（转生那条路）当场变红。
        for origin in [
            crate::menu_screen::SpawnOrigin::WorldSetup,
            crate::menu_screen::SpawnOrigin::CharacterCreation,
        ] {
            // Arrange
            let mut input = 文本输入态();
            input.press(GameKey::Cancel);
            let mut field = NameField::new();

            // Act
            let update = update_save_name(&mut field, &input, origin);

            // Assert
            assert_eq!(update.next, Some(ScreenState::SpawnPick { origin }));
        }
    }
}
