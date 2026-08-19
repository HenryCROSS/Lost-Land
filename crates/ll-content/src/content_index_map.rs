//! `ContentIndex` ↔ 字符串映射表的真实接线：把 `Registry::snapshot()`/
//! `Registry::rebuild_from()` 这两个此前只在内存里往返的接口，接进
//! 存档头 [`crate::header::SaveHeader::content_index_map`] 字段的真实
//! 读写路径。
//!
//! # 本模块只做机械转换，不做降级判断
//!
//! 读档时某个字符串解析失败，正是规格 §10.4「缺失 mod 不得崩溃」的
//! 检测点之一——但**如何降级**（丢弃、占位、拒绝、只读模式）不是本
//! 模块的职责。本模块产出的 [`ContentIndexMapError`] 只负责把"解析
//! 哪一条失败了"这个事实明确地报告出去，交给调用方（任务 6 的降级
//! 策略，本批次未实现）决定后续动作。这与
//! [0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
//! 「注册校验是解析,不是不变式」是同一条分工原则的延伸：`Registry`
//! 本身不知道"缺 mod 该怎么办",这里同样不知道。

use ll_core::ident::NamespacedId;
use ll_mod::registry::Registry;
use std::fmt;

/// 存档时调用：把当前会话的 `Registry` 状态编码进存档头字段。
///
/// 按 `ContentIndex` 从 0 开始的顺序排列（`Registry::snapshot()` 已经
/// 保证这条顺序，见其文档），逐条转换成 `NamespacedId` 的字符串形式
/// （`命名空间:路径`）。
pub fn snapshot_for_header(registry: &Registry) -> Vec<String> {
    registry
        .snapshot()
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// 读档时调用：从存档头字段重建一个 `Registry`。
///
/// 逐条把字符串解析回 `NamespacedId` 再交给 `Registry::rebuild_from`——
/// 「重建后的顺序与快照顺序一一对应」这条不变式已经由
/// `Registry::rebuild_from` 内部保证（它按传入切片的顺序依次
/// `intern`），这里额外做的是防御性的一步：确认切片里**每一条**字符
/// 串本身都能被解析,而不是让某条格式错误的字符串在 `rebuild_from`
/// 内部悄悄被跳过或 panic。
///
/// 解析失败时返回 [`ContentIndexMapError::MalformedId`]，附带出错的
/// 原始字符串与它在列表中的位置——**这不代表"缺 mod"这件事本身已经
/// 发生**（缺 mod 的典型表现是字符串本身合法、但当前会话没有任何 mod
/// 注册出这个 ID，那种情况读到的是一个完全成功的 `Registry`,只是
/// 后续查询查不到而已）；这里报告的是更基础的一层：存档头这段数据
/// 本身格式不对，可能意味着文件损坏,调用方应当把这与"缺 mod"分开
/// 处理。
pub fn rebuild_from_header(entries: &[String]) -> Result<Registry, ContentIndexMapError> {
    let ids = parse_content_index_map(entries)?;
    Ok(Registry::rebuild_from(&ids))
}

/// 只做「字符串 → `NamespacedId`」这一步解析，不重建 `Registry`。
///
/// 存档主体读写管线（任务 9）的重映射步骤需要的不是一个凭空重建的
/// 幻影 `Registry`（那个的索引分配只是「存档写出时的顺序」的镜像，
/// 见 [`rebuild_from_header`] 文档），而是「旧索引 → 字符串」这张查表
/// ——真正的索引换算要查**当前会话**已经装载好的那个 `Registry`
/// （见 `ll_content::remap` 模块文档）。抽出这个函数，让 [`rebuild_from_header`]
/// （防御性校验用途）与重映射（真正的读档路径）共享同一份解析逻辑，
/// 不必各自维护一份等价代码。
pub fn parse_content_index_map(
    entries: &[String],
) -> Result<Vec<NamespacedId>, ContentIndexMapError> {
    entries
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            NamespacedId::parse(raw).map_err(|_| ContentIndexMapError::MalformedId {
                index,
                raw: raw.clone(),
            })
        })
        .collect()
}

/// `content_index_map` 重建过程中可能出现的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentIndexMapError {
    /// 某一条字符串不是合法的 `命名空间:路径` 形式，附带其在列表中的
    /// 下标与原始内容，便于定位是存档头的哪一行坏了。
    MalformedId {
        /// 出错字符串在 `content_index_map` 列表中的下标。
        index: usize,
        /// 无法解析的原始字符串内容。
        raw: String,
    },
}

impl fmt::Display for ContentIndexMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContentIndexMapError::MalformedId { index, raw } => {
                write!(
                    f,
                    "content_index_map[{index}] is not a valid namespaced id: {raw:?}"
                )
            }
        }
    }
}

impl std::error::Error for ContentIndexMapError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn snapshot与rebuild往返后registry的字符串索引映射逐条一致() {
        // Arrange
        let mut original = Registry::new();
        let mountain = original.intern(id("lostland:mountain"));
        let fireball = original.intern(id("yourmod:fireball"));

        // Act
        let header_entries = snapshot_for_header(&original);
        let rebuilt = rebuild_from_header(&header_entries).expect("合法快照必须重建成功");

        // Assert：同一个字符串在重建前后查到的索引完全一致。
        assert_eq!(rebuilt.get(&id("lostland:mountain")), Some(mountain));
        assert_eq!(rebuilt.get(&id("yourmod:fireball")), Some(fireball));
    }

    #[test]
    fn content_index_map含有非法格式字符串时返回错误而非panic() {
        // Arrange：第二条是非法字符串（缺冒号）。
        let entries = vec![
            "lostland:mountain".to_string(),
            "not_a_valid_id".to_string(),
        ];

        // Act
        let result = rebuild_from_header(&entries);

        // Assert：Registry 未实现 PartialEq（注册表整体比较意义不大），
        // 只能取出错误分支单独比较。
        let error = result.expect_err("非法字符串应当导致重建失败");
        assert_eq!(
            error,
            ContentIndexMapError::MalformedId {
                index: 1,
                raw: "not_a_valid_id".to_string(),
            }
        );
    }

    #[test]
    fn 空的content_index_map重建出空registry() {
        // 边界场景：全新存档理论上不会出现空快照（至少有本体内容），
        // 但接口不应该对空输入特殊报错——重建出一个空 Registry 才是
        // 诚实的行为。
        // Arrange
        let entries: Vec<String> = Vec::new();

        // Act
        let rebuilt = rebuild_from_header(&entries).expect("空列表应当重建成功");

        // Assert
        assert_eq!(rebuilt.snapshot(), Vec::new());
    }
}
