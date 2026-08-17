//! 地形种类与其游戏规则属性。

/// 地形种类。
///
/// 用 `u16` 而非枚举：mod 需要能注册新地形，而枚举无法在运行时扩展。
/// 本体的常量由注册表负责把命名空间 ID 映射到数值。
///
/// # 编号分组
///
/// 自然地形占 `0..8`，建筑地形从 `100` 起。两组之间刻意空开
/// `8..100` 这段编号：自然地形以后要加沼泽、熔岩之类新种类，建筑地形
/// 以后要加桥梁、陷阱之类新种类，各自往后接着编即可，不必为了插入
/// 新种类而把已经序列化进存档的既有数值往后挪。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TerrainKind(pub u16);

impl TerrainKind {
    // ---- 自然地形：0..8 ----
    /// 深水：不可通行，不阻挡视线（水面开阔）。
    pub const DEEP_WATER: TerrainKind = TerrainKind(0);
    /// 浅水：可通行但比平地慢，不阻挡视线。
    ///
    /// 「可过但更慢」是刻意的通行性分级：只用「能过/不能过」两档没法
    /// 表达浅水这种「过得去、但要放慢」的地形，所以 [`Self::move_cost`]
    /// 用具体数值而非布尔值承载这层信息。
    pub const SHALLOW_WATER: TerrainKind = TerrainKind(1);
    /// 沙地：可通行，比平地略慢，不阻挡视线。
    pub const SAND: TerrainKind = TerrainKind(2);
    /// 草地：可通行，移动代价为基准值，不阻挡视线。
    pub const GRASS: TerrainKind = TerrainKind(3);
    /// 森林：可通行但较慢，树冠阻挡视线。
    pub const FOREST: TerrainKind = TerrainKind(4);
    /// 丘陵：可通行但较慢，不阻挡视线。
    pub const HILL: TerrainKind = TerrainKind(5);
    /// 山地：可通行但代价极高，山体阻挡视线。
    ///
    /// 刻意不设为不可通行：保留翻山的可能性比一刀切更有玩法空间，
    /// 极高的移动代价已经足以让寻路算法在有替代路线时绕开它。
    pub const MOUNTAIN: TerrainKind = TerrainKind(6);
    /// 雪地：可通行但较慢，不阻挡视线。
    pub const SNOW: TerrainKind = TerrainKind(7);

    // ---- 建筑地形：100.. ----
    /// 木地板：可通行，移动代价为基准值，不阻挡视线。
    pub const FLOOR_WOOD: TerrainKind = TerrainKind(100);
    /// 石地板：可通行，移动代价为基准值，不阻挡视线。
    pub const FLOOR_STONE: TerrainKind = TerrainKind(101);
    /// 木墙：不可通行，阻挡视线。
    pub const WALL_WOOD: TerrainKind = TerrainKind(102);
    /// 石墙：不可通行，阻挡视线。
    pub const WALL_STONE: TerrainKind = TerrainKind(103);
    /// 关着的门：不可通行，阻挡视线。
    ///
    /// 门的开合状态用两个独立的 `TerrainKind`（本变体与
    /// [`Self::DOOR_OPEN`]）表示，而不是一个 `DOOR` 种类外加一张开合
    /// 状态表。开门就是一个 `Effect`，把该格从这个值换成
    /// [`Self::DOOR_OPEN`]，状态天然随地形网格一起序列化，不需要额外
    /// 的状态层，也完全落在「意图—结算—效果」架构上。反过来若做成
    /// 「一个种类 + 一张状态表」，就多出一份必须与地形网格保持同步的
    /// 状态——这类同步失败极难排查，看似「省了一个枚举值」，实际是
    /// 拿架构简单性去换省下的那点编号空间，不划算。
    pub const DOOR_CLOSED: TerrainKind = TerrainKind(104);
    /// 开着的门：可通行，不阻挡视线。理由见 [`Self::DOOR_CLOSED`]。
    pub const DOOR_OPEN: TerrainKind = TerrainKind(105);
    /// 窗：不可通行，但不阻挡视线。
    ///
    /// 这个组合是刻意设计，不是疏漏：窗户可以隔窗放箭、也会被隔窗
    /// 看见，是有价值的战术要素。**不要把这一格「修」成阻挡视线**——
    /// 那会让弓箭手在窗后失去视野，等于把这个战术点废掉。
    pub const WINDOW: TerrainKind = TerrainKind(106);
    /// 上楼梯：可通行，比平地略慢，不阻挡视线。
    pub const STAIRS_UP: TerrainKind = TerrainKind(107);
    /// 下楼梯：可通行，比平地略慢，不阻挡视线。
    pub const STAIRS_DOWN: TerrainKind = TerrainKind(108);

