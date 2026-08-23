//! 厚层容器：行式（AoS）泛型实体池，带世代索引。

use serde::{Deserialize, Serialize};

use super::EntityId;

/// 一个槽位的状态：占用中（带值）或空闲（只留世代号）。
///
/// 世代号即使在空闲时也要保留：[`Arena::despawn`] 复用这个下标前必须
/// 知道上一次占用用的是哪个世代，才能算出下一个世代号——这正是「旧
/// 标识因世代不符而失效」的账本。
#[derive(Debug, Clone, Serialize, Deserialize)]
enum Slot<T> {
    Occupied {
        generation: u32,
        value: T,
    },
    Vacant {
        generation: u32,
    },
    /// 世代号已达上限，永久弃用——见 [`Arena::despawn`] 文档「世代号
    /// 溢出」一节。这个下标之后再也不会被 [`Arena::spawn`] 选中。
    Retired,
}

/// 厚层实体池：行式（AoS）排布，数量少、按实体随机访问、一次读全部
/// 字段——见 `crate::entity::Agent` 模块文档「两层用不同排布」一节。
///
/// 世代索引解决悬垂 ID：[`EntityId`] 由 `(下标, 世代)` 构成，`despawn`
/// 后下标可复用但世代号递增，旧 `EntityId` 因世代不符而查询失败，而
/// 不是静默指向复用后的新实体。
#[derive(Debug, Clone)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    /// 可复用的空闲下标。**不变式**：其中每个下标在 `slots` 里都必须是
    /// `Vacant`，且不重复出现——序列化往返后这条不变式由
    /// `TryFrom<ArenaOwnedRepr<T>>` 重新校验。
    free: Vec<u32>,
}

impl<T> Arena<T> {
    /// 建一个空池。
    pub fn new() -> Self {
        Arena {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    /// 放入一个新实体，返回其标识。
    ///
    /// 优先复用空闲下标：复用时沿用该下标上一次 `despawn` 时已经递增
    /// 过的世代号（见该方法文档），因此每次复用发出的 `EntityId` 世代
    /// 号严格递增，指向旧世代的标识永远失效。
    pub fn spawn(&mut self, value: T) -> EntityId {
        if let Some(index) = self.free.pop() {
            let generation = match &self.slots[index as usize] {
                Slot::Vacant { generation } => *generation,
                // free 只应包含 Vacant 下标；这个不变式由 spawn/despawn
                // 自身维护，序列化往返则由 ArenaRepr::try_from 校验。
                _ => unreachable!("free 列表里的下标必须指向 Vacant 槽位"),
            };
            self.slots[index as usize] = Slot::Occupied { generation, value };
            return EntityId::new(index, generation);
        }

        let index = self.slots.len() as u32;
        self.slots.push(Slot::Occupied {
            generation: 0,
            value,
        });
        EntityId::new(index, 0)
    }

    /// 按标识取回不可变引用；世代不符或下标越界均返回 [`None`]。
    pub fn get(&self, id: EntityId) -> Option<&T> {
        match self.slots.get(id.index() as usize)? {
            Slot::Occupied { generation, value } if *generation == id.generation() => Some(value),
            _ => None,
        }
    }

    /// 按标识取回可变引用；规则同 [`Self::get`]。
    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut T> {
        match self.slots.get_mut(id.index() as usize)? {
            Slot::Occupied { generation, value } if *generation == id.generation() => Some(value),
            _ => None,
        }
    }

    /// 销毁一个实体，使其标识失效。
    ///
    /// 已死亡、世代不符或下标越界均返回 `false` 而非崩溃——时间轴队列
    /// 里可能残留已死实体的条目，销毁一个不存在的实体是正常的运行时
    /// 状况，不是需要中断整个模拟的错误。
    ///
    /// # 世代号溢出
    ///
    /// 世代号已是 [`u32::MAX`] 时，正常做法（`generation + 1`）会回绕
    /// 到 `0`——而 `0` 正是这个下标第一次被占用时发出的世代号。若那个
    /// 最早的 `EntityId` 仍残留在某处（例如时间轴队列或玩家的旧引用），
    /// 回绕后它会被误判为「世代吻合」，静默复活成完全无关的新实体。
    /// 因此世代号到达上限时改为把槽位标记 [`Slot::Retired`] 并永久
    /// 排除在复用之外——以少量下标的永久闲置，换取「世代号一旦分配
    /// 就绝不会与另一个更早的实体重合」这条更重要的保证。这个下标
    /// 之后不再进入 [`Self::spawn`] 的复用候选。
    pub fn despawn(&mut self, id: EntityId) -> bool {
        let Some(slot) = self.slots.get_mut(id.index() as usize) else {
            return false;
        };
        match slot {
            Slot::Occupied { generation, .. } if *generation == id.generation() => {
                if *generation == u32::MAX {
                    *slot = Slot::Retired;
                } else {
                    let next_generation = *generation + 1;
                    *slot = Slot::Vacant {
                        generation: next_generation,
                    };
                    self.free.push(id.index());
                }
                true
            }
            _ => false,
        }
    }

    /// 依次访问全部存活实体。
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| match slot {
            Slot::Occupied { value, .. } => Some(value),
            _ => None,
        })
    }

