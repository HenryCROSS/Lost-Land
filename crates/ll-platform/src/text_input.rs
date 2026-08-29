//! 文本输入通道：把「玩家实际输入了什么文字」接出来。
//!
//! # 它与物理键通道回答的是两个不同的问题
//!
//! | 通道 | 回答什么 | 唯一正当用途 |
//! |---|---|---|
//! | [`crate::input::InputState::last_physical_key`] | 玩家按下了键盘上**哪个位置** | **键位重绑**——玩家要绑的是「那个键」，不是「那个键打出的字」。AZERTY 键盘上 `KeyCode::KeyW` 那个位置印着的是 Z，重绑必须记位置 |
//! | 本模块（[`TextInput`]） | 玩家实际**输入了什么文字** | 任何文本输入框：存档名、角色名、将来的聊天与 mod 搜索 |
//!
//! 两条通道并存**不是冗余**。它们在 QWERTY + 英文输入法这一种情况下
//! 看起来重合，一旦换成非 QWERTY 布局或打开中文输入法就立刻分叉：一次
//! 上屏的「你好」根本没有对应的物理键，而一个物理键在不同布局下打出
//! 不同的字。上一批的存档命名之所以只能输入小写 ASCII，根因不是那张
//! `KeyCode -> char` 映射表写得不好，是**它在回答错误的问题**。
//!
//! # 两条来源都要接
//!
//! - `winit::event::WindowEvent::Ime`（需先 `Window::set_ime_allowed(true)`）
//!   ——中文/日文/韩文等经输入法上屏的文本，以及**正在拼写、尚未上屏**
//!   的预编辑串；
//! - `winit::event::KeyEvent::text`——直接键入（含非 ASCII 键盘布局、
//!   大写字母、标点，以及 Windows 上死键组合出来的字符）。
//!
//! **只接一条都会漏**：只接 IME 会漏掉直接键入（拉丁布局下输入法根本
//! 不参与），只接键盘文本会漏掉中文。
//!
//! # 为什么是「有序的编辑序列」而不是「本帧文本 + 退格次数」
//!
//! 一帧之内玩家完全可能「打 abc → 退格 → 打 d」。两个标量字段表达不了
//! 顺序，合成回去只能猜；[`TextEdit`] 序列把顺序**存进类型里**，消费方
//! 照着重放就一定对。这与本仓库反复吃过亏的那类缺陷（把一份真相拆成
//! 两份互相依赖的副本，分叉时没有任何东西会报错）是同一条纪律。
//!
//! # 预编辑串跨帧保持，编辑序列每帧清空
//!
//! [`TextInput::edits`] 是**事件**——发生过一次就该被消费掉，
//! [`TextInput::end_frame`] 清空它，与 `last_physical_key` 的生命周期
//! 一致。[`TextInput::preedit`] 是**状态**——玩家还在拼，屏上必须一直
//! 显示着，`end_frame` 不动它。

/// 一次文本编辑动作，**按发生顺序**排进 [`TextInput::edits`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEdit {
    /// 插入一段**已经上屏**的文本。
    ///
    /// 是一整段而不是单个字符：输入法一次上屏一个词（「世界」是两个
    /// 字符一次到达），且 winit 的 `KeyEvent::text` 文档明确写着
    /// Windows 上死键无法合成时**一次会产出两个字符**。
    Insert(String),
    /// 删掉插入点前**一个字符**（不是一个字节）。
    ///
    /// 消费方必须按字符退——一个汉字是 3 个 UTF-8 字节，按字节退会
    /// 切出非法 UTF-8。
    Backspace,
}

/// 文本输入通道的一帧状态。
///
/// 由 [`crate::window`] 的事件循环填充，挂在
/// [`crate::input::InputState`] 上供上层读取——见模块文档。
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    active: bool,
    edits: Vec<TextEdit>,
    preedit: String,
}

impl TextInput {
    /// 建一个关闭的空通道。
    pub const fn new() -> TextInput {
        TextInput {
            active: false,
            edits: Vec::new(),
            preedit: String::new(),
        }
    }

