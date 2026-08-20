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
use ll_mod::manifest::ModManifest;
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
    /// 内容哈希算法已升级：存档头记录的算法版本
    /// （[`crate::header::SaveHeader::content_hash_algorithm_version`]）
    /// 低于当前游戏使用的算法版本
    /// （[`ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION`]）——
    /// 与 [`Self::ModContentMismatch`] 是完全不同的原因，必须分开报错
    /// （见 [`check_content_hash_algorithm`] 文档「为什么必须与
    /// `ModContentMismatch` 分开」一节）：这份存档记录的哈希是用一套
    /// **已经不再使用**的编码方式算出来的，与当前算法算出来的哈希
    /// 不可比较——不是 mod 内容真的变了，是量尺本身换了。若把这种
    /// 情况也报成 `ModContentMismatch`，mod 作者会被误导去检查自己
    /// 的 mod 是不是改坏了，而实际上什么都没改坏。
    ContentHashAlgorithmUpgraded {
        /// 存档头记录的算法版本。
        save_algorithm_version: u32,
        /// 当前游戏使用的算法版本。
        current_algorithm_version: u32,
    },
    /// mod 内容不兼容：存档头记录的生成期内容哈希与当前会话实际拿到
    /// 的内容哈希不一致——版本号相同也会触发，因为哈希本来就是为了
    /// 覆盖"版本号没变但内容变了"这种情况才存在的（`identity-and-ids.md`
    /// 六、①）。
    ModContentMismatch {
        /// 不兼容的 mod 命名空间。
        namespace: String,
        /// 存档头记录的生成期内容哈希——`None` 表示生成那一刻这个
        /// 命名空间就没有贡献任何内容（裁定 P5-8，与
        /// `crate::header::ModHeaderEntry::content_hash` 同一个类型，
        /// 转换点不折叠这条区分）。
        expected_hash: Option<u64>,
        /// 当前会话实际查到的内容哈希——`None` 表示当前会话里这个
        /// 命名空间完全没有贡献任何内容（可能是"这个 mod 从来就不贡献
        /// 内容"，也可能是"mod 整个没装"——两者在这个字段上看起来一样，
        /// 但 `expected_hash` 同为 `None` 时说明生成期也是"无贡献"，
        /// 不构成真正的不匹配，见 `check_mod_content` 的比较逻辑）。
        actual_hash: Option<u64>,
    },
    /// 存档文件本身损坏（截断/篡改），与上面两类都无关——既不是我们
    /// 的格式变了，也不是某个 mod 的内容变了，是这份数据本身就读不
    /// 出一个自洽的结构。
    Corrupted(String),
    /// 硬门禁（决策二，项目所有者拍板）：存档记录的某个生成期 mod 在
    /// 当前会话完全不存在，或者存在但版本号与存档记录不一致——见
    /// [`check_mod_set`] 文档。与 [`Self::ModContentMismatch`] 是两种
    /// 不同粒度的失败：后者要求 mod 仍在场、版本号也没变，只是贡献的
    /// 内容哈希变了；本变体覆盖更前一步、更粗粒度的事实（在不在、
    /// 版本号对不对得上），两者互不掩盖，见 [`check_mod_set`] 文档
    /// 「与 `check_mod_content` 的关系」一节。
    ModSetMismatch(ModSetMismatch),
}

