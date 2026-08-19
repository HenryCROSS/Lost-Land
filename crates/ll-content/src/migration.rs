//! schema 迁移框架：把存档主体的原始字节从旧版本升级到新版本。
//!
//! # 与 mod 版本的分工（`knowledge/design/identity-and-ids.md` 六、④）
//!
//! schema 版本变了 = 我们自己的存档格式变了，本模块的迁移链能修；
//! mod 内容变了 = 别人的内容变了，迁移链修不了——那是任务 7
//! （`LoadError::ModContentMismatch`，本批次未实现）的职责。本模块只
//! 处理前一种失败轴，不涉及、也不应该涉及 mod 兼容性判断。
//!
//! # 本任务只搭机制，不接入真实迁移函数
//!
//! [`MigrationChain`] 本身不知道"当前游戏支持到哪个 schema 版本"这类
//! 全局知识——它只回答"给定一个起始版本号,能不能沿着已注册的迁移
//! 函数一路走下去"。真正把具体的迁移函数（例如"v1 存档缺
//! `player_entity` 字段,升级时补一个 `None`"）注册进链条,是各次
//! schema 变更自己的职责,不属于本任务范围。

use std::fmt;

/// 单步 schema 迁移：把存档主体字节从 `source_version` 升级到
/// `target_version`。
pub trait Migration {
    /// 本迁移函数适用的起始版本。
    fn source_version(&self) -> u32;
    /// 本迁移函数升级后的目标版本。
    fn target_version(&self) -> u32;
    /// 对存档主体的原始字节做版本升级。
    ///
    /// 签名故意保持"原始字节进、原始字节出"而不是先反序列化成某个
    /// 中间表示——`postcard` 的版本兼容策略（任务 9 落地）可能要求
    /// 迁移函数直接操作字节布局（例如新增字段需要在特定偏移插入默认
    /// 值），中间表示会强迫每个迁移函数依赖一个可能早已过期的旧版本
    /// 反序列化类型,徒增维护负担。
    fn migrate(&self, body: Vec<u8>) -> Result<Vec<u8>, MigrationError>;
}

