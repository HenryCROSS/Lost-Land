//! 内容的命名空间标识符与运行时索引池。
//!
//! # 为什么 ID 必须是字符串而不是整数
//!
//! 本项目遵循「本体即 Mod」原则：本体内容与 mod 内容走完全相同的注册
//! 通道。若 ID 是裸整数，两个 mod 必然撞号。命名空间字符串
//! （`lostland:fireball`、`yourmod:fireball`）从根本上杜绝冲突。
//!
//! # 为什么还需要整数索引
//!
//! 字符串比较与哈希对每帧执行的热路径来说太慢。因此装载完成后把所有
//! 字符串 ID 一次性映射为紧凑整数：**外部看字符串保证不冲突，内部用
//! 整数保证性能**。
//!
//! # 存档必须写字符串
//!
//! 索引依赖加载顺序。若存档里写的是索引，玩家调整 mod 顺序后，存档中
//! 的火球会变成一把椅子。故存档需持久化字符串，或在存档头保存
//! 「索引 ↔ 字符串」映射表。

use crate::error::CoreError;
use std::collections::HashMap;
use std::fmt;

/// 内容标识符，形如 `命名空间:路径`。
///
/// # `serde` 走 `try_from`/`into`，不裸派生（ADR 0011）
///
/// 本类型**有**一条不依赖任何运行期上下文的不变式：两段都非空、且只由
/// 小写字母/数字/下划线/连字符/点号组成（见 [`NamespacedId::parse`]）。
/// 裸派生会让反序列化绕过 `parse` 直接填两个 `Box<str>` 字段，凭空造出
/// 一个 `NamespacedId { namespace: "", path: "MyMod:x" }` 这类**结构上
/// 非法**的值——ADR 0011 点名的正是这个失效模式。因此线格式是一个
/// **字符串**，反序列化必经 `parse`。
///
/// 这与紧邻下方 [`ContentIndex`] 的裸派生**不矛盾**：那个类型没有任何
/// 无上下文不变式（任意 `u32` 都形状合法），见它自己的文档。
///
/// **谁需要它**：势力播种批次把 [`ContentIndex`] 之外的
/// `ll_world::entity::OrgInstance::authored`（`Option<NamespacedId>`）
/// 放进了存档主体——那是本类型第一次需要持久化。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamespacedId {
    namespace: Box<str>,
    path: Box<str>,
}

impl NamespacedId {
    /// 解析 `命名空间:路径` 形式的标识符。
    ///
    /// 两部分均只允许小写字母、数字、下划线、连字符与点号，且不得为空。
    /// 强制小写是为了避免 `MyMod:Fire` 与 `mymod:fire` 这类肉眼难辨的
    /// 重复 ID——这种冲突在 mod 生态里极难排查。
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        let invalid = || CoreError::InvalidIdentifier(raw.to_owned());

        // 用 split_once 而非 split(':')，因为路径中不允许再出现冒号；
        // 出现即视为非法，而不是静默忽略后半段。
        let (namespace, path) = raw.split_once(':').ok_or_else(invalid)?;

        if !is_valid_segment(namespace) || !is_valid_segment(path) {
            return Err(invalid());
        }

        Ok(NamespacedId {
            namespace: namespace.into(),
            path: path.into(),
        })
    }

    /// 命名空间部分，通常是 mod 的唯一名称。
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// 路径部分，标识该命名空间内的具体内容。
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// 判断标识符的一个段落是否合法。
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

impl fmt::Display for NamespacedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

/// 线格式的写出侧，见 [`NamespacedId`] 文档「`serde` 走 `try_from`/`into`」。
impl From<NamespacedId> for String {
    fn from(id: NamespacedId) -> String {
        id.to_string()
    }
}

/// 线格式的读入侧：必经 [`NamespacedId::parse`]，因此反序列化产不出
/// 结构非法的值。
impl TryFrom<String> for NamespacedId {
    type Error = CoreError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        NamespacedId::parse(&raw)
    }
}