/// [`LoadError::ModSetMismatch`] 携带的详情。
///
/// # 为什么没有一句现成的中文/英文句子
///
/// 规格 §11.3：用户可见文本不得硬编码字面量，必须走 Fluent `.ftl`。
/// 本类型因此只携带结构化数据（缺哪个 mod、要什么版本、当前是什么
/// 版本）与一个技术标识符 [`Self::message_key`]——与
/// `ClassDef::display_name_key`/`ModManifest::display_name_key` 等既有
/// 「内容文本走本地化键，不存字面字符串」字段同一条纪律（见
/// `knowledge/design/mod-package-structure.md`「本地化文件」一节）。
/// 真正给玩家看的句子留给 UI 层用 `message_key` 查
/// `locales/<lang>.ftl` 拼出，本类型自己不预先拼好任何一种语言的
/// 文案。下方的 [`std::fmt::Display`] 实现（挂在 [`LoadError`] 上）
/// 只把这些结构化字段原样打印出来，服务开发者/日志（与
/// `LoadError` 其余变体的既有定位一致，见本模块文档），不是玩家会
/// 看到的文案，因此同样不含任何硬编码的自然语言句子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModSetMismatch {
    /// Fluent 消息 id——不是自然语言文案，是查 `.ftl` 用的技术标识符
    /// （取值见 [`SAVE_MOD_MISSING_MESSAGE_KEY`]/
    /// [`SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY`]），UI 层负责拼出真正
    /// 给玩家看的句子。
    pub message_key: &'static str,
    /// 缺失/版本不对的 mod 命名空间。
    pub namespace: String,
    /// 存档记录的生成期版本号——玩家需要装上这个版本才能进入存档。
    pub required_version: String,
    /// 当前会话实际装载到的版本号。`None` 表示这个 mod 在当前会话完全
    /// 不存在（[`SAVE_MOD_MISSING_MESSAGE_KEY`] 分支）；`Some` 表示 mod
    /// 在场但版本号不同（[`SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY`] 分支）。
    pub current_version: Option<String>,
}

/// [`ModSetMismatch::message_key`] 取值之一：这个 mod 在当前会话完全
/// 不存在。
pub const SAVE_MOD_MISSING_MESSAGE_KEY: &str = "save-mod-missing";
/// [`ModSetMismatch::message_key`] 取值之一：这个 mod 在场，但版本号
/// 与存档记录的生成期版本不一致。
pub const SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY: &str = "save-mod-version-mismatch";

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
            LoadError::ContentHashAlgorithmUpgraded {
                save_algorithm_version,
                current_algorithm_version,
            } => write!(
                f,
                "存档记录的内容哈希算法版本 {save_algorithm_version} 早于当前游戏使用的版本 {current_algorithm_version}，这份存档写于内容哈希升级之前，无法用当前算法核对内容是否变化（不是 mod 内容真的变了）"
            ),
            LoadError::ModContentMismatch {
                namespace,
                expected_hash,
                actual_hash,
            } => write!(
                f,
                "mod 「{namespace}」内容已变化，无法确认与生成时的兼容性（期望哈希 {expected_hash:?}，当前 {actual_hash:?}）"
            ),
            LoadError::Corrupted(reason) => write!(f, "存档文件已损坏：{reason}"),
            // 不拼一句现成的中文/英文句子——见 ModSetMismatch 文档
            // 「为什么没有一句现成的中文/英文句子」一节：这里只把结构化
            // 字段原样打印，真正给玩家看的文案留给 UI 层查 message_key。
            LoadError::ModSetMismatch(detail) => write!(f, "{detail:?}"),
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

/// 校验存档头记录的内容哈希算法版本是否与当前游戏使用的版本一致。
///
/// # 为什么必须与 `check_mod_content` 分开、且排在它之前
///
/// [`check_mod_content`] 靠比较 `content_hash` 数值判断"内容是否真的
/// 变了"——但这个比较隐含一个前提：两边的哈希是用**同一套编码方式**
/// 算出来的。内容哈希本身经历过一次不兼容升级
/// （[`ll_mod::content_hash`] 模块文档「为什么不能在 `intern` 内部做」
/// 一节：从"只追踪 id 集合"升级成"追踪字段值"），升级前写出的存档记录
/// 的哈希与升级后当前会话算出来的哈希，即便对应的 mod 内容真的一个
/// 字节都没变，数值也几乎必然不同——旧算法根本没把字段值编码进去，
/// 新算法编码了，两者不是同一个函数的两次求值,不能直接比较。
///
/// 若不先做这条检查，[`check_mod_content`] 会把"存档写于算法升级之前"
/// 误判成"mod 内容真的变了"（`LoadError::ModContentMismatch`）——这对
/// mod 作者是一条彻头彻尾的假警报：诊断信息与"真的改坏了内容"完全
/// 一样，排查者会被引导去检查自己的 mod，而实际上什么都没改坏。本函数
/// 因此必须排在 [`check_mod_content`] 之前调用,一旦命中就提前用
/// [`LoadError::ContentHashAlgorithmUpgraded`] 报出来,不让流程走到
/// `check_mod_content` 那个会产出误导性错误的分支。
///
/// # 为什么不排在 `check_mod_set` 之前
///
/// [`check_mod_set`] 只比较"mod 在不在、版本号字符串是否相等"，完全不
/// 涉及内容哈希，算法升级对它没有任何影响——一份存档记录的 mod 若真的
/// 缺失或版本号不对，即便算法版本也顺带过期了，`check_mod_set` 报出的
/// 「mod 缺失/版本不对」仍然是更准确、更该优先展示的原因（决策二的
/// 硬门禁本就优先于内容哈希这一档，见 [`check_mod_set`] 文档）。两者
/// 调用顺序因此维持 `check_mod_set` 在先不变，本函数只需要排在
/// [`check_mod_content`] 之前。
pub fn check_content_hash_algorithm(
    save_algorithm_version: u32,
    current_algorithm_version: u32,
) -> Result<(), LoadError> {
    // 用不等而非"小于"：两边只要不是同一个版本号，就不是同一个哈希
    // 函数的两次求值,不可比较——不局限于"存档更旧"这一个方向（哪怕
    // 现实中反方向几乎不会发生：游戏本体不会把算法版本号往回调）,
    // 判据本身不应该隐含一个未经声明的方向性假设。
    if save_algorithm_version != current_algorithm_version {
        Err(LoadError::ContentHashAlgorithmUpgraded {
            save_algorithm_version,
            current_algorithm_version,
        })
    } else {
        Ok(())
    }
}