    /// 是否是当前版本已知的地形 ID。
    ///
    /// 用于 [`Self::blocks_sight`]/[`Self::blocks_move`]/[`Self::move_cost`]
    /// 内部的 `debug_assert!`：P2 阶段还没有 mod 注册表（见
    /// `docs/superpowers/specs/2026-08-16-lostland-design.md` §15 P4 行的
    /// 迁移债务记录），任何不在本体常量之列的 ID 在这个阶段只可能来自
    /// 两种情况——本体新增了地形常量却忘了把它加进这里，或者调用方
    /// 传入了垃圾 ID。二者都该在开发期被发现。
    fn is_known(self) -> bool {
        matches!(
            self,
            TerrainKind::DEEP_WATER
                | TerrainKind::SHALLOW_WATER
                | TerrainKind::SAND
                | TerrainKind::GRASS
                | TerrainKind::FOREST
                | TerrainKind::HILL
                | TerrainKind::MOUNTAIN
                | TerrainKind::SNOW
                | TerrainKind::FLOOR_WOOD
                | TerrainKind::FLOOR_STONE
                | TerrainKind::WALL_WOOD
                | TerrainKind::WALL_STONE
                | TerrainKind::DOOR_CLOSED
                | TerrainKind::DOOR_OPEN
                | TerrainKind::WINDOW
                | TerrainKind::STAIRS_UP
                | TerrainKind::STAIRS_DOWN
        )
    }

    /// 该地形是否阻挡视线。
    ///
    /// 只有实体足够高大、足够密实的地形才返回真：森林的树冠、山地的
    /// 山体、墙体、关着的门。窗户特意不在此列——见 [`Self::WINDOW`]。
    ///
    /// 这是 FOV 每格都要过的热路径，未知 ID 用 `debug_assert!` 而非
    /// `tracing::warn!` 提示：`debug_assert!` 只在 debug 构建生效，
    /// release 零开销；无条件打日志的开销会在成千上万格的视野计算里
    /// 累积起来，而且没法在 release 构建里关掉。
    pub fn blocks_sight(&self) -> bool {
        debug_assert!(self.is_known(), "未注册的地形 ID: {self:?}");
        matches!(
            *self,
            TerrainKind::FOREST
                | TerrainKind::MOUNTAIN
                | TerrainKind::WALL_WOOD
                | TerrainKind::WALL_STONE
                | TerrainKind::DOOR_CLOSED
        )
    }

    /// 该地形是否完全不可通行。理由见 [`Self::blocks_sight`] 关于
    /// `debug_assert!` 的说明——寻路同样每格都要调用这个函数。
    pub fn blocks_move(&self) -> bool {
        debug_assert!(self.is_known(), "未注册的地形 ID: {self:?}");
        matches!(
            *self,
            TerrainKind::DEEP_WATER
                | TerrainKind::WALL_WOOD
                | TerrainKind::WALL_STONE
                | TerrainKind::DOOR_CLOSED
                | TerrainKind::WINDOW
        )
    }

