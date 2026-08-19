//! schema 版本与 mod 版本：两条正交的失败轴，分别报错。
//!
//! `knowledge/design/identity-and-ids.md` 六、④：
//!
//! ```text
//! schema 版本变了  = 我们的格式变了     → 迁移函数链（crate::migration）能修
//! mod 内容变了     = 别人的内容变了     → 迁移链修不了，我们不知道对方改了什么
//! ```
//!
//! 一个存档完全可能 schema 已经是最新版、却因为 mod 内容不兼容打不开。
//! 这两种失败必须分别报错——混在一起报「存档版本不兼容」会让玩家往
//! 错误的方向排查：他会去找存档管理器要不要更新，而不是去检查 mod
//! 列表。批次 A 已经在类型上把两者解耦（[`crate::migration::MigrationError`]
//! 不含任何 mod 相关的变体，[`crate::header::ModHeaderEntry::content_hash`]
//! 是独立字段，两者的变化互不影响对方的判定），本模块把这条解耦落到
//! 一个统一的 [`LoadError`] 上，供任务 9 的读档管线在两个完全独立的
//! 检查点分别产出对应的变体。
//!
//! # 本模块只交付类型与判定逻辑
//!
//! 各变体应该各自映射到不同的用户可见文案（「存档版本过旧，正在
//! 迁移」与「某 mod 内容已变化，无法确认兼容性」传达的是完全不同的
//! 信息），但文案本地化（Fluent `.ftl`）留给 P7 UI 落地时接线——这里
//! 只保证判定逻辑区分得够细，不会在这一步就把两种原因合并。

use std::fmt;

use crate::header::ModHeaderEntry;
use crate::migration::MigrationError;
use ll_mod::registry::Registry;