/// 迁移失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    /// 给定的起始版本不在本链条认识的任何一环——既不是任何一步迁移
    /// 的起点，也不是任何一步迁移的终点。这不代表存档一定损坏：更
    /// 常见的原因是链条本身有缺口（某个中间版本的迁移函数没有被
    /// 注册进来），调用方应当把这当成迁移链本身的缺陷来处理，而不是
    /// 存档文件的问题。
    NoPathFrom(u32),
    /// 某一步迁移函数自身执行失败（例如迁移逻辑在处理具体数据时发现
    /// 不满足假设），附带失败时所在的版本号与原因文案。
    StepFailed {
        /// 失败发生时的起始版本号。
        at_version: u32,
        /// 失败原因文案。
        reason: String,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::NoPathFrom(version) => {
                write!(f, "no migration path known from schema version {version}")
            }
            MigrationError::StepFailed { at_version, reason } => {
                write!(f, "migration step at version {at_version} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// 按 `source_version` 串联起来的迁移函数集合。
///
/// **内部用 `Vec` 而非 `HashMap`**（C5：禁止 `HashMap`/`HashSet`
/// 迭代顺序参与逻辑判断）——虽然本类型的查找是按 `source_version` 精确
/// 匹配、理论上适合哈希表，但迁移链条数量级是个位数到几十条，线性
/// 查找的性能代价可忽略，用 `Vec` 换来的是"不需要再论证一遍这里的
/// 遍历顺序是否影响正确性"这份省心。
pub struct MigrationChain {
    migrations: Vec<Box<dyn Migration>>,
}

impl MigrationChain {
    /// 用一组迁移函数建立迁移链。传入顺序不影响查找结果——查找按
    /// `source_version` 精确匹配，不依赖列表顺序。
    pub fn new(migrations: Vec<Box<dyn Migration>>) -> Self {
        Self { migrations }
    }

    /// 从 `from` 版本开始，沿着已注册的迁移函数一路升级，直到找不到
    /// 下一步为止，返回升级后的字节。
    ///
    /// 若 `from` 从未出现在任何一步迁移的起点或终点里，说明链条对这
    /// 个版本一无所知，返回 [`MigrationError::NoPathFrom`]；若 `from`
    /// 本身就是某一步迁移的终点、且没有更进一步的迁移，视为"已经是
    /// 该链条已知的最新版本"，原样返回字节，不是错误。
    pub fn apply(&self, from: u32, body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
        let mut current_version = from;
        let mut current_body = body;
        let mut applied_any = false;

        while let Some(migration) = self.next_step(current_version) {
            current_body = migration.migrate(current_body)?;
            current_version = migration.target_version();
            applied_any = true;
        }

        if applied_any || self.is_known_version(from) {
            Ok(current_body)
        } else {
            Err(MigrationError::NoPathFrom(from))
        }
    }

    /// 找出以 `version` 为起点的下一步迁移（若存在）。
    fn next_step(&self, version: u32) -> Option<&dyn Migration> {
        self.migrations
            .iter()
            .find(|migration| migration.source_version() == version)
            .map(|migration| migration.as_ref())
    }

    /// `version` 是否出现在链条的任何一步（起点或终点）里。
    fn is_known_version(&self, version: u32) -> bool {
        self.migrations.iter().any(|migration| {
            migration.source_version() == version || migration.target_version() == version
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用迁移函数：把 `body` 追加一个标记字节，模拟"这一步真的
    /// 改动了数据"，从而让断言能区分"经过了哪几步"。
    struct AppendByteMigration {
        from: u32,
        to: u32,
        marker: u8,
    }

    impl Migration for AppendByteMigration {
        fn source_version(&self) -> u32 {
            self.from
        }

        fn target_version(&self) -> u32 {
            self.to
        }

        fn migrate(&self, mut body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
            body.push(self.marker);
            Ok(body)
        }
    }

    fn chain_v1_to_v3() -> MigrationChain {
        MigrationChain::new(vec![
            Box::new(AppendByteMigration {
                from: 1,
                to: 2,
                marker: 0xAA,
            }),
            Box::new(AppendByteMigration {
                from: 2,
                to: 3,
                marker: 0xBB,
            }),
        ])
    }

    #[test]
    fn 对相邻版本能找到迁移路径() {
        // Arrange
        let chain = chain_v1_to_v3();

        // Act
        let result = chain
            .apply(2, Vec::new())
            .expect("v2 到 v3 的一步迁移应当成功");

        // Assert
        assert_eq!(result, vec![0xBB]);
    }

    #[test]
    fn 对不存在的迁移路径返回明确错误而不panic() {
        // Arrange：链条只认识 v1/v2/v3，99 完全不在其中。
        let chain = chain_v1_to_v3();

        // Act
        let result = chain.apply(99, Vec::new());

        // Assert
        assert_eq!(result, Err(MigrationError::NoPathFrom(99)));
    }

    #[test]
    fn 跳级迁移能正确串联两步迁移函数() {
        // Arrange：从 v1 出发，中间经过 v2，最终到 v3——两步迁移函数
        // 都必须依次执行,顺序不能颠倒。
        let chain = chain_v1_to_v3();

        // Act
        let result = chain
            .apply(1, Vec::new())
            .expect("v1 到 v3 的两步迁移应当成功");

        // Assert
        assert_eq!(result, vec![0xAA, 0xBB]);
    }

    #[test]
    fn 已经是链条已知最新版本时原样返回不报错() {
        // v3 是链条终点，没有更进一步的迁移——这不是"缺路径"，是
        // "已经不需要迁移"。
        // Arrange
        let chain = chain_v1_to_v3();

        // Act
        let result = chain.apply(3, vec![0x01]).expect("已是最新版本不应报错");

        // Assert
        assert_eq!(result, vec![0x01]);
    }

    #[test]
    fn 迁移步骤自身失败时错误向上传播而不panic() {
        // Arrange
        struct FailingMigration;
        impl Migration for FailingMigration {
            fn source_version(&self) -> u32 {
                1
            }
            fn target_version(&self) -> u32 {
                2
            }
            fn migrate(&self, _body: Vec<u8>) -> Result<Vec<u8>, MigrationError> {
                Err(MigrationError::StepFailed {
                    at_version: 1,
                    reason: "测试用失败".to_string(),
                })
            }
        }
        let chain = MigrationChain::new(vec![Box::new(FailingMigration)]);

        // Act
        let result = chain.apply(1, Vec::new());

        // Assert
        assert_eq!(
            result,
            Err(MigrationError::StepFailed {
                at_version: 1,
                reason: "测试用失败".to_string(),
            })
        );
    }
}