    /// 依次访问全部存活实体，附带各自的 [`EntityId`]。
    ///
    /// 供需要「这份数据属于哪个具体实体」的调用方使用——例如脚本状态
    /// 存储（`ll_world::mod_state`）的配额判定需要按实体过滤某个
    /// mod 的每实体存储占用，仅有 [`Self::iter`] 拿不到实体标识，无法
    /// 区分「这条记录属于哪个实体」。下标本身即槽位下标，世代号取自
    /// 该槽位当前的 `Occupied` 状态——与 [`Self::spawn`]/[`Self::get`]
    /// 使用的是同一份世代号账本，不会出现下标相同但世代不一致的情况。
    pub fn iter_with_id(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => {
                    Some((EntityId::new(index as u32, *generation), value))
                }
                _ => None,
            })
    }

    /// 依次访问全部存活实体，附带各自的 [`EntityId`]，可写。
    ///
    /// 供存档读入后的 `ContentIndex` 重映射（`ll-content` 任务 9）使用
    /// ——重映射需要同时知道「这是哪个实体」（用于比对
    /// `WorldState::player_entity`，决定降级策略该按玩家还是 NPC 的
    /// 规则走）与「改它的字段」，[`Self::iter_with_id`] 只给不可变
    /// 引用满足不了后者，[`Self::iter`] 不带标识满足不了前者。
    pub fn iter_mut_with_id(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => {
                    Some((EntityId::new(index as u32, *generation), value))
                }
                _ => None,
            })
    }

    /// 存活实体数量（不含空闲与已弃用槽位）。
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// 池中是否没有任何存活实体。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 保留槽位下标与世代号不变，把每个存活实体的值按 `f` 变换成新
    /// 类型——供存档 schema 迁移（`ll_content::migrations`）在实体自身
    /// 的形状变化（例如 `Agent` 新增字段）时重建整座 `Arena`，同时不
    /// 破坏迁移前后 [`EntityId`] 的一致性：`Vacant`/`Retired` 槽位与
    /// `free` 列表原样保留，只有 `Occupied` 槽位的 `value` 换了类型，
    /// 下标与世代号完全不受影响，任何指向旧类型某个实体的 `EntityId`
    /// 在新类型的 `Arena` 里仍然指向同一个逻辑实体。
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Arena<U> {
        let slots = self
            .slots
            .into_iter()
            .map(|slot| match slot {
                Slot::Occupied { generation, value } => Slot::Occupied {
                    generation,
                    value: f(value),
                },
                Slot::Vacant { generation } => Slot::Vacant { generation },
                Slot::Retired => Slot::Retired,
            })
            .collect();
        Arena {
            slots,
            free: self.free,
        }
    }
}

impl<T> Serialize for Arena<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ArenaRepr {
            slots: &self.slots,
            free: &self.free,
        }
        .serialize(serializer)
    }
}

/// [`Arena`] 序列化用的镜像表示，仅字段名与 [`Arena`] 一致，供
/// `#[derive(Serialize)]` 借用。
#[derive(Serialize)]
struct ArenaRepr<'a, T> {
    slots: &'a Vec<Slot<T>>,
    free: &'a Vec<u32>,
}

/// [`Arena`] 反序列化的中转表示。
///
/// 见 [`Arena::free`] 字段文档的不变式：`free` 里的每个下标都必须指向
/// 一个 `Vacant` 槽位、且不重复。这个不变式跨两个字段，任何一个字段
/// 单独反序列化都验证不出来，必须像 `ll-world` 的 `WorldState` 那样
/// 用 `#[serde(try_from = "...")]` 中转一次交叉校验，而不是直接派生
/// `Deserialize`——否则一份被篡改的存档可以让 `free` 指向一个仍然
/// `Occupied` 的下标，`spawn` 复用它时会覆盖一个存活实体的数据而不报
/// 任何错误。
#[derive(Deserialize)]
struct ArenaOwnedRepr<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<'de, T> Deserialize<'de> for Arena<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = ArenaOwnedRepr::deserialize(deserializer)?;
        Arena::try_from(repr).map_err(serde::de::Error::custom)
    }
}

impl<T> TryFrom<ArenaOwnedRepr<T>> for Arena<T> {
    type Error = String;