/// 存档打不开的原因——两条正交轴（schema / mod 内容）外加「文件本身
/// 损坏」，三者互不掩盖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// schema 版本高于当前游戏能处理的最新版本——需要更新游戏本体。
    /// 这条轴上唯一"不是迁移链的错"的失败：不是链条有缺口，是这个
    /// 版本本来就还不存在于当前游戏认识的范围内。
    SchemaTooNew {
        /// 存档记录的 schema 版本。
        save_version: u32,
        /// 当前游戏支持到的最新 schema 版本。
        max_supported: u32,
    },
    /// schema 迁移链找不到路径——不应该发生，除非迁移链本身有缺口
    /// （某个中间版本的迁移函数没有被注册进来）。与 `SchemaTooNew`
    /// 的区别：这个版本号本身不算"太新"（可能比 `max_supported` 还
    /// 旧），只是链条恰好在这一段断掉了，指向的修复动作是"补一个迁移
    /// 函数"而不是"存档版本过旧"。
    SchemaMigrationGap {
        /// 迁移链找不到路径的起始版本。
        from: u32,
    },
    /// mod 内容不兼容：存档头记录的生成期内容哈希与当前会话实际拿到
    /// 的内容哈希不一致——版本号相同也会触发，因为哈希本来就是为了
    /// 覆盖"版本号没变但内容变了"这种情况才存在的（`identity-and-ids.md`
    /// 六、①）。
    ModContentMismatch {
        /// 不兼容的 mod 命名空间。
        namespace: String,
        /// 存档头记录的生成期内容哈希。
        expected_hash: u64,
        /// 当前会话实际查到的内容哈希——`None` 表示当前会话里这个
        /// 命名空间完全没有贡献任何内容（比"哈希对不上"更严重的一种
        /// 缺失：mod 可能整个没装）。
        actual_hash: Option<u64>,
    },
    /// 存档文件本身损坏（截断/篡改），与上面两类都无关——既不是我们
    /// 的格式变了，也不是某个 mod 的内容变了，是这份数据本身就读不
    /// 出一个自洽的结构。
    Corrupted(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::SchemaTooNew {
                save_version,
                max_supported,
            } => write!(
                f,
                "存档 schema 版本 {save_version} 高于当前游戏支持的最新版本 {max_supported}，需要更新游戏本体"
            ),
            LoadError::SchemaMigrationGap { from } => write!(
                f,
                "schema 迁移链找不到从版本 {from} 开始的升级路径（迁移链本身有缺口）"
            ),
            LoadError::ModContentMismatch {
                namespace,
                expected_hash,
                actual_hash,
            } => write!(
                f,
                "mod 「{namespace}」内容已变化，无法确认与生成时的兼容性（期望哈希 {expected_hash:#x}，当前 {actual_hash:?}）"
            ),
            LoadError::Corrupted(reason) => write!(f, "存档文件已损坏：{reason}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<MigrationError> for LoadError {
    /// [`MigrationError::NoPathFrom`] 直接对应 `SchemaMigrationGap`——
    /// 两者是同一件事在两个模块里的表达。[`MigrationError::StepFailed`]
    /// 没有专属的 `LoadError` 变体：一个具体的迁移步骤在处理实际字节
    /// 时失败，最常见的原因是这份数据本身不符合该步骤的假设（截断/
    /// 篡改），归入 `Corrupted` 比发明一个新变体更贴切——它既不是
    /// "版本太新"，也不是"链条有缺口"，链条本身找到了正确的一步，只是
    /// 这一步处理的数据有问题。
    fn from(err: MigrationError) -> Self {
        match err {
            MigrationError::NoPathFrom(from) => LoadError::SchemaMigrationGap { from },
            MigrationError::StepFailed { at_version, reason } => LoadError::Corrupted(format!(
                "schema 迁移在版本 {at_version} 这一步失败：{reason}"
            )),
        }
    }
}

/// 校验存档头记录的 schema 版本是否在当前游戏支持范围内。
///
/// 与 [`check_mod_content`] 是两个完全独立的检查点——调用方应该分别
/// 调用两者，不应该把两者的结果合并成一次判断，否则就重新引入了本
/// 模块要解决的那个问题（见模块文档）。
pub fn check_schema_version(save_version: u32, max_supported: u32) -> Result<(), LoadError> {
    if save_version > max_supported {
        Err(LoadError::SchemaTooNew {
            save_version,
            max_supported,
        })
    } else {
        Ok(())
    }
}

/// 校验存档头记录的生成期 mod 集合与当前会话（`registry`）实际拿到的
/// 内容哈希是否逐条一致。
///
/// 只要有一条不一致就立即返回——第一条不匹配的记录已经足够定位问题，
/// 不需要收集全部不一致再报告（存档打不开这件事本身已经足够阻塞，
/// 逐条报告不会让玩家更容易修复）。
pub fn check_mod_content(
    generation_mods: &[ModHeaderEntry],
    registry: &Registry,
) -> Result<(), LoadError> {
    for entry in generation_mods {
        let actual_hash = registry.content_hash_of(&entry.namespace);
        if actual_hash != Some(entry.content_hash) {
            return Err(LoadError::ModContentMismatch {
                namespace: entry.namespace.clone(),
                expected_hash: entry.content_hash,
                actual_hash,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn mod_entry(namespace: &str, content_hash: u64) -> ModHeaderEntry {
        ModHeaderEntry {
            namespace: namespace.to_string(),
            version: "1.0.0".to_string(),
            content_hash,
        }
    }

    #[test]
    fn schema版本高于当前支持的最新版本时返回schematoonew() {
        // Arrange & Act
        let result = check_schema_version(5, 3);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::SchemaTooNew {
                save_version: 5,
                max_supported: 3,
            })
        );
    }

    #[test]
    fn schema版本不高于当前支持的最新版本时校验通过() {
        // Arrange & Act
        let result = check_schema_version(3, 3);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn mod内容哈希与生成期记录不一致时返回modcontentmismatch即便版本号相同() {
        // 「版本号相同」是本条测试的关键——mod 作者改内容不改版本号是
        // 常态，哈希校验本来就是为了覆盖这种情况才存在的。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:river")); // 内容与生成时不同
        let generation_mods = vec![mod_entry("lostland", 999_999)];

        // Act
        let result = check_mod_content(&generation_mods, &registry);

        // Assert
        assert!(matches!(
            result,
            Err(LoadError::ModContentMismatch { namespace, .. }) if namespace == "lostland"
        ));
    }

    #[test]
    fn mod内容哈希一致时校验通过() {
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let actual_hash = registry.content_hash_of("lostland").expect("已注册过内容");
        let generation_mods = vec![mod_entry("lostland", actual_hash)];

        // Act
        let result = check_mod_content(&generation_mods, &registry);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 当前会话完全没有该命名空间时actual_hash为空() {
        // 比"哈希对不上"更严重的一种缺失：mod 可能整个没装。
        // Arrange
        let registry = Registry::new();
        let generation_mods = vec![mod_entry("missingmod", 123)];

        // Act
        let result = check_mod_content(&generation_mods, &registry);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::ModContentMismatch {
                namespace: "missingmod".to_string(),
                expected_hash: 123,
                actual_hash: None,
            })
        );
    }

    #[test]
    fn schema版本正常但mod内容不兼容时不会被误判为schematoonew() {
        // 本模块存在的核心理由：两个检查点各自独立,一个通过不代表另一
        // 个也通过,也不会把另一个的失败错误分类成自己这一类。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:river"));
        let generation_mods = vec![mod_entry("lostland", 999_999)];

        // Act
        let schema_result = check_schema_version(3, 3);
        let mod_result = check_mod_content(&generation_mods, &registry);

        // Assert：schema 这条轴完全正常。
        assert_eq!(schema_result, Ok(()));
        // mod 内容这条轴报错，且报的是 ModContentMismatch，不是
        // SchemaTooNew——两条轴互不掩盖。
        assert!(matches!(
            mod_result,
            Err(LoadError::ModContentMismatch { .. })
        ));
    }

    #[test]
    fn migrationerror的nopathfrom转换为schemamigrationgap() {
        // Arrange
        let migration_error = MigrationError::NoPathFrom(7);

        // Act
        let load_error: LoadError = migration_error.into();

        // Assert
        assert_eq!(load_error, LoadError::SchemaMigrationGap { from: 7 });
    }

    #[test]
    fn migrationerror的stepfailed转换为corrupted() {
        // Arrange
        let migration_error = MigrationError::StepFailed {
            at_version: 2,
            reason: "测试用失败".to_string(),
        };

        // Act
        let load_error: LoadError = migration_error.into();

        // Assert
        assert!(matches!(load_error, LoadError::Corrupted(_)));
    }
}
