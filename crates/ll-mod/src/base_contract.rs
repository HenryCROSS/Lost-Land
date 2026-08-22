//! 本体内容契约解析：内容定义搬进 mod 脚本之后，把「Rust 代码引用了
//! 哪些本体内容」这件事重新变成一条**会失败的、响亮的**检查。
//!
//! # 起因：搬走内容会丢掉编译期检查
//!
//! 本体内容（种族/职业/……）此前硬编码在 Rust 里，`materialize_base_*`
//! 一类函数同时干了两件事：**声明内容的字段值**，以及**产出一个句柄
//! 结构体**（`ll_mod::race::BaseRaceIds` 这样的 `ContentIndex` 缓存）。
//! 项目所有者裁定「本体 = 框架能力，内容 = mod 实例」之后，前一半搬进
//! `mods/lostland/*.scm`，后一半必须留在 Rust——因为**使用点的编译期
//! 安全全靠它**：`content.race_ids.human` 这行代码里，字段没了就编译
//! 不过，没有任何字符串拼写错误的空间。
//!
//! 但句柄结构体的**填充**从此不再由 Rust 自己完成：`BaseRaceIds.human`
//! 里那个索引，现在要靠「装载完毕后，去注册表里按 `lostland:human`
//! 这个 id 查一次」才能拿到。这一步是可能失败的（玩家误删
//! `mods/lostland/`、脚本写错语法、内容改了 id），而失败的默认表现
//! 会是最糟的那一种：一个查不到的索引静默退化成
//! [`ContentIndex::default`]，游戏照常启动，直到某个 NPC 生成不出来
//! 才第一次表现出来。
//!
//! 本模块就是补回那道检查：**装载后按 id 逐字段解析，缺任何一个就整批
//! 失败**，错误里点名缺了哪几条、各自缺在哪一层。
//!
//! # 为什么把「缺失」收集完再报，而不是撞见第一条就返回
//!
//! [`crate::topo`]/`ll_content::load_error::check_mod_content` 那类
//! 检查「第一条不匹配已经足够定位问题」，因为它们面对的是**开发者**
//! 在调一个 mod。本模块面对的是**玩家**看到的启动失败：一次只说
//! 「缺 `lostland:human`」，玩家补好之后重启，再被告知「缺
//! `lostland:dwarf`」，是最难受的一种错误呈现。一次把三条都列出来，
//! 玩家一眼就能判断出「整个 `mods/lostland/` 都不在了」而不是
//! 「某一条内容改了名」。
//!
//! # 两层判定：`NotInterned` 与 `NotDefined` 不是同一件事
//!
//! [`Registry::get`](crate::registry::Registry::get) 查得到只说明
//! 「这个字符串 id 被 intern 过」，不说明「对应的内容表真的登记了它的
//! 字段值」——ADR 0017「注册期完整校验」下这两步天然分离（先换索引，
//! 再 `*Table::define`）。一条内容可能被别的脚本当作跨表引用 intern
//! 出来、却从来没有人定义过它。[`MissingReason`] 因此分两个取值：
//! 前者指向「这个 id 压根没出现」，后者指向「id 在、定义不在」，两者
//! 对玩家/mod 作者意味着完全不同的排查方向。
//!
//! # 与 `ll_content::load_error` 的分工
//!
//! `ll_content` 那一层管的是**存档**与当前内容集合兼容不兼容（存档头
//! 记了什么、现在装了什么）。本模块管的是**本次装载自身**是否自洽：
//! Rust 代码引用的本体内容此刻在不在。两者互不替代——本模块通过了，
//! 存档仍可能不兼容；存档兼容检查通过了，也不代表本体内容齐全（旧档
//! 根本不知道本体有哪些内容）。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::registry::Registry;

/// 一条本体内容没能解析成功的原因，见模块文档「两层判定」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingReason {
    /// 这个 id 在本次装载的 [`Registry`] 里压根没有被 intern 过——
    /// 多半是声明它的那个脚本文件不在了，或整个 mod 目录不在了。
    NotInterned,
    /// id 在 [`Registry`] 里有（有人引用过它），但对应内容表里没有它的
    /// 字段值定义——多半是脚本只把它当作跨表引用写了字符串，却没有
    /// 真的调用对应的 `register-*` 注册它。
    NotDefined,
}