/// 内容在运行时的紧凑索引。
///
/// **不可持久化**——索引依赖 mod 加载顺序，存档必须写字符串 ID。
///
/// # 为什么可以直接派生 `Serialize`/`Deserialize`（不需要 `try_from`）
///
/// [0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
/// 把「结构合法」与「已注册」拆成两件事：本类型自身没有任何不依赖
/// 运行期上下文就能判断对不对的不变式——任意 `u32` 都是一个「形状
/// 合法」的裸索引，它是否对应一条真实注册过的内容，只有查当前的
/// `Interner`/`Registry` 才知道。这正是 0011 的 `try_from` 模式**不**
/// 适用的情形（该模式只管无上下文的不变式，见 0015「为什么这是两件
/// 事」一节）。因此这里直接派生，反序列化只做「把整数落地成
/// `ContentIndex`」这一步结构转换；「这个索引当前是否已注册」的校验
/// 由拿到注册表之后的调用方显式完成（例如
/// `ll_world::terrain::TerrainTable::validate_grid`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContentIndex(u32);

impl Default for ContentIndex {
    /// 占位默认值（索引 0）——**不代表任何具体已注册内容**。
    ///
    /// 只用于像 `ll_world::state::WorldState::surface_profile` 这类
    /// 「依赖当前会话注册表上下文，构造时未必已知」的字段需要一个初始
    /// 值占位的场景（与 `terrain_table` 字段同一类已知限制，见
    /// `ll_world::state` 模块文档）。调用方必须在拿到真实注册表之后
    /// 显式替换这个占位值，不能把它当成任何具体内容的索引来使用——
    /// 索引 `0` 在不同会话、不同 mod 组合下可能对应完全不同的内容。
    fn default() -> Self {
        ContentIndex(0)
    }
}

impl ContentIndex {
    /// 取出底层原始索引值，供数组下标使用。
    pub const fn get(&self) -> u32 {
        self.0
    }
}

/// 字符串标识符与运行时索引之间的双向映射池。
///
/// **不变式：内部的哈希表永远不得被遍历。** 索引只能来自 `to_id` 的插入
/// 顺序，而哈希表的遍历顺序不保证跨运行稳定——一旦有任何逻辑依赖它，
/// 确定性存档与跨平台一致性会同时失效。若将来需要枚举全部标识符，
/// 请遍历 `to_id`。
#[derive(Debug, Default)]
pub struct Interner {
    to_index: HashMap<NamespacedId, ContentIndex>,
    to_id: Vec<NamespacedId>,
}

impl Interner {
    /// 建立空池。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个标识符并返回其索引。已登记者返回原索引。
    pub fn intern(&mut self, id: NamespacedId) -> ContentIndex {
        if let Some(existing) = self.to_index.get(&id) {
            return *existing;
        }
        // 四十亿条内容 ID 在现实中不会出现，但静默截断会让两个不同的
        // 标识符映射到同一索引，属于最难排查的一类缺陷，故留一道断言。
        debug_assert!(self.to_id.len() < u32::MAX as usize);
        // 索引即插入顺序下标，故 to_id 与 to_index 恒保持一致。
        let index = ContentIndex(self.to_id.len() as u32);
        self.to_id.push(id.clone());
        self.to_index.insert(id, index);
        index
    }

    /// 由索引反查标识符。存档写出时依赖此方法。
    pub fn resolve(&self, index: ContentIndex) -> Option<&NamespacedId> {
        self.to_id.get(index.get() as usize)
    }

    /// 由标识符查索引，**不登记**——查不到就返回 `None`，不会像
    /// [`Interner::intern`] 那样顺手创建一条新记录。
    ///
    /// 这是 [0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
    /// 「注册校验是解析，不是不变式」分工里，「解析」那一半的具体落点：
    /// mod 内容互相引用时（例如某技能声明里写的职业 ID），要查的是
    /// 「这个字符串现在是否已注册」，而不是「把它也顺便注册进来」——
    /// 两者是完全不同的操作，混在一起会让「引用了不存在的内容」这类
    /// 缺失 mod 场景静默变成「凭空多注册出一条从未有人定义过的内容」。
    /// 查不到时，调用方应把它当成规格 §10.4「缺失 mod」的检测点。
    pub fn get(&self, id: &NamespacedId) -> Option<ContentIndex> {
        self.to_index.get(id).copied()
    }