    /// 现在是不是文本输入态。
    ///
    /// 这个标志与「IME 是否开启」「游戏按键是否查得到绑定」**共用同一个
    /// 判据**（`AppHandler::input_context() == InputContext::TextEntry`），
    /// 由事件循环一处设置，见 [`crate::window`]。三者共用一个判据，就
    /// 不可能出现「IME 开了但通道没收数据」这种半接线状态。
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// 进入/退出文本输入态。
    ///
    /// **退出时同时清空编辑队列与预编辑串**：文本输入态一结束，还没
    /// 上屏的那半句拼音就作废了，把它留到下一次开框会凭空多出几个字母。
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.edits.clear();
            self.preedit.clear();
        }
    }

    /// 本帧发生的编辑，按顺序。
    pub fn edits(&self) -> &[TextEdit] {
        &self.edits
    }

    /// 正在拼写、**尚未上屏**的串。
    ///
    /// 候选窗（列着「你/尼/泥/逆」的那个小窗）由操作系统绘制，我们既
    /// 画不了也不该画；但**这一串必须由调用方显示在输入框里**，否则
    /// 玩家看不见自己打了什么——这是中文输入可用与不可用的分界线。
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// 记一段**已上屏**的文本。
    ///
    /// # 过滤规则：只留可打印字符
    ///
    /// winit 的 `KeyEvent::text` 会把「任何有文本表示的键」都填进来
    /// ——`Enter` 是 `"\r"`、`Tab` 是 `"\t"`、`Backspace` 是 `"\u{8}"`、
    /// `Esc` 是 `"\u{1b}"`。这些必须挡掉，否则回车会把一个控制字符插进
    /// 名字里。
    ///
    /// 判据是「保留非控制字符」而不是「排除这几个已知的」——白名单式
    /// 判据漏掉一个只是「那个字符打不出来」，黑名单式判据漏掉一个就是
    /// 「奇怪的东西钻进了数据」。仓库在 `ll_mod::asset_vfs` 那次路径
    /// 校验事故里已经为后者付过一次代价。
    ///
    /// **刻意不做「只留 ASCII」的过滤**：那正是本通道要消灭的东西。
    /// 中文、标点、大写字母全部原样通过。
    ///
    /// 未处于文本输入态时**整段丢弃**——玩家在世界里按 W 不该悄悄往
    /// 某个看不见的缓冲区里堆字符。过滤后为空也不入队（不产生一条
    /// 什么都不插入的 `Insert`）。
    pub fn push_committed(&mut self, text: &str) {
        if !self.active {
            return;
        }
        let kept: String = text.chars().filter(|c| !c.is_control()).collect();
        if kept.is_empty() {
            return;
        }
        self.edits.push(TextEdit::Insert(kept));
    }

    /// 记一次退格。未处于文本输入态时丢弃，理由同
    /// [`Self::push_committed`]。
    pub fn push_backspace(&mut self) {
        if !self.active {
            return;
        }
        self.edits.push(TextEdit::Backspace);
    }

    /// 覆盖式地设置预编辑串（不是追加）。
    ///
    /// 输入法每敲一个字母就重发一次完整的预编辑串，追加会得到
    /// 「sshshi...」。未处于文本输入态时丢弃。
    pub fn set_preedit(&mut self, text: impl Into<String>) {
        if !self.active {
            return;
        }
        self.preedit = text.into();
    }

    /// 清空预编辑串——输入法启用/停用那一刻调用。
    pub fn clear_preedit(&mut self) {
        self.preedit.clear();
    }

    /// 结束当前帧：清空编辑队列，**保住预编辑串**。见模块文档最后一节。
    pub fn end_frame(&mut self) {
        self.edits.clear();
    }

    /// 清空编辑队列与预编辑串，**但不改变 [`Self::is_active`]**。
    ///
    /// # 为什么 `active` 不在此列
    ///
    /// 本方法由 [`crate::input::InputState::clear`] 调用，那个方法的
    /// 语义是「这一刻按住的键全部视为松开」（窗口失焦、
    /// `UiModeStack` 上下文切换）。`active` 不是按键状态，是事件循环
    /// 设的**模式**。把它一起清掉会踩一个真实存在的坑：
    /// `ll_ui::widget::ui_mode::UiModeStack::push` 的定义里就带着
    /// `input.clear()`，于是「进入文本输入态」那一次压栈会把刚打开的
    /// 通道当场关上。
    pub fn clear(&mut self) {
        self.edits.clear();
        self.preedit.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 已激活() -> TextInput {
        let mut text = TextInput::new();
        text.set_active(true);
        text
    }

    #[test]
    fn 中文原样进队列且不做ascii过滤() {
        // 本批存在的全部理由：上一批那条通道只认得小写 ASCII。
        // Arrange
        let mut text = 已激活();

        // Act
        text.push_committed("你好");
        text.push_committed("Hello, 世界！");

        // Assert
        assert_eq!(
            text.edits(),
            [
                TextEdit::Insert("你好".to_string()),
                TextEdit::Insert("Hello, 世界！".to_string()),
            ]
        );
    }

    #[test]
    fn 控制字符被挡掉且整段被挡时不入队() {
        // winit 的 `KeyEvent::text` 把 Enter 填成 "\r"、Backspace 填成
        // "\u{8}"——不挡掉，按一次回车就会有一个控制字符钻进存档名。
        // Arrange
        let mut text = 已激活();

        // Act
        text.push_committed("\r");
        text.push_committed("\u{8}");
        text.push_committed("\t");
        text.push_committed("a\rb");

        // Assert
        assert_eq!(
            text.edits(),
            [TextEdit::Insert("ab".to_string())],
            "整段都是控制字符时不该留下一条什么都不插入的编辑"
        );
    }

    #[test]
    fn 未处于文本输入态时文本与退格全部丢弃() {
        // 玩家在世界里按 W 不该悄悄堆进某个看不见的缓冲区。
        // Arrange
        let mut text = TextInput::new();

        // Act
        text.push_committed("w");
        text.push_backspace();
        text.set_preedit("ni");

        // Assert
        assert!(text.edits().is_empty());
        assert_eq!(text.preedit(), "");
    }

    #[test]
    fn 结束一帧清编辑队列但保住预编辑串() {
        // 编辑是事件（消费掉就没了），预编辑是状态（玩家还在拼，屏上
        // 得一直显示着）。
        // Arrange
        let mut text = 已激活();
        text.push_committed("a");
        text.set_preedit("shijie");

        // Act
        text.end_frame();

        // Assert
        assert!(text.edits().is_empty());
        assert_eq!(text.preedit(), "shijie");
    }

    #[test]
    fn 退出文本输入态时预编辑串一并作废() {
        // Arrange
        let mut text = 已激活();
        text.push_committed("a");
        text.set_preedit("shijie");

        // Act
        text.set_active(false);

        // Assert
        assert!(text.edits().is_empty());
        assert_eq!(text.preedit(), "");
        assert!(!text.is_active());
    }

    #[test]
    fn 清空不改变是否处于文本输入态() {
        // `UiModeStack::push` 自带 `input.clear()`——若 clear 把 active
        // 一并清掉，「进入文本输入态」那一次压栈会把刚打开的通道当场
        // 关上，见 `TextInput::clear` 文档。
        // Arrange
        let mut text = 已激活();
        text.push_committed("a");
        text.set_preedit("shijie");

        // Act
        text.clear();

        // Assert
        assert!(text.is_active(), "clear 清的是按键状态，不是模式");
        assert!(text.edits().is_empty());
        assert_eq!(text.preedit(), "");
    }

    #[test]
    fn 预编辑串是覆盖不是追加() {
        // 输入法每敲一个字母都重发整串，追加会得到 "sshshi..."。
        // Arrange
        let mut text = 已激活();

        // Act
        text.set_preedit("s");
        text.set_preedit("sh");
        text.set_preedit("shi");

        // Assert
        assert_eq!(text.preedit(), "shi");
    }
}