impl fmt::Display for MissingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MissingReason::NotInterned => write!(f, "注册表里没有这个 id"),
            MissingReason::NotDefined => write!(f, "id 在注册表里，但内容表里没有它的定义"),
        }
    }
}

/// 一条解析失败的明细：句柄结构体的哪个字段、要的是哪个 id、缺在哪
/// 一层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingBaseContent {
    /// 句柄结构体上对应的字段名，例如 `"BaseRaceIds::human"`——错误
    /// 文案里点名字段而不只是 id，是因为读到这条错误的人（mod 作者、
    /// 排查启动失败的玩家）下一步多半要去看是谁在用它。
    pub field: &'static str,
    /// 这个字段要求必须存在的内容 id。
    pub id: NamespacedId,
    /// 缺在哪一层。
    pub reason: MissingReason,
}

/// 一整份本体内容契约解析失败：至少缺了一条。
///
/// 携带**全部**缺失明细，不是第一条，理由见模块文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseContractError {
    /// 这份契约的人类可读名字，例如 `"本体种族"`——一次装载会解析
    /// 多份契约（种族一份、职业一份、……），错误里必须能分清是哪份。
    pub contract: &'static str,
    /// 契约总共要求几条内容——与 `missing.len()` 一起，读者一眼就能
    /// 区分「全都不在（整个目录没了）」和「少了一条（某条改名了）」。
    pub required: usize,
    /// 全部缺失明细，顺序与 [`BaseContractResolver::require`] 的调用
    /// 顺序一致（即句柄结构体字段的声明顺序），不依赖任何哈希容器的
    /// 迭代顺序——规格 C5。
    pub missing: Vec<MissingBaseContent>,
}

impl fmt::Display for BaseContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{}契约解析失败：{} 条必需内容里有 {} 条没能在本次装载里找到。",
            self.contract,
            self.required,
            self.missing.len()
        )?;
        for entry in &self.missing {
            writeln!(f, "  - {}（{}）：{}", entry.field, entry.id, entry.reason)?;
        }
        write!(
            f,
            "本体内容住在 mod 脚本里，不再硬编码在程序内部——请确认本体内容目录完整存在、\
             未被改名或删除；若它的脚本报了错，本次装载报告里会有一条对应的失败记录。"
        )
    }
}

impl std::error::Error for BaseContractError {}

/// 契约解析器：逐字段声明「这个句柄字段要哪个 id」，最后一次性收口。
///
/// 典型用法（`ll_mod::race::resolve_base_races` 是真实调用点）：
///
/// ```
/// use ll_mod::base_contract::BaseContractResolver;
/// use ll_mod::race::RaceTable;
/// use ll_mod::registry::Registry;
///
/// let registry = Registry::new();
/// let table = RaceTable::new();
///
/// let mut resolver = BaseContractResolver::new("本体种族", &registry);
/// let _human = resolver.require("BaseRaceIds::human", "lostland:human", |index| {
///     table.is_defined(index)
/// });
/// // 空注册表里当然什么都查不到，收口时如实失败。
/// let error = resolver.finish().expect_err("空注册表解析不出任何本体内容");
/// assert_eq!(error.missing.len(), 1);
/// ```
///
/// # 为什么 `require` 在失败时也要返回一个索引
///
/// 句柄结构体（`BaseRaceIds { human, dwarf, elf }`）必须先被构造出来
/// 才谈得上返回，而构造要求每个字段都有值——若 `require` 返回
/// `Option`/`Result`，调用方就只能在第一条缺失处 `?` 提前返回，那正是
/// 模块文档否掉的「一次只报一条」。这里选择返回
/// [`ContentIndex::default`] 占位并把缺失记进内部列表，由
/// [`Self::finish`] 统一收口——**占位值永远不会流到调用方之外**：
/// `finish` 返回 `Err` 时调用方按约定丢弃整个句柄结构体，返回 `Ok`
/// 时列表为空、每个字段都是真实解析出来的索引。这条约定由本类型的
/// 签名形状保证（`finish` 消费 `self`，调用方拿不到"部分成功"的中间
/// 状态），不是靠调用方自觉。
pub struct BaseContractResolver<'a> {
    contract: &'static str,
    registry: &'a Registry,
    required: usize,
    missing: Vec<MissingBaseContent>,
}

