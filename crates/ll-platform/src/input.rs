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

/// 全部动作键，顺序必须与 [`GameKey`] 的变体声明顺序一致。
///
/// 存在的意义是让「新增变体」变成编译期错误：下方的穷尽 match 测试会在
/// 漏登记时拒绝编译。若只靠手抄一个数字常量，新增变体后代码照常编译、
/// 测试照常通过，直到运行时数组越界才暴露。
#[cfg_attr(not(test), allow(dead_code))]
const ALL_KEYS: [GameKey; KEY_COUNT] = [
    GameKey::Up,
    GameKey::Down,
    GameKey::Left,
    GameKey::Right,
    GameKey::Confirm,
    GameKey::Cancel,
    GameKey::Menu,
    GameKey::Map,
    GameKey::Wait,
];

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
    /// **刻意不清除「刚按下」标志**：`just_pressed` 只能由 `end_frame` 清除。
    /// 「本帧内曾按下过」与「当前是否按住」是两个独立事实——若在此处一并
    /// 清除，同一帧内按下又松开的快速点击就会被静默丢弃。本项目主循环会在
    /// 玩家思考的空窗期推进离屏世界模拟，慢帧属预期常态，届时丢输入的概率
    /// 恰恰最高。
    pub fn release(&mut self, key: GameKey) {
        self.held[key.index()] = false;
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

    /// 清空全部按键状态。
    ///
    /// 窗口失去焦点时必须调用。操作系统只把按键事件送给焦点窗口，
    /// 玩家按住方向键时切走，对应的松开事件永远不会送达——不清空的话
    /// `held` 会永久为真，切回来后角色持续移动且没有任何按键能解除。
    ///
    /// 「刚按下」标志一并清空：失焦瞬间尚未被消费的输入已经失去意义。
    pub fn clear(&mut self) {
        self.held = [false; KEY_COUNT];
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

    #[test]
    fn 松开不会撤销本帧的刚按下判定() {
        // 同一帧内按下又松开的快速点击必须仍被游戏看到。
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.release(GameKey::Confirm);

        // Assert
        assert!(input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 清空后不再有任何按键处于按住状态() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Right);

        // Act
        input.clear();

        // Assert
        assert!(!input.is_held(GameKey::Right));
    }

    #[test]
    fn 清空后刚按下的判定一并失效() {
        // Arrange
        let mut input = InputState::new();
        input.press(GameKey::Confirm);

        // Act
        input.clear();

        // Assert
        assert!(!input.was_just_pressed(GameKey::Confirm));
    }

    #[test]
    fn 每个动作键都已登记且下标互不重复() {
        // 这个测试的真正作用在编译期：下面的 match 是穷尽的，新增 GameKey
        // 变体而忘记同步 ALL_KEYS 与 KEY_COUNT 时，它会拒绝编译，
        // 而不是等到运行时数组越界。
        // Arrange
        let mut seen = [false; KEY_COUNT];

        // Act
        for key in ALL_KEYS {
            // 穷尽 match：新增变体时此处必须补齐，否则编译失败。
            let expected_index = match key {
                GameKey::Up => 0,
                GameKey::Down => 1,
                GameKey::Left => 2,
                GameKey::Right => 3,
                GameKey::Confirm => 4,
                GameKey::Cancel => 5,
                GameKey::Menu => 6,
                GameKey::Map => 7,
                GameKey::Wait => 8,
            };
            assert_eq!(key.index(), expected_index);
            seen[key.index()] = true;
        }

        // Assert
        assert!(seen.iter().all(|slot| *slot));
    }
}
