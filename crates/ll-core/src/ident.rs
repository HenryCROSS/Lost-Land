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

/// 内容在运行时的紧凑索引。
///
/// **不可持久化**——索引依赖 mod 加载顺序，存档必须写字符串 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentIndex(u32);

impl ContentIndex {
    /// 取出底层原始索引值，供数组下标使用。
    pub const fn get(&self) -> u32 {
        self.0
    }
}

/// 字符串标识符与运行时索引之间的双向映射池。
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

    /// 已登记的标识符数量。
    pub fn len(&self) -> usize {
        self.to_id.len()
    }

    /// 池中是否尚无任何标识符。
    pub fn is_empty(&self) -> bool {
        self.to_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