impl<'a> BaseContractResolver<'a> {
    /// 开一份契约。`contract` 是人类可读的契约名，进错误文案。
    pub fn new(contract: &'static str, registry: &'a Registry) -> Self {
        Self {
            contract,
            registry,
            required: 0,
            missing: Vec::new(),
        }
    }

    /// 要求 `id` 这条内容必须已经被注册、且已经被对应内容表定义。
    ///
    /// `field` 是句柄结构体上的字段名（例如 `"BaseRaceIds::human"`），
    /// 只进错误文案。`is_defined` 是「对应内容表认不认这个索引」的判定
    /// ——本模块不认识任何一张具体的内容表（那会让本模块与全部内容表
    /// 的类型互相耦合，与 [`crate::content_hash`] 模块文档
    /// 「为什么不能在 `intern` 内部做」是同一条理由），由调用方把
    /// `|index| table.is_defined(index)` 传进来。
    ///
    /// `id` 是 Rust 侧的字面量（本体内容的 id 就写在调用点上，那正是
    /// 「句柄留在 Rust」的含义），非法字面量是程序员错误而不是运行期
    /// 情形，因此这里 panic 而不是多出一条运行期错误分支——与本仓库
    /// `NamespacedId::parse(...).expect("本体 id 字面量恒合法")` 的既有
    /// 写法一致。
    pub fn require(
        &mut self,
        field: &'static str,
        id: &str,
        is_defined: impl Fn(ContentIndex) -> bool,
    ) -> ContentIndex {
        self.required += 1;
        let parsed = NamespacedId::parse(id).expect("本体内容 id 字面量恒合法");

        let Some(index) = self.registry.get(&parsed) else {
            self.missing.push(MissingBaseContent {
                field,
                id: parsed,
                reason: MissingReason::NotInterned,
            });
            return ContentIndex::default();
        };

        if !is_defined(index) {
            self.missing.push(MissingBaseContent {
                field,
                id: parsed,
                reason: MissingReason::NotDefined,
            });
            return ContentIndex::default();
        }

        index
    }

