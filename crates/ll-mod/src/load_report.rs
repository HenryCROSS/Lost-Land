//! 加载管理界面（任务 11）依赖的数据形状：把加载管线每个阶段的结果
//! 归纳成一份可展示的报告。
//!
//! 本模块只定义**数据**，不做任何渲染——渲染属于 `ll-ui`（依赖方向
//! `ll-mod` ← `ll-ui`，见工作区顶层裁定 P4-1）。放在 `ll-mod` 而不是
//! `ll-ui` 的理由：这是加载管线（[`crate::pipeline`]）的直接产物，
//! `ll-mod` 已经是「发现→解析→排序→加载脚本→注册」这条管线的归属
//! crate，报告数据的生产者与消费者不该反过来。
//!
//! # 规格 §10.6 的六个阶段
//!
//! 「加载按阶段推进（发现→解析清单→依赖拓扑排序→加载脚本→注册内容→
//! 交叉引用校验）」——[`LoadStage`] 逐一对应。本项目当前唯一注册的
//! 内容类型是地形（Task 8），交叉引用校验的真实体现是
//! `ll_world::terrain::TerrainTable::validate_grid`，它天然是「整张
//! 地图」级别的检查，不落在某一个具体 mod 头上，因此不在
//! [`LoadReport::entries`] 里按 mod 归类，而是单独放在
//! [`LoadReport::cross_validate`]——见该字段文档。

use std::path::PathBuf;

use ll_core::ident::NamespacedId;

/// 加载管线的六个阶段（规格 §10.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStage {
    /// 在 mod 根目录下列出候选子目录（[`crate::discover::discover_mods`]）。
    Discover,
    /// 解析单个候选的 `mod.toml`（[`crate::manifest::parse_manifest`]）。
    Parse,
    /// 依赖拓扑排序，含重复命名空间/缺失依赖/成环三类失败
    /// （[`crate::topo::topo_sort`]）。
    Topo,
    /// 求值 mod 的 `.scm` 脚本入口（`ll_script::host::ScriptEngine::load_source`）。
    LoadScript,
    /// 脚本内调用注册函数（如 `register-terrain`）把内容写进
    /// [`crate::registry::Registry`]/内容表。
    Register,
    /// 交叉引用校验：已加载内容之间的引用是否都能解析。见模块文档
    /// 「规格 §10.6 的六个阶段」一节——本阶段当前只有整张地图级别的
    /// 落点，不出现在按 mod 归类的 [`LoadReport::entries`] 里。
    CrossValidate,
}

/// 错误发生的源码位置，尽力而为。
///
/// `line` 是 `Option`：并非所有阶段的失败都定位得到具体行——发现阶段
/// 失败（目录读不到）、清单 IO 错误只知道文件、脚本超时没有一个能
/// 归咎的具体位置（见 `ll_script::host::ScriptError` 文档）。宁可让
/// `line` 诚实地留空，也不要编造一个假的行号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// 出问题的文件路径。
    pub file: PathBuf,
    /// 行号（从 1 开始），能定位到具体行时才有值。
    pub line: Option<u32>,
}

/// 单个 mod 的加载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadStatus {
    /// 全部阶段成功。
    Loaded,
    /// 加载成功但有值得作者注意的问题（当前管线暂无产出这类状态的
    /// 路径，字段先留出形状——例如未来「声明的依赖版本号格式不规范但
    /// 不影响加载」这类非致命问题）。
    Warning(String),
    /// 某个阶段失败，mod 未能完整加载。
    Failed(LoadError),
}

/// 加载失败的具体信息：哪个 mod、哪个阶段、为什么，以及尽力而为的
/// 源码位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// 出问题的 mod（[`crate::manifest::mod_self_id`] 约定的自我标识）。
    ///
    /// 有些失败（发现阶段目录整体读不到、清单命名空间字段本身非法）
    /// 拿不到一个真正合法的 mod 身份——这类情况按
    /// [`crate::manifest::mod_self_id`] 同样的降级策略，退化成一个
    /// 由目录名（或固定占位符）拼出的 id，而不是让整条错误无处安放。
    pub mod_id: NamespacedId,
    /// 失败发生在哪个阶段。
    pub stage: LoadStage,
    /// 面向 mod 作者的错误消息。
    pub message: String,
    /// 源码位置，尽力而为（见 [`SourceLocation`] 文档）。
    pub location: Option<SourceLocation>,
}

/// 一次完整加载会话的报告。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadReport {
    /// 按 mod 归类的结果，保持加载管线处理它们的顺序（拓扑序，或者
    /// 因为更早的阶段失败而根本没排进拓扑序的，保持发现/解析顺序）——
    /// **不是**任何 `HashMap`/`HashSet` 产出的顺序，因此这里天然满足
    /// 规格 C4「禁止无序容器迭代顺序参与逻辑判断」，展示层直接按这个
    /// 顺序渲染即可，不需要额外排序。
    pub entries: Vec<(NamespacedId, LoadStatus)>,
    /// 整张地图级别的交叉引用校验结果，`None` 表示本次加载会话没有
    /// 执行这一步（例如还没有可供校验的世界地图）。`Ok(())` 表示地图
    /// 上出现的每一个内容索引当前都在注册表里登记过；`Err` 携带面向
    /// 作者/玩家的说明。
    pub cross_validate: Option<Result<(), String>>,
}

