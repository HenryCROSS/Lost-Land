//! 存档头：明文 JSON，独立于存档主体可读。
//!
//! # 为什么头部不能引用 `ContentIndex`
//!
//! 规格 §11.2 要求「存档列表界面只读头部」——玩家浏览存档列表、决定
//! 要不要打开某一份存档时，读取的只是这个类型，不会触发主体解压
//! （那是任务 9 `load_from_header_only` 的职责）。而玩家最需要头部
//! 告诉自己出了什么事的时刻，恰恰是**缺 mod、主体解析不出来**的时刻
//! ——这时候如果头部本身的类型也依赖 `ContentIndex`（`ll_core::ident`
//! 模块文档：「不可持久化——索引依赖 mod 加载顺序」），头部自己都读不
//! 出来，玩家连"缺了什么 mod"这句话都看不到。
//!
//! 因此本模块在类型层面强制这条约束：**本文件不 `use`
//! `ll_core::ident` 的任何类型**，全部字段只用 `String`/整数/枚举这类
//! 不需要运行期注册表上下文就能解释的原始类型。`content_index_map`
//! 字段是 `Vec<String>` 而不是 `Vec<ContentIndex>`——它是索引→字符串
//! 反查表的**结果**（写出时已经把索引换回了字符串，见任务 2
//! `content_index_map` 模块），本身不含任何索引。
//!
//! 这不是一条只写在注释里、容易被后人绕过的约定：`SaveHeader` 与
//! `ModHeaderEntry` 的每个字段本身就是 `String`/`u32`/`u64`/`i64`，
//! 想要塞进一个 `ContentIndex` 就必须先改字段类型——那会在 code
//! review 与本文件顶部这段说明处被直接看见,而不是靠“希望没人写错”。
//! 模块末尾的测试额外钉住这条约束的一个可观测后果：`content_index_map`
//! 序列化后的 JSON 元素是字符串,不是数字——`ContentIndex` 派生的
//! `Serialize` 会把自己序列化成裸整数,若这里误把字段类型换成
//! `Vec<ContentIndex>`,即便编译能通过,这条测试也会因为 JSON 形状从
//! 字符串数组变成数字数组而失败。

use serde::{Deserialize, Serialize};

/// 存档头：schema 版本、存档时间、角色名、当前区域、游玩时长、
/// 启用 mod 列表（分生成期/当前两组）、`ContentIndex` ↔ 字符串映射表、
/// 世界身份要素之一（尺寸）。
///
/// 世界身份的第三要素——种子——与生成期 mod 集合的真正封存时机一并
/// 由任务 4（`ll_content::world_identity`，本批次未实现）落地；本类型
/// 现在先把「生成期/当前 mod 集合各自独立记录」「尺寸」这两块骨架
/// 立好，避免任务 4 落地时才发现头部结构需要推倒重来。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveHeader {
    /// 存档主体的格式版本号——迁移链（[`crate::migration`]）按这个
    /// 版本号找升级路径。与 mod 版本是两条正交的轴（见
    /// `knowledge/design/identity-and-ids.md` 六、④）：这个字段只反映
    /// 「我们自己的存档格式变了没有」，不反映 mod 内容是否兼容。
    pub schema_version: u32,
    /// 存档时间，Unix 时间戳（秒）。项目现有依赖里没有 `chrono`，用
    /// `std::time::SystemTime` 换算出的 `i64` 足够表达游戏存档场景的
    /// 时间戳，不为此新增一个重依赖。
    pub saved_at: i64,
    /// 角色名，供存档列表界面展示。
    pub character_name: String,
    /// 当前所在区域，人类可读的展示文本——**不是** `ContentIndex`，
    /// 不需要注册表就能显示，这正是本模块顶部约束要求的形状。
    pub current_region: String,
    /// 已游玩的 tick 数。
    pub playtime_ticks: i64,
    /// 生成期 mod 集合快照：这个世界是用这一批 mod 生成的，写入后
    /// 永久不变，只有它能用来复现世界（见模块文档与
    /// `knowledge/design/identity-and-ids.md` 六、③）。
    pub generation_mods: Vec<ModHeaderEntry>,
    /// 当前 mod 集合快照：玩家现在实际开着的这一批，会随时间漂移，
    /// 不满足「同一个种子 + 同一批内容 ⇒ 同一个世界」这条前提，不能
    /// 用来复现世界，只用来与生成期集合比对、判定缺失/内容变化。
    pub current_mods: Vec<ModHeaderEntry>,
    /// `ContentIndex` ↔ `NamespacedId` 字符串形式映射表，按
    /// `ContentIndex` 从 0 开始的顺序排列（来自
    /// `Registry::snapshot()`，见 [`crate::content_index_map`]）。
    pub content_index_map: Vec<String>,
    /// 世界身份三要素之二：地图尺寸（`(zone_span_x, zone_span_y)` 或
    /// 等价的宽高对，具体语义留给任务 4 的 `world_identity` 模块与
    /// `ZoneLayout` 对齐）。地图大小在开局建档前由玩家选择、世界可
    /// 长方形——种子相同但尺寸不同产出的不是同一个世界，因此尺寸与
    /// 种子、生成期 mod 集合一样，缺一不可。
    pub world_size: (u32, u32),
}