    /// 收口：一条都不缺才算成功。
    ///
    /// 消费 `self`——调用方拿不到「解析了一半」的中间状态，见类型文档。
    pub fn finish(self) -> Result<(), BaseContractError> {
        if self.missing.is_empty() {
            return Ok(());
        }
        Err(BaseContractError {
            contract: self.contract,
            required: self.required,
            missing: self.missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{RaceAttrs, RaceTable};
    use ll_world::entity::BaseStats;

    /// 造一个「id 已 intern、内容表也已 define」的最小注册表 + 种族表。
    fn registry_with_human() -> (Registry, RaceTable) {
        let mut registry = Registry::new();
        let mut table = RaceTable::new();
        let index = registry.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        table
            .define(
                index,
                RaceAttrs {
                    display_name_key: NamespacedId::parse("lostland:race.human.display_name")
                        .expect("合法标识符"),
                    stat_modifiers: BaseStats {
                        strength: 0,
                        dexterity: 0,
                        constitution: 0,
                        intelligence: 0,
                        willpower: 0,
                        charisma: 0,
                        luck: 0,
                    },
                    darkvision_floor: 0,
                    footprint: (1, 1),
                    lifespan_years: 80,
                    xp_reward: 0,
                    traits: Vec::new(),
                    starting_items: Vec::new(),
                },
            )
            .expect("首次定义应当成功");
        (registry, table)
    }

    #[test]
    fn 全部内容都在时解析成功且返回真实索引() {
        // Arrange
        let (registry, table) = registry_with_human();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);

        // Act
        let human = resolver.require("BaseRaceIds::human", "lostland:human", |index| {
            table.is_defined(index)
        });
        let result = resolver.finish();

        // Assert
        assert!(result.is_ok());
        assert_eq!(
            registry.resolve(human).map(|id| id.to_string()),
            Some("lostland:human".to_string())
        );
    }

    #[test]
    fn id从未注册时报notinterned() {
        // Arrange
        let registry = Registry::new();
        let table = RaceTable::new();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);

        // Act
        resolver.require("BaseRaceIds::human", "lostland:human", |index| {
            table.is_defined(index)
        });
        let error = resolver.finish().expect_err("空注册表必须解析失败");

        // Assert
        assert_eq!(error.missing.len(), 1);
        assert_eq!(error.missing[0].reason, MissingReason::NotInterned);
        assert_eq!(error.missing[0].field, "BaseRaceIds::human");
    }

    #[test]
    fn id已注册但内容表没定义时报notdefined() {
        // 这正是模块文档「两层判定」一节说的第二种情形：别的脚本把这个
        // id 当跨表引用 intern 出来了，却没有人真的注册它。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let table = RaceTable::new();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);

        // Act
        resolver.require("BaseRaceIds::human", "lostland:human", |index| {
            table.is_defined(index)
        });
        let error = resolver
            .finish()
            .expect_err("只 intern 未 define 必须解析失败");

        // Assert
        assert_eq!(error.missing.len(), 1);
        assert_eq!(error.missing[0].reason, MissingReason::NotDefined);
    }

    #[test]
    fn 多条缺失一次性全部列出而不是只报第一条() {
        // 模块文档「为什么把缺失收集完再报」那条纪律的守卫：谁把
        // require 改成撞见第一条就提前返回，本条立刻变红。
        // Arrange
        let registry = Registry::new();
        let table = RaceTable::new();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);

        // Act
        resolver.require("BaseRaceIds::human", "lostland:human", |i| {
            table.is_defined(i)
        });
        resolver.require("BaseRaceIds::dwarf", "lostland:dwarf", |i| {
            table.is_defined(i)
        });
        resolver.require("BaseRaceIds::elf", "lostland:elf", |i| table.is_defined(i));
        let error = resolver.finish().expect_err("三条都缺必须解析失败");

        // Assert：三条都在，且顺序就是 require 的调用顺序。
        assert_eq!(error.required, 3);
        assert_eq!(
            error
                .missing
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            vec!["lostland:human", "lostland:dwarf", "lostland:elf"]
        );
    }

    #[test]
    fn 错误文案点名每一条缺失的字段与id() {
        // 玩家看到的就是这段文字，它必须能直接指向下一步动作。
        // Arrange
        let registry = Registry::new();
        let table = RaceTable::new();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);
        resolver.require("BaseRaceIds::human", "lostland:human", |i| {
            table.is_defined(i)
        });

        // Act
        let text = resolver.finish().expect_err("必须失败").to_string();

        // Assert
        assert!(text.contains("本体种族"));
        assert!(text.contains("BaseRaceIds::human"));
        assert!(text.contains("lostland:human"));
        assert!(text.contains("注册表里没有这个 id"));
    }

    #[test]
    fn 部分缺失时只报缺的那几条() {
        // Arrange
        let (registry, table) = registry_with_human();
        let mut resolver = BaseContractResolver::new("本体种族", &registry);

        // Act
        resolver.require("BaseRaceIds::human", "lostland:human", |i| {
            table.is_defined(i)
        });
        resolver.require("BaseRaceIds::dwarf", "lostland:dwarf", |i| {
            table.is_defined(i)
        });
        let error = resolver.finish().expect_err("矮人缺失必须解析失败");

        // Assert
        assert_eq!(error.required, 2);
        assert_eq!(error.missing.len(), 1);
        assert_eq!(error.missing[0].field, "BaseRaceIds::dwarf");
    }
}