    /// 移动经过该地形的代价，以平地（[`Self::GRASS`]）的 100 为基准。
    ///
    /// 用 `u32::MAX` 而非 `Option` 表示不可通行，让寻路算法不必对每格
    /// 做分支判断：不可通行地形自然地成为「代价无穷大，永远不会被
    /// 选中」。可通行地形之间的代价分级（浅水 200、山地 400 等）让
    /// 「过得去但更慢」这种地形能被正确表达，而不是被压扁成布尔值。
    ///
    /// 未识别的自定义地形（mod 注册的 ID）默认按平地基准处理：这是
    /// 对扩展 ID 最安全的兜底——既不无故挡路，也不无故挡视线。开发期
    /// 仍会经由 [`Self::blocks_move`] 内的 `debug_assert!` 被提示。
    pub fn move_cost(&self) -> u32 {
        if self.blocks_move() {
            return u32::MAX;
        }
        match *self {
            TerrainKind::SHALLOW_WATER => 200,
            TerrainKind::SAND => 120,
            TerrainKind::FOREST => 150,
            TerrainKind::HILL => 150,
            TerrainKind::MOUNTAIN => 400,
            TerrainKind::SNOW => 150,
            TerrainKind::STAIRS_UP | TerrainKind::STAIRS_DOWN => 150,
            _ => 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 山地阻挡视线() {
        // Arrange & Act & Assert
        assert!(TerrainKind::MOUNTAIN.blocks_sight());
    }

    #[test]
    fn 草地不阻挡视线() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::GRASS.blocks_sight());
    }

    #[test]
    fn 深水不可通行() {
        // Arrange & Act & Assert
        assert!(TerrainKind::DEEP_WATER.blocks_move());
    }

    #[test]
    fn 浅水可以通行() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::SHALLOW_WATER.blocks_move());
    }

    #[test]
    fn 浅水的移动代价高于草地() {
        // Arrange & Act
        let shallow_water_cost = TerrainKind::SHALLOW_WATER.move_cost();
        let grass_cost = TerrainKind::GRASS.move_cost();

        // Assert
        assert!(shallow_water_cost > grass_cost);
    }

    #[test]
    fn 山可以通行() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::MOUNTAIN.blocks_move());
    }

    #[test]
    fn 山的移动代价远高于平地() {
        // Arrange & Act
        let mountain_cost = TerrainKind::MOUNTAIN.move_cost();
        let grass_cost = TerrainKind::GRASS.move_cost();

        // Assert
        assert!(mountain_cost > grass_cost * 2);
    }

    #[test]
    fn 森林阻挡视线() {
        // Arrange & Act & Assert
        assert!(TerrainKind::FOREST.blocks_sight());
    }

    #[test]
    fn 森林可以通行() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::FOREST.blocks_move());
    }

    #[test]
    fn 窗不可通行() {
        // 这是刻意设计而非疏漏：窗户可以隔窗放箭、也会被隔窗看见，
        // 详见 TerrainKind::WINDOW 的文档注释。不要把这条断言删掉或
        // 改成 assert!(!...)——那意味着有人把窗「修」成了墙。
        // Arrange & Act & Assert
        assert!(TerrainKind::WINDOW.blocks_move());
    }

    #[test]
    fn 窗不阻挡视线() {
        // 与上一条断言配对：窗挡路但不挡视线，这是刻意设计而非疏漏。
        // Arrange & Act & Assert
        assert!(!TerrainKind::WINDOW.blocks_sight());
    }

    #[test]
    fn 关着的门不可通行() {
        // Arrange & Act & Assert
        assert!(TerrainKind::DOOR_CLOSED.blocks_move());
    }

    #[test]
    fn 关着的门阻挡视线() {
        // Arrange & Act & Assert
        assert!(TerrainKind::DOOR_CLOSED.blocks_sight());
    }

    #[test]
    fn 开着的门可以通行() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::DOOR_OPEN.blocks_move());
    }

    #[test]
    fn 开着的门不阻挡视线() {
        // Arrange & Act & Assert
        assert!(!TerrainKind::DOOR_OPEN.blocks_sight());
    }

    #[test]
    fn 不可通行地形的移动代价为最大值() {
        // 用 u32::MAX 而非 Option，让寻路算法不必对每格做分支判断。
        // Arrange & Act & Assert
        assert_eq!(TerrainKind::DEEP_WATER.move_cost(), u32::MAX);
    }
}
