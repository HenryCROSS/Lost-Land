//! 地形种类与其游戏规则属性。

/// 地形种类。
///
/// 用 `u16` 而非枚举：mod 需要能注册新地形，而枚举无法在运行时扩展。
/// 本体的八种作为常量提供，注册表负责把命名空间 ID 映射到数值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TerrainKind(pub u16);

impl TerrainKind {
    /// 深水：不可通行，但不阻挡视线（水面开阔）。
    pub const DEEP_WATER: TerrainKind = TerrainKind(0);
    /// 浅水：可通行但较慢，不阻挡视线。
    pub const SHALLOW_WATER: TerrainKind = TerrainKind(1);
    /// 沙地：可通行，移动代价与平地相同。
    pub const SAND: TerrainKind = TerrainKind(2);
    /// 草地：可通行，移动代价与平地相同，不阻挡视线。
    pub const GRASS: TerrainKind = TerrainKind(3);
    /// 森林：可通行但较慢，树冠阻挡视线。
    pub const FOREST: TerrainKind = TerrainKind(4);
    /// 丘陵：可通行但较慢，不阻挡视线。
    pub const HILL: TerrainKind = TerrainKind(5);
    /// 山地：不可通行，山体阻挡视线。
    pub const MOUNTAIN: TerrainKind = TerrainKind(6);
    /// 雪地：可通行但较慢，不阻挡视线。
    pub const SNOW: TerrainKind = TerrainKind(7);

    /// 该地形是否阻挡视线。
    ///
    /// 只有实体足够高大、能挡住视线的地形才返回真：森林的树冠、
    /// 山地的山体。水面与平地无论种类都开阔可视。
    pub fn blocks_sight(&self) -> bool {
        matches!(*self, TerrainKind::FOREST | TerrainKind::MOUNTAIN)
    }

    /// 该地形是否完全不可通行。
    pub fn blocks_move(&self) -> bool {
        matches!(*self, TerrainKind::DEEP_WATER | TerrainKind::MOUNTAIN)
    }

    /// 移动经过该地形的代价。
    ///
    /// 用 `u32::MAX` 而非 `Option`，让寻路算法不必对每格做分支判断：
    /// 不可通行地形自然地成为「代价无穷大，永远不会被选中」。
    pub fn move_cost(&self) -> u32 {
        if self.blocks_move() {
            return u32::MAX;
        }
        match *self {
            TerrainKind::SHALLOW_WATER
            | TerrainKind::FOREST
            | TerrainKind::HILL
            | TerrainKind::SNOW => 2,
            _ => 1,
        }
    }
}
