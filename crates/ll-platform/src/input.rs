//! 输入状态聚合。
//!
//! 本模块把操作系统的按键事件归约为**游戏语义的动作**，而不是把物理
//! 按键直接暴露给上层。这样按键重绑定与手柄支持只需改本模块的映射表，
//! 上层逻辑一行都不用动。
//!
//! # 为什么要区分「按住」与「刚按下」
//!
//! 回合制里这两者语义完全不同：按住方向键应当连续移动，但按住确认键
//! 绝不能反复触发同一个菜单项。操作系统还会发送按键重复事件，若不去重，
//! 长按确认键会把整个菜单一路点穿。

/// 游戏语义的动作键。
///
/// 上层只认这些，不认物理按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameKey {
    /// 向上移动或菜单上移。
    Up,
    /// 向下移动或菜单下移。
    Down,
    /// 向左移动或菜单左移。
    Left,
    /// 向右移动或菜单右移。
    Right,
    /// 确认。
    Confirm,
    /// 取消或返回。
    Cancel,
    /// 打开主菜单。
    Menu,
    /// 打开世界地图。
    Map,
    /// 原地等待一回合。
    Wait,
}

/// 动作键总数，用于状态数组定长。
const KEY_COUNT: usize = 9;

impl GameKey {
    /// 在状态数组中的下标。
    const fn index(self) -> usize {
        self as usize
    }
}

/// 一帧内的输入状态。
///
/// 用定长数组而非哈希集合：动作键数量固定且很少，数组查询是一次下标
/// 访问，而哈希查询涉及哈希计算与可能的冲突处理。输入查询在每帧的 UI
/// 与逻辑中被调用数十次，这个差别值得。
#[derive(Debug, Clone)]
pub struct InputState {
    held: [bool; KEY_COUNT],
    just_pressed: [bool; KEY_COUNT],
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    /// 建立全部松开的初始状态。
    pub const fn new() -> Self {
        InputState {
            held: [false; KEY_COUNT],
            just_pressed: [false; KEY_COUNT],
        }
    }

    /// 记录一次按下。
    ///
    /// 若该键本已按住，则不重新置起「刚按下」标志——这就是对操作系统
    /// 按键重复事件的去重。
    pub fn press(&mut self, key: GameKey) {
        let index = key.index();
        if !self.held[index] {
            self.just_pressed[index] = true;
        }
        self.held[index] = true;
    }

    /// 记录一次松开。
    ///
    /// 同时清「刚按下」标志：若不清，按下后未等 `end_frame` 就松开会留下
    /// 「刚按下为真但未按住」的自相矛盾状态——这正是黑箱属性测试
    /// （见 `tests/input_blackbox.rs`）撞出的问题。
    pub fn release(&mut self, key: GameKey) {
        let index = key.index();
        self.held[index] = false;
        self.just_pressed[index] = false;
    }

    /// 该键当前是否被按住。
    pub fn is_held(&self, key: GameKey) -> bool {
        self.held[key.index()]
    }

    /// 该键是否在本帧刚刚被按下。
    pub fn was_just_pressed(&self, key: GameKey) -> bool {
        self.just_pressed[key.index()]
    }

    /// 结束当前帧，清空「刚按下」标志。
    ///
    /// 必须在每帧逻辑处理**之后**调用。放在处理之前会让所有「刚按下」
    /// 判定永远为假。
    pub fn end_frame(&mut self) {
        self.just_pressed = [false; KEY_COUNT];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 按下后处于按住状态() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(input.is_held(GameKey::Confirm));
    }

    #[test]
    fn 刚按下的判定在帧结束后失效() {
        // 回合制里「刚按下」与「按住」必须区分：按住方向键应连续移动，
        // 但按住确认键不能反复触发同一个菜单项。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.end_frame();

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 帧结束后按住状态依然保留() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        input.end_frame();

        // Assert
        assert!(input.is_held(GameKey::Right));
    }

    #[test]
    fn 松开后不再处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Left);

        // Act
        input.release(GameKey::Left);

        // Assert
        assert!(!input.is_held(GameKey::Left));
    }

    #[test]
    fn 重复按下不会重新触发刚按下判定() {
        // 操作系统的按键重复事件会连续发送按下，若不去重，长按确认键会
        // 把整个菜单一路点穿。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);
        input.end_frame();

        // Act
        input.press(GameKey::Confirm);

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 不同按键的状态互不干扰() {
        // Arrange
        let mut input = InputState::new();

        // Act
        input.press(GameKey::Up);

        // Assert
        assert!(!input.is_held(GameKey::Down));
    }
}