    /// 按索引顺序（即登记顺序）列出全部标识符。
    ///
    /// 返回的是 `to_id`——一个 `Vec`，天然保证顺序稳定，不是遍历
    /// `to_index` 这个哈希表（模块文档已强调该表不得被遍历）。存档头
    /// 需要写出「索引 ↔ 字符串」映射表时，遍历本方法的返回值即可按
    /// `ContentIndex` 从 0 开始的顺序拿到对应字符串。
    pub fn ids(&self) -> &[NamespacedId] {
        &self.to_id
    }

    /// 已登记的标识符数量。
    pub fn len(&self) -> usize {
        self.to_id.len()
    }

    /// 池中是否尚无任何标识符。
    pub fn is_empty(&self) -> bool {
        self.to_id.is_empty()
    }
}

/// 世界生成实例的持久标识——势力、家族、聚落、宗教团体、历史事件这类
/// 「个体」用它，与 mod 定义「种类」用的 [`ContentIndex`] 分开（判据见
/// `knowledge/design/identity-and-ids.md` 二）。
///
/// **永不复用，不需要代际号**：历史事件要求即便指向的对象已经消亡，
/// 引用依然能正确解析——王朝覆灭多年后，「卡拉克第三王朝与铁血兄弟会
/// 的战争」这条事件记录里的 `WorldId` 仍要能解析回那个已灭亡的王朝，
/// 而不是解析成号码被回收后新分配给别的势力的东西。这与
/// [`crate::error::CoreError`] 一类「拒绝非法输入」的校验无关，是构造
/// 方式本身的责任：本类型不提供任何会导致号码倒退或重复分配的构造
/// 途径，唯一的构造入口 [`WorldId::next`] 只会让传入的计数器单调前进。
///
/// 反直觉之处（免得后人给它「顺手」加代际号）：`ll-world` 的
/// `entity::EntityId`（本 crate 不依赖 `ll-world`，故此处不能用 doc
/// link，只能点名）用代际号防悬垂引用——槽位复用后旧 ID 因世代号不
/// 匹配而查询失败，这是**故意让引用失效**。`WorldId` 的需求方向正好
/// 相反：历史记录**故意**要指向一个已经不存在的东西，且必须永远解析
/// 成功。两者设计目标互斥，不能共用同一套机制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldId(u32);

impl WorldId {
    /// 从计数器分配下一个 `WorldId`：取计数器当前值构造 ID，再让计数器
    /// 前进一步。
    ///
    /// 调用方须让同一个计数器贯穿整个世界生成过程、从不倒退——这是
    /// 「永不复用」的唯一来源，类型本身无法阻止调用方另起一个从零开始
    /// 的计数器或倒拨已有计数器；本方法能保证的只是「同一个计数器只会
    /// 前进，绝不会在耗尽 `u32` 空间时静默回绕到已分配过的号码」。
    ///
    /// `u32::MAX` 本身保留作「已耗尽」哨兵、不作为合法 ID 发放——发放
    /// 区间因此是 `0..u32::MAX`，仍有约 40 亿个号可用（500 年世界只
    /// 用掉两万多个，见模块级别 `Interner` 的同类论证）。这样设计是
    /// 为了让每次调用要么完整成功（返回 ID 且计数器前进）、要么直接
    /// panic，不存在「返回了 ID 但计数器没能前进」这种半成功状态——
    /// 若允许 `counter` 真的加到溢出，成功返回 `u32::MAX` 那次调用会
    /// 让计数器自身无法再前进，下一次调用要么 panic（安全但语义混乱：
    /// 明明还没复用号码却报错）要么被迫做特判，不如直接少留一个号。
    pub fn next(counter: &mut u32) -> Self {
        assert!(
            *counter < u32::MAX,
            "WorldId 计数器已耗尽 u32 空间，不应在合理游戏时长内发生"
        );
        let id = WorldId(*counter);
        *counter += 1;
        id
    }