impl LoadReport {
    /// 建立一份空报告。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条按 mod 归类的结果。
    pub fn push(&mut self, mod_id: NamespacedId, status: LoadStatus) {
        self.entries.push((mod_id, status));
    }

    /// 把某个已存在条目的状态原地替换——供
    /// [`crate::pipeline::reload_mod`] 刷新单个 mod 的结果时使用，
    /// 找不到匹配的 `mod_id` 时退化为追加一条新条目（防御性处理，正常
    /// 调用路径下不会走到这个分支：重载前该 mod 必然已经在报告里）。
    pub fn replace(&mut self, mod_id: &NamespacedId, status: LoadStatus) {
        match self.entries.iter_mut().find(|(id, _)| id == mod_id) {
            Some((_, existing)) => *existing = status,
            None => self.entries.push((mod_id.clone(), status)),
        }
    }

    /// 按状态种类过滤出条目的迭代器，供渲染层分组展示（已加载/有警告/
    /// 失败），也供测试断言「某个 mod 归入了哪一组」。
    pub fn entries_with<'a>(
        &'a self,
        matches: impl Fn(&LoadStatus) -> bool + 'a,
    ) -> impl Iterator<Item = &'a (NamespacedId, LoadStatus)> + 'a {
        self.entries
            .iter()
            .filter(move |(_, status)| matches(status))
    }

    /// 已加载成功的 mod 数量。
    pub fn loaded_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, status)| matches!(status, LoadStatus::Loaded))
            .count()
    }

    /// 加载失败的 mod 数量。
    pub fn failed_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, status)| matches!(status, LoadStatus::Failed(_)))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn sample_error(namespace: &str, stage: LoadStage) -> LoadError {
        LoadError {
            mod_id: id(&format!("{namespace}:self")),
            stage,
            message: "示例错误".to_string(),
            location: None,
        }
    }

    #[test]
    fn 失败的mod归入failed分组而不影响其他mod的加载结果() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(id("good:self"), LoadStatus::Loaded);
        report.push(
            id("bad:self"),
            LoadStatus::Failed(sample_error("bad", LoadStage::LoadScript)),
        );

        // Act
        let failed: Vec<_> = report
            .entries_with(|status| matches!(status, LoadStatus::Failed(_)))
            .collect();
        let loaded: Vec<_> = report
            .entries_with(|status| matches!(status, LoadStatus::Loaded))
            .collect();

        // Assert：一个失败不会把另一个也带进失败分组，各自归类准确。
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, id("bad:self"));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, id("good:self"));
    }

    #[test]
    fn loaderror的stage字段能区分发现解析注册三个阶段() {
        // Arrange
        let discover_err = sample_error("a", LoadStage::Discover);
        let parse_err = sample_error("b", LoadStage::Parse);
        let register_err = sample_error("c", LoadStage::Register);

        // Act & Assert：三者互不相同，类型层面能区分。
        assert_ne!(discover_err.stage, parse_err.stage);
        assert_ne!(parse_err.stage, register_err.stage);
        assert_ne!(discover_err.stage, register_err.stage);
    }

    #[test]
    fn 一键重载后该mod的状态被刷新不影响其余mod状态() {
        // Arrange：mod "flaky" 首次加载失败，其余 mod 正常。
        let mut report = LoadReport::new();
        report.push(id("stable:self"), LoadStatus::Loaded);
        report.push(
            id("flaky:self"),
            LoadStatus::Failed(sample_error("flaky", LoadStage::LoadScript)),
        );

        // Act：作者修好了脚本，重新加载 flaky，替换它的状态。
        report.replace(&id("flaky:self"), LoadStatus::Loaded);

        // Assert：flaky 变成 Loaded，stable 完全没被触碰。
        assert_eq!(report.entries.len(), 2);
        let flaky_status = &report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "flaky")
            .unwrap()
            .1;
        assert_eq!(*flaky_status, LoadStatus::Loaded);
        let stable_status = &report
            .entries
            .iter()
            .find(|(id, _)| id.namespace() == "stable")
            .unwrap()
            .1;
        assert_eq!(*stable_status, LoadStatus::Loaded);
    }

    #[test]
    fn loaded_count与failed_count分别统计对应状态数量() {
        // Arrange
        let mut report = LoadReport::new();
        report.push(id("a:self"), LoadStatus::Loaded);
        report.push(id("b:self"), LoadStatus::Loaded);
        report.push(
            id("c:self"),
            LoadStatus::Failed(sample_error("c", LoadStage::Parse)),
        );

        // Act & Assert
        assert_eq!(report.loaded_count(), 2);
        assert_eq!(report.failed_count(), 1);
    }

    #[test]
    fn replace找不到匹配id时退化为追加新条目() {
        // Arrange：报告里原本没有 "ghost" 这个条目——防御性分支，正常
        // 调用路径不会走到，但不该 panic 或丢数据。
        let mut report = LoadReport::new();

        // Act
        report.replace(&id("ghost:self"), LoadStatus::Loaded);

        // Assert
        assert_eq!(report.entries, vec![(id("ghost:self"), LoadStatus::Loaded)]);
    }

    #[test]
    fn 新建报告的cross_validate默认未执行() {
        // Arrange & Act
        let report = LoadReport::new();

        // Assert
        assert_eq!(report.cross_validate, None);
    }
}