/// 校验存档头记录的生成期 mod 集合与当前会话（`registry`/
/// `current_manifests`）是否兼容。
///
/// # 「内容变了」与「mod 不在了」是两件不同的事（P5-A 任务 14 断链二
/// 修复）
///
/// 早先版本只看 [`Registry::content_hash_of`] 是否与生成期记录的哈希
/// 相等——但 `content_hash_of` 对「这个命名空间在场但从未贡献内容」与
/// 「这个命名空间在当前会话压根不存在」返回的都是 `None`，两者曾经在
/// 这里被当成同一种「不兼容」直接硬拒绝（[`LoadError::ModContentMismatch`]，
/// `LoadOutcome::Rejected`）。这个判断对第二种情形是错的：玩家完整
/// 卸载一个 mod 是规格 §10.4 明确要求「不得崩溃、按内容类型优雅降级」
/// 的最直观场景，不应该在细粒度降级（[`crate::remap::remap_world`]）
/// 有机会运行之前就被这里拦下——那会让「装了个 mod、玩了二十小时、
/// 卸载它」这个操作直接打不开存档，而不是丢弃缺失内容后仍可只读/游玩。
///
/// 修复：借助 `current_manifests`（当前会话实际装载的 mod 清单，来自
/// `ll_mod::pipeline::load_all` 或调用方自行拼装的本体清单）分清两种
/// 情形——
/// - **mod 仍在场**（`current_manifests` 里能找到同名命名空间）：哈希
///   不一致就是真的「内容变了」，可能真的打不开，在这里硬拒绝。
/// - **mod 完全不在场**（`current_manifests` 里找不到）：不是「内容
///   变了」，是「mod 不在了」，这里放行，把判断交给
///   [`crate::remap::remap_world`] 按内容类型（物品丢弃、NPC 占位、
///   玩家角色拒绝降级、结构性内容报损坏）逐条决定——那才是规格要求的
///   「优雅降级」，不是本函数该抢先做的事。
///
/// 只要有一条判定为「仍在场但内容变了」就立即返回——第一条不匹配的
/// 记录已经足够定位问题，不需要收集全部不一致再报告。
///
/// 与 [`check_schema_version`] 是两个完全独立的检查点，见模块文档。
pub fn check_mod_content(
    generation_mods: &[ModHeaderEntry],
    registry: &Registry,
    current_manifests: &[ModManifest],
) -> Result<(), LoadError> {
    for entry in generation_mods {
        let mod_present = current_manifests
            .iter()
            .any(|manifest| manifest.id.namespace() == entry.namespace);
        if !mod_present {
            // mod 不在了，不是内容变了——留给 remap_world 逐条降级，见
            // 本函数文档。
            continue;
        }

        let actual_hash = registry.content_hash_of(&entry.namespace);
        // 两边都是 Option<u64>（裁定 P5-8），直接比较——`None == None`
        // （生成期与当前都「在场但无贡献」）视为匹配，不构成不兼容；
        // 一边 Some 一边 None，或两边 Some 但数值不同，才是真正的
        // 「内容变了」。
        if actual_hash != entry.content_hash {
            return Err(LoadError::ModContentMismatch {
                namespace: entry.namespace.clone(),
                expected_hash: entry.content_hash,
                actual_hash,
            });
        }
    }
    Ok(())
}