/// 存档头里记录的单个 mod 条目：命名空间、版本号、内容哈希。
///
/// **只存版本号不够**——mod 作者改内容却不改版本号是常态。
/// `content_hash` 取自 `Registry::content_hash_of`
/// （`ll_mod::registry`），版本号相同但哈希不同就是「内容变了、版本号
/// 没跟上」的信号，见 `knowledge/design/identity-and-ids.md` 六、①。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModHeaderEntry {
    /// mod 的命名空间（不含路径部分）。
    pub namespace: String,
    /// mod 作者填写的版本号，原样保留，不做语义化版本解析——版本号
    /// 比较不是本类型的职责,内容哈希才是判定"是否真的变了"的依据。
    pub version: String,
    /// 该命名空间贡献的全部内容的哈希摘要（`Registry::content_hash_of`
    /// 的返回值）。
    pub content_hash: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> SaveHeader {
        SaveHeader {
            schema_version: 1,
            saved_at: 1_755_000_000,
            character_name: "旅人".to_string(),
            current_region: "初始村落".to_string(),
            playtime_ticks: 42,
            generation_mods: vec![ModHeaderEntry {
                namespace: "lostland".to_string(),
                version: "0.1.0".to_string(),
                content_hash: 123,
            }],
            current_mods: vec![
                ModHeaderEntry {
                    namespace: "lostland".to_string(),
                    version: "0.1.0".to_string(),
                    content_hash: 123,
                },
                ModHeaderEntry {
                    namespace: "yourmod".to_string(),
                    version: "0.2.0".to_string(),
                    content_hash: 456,
                },
            ],
            content_index_map: vec![
                "lostland:fireball".to_string(),
                "yourmod:iceball".to_string(),
            ],
            world_size: (48, 32),
        }
    }

    #[test]
    fn 存档头可以序列化为可读的json字符串() {
        // Arrange
        let header = sample_header();

        // Act
        let json = serde_json::to_string_pretty(&header).expect("SaveHeader 应当总是可序列化");

        // Assert：肉眼可读——字段名与字符串值直接出现在文本里，不是
        // 二进制或经过压缩的载荷。
        assert!(json.contains("\"character_name\": \"旅人\""));
    }

    #[test]
    fn 存档头序列化再反序列化后与原值相等() {
        // Arrange
        let header = sample_header();

        // Act
        let json = serde_json::to_string(&header).expect("序列化不应失败");
        let restored: SaveHeader = serde_json::from_str(&json).expect("反序列化不应失败");

        // Assert
        assert_eq!(restored, header);
    }

    #[test]
    fn contentindexmap序列化为字符串数组而非整数索引() {
        // 钉住模块文档顶部的类型约束的一个可观测后果：若有人误把字段
        // 类型改成 Vec<ContentIndex>，即便编译通过，JSON 里的元素会从
        // 字符串变成裸整数，本测试会失败。
        // Arrange
        let header = sample_header();

        // Act
        let json = serde_json::to_value(&header).expect("序列化不应失败");

        // Assert
        let entries = json["content_index_map"]
            .as_array()
            .expect("content_index_map 应当是一个 JSON 数组");
        assert!(entries.iter().all(|entry| entry.is_string()));
    }

    #[test]
    fn 生成期mod集合与当前mod集合各自独立记录() {
        // 世界身份的锚点（生成期集合）与玩家当前实际开着的集合
        // （当前集合）是两个独立字段，互不覆盖——即便两者初始相同，
        // 后续更新其中一个不影响另一个存量。
        // Arrange
        let mut header = sample_header();
        let generation_len_before = header.generation_mods.len();

        // Act：模拟玩家中途新增了一个 mod，只应体现在当前集合。
        header.current_mods.push(ModHeaderEntry {
            namespace: "thirdmod".to_string(),
            version: "1.0.0".to_string(),
            content_hash: 789,
        });

        // Assert
        assert_eq!(header.generation_mods.len(), generation_len_before);
    }
}