    /// 取出底层原始值。仅用于日志、调试展示；游戏逻辑不应依赖具体数值。
    pub const fn get(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worldid单调递增不重复() {
        // Arrange
        let mut counter = 0u32;

        // Act
        let first = WorldId::next(&mut counter);
        let second = WorldId::next(&mut counter);
        let third = WorldId::next(&mut counter);

        // Assert
        assert!(first.get() < second.get() && second.get() < third.get());
    }

    #[test]
    fn worldid在计数器接近上限时继续递增而非回绕() {
        // 构造边界情形：计数器逼近 u32::MAX，验证递增逻辑本身在临界值
        // 附近仍然逐一前进，不会提前折返成一个更小（意味着已被分配过）
        // 的号码。正常游戏时长内不会触发这个区间。
        // Arrange
        let mut counter = u32::MAX - 3;

        // Act
        let first = WorldId::next(&mut counter);
        let second = WorldId::next(&mut counter);
        let third = WorldId::next(&mut counter);

        // Assert
        assert_eq!(
            [first.get(), second.get(), third.get()],
            [u32::MAX - 3, u32::MAX - 2, u32::MAX - 1]
        );
    }

    #[test]
    #[should_panic(expected = "耗尽")]
    fn worldid计数器真正耗尽时panic而非静默回绕() {
        // 验证「不回绕」不是靠运气：计数器到达保留哨兵值 u32::MAX 后，
        // 再分配必须 panic，而不是让 *counter 溢出后静默变回 0。
        // Arrange
        let mut counter = u32::MAX;

        // Act
        let _ = WorldId::next(&mut counter);
    }

    #[test]
    fn 解析合法标识符拆出命名空间与路径() {
        // Arrange
        let raw = "lostland:fireball";

        // Act
        let id = NamespacedId::parse(raw).expect("这是合法标识符");

        // Assert
        assert_eq!((id.namespace(), id.path()), ("lostland", "fireball"));
    }

    #[test]
    fn 缺少冒号时解析失败() {
        // Arrange
        let raw = "fireball";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 含大写字母时解析失败() {
        // 强制小写是为了避免 MyMod:fire 与 mymod:fire 这类肉眼难辨的
        // 重复 ID。
        // Arrange
        let raw = "MyMod:fire";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 路径中出现第二个冒号时解析失败() {
        // Arrange
        let raw = "mod:a:b";

        // Act
        let result = NamespacedId::parse(raw);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一标识符重复登记返回相同索引() {
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse("lostland:fireball").expect("合法");

        // Act
        let first = interner.intern(id.clone());
        let second = interner.intern(id);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 索引可反查回原标识符() {
        // 存档必须能把整数索引写回字符串，否则玩家调整 mod 加载顺序后，
        // 存档里的火球会变成一把椅子。
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse("yourmod:super_fire").expect("合法");
        let index = interner.intern(id.clone());

        // Act
        let resolved = interner.resolve(index);

        // Assert
        assert_eq!(resolved, Some(&id));
    }

    #[test]
    fn get查询已登记标识符返回索引且不改变池大小() {
        // get 是「解析」不是「登记」——查询已存在的 ID 不应产生副作用。
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse("lostland:fireball").expect("合法");
        let index = interner.intern(id.clone());

        // Act
        let found = interner.get(&id);

        // Assert
        assert_eq!(found, Some(index));
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn get查询未登记标识符返回none且不登记它() {
        // 这是「缺失 mod」检测点：查不到不能顺手创建一条新记录，否则
        // 「引用了不存在的内容」会静默变成「凭空注册出这条内容」。
        // Arrange
        let interner = Interner::new();
        let unregistered = NamespacedId::parse("yourmod:never_registered").expect("合法");

        // Act
        let found = interner.get(&unregistered);

        // Assert
        assert_eq!(found, None);
        assert!(interner.is_empty());
    }

    #[test]
    fn ids按登记顺序列出全部标识符() {
        // Arrange
        let mut interner = Interner::new();
        let first = NamespacedId::parse("lostland:mountain").expect("合法");
        let second = NamespacedId::parse("lostland:fireball").expect("合法");
        interner.intern(first.clone());
        interner.intern(second.clone());

        // Act
        let ids = interner.ids();

        // Assert
        assert_eq!(ids, &[first, second]);
    }
}