    fn try_from(repr: ArenaOwnedRepr<T>) -> Result<Self, Self::Error> {
        let mut seen = std::collections::HashSet::with_capacity(repr.free.len());
        for &index in &repr.free {
            if !seen.insert(index) {
                return Err(format!("空闲列表包含重复下标 {index}"));
            }
            match repr.slots.get(index as usize) {
                Some(Slot::Vacant { .. }) => {}
                Some(_) => {
                    return Err(format!("空闲列表引用的下标 {index} 并非空闲槽位"));
                }
                None => {
                    return Err(format!("空闲列表引用的下标 {index} 超出槽位数量"));
                }
            }
        }
        Ok(Arena {
            slots: repr.slots,
            free: repr.free,
        })
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 新生成的实体可以按标识取回() {
        // Arrange
        let mut arena = Arena::new();

        // Act
        let id = arena.spawn(42);

        // Assert
        assert_eq!(arena.get(id), Some(&42));
    }

    #[test]
    fn 销毁后原标识无法再取到实体() {
        // Arrange
        let mut arena = Arena::new();
        let id = arena.spawn(42);

        // Act
        arena.despawn(id);

        // Assert
        assert_eq!(arena.get(id), None);
    }

    #[test]
    fn 槽位被复用后旧标识因世代不符而失效() {
        // Arrange
        let mut arena = Arena::new();
        let first = arena.spawn(1);
        arena.despawn(first);

        // Act：复用同一个下标
        let second = arena.spawn(2);

        // Assert：下标相同，但旧标识查不到新值
        assert_eq!(first.index(), second.index());
        assert_eq!(arena.get(first), None);
        assert_eq!(arena.get(second), Some(&2));
    }

    #[test]
    fn 销毁不存在的实体返回假而非崩溃() {
        // Arrange
        let mut arena: Arena<i32> = Arena::new();
        let bogus = EntityId::new(0, 0);

        // Act
        let destroyed = arena.despawn(bogus);

        // Assert
        assert!(!destroyed);
    }

    #[test]
    fn iter_with_id产出的标识可以用来查回同一个值() {
        // Arrange：混入一次销毁，确保下标复用的场景也被覆盖——复用后
        // 的世代号必须与 iter_with_id 报出的世代号一致，否则用它反查
        // 会失败。
        let mut arena = Arena::new();
        let doomed = arena.spawn(1);
        arena.despawn(doomed);
        let survivor = arena.spawn(2);

        // Act
        let seen: Vec<(EntityId, i32)> = arena.iter_with_id().map(|(id, v)| (id, *v)).collect();

        // Assert
        assert_eq!(seen, vec![(survivor, 2)]);
        assert_eq!(arena.get(seen[0].0), Some(&2));
    }

    #[test]
    fn 序列化往返后实体数量不变() {
        // Arrange：混入一次销毁，确保空闲列表也参与往返。
        let mut arena = Arena::new();
        arena.spawn(1);
        let doomed = arena.spawn(2);
        arena.spawn(3);
        arena.despawn(doomed);

        // Act
        let json = serde_json::to_string(&arena).expect("合法的 Arena 必可序列化");
        let decoded: Arena<i32> = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.len(), arena.len());
    }

    #[test]
    fn map变换值类型后原有标识仍能查到变换后的值() {
        // 供存档迁移重建 Arena<Agent> 使用——这里用 i32 -> String 验证
        // 核心性质：下标/世代号不变，只有值的类型与内容换了。
        // Arrange：混入一次销毁，确保空闲槽位与世代号递增也参与验证。
        let mut arena = Arena::new();
        let doomed = arena.spawn(1);
        arena.despawn(doomed);
        let survivor = arena.spawn(2);

        // Act
        let mapped = arena.map(|n| format!("v{n}"));

        // Assert：旧标识（世代已递增）查不到,存活者标识能查到变换后的值。
        assert_eq!(mapped.get(doomed), None);
        assert_eq!(mapped.get(survivor), Some(&"v2".to_string()));
    }

    #[test]
    fn 空闲列表引用非空闲槽位时反序列化失败() {
        // 模拟被篡改的存档：free 指向一个仍是 Occupied 的下标。
        // Arrange
        let json = r#"{"slots":[{"Occupied":{"generation":0,"value":1}}],"free":[0]}"#;

        // Act
        let result: Result<Arena<i32>, _> = serde_json::from_str(json);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 世代号溢出时槽位被弃用而非回绕() {
        // 直接构造一个世代号已达上限的槽位，规避真的循环 u32::MAX 次。
        // 本测试位于同一模块内，可以访问私有字段。
        // Arrange
        let mut arena = Arena {
            slots: vec![Slot::Occupied {
                generation: u32::MAX,
                value: 1,
            }],
            free: Vec::new(),
        };
        let id = EntityId::new(0, u32::MAX);

        // Act
        let destroyed = arena.despawn(id);

        // Assert：销毁本身成功，但下标不得进入复用候选。
        assert!(destroyed);
        assert!(arena.free.is_empty());
        let respawned = arena.spawn(2);
        assert_ne!(respawned.index(), 0, "已弃用的下标不该被复用");
    }
}
