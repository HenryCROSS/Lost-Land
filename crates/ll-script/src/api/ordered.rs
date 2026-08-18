//! 有序遍历原语：脚本需要顺序参与逻辑时的确定性迭代设施。
//!
//! ADR 0012 实测：Steel 内置哈希表遍历顺序不稳定——同一进程内，用完全
//! 相同的构造语句造出的两个哈希表，`hash-keys->list` 的结果都不相等。
//! 因此脚本侧绝不能拿到任何原生哈希表的裸遍历原语；凡是需要顺序参与
//! 逻辑（例如"按某个键排序后依次处理"）的场景，必须先在 Rust 侧用本
//! 模块排好序，再把排好序的结果喂给脚本——脚本只消费已经有序的数据，
//! 不接触任何无序容器本身。

/// 按 `key` 稳定排序 `items`，返回排序后的新序列。
///
/// **稳定**是关键：键相同的元素保持输入中的相对顺序不变，这样"排序
/// 结果只由排序键决定"这条断言在键有重复时依然成立——不稳定排序会让
/// 键相同的元素之间的相对顺序变成未定义的实现细节，同样破坏确定性。
///
/// 消费 `items` 而非借用：排序通常是"整理一批数据准备喂给脚本"这条
/// 链路的最后一步，没有必要在这里额外拷贝一份。
pub fn sorted_by_key<T, K: Ord>(mut items: Vec<T>, mut key: impl FnMut(&T) -> K) -> Vec<T> {
    items.sort_by_key(&mut key);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 排序结果与输入顺序无关只由排序键决定() {
        // Arrange：两份内容相同、初始顺序不同的输入。
        let ascending = vec![(1, "a"), (2, "b"), (3, "c")];
        let shuffled = vec![(3, "c"), (1, "a"), (2, "b")];

        // Act
        let sorted_ascending = sorted_by_key(ascending, |&(key, _)| key);
        let sorted_shuffled = sorted_by_key(shuffled, |&(key, _)| key);

        // Assert
        assert_eq!(sorted_ascending, sorted_shuffled);
    }

    #[test]
    fn 排序键相同的元素保持输入中的相对顺序() {
        // Arrange：键全部相同，唯一能区分元素的是它们的原始位置。
        let items = vec![("first", 0), ("second", 0), ("third", 0)];

        // Act
        let sorted = sorted_by_key(items, |&(_, key)| key);

        // Assert
        assert_eq!(sorted, vec![("first", 0), ("second", 0), ("third", 0)]);
    }

    #[test]
    fn 排序后的序列本身按键升序排列() {
        // Arrange
        let items = vec![5, 3, 1, 4, 2];

        // Act
        let sorted = sorted_by_key(items, |&value| value);

        // Assert
        assert_eq!(sorted, vec![1, 2, 3, 4, 5]);
    }
}