/// 硬门禁（决策二，项目所有者拍板：「存档的 mod 如果不存在或者版本对
/// 不上就不能进入这个存档」）：存档头记录的生成期 mod 集合，逐条核对
/// 是否仍在当前会话装载的 mod 里、版本号是否仍然一致。
///
/// # 这是一条新的、更粗粒度、优先于细粒度降级的检查
///
/// [`check_mod_content`] 检查的是「mod 仍在场、版本号也没变、只是贡献
/// 的内容哈希变了」这一档；对「mod 完全不在了」这一档，它此前故意
/// 放行（P5-A 任务 14 断链二修复，见其文档），把决定权交给
/// [`crate::remap::remap_world`] 按内容类型细粒度降级（NPC 种族占位、
/// 玩家角色只读……）。
///
/// 项目所有者的决策二推翻了这条既有设计对「完全不在」这一档、以及
/// 「版本号不一致」这一档的处理：两者一律拒绝载入存档，不进入细粒度
/// 降级。调用方（[`crate::save_file::load_full_from_bytes`]）因此必须
/// 把本函数排在 [`check_mod_content`]（乃至 `remap_world`）之前调用—
/// —一旦命中，直接产出 [`LoadError::ModSetMismatch`]，
/// [`crate::degrade`] 模块那一整套按内容类型分级降级的策略不再有机会
/// 介入「mod 完全缺失」「mod 版本不对」这两个场景（该模块其余场景——
/// 单个物品/目标/归属缺失、内容哈希不一致但 mod 本身仍在场且版本号
/// 对得上——不受影响）。
///
/// # 版本号比较：原样字符串相等，不做语义化版本解析
///
/// 与 [`crate::header::ModHeaderEntry::version`] 字段文档同一条既有
/// 纪律——版本号比较本不是这一层该做的事，内容哈希才是判定「是否真的
/// 变了」的依据。但决策二明确把「版本号对不对得上」本身列为一个独立
/// 的硬门禁判据，这里因此只做最朴素的字符串相等比较，不引入语义化
/// 版本（`1.2.0` 与 `1.2` 是否兼容之类的判断）——那是一个更大的独立
/// 设计问题，不在决策二的范围内。
///
/// # 与 [`check_mod_content`] 的关系
///
/// 两者都读 `generation_mods`，判据完全独立：本函数只看「在不在、版本
/// 号是不是原样相等」，`check_mod_content` 只看「内容哈希是否一致」
/// （版本号相同但内容变了的场景，`identity-and-ids.md` 六、①点名的
/// 真实存在的作者疏忽）。调用方依次调用两者，任一个失败都提前拒绝，
/// 不需要合并成一次判断——同一套「两条正交轴分别报错」的既有纪律
/// （见本模块文档），只是这里多出的是第三条轴。
///
/// 只要有一条判定为「不在场」或「版本不一致」就立即返回——第一条不
/// 匹配的记录已经足够定位问题，不需要收集全部不一致再报告。
pub fn check_mod_set(
    generation_mods: &[ModHeaderEntry],
    current_manifests: &[ModManifest],
) -> Result<(), LoadError> {
    for entry in generation_mods {
        let current = current_manifests
            .iter()
            .find(|manifest| manifest.id.namespace() == entry.namespace);
        match current {
            None => {
                return Err(LoadError::ModSetMismatch(ModSetMismatch {
                    message_key: SAVE_MOD_MISSING_MESSAGE_KEY,
                    namespace: entry.namespace.clone(),
                    required_version: entry.version.clone(),
                    current_version: None,
                }));
            }
            Some(manifest) if manifest.version != entry.version => {
                return Err(LoadError::ModSetMismatch(ModSetMismatch {
                    message_key: SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY,
                    namespace: entry.namespace.clone(),
                    required_version: entry.version.clone(),
                    current_version: Some(manifest.version.clone()),
                }));
            }
            Some(_) => {}
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
            content_hash: Some(content_hash),
        }
    }

    /// 「在场但从未贡献任何内容」的条目——`content_hash` 为 `None`，
    /// 见裁定 P5-8。
    fn empty_mod_entry(namespace: &str) -> ModHeaderEntry {
        ModHeaderEntry {
            namespace: namespace.to_string(),
            version: "1.0.0".to_string(),
            content_hash: None,
        }
    }

    /// 「当前会话仍装载着这个命名空间」的清单条目——供需要区分「mod
    /// 仍在场」与「mod 完全不在场」的测试使用。
    fn manifest(namespace: &str) -> ModManifest {
        ModManifest {
            id: NamespacedId::parse(&format!("{namespace}:self")).expect("测试用命名空间恒合法"),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
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
    fn 算法版本一致时校验通过() {
        // Arrange & Act
        let result = check_content_hash_algorithm(1, 1);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 存档记录的算法版本早于当前版本时返回contenthashalgorithmupgraded() {
        // 核心场景：存档写于值哈希升级之前（旧算法版本 0），当前游戏
        // 已经用上新算法（版本 1）——必须报专门的
        // ContentHashAlgorithmUpgraded，不能被后续的 check_mod_content
        // 误判成 ModContentMismatch。
        // Arrange & Act
        let result = check_content_hash_algorithm(0, 1);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::ContentHashAlgorithmUpgraded {
                save_algorithm_version: 0,
                current_algorithm_version: 1,
            })
        );
    }

    #[test]
    fn 存档记录的算法版本晚于当前版本时同样返回contenthashalgorithmupgraded() {
        // 判据不预设方向性——只要两边不是同一个版本号，就不可比较。
        // Arrange & Act
        let result = check_content_hash_algorithm(2, 1);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::ContentHashAlgorithmUpgraded {
                save_algorithm_version: 2,
                current_algorithm_version: 1,
            })
        );
    }

    #[test]
    fn mod内容哈希与生成期记录不一致时返回modcontentmismatch即便版本号相同() {
        // 「版本号相同」是本条测试的关键——mod 作者改内容不改版本号是
        // 常态，哈希校验本来就是为了覆盖这种情况才存在的。
        // Arrange：mod 仍在场（manifests 里能找到它），内容却变了。
        let mut registry = Registry::new();
        registry.intern(id("lostland:river")); // 内容与生成时不同
        let generation_mods = vec![mod_entry("lostland", 999_999)];
        let manifests = vec![manifest("lostland")];

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

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
        let manifests = vec![manifest("lostland")];

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 生成期与当前都无贡献时视为匹配不报mismatch() {
        // 裁定 P5-8 配套：一个「在场但从未贡献任何内容」的 mod，生成期
        // 与当前会话都是 None，不构成"内容变了"——两边同为 None 时是
        // 匹配，不是需要报出的不兼容。
        // Arrange：命名空间本身要"在场"（manifests 里出现过），但不
        // intern 任何内容，content_hash_of 因此返回 None（与
        // ModSetEntry::content_hash 文档「从未贡献内容」同一种状态）。
        let registry = Registry::new();
        let generation_mods = vec![empty_mod_entry("emptymod")];
        let manifests = vec![manifest("emptymod")];

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 生成期无贡献但当前会话有贡献时报mismatch() {
        // 与上一条相反：生成期是 None，当前会话变成 Some——同样是一种
        // "内容变了"，不能因为其中一边是 None 就特殊放行。mod 本身仍在
        // 场（manifests 里能找到），所以这条差异必须被判定为不兼容,
        // 不能被「mod 不在了」那条放行分支捎带过去。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("emptymod:newly_added"));
        let generation_mods = vec![empty_mod_entry("emptymod")];
        let manifests = vec![manifest("emptymod")];

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

        // Assert
        assert!(matches!(
            result,
            Err(LoadError::ModContentMismatch {
                expected_hash: None,
                actual_hash: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn mod仍在场但当前会话内容彻底清空时依然判定不兼容() {
        // 「mod 在场，内容却从有变没有」是 mod 内容真的变了的另一种
        // 表现（例如作者删掉了整个命名空间下的内容却没卸载 mod 本身），
        // 不应该被「mod 不在了」那条放行分支误伤——两者的区分依据是
        // manifests 里是否还有这个命名空间，不是 content_hash 是否为
        // None。
        // Arrange
        let registry = Registry::new(); // 当前会话该命名空间无任何贡献
        let generation_mods = vec![mod_entry("lostland", 123)];
        let manifests = vec![manifest("lostland")]; // 但 mod 本身仍在场

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

        // Assert
        assert!(matches!(
            result,
            Err(LoadError::ModContentMismatch {
                expected_hash: Some(123),
                actual_hash: None,
                ..
            })
        ));
    }

    #[test]
    fn mod完全不在当前manifests中出现时不判定为不兼容留给remap_world处理() {
        // 断链二的核心修复：完整卸载一个 mod（manifests 里连它的命名
        // 空间都找不到）不是「内容变了」，是「mod 不在了」——这里不再
        // 硬拒绝，把判断交给 remap_world 按内容类型逐条降级。此前版本
        // 这里会返回 ModContentMismatch,把「玩家卸载一整个 mod」这个
        // 最直观的场景硬生生挡在细粒度降级之前。
        // Arrange
        let registry = Registry::new();
        let generation_mods = vec![mod_entry("missingmod", 123)];
        let manifests: Vec<ModManifest> = Vec::new(); // missingmod 完全不在场

        // Act
        let result = check_mod_content(&generation_mods, &registry, &manifests);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn schema版本正常但mod内容不兼容时不会被误判为schematoonew() {
        // 本模块存在的核心理由：两个检查点各自独立,一个通过不代表另一
        // 个也通过,也不会把另一个的失败错误分类成自己这一类。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:river"));
        let generation_mods = vec![mod_entry("lostland", 999_999)];
        let manifests = vec![manifest("lostland")];

        // Act
        let schema_result = check_schema_version(3, 3);
        let mod_result = check_mod_content(&generation_mods, &registry, &manifests);

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

    #[test]
    fn 生成期mod在当前会话完全找不到时判定为modsetmismatch() {
        // 决策二核心场景之一：mod 完全不在了（current_manifests 里连
        // 命名空间都找不到）——不再像 check_mod_content 那样放行给
        // remap_world 降级，这里直接硬拒绝。
        // Arrange
        let generation_mods = vec![mod_entry("vanishedmod", 123)];
        let current_manifests: Vec<ModManifest> = Vec::new();

        // Act
        let result = check_mod_set(&generation_mods, &current_manifests);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::ModSetMismatch(ModSetMismatch {
                message_key: SAVE_MOD_MISSING_MESSAGE_KEY,
                namespace: "vanishedmod".to_string(),
                required_version: "1.0.0".to_string(),
                current_version: None,
            }))
        );
    }

    #[test]
    fn 生成期mod在场但版本号不一致时判定为modsetmismatch() {
        // 决策二核心场景之二：mod 仍在场,但版本号跟存档记录的不一样。
        // Arrange
        let generation_mods = vec![mod_entry("lostland", 123)]; // version "1.0.0"
        let current_manifests = vec![ModManifest {
            id: NamespacedId::parse("lostland:self").expect("测试用命名空间恒合法"),
            version: "2.0.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
        }];

        // Act
        let result = check_mod_set(&generation_mods, &current_manifests);

        // Assert
        assert_eq!(
            result,
            Err(LoadError::ModSetMismatch(ModSetMismatch {
                message_key: SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY,
                namespace: "lostland".to_string(),
                required_version: "1.0.0".to_string(),
                current_version: Some("2.0.0".to_string()),
            }))
        );
    }

    #[test]
    fn 生成期mod在场且版本号一致时校验通过() {
        // Arrange
        let generation_mods = vec![mod_entry("lostland", 123)]; // version "1.0.0"
        let current_manifests = vec![manifest("lostland")]; // version "1.0.0"

        // Act
        let result = check_mod_set(&generation_mods, &current_manifests);

        // Assert
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn 生成期mod集合为空时校验恒通过() {
        // Arrange & Act
        let result = check_mod_set(&[], &[]);

        // Assert
        assert_eq!(result, Ok(()));
    }
}
