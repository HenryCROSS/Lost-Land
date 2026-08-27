//! 职业 → 行为原型的绑定表：`classes.json5` 的 `behavior` 字段落点。
//!
//! # 这张表来解的那个缺陷
//!
//! NPC 生成批次（`bc2fc81`）落地之后，`ll_game::app::npc_behavior_source`
//! 把**卫兵那棵树**发给了全部物化出来的 NPC，理由是当时「没有『哪个
//! 生物用哪棵树』的内容绑定」。后果是卫兵那棵树的兜底分支（「看得见人
//! 就走近一步」）套在了农夫、屠夫、猎户身上——**整座村子的居民都会朝
//! 玩家走过来**。本模块是那条内容绑定，`settlements-structures-and-npc-
//! spawning.md` 六节 6.1 提出的 `ClassBehaviorBindings`。
//!
//! # 形状：照抄 [`crate::xp_curve::XpCurveBindings`]
//!
//! 那个先例的确切形状（已核实，不是照设计文档的行文转述）：
//!
//! - 一张**只存绑定关系**的表，`BTreeMap<ContentIndex, ...>`，
//!   **不为绑定关系本身分配 `ContentIndex`**。
//! - 因此它不落在 `Registry::snapshot` 遍历到的任何一个 id 上，
//!   [`crate::content_hash::classify_index`] **不加分支**，
//!   `ContentTableKind` 不多一个变体——见
//!   [`crate::content_hash`] 模块文档「例外，且是刻意的例外」一段，
//!   本表与 `XpCurveBindings` 落在同一条例外里。
//! - 内容侧的入口是**职业自己那一行上的一个可选字段**
//!   （`RawClass::xp_curve` 的同位物 `RawClass::behavior`），不是一个
//!   要把职业 id 再写一遍的独立指令。
//!
//! **代价如实标注，与先例逐字相同**：绑定关系因此**不进内容值哈希**。
//! 一个 mod 把农夫的行为原型从「平民」改成「野兽」，内容哈希一个字节
//! 都不变，存档兼容性检查不会察觉。这与 `XpCurveBindings` 是同一条
//! 已知缺口（同一段模块文档已经记着它），本批次不单独修——修它需要
//! 「职业自己的哈希函数里再多查一张绑定表」这类结构性改动，那是那条
//! 缺口自己的批次。
//!
//! # 为什么绑在职业上，不建「生物模板」类型
//!
//! 设计文档六节 6.1 的论证在无脚本时代逐条仍然成立，且更强了：
//!
//! - 一个 `NpcTemplateDef {race, class, archetype}` 的全部价值是允许
//!   「同职业不同树」。**没有任何一份内容设计需要这种错位**（YAGNI）。
//! - 模板层要在 `Agent` 上多存一个字段 → 存档变更 + remap + 哈希覆盖。
//!   绑在职业上则 **`Agent` 一个字段都不加**：`profession` 已经在那儿。
//! - 真出现「同职业两种行为」的需求时，正确的表达是**两个职业**。
//!
//! # 确定性（C5）
//!
//! `BTreeMap` 按 `ContentIndex` 有序，本模块也从不遍历它——查询是单点
//! `get`。选树因此与任何迭代顺序无关。

use std::collections::BTreeMap;

use ll_core::ident::ContentIndex;

/// 一种**行为原型**——多条职业共用的一棵树。
///
/// # 为什么是原型，不是「一条职业一棵树」
///
/// 本体现在有十三条职业。给每条写一棵独立的树是十三份几乎相同的代码，
/// 而 ADR 0021 的判据是「**有没有一份算法要被共用**」：农夫、屠夫、
/// 铁匠、渔夫、牧羊人、石匠、据点管理者这七条，在「这一回合该干什么」
/// 这个问题上**一个字都不差**——他们共用的是同一份算法，因此共用同一
/// 个原型。真正分叉的只有三种：
///
/// | 原型 | 分叉在哪 | 哪些职业 |
/// |---|---|---|
/// | [`BehaviorArchetype::Townsfolk`] | 不找目标、不接近任何人 | 据点管理者/农夫/猎户/屠夫/铁匠/渔夫/牧羊人/石匠 |
/// | [`BehaviorArchetype::Sentry`] | 主动走向视野内的人，卫兵还会盘查 | 卫兵/民兵 |
/// | [`BehaviorArchetype::Beast`] | 见到敌对目标就放技能/近战 | 本体无（怪物用） |
///
/// 三者之间不共享任何一段判断：平民那棵树**连一次目标查询都不做**，
/// 守卫那棵树的兜底恰恰是「查到目标就走过去」，野兽那棵树问的是敌对
/// 关系而不是可见性。把其中任意两个合并都要引入一个「要不要接近」的
/// 开关参数，那正是 ADR 0021 点名要避免的「为对称而抽象」的反面——
/// 把三份真的不同的算法挤进一个带旗标的函数。
///
/// # 为什么是枚举，不是一张可扩展的表
///
/// 树是 Rust 写的（脚本系统已整个拆除，ADR 0028），**第三方加不了新
/// 的树**——那要改引擎源码。一张可扩展的表能换来的能力因此不存在，
/// 而 `match` 一次列全由编译器保证不漏。这与
/// [`crate::native_behavior::NativeBehaviorTree`] 自己是个封闭枚举是
/// 同一条理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BehaviorArchetype {
    /// **平民型**：在原地一带走动，**不主动接近任何人**。
    ///
    /// 这是本批次存在的理由——农夫不该朝玩家走过来。
    Townsfolk,
    /// **守卫型**：走向视野内最近的人；是卫兵职业的还会先掷一次盘查
    /// 对抗判定。
    Sentry,
    /// **野兽型**：见到敌对目标就优先放技能，放不了就近战，都不行就
    /// 靠近。
    Beast,
}

impl BehaviorArchetype {
    /// 全部原型，**固定顺序**——遍历唯一允许的来源（约束 C5）。
    pub const ALL: [BehaviorArchetype; 3] = [
        BehaviorArchetype::Townsfolk,
        BehaviorArchetype::Sentry,
        BehaviorArchetype::Beast,
    ];

    /// 内容文件里写的那个字面量（`classes.json5` 的 `behavior` 字段）。
    pub const fn as_str(self) -> &'static str {
        match self {
            BehaviorArchetype::Townsfolk => "townsfolk",
            BehaviorArchetype::Sentry => "sentry",
            BehaviorArchetype::Beast => "beast",
        }
    }

    /// 从内容文件里的字面量解析；不认识的写法返回 `None`，由装载期当场
    /// 报错点名（ADR 0017「注册期完整校验」）——**不静默落回某个默认
    /// 原型**：`behavior: "farmer"`（写成了职业名）静默变成守卫，正是
    /// 本模块要修的那个缺陷的翻版。
    pub fn parse(raw: &str) -> Option<BehaviorArchetype> {
        BehaviorArchetype::ALL
            .into_iter()
            .find(|archetype| archetype.as_str() == raw)
    }
}

impl std::fmt::Display for BehaviorArchetype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 职业 → 行为原型的绑定表。
///
/// 形状与理由见模块文档。**不产生新的 `ContentIndex`**。
#[derive(Debug, Default, Clone)]
pub struct ClassBehaviorBindings {
    class: BTreeMap<ContentIndex, BehaviorArchetype>,
}

impl ClassBehaviorBindings {
    /// 建立空绑定表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 把 `class_id` 绑定到一个行为原型。
    ///
    /// 同一个职业重复绑定按**最后一次**生效（不是错误）——与
    /// [`crate::xp_curve::XpCurveBindings::bind_class`] 逐字相同的语义，
    /// 理由也相同：绑定关系不像定义那样需要「只能有一份权威声明」。
    ///
    /// 没有 `Result`：这里没有一条能失败的校验（原型是枚举，装载期
    /// 解析那一步已经把「不认识的写法」拦掉了），给它一个恒 `Ok` 的
    /// 返回类型只会让调用点多一行 `?`。
    pub fn bind_class(&mut self, class_id: ContentIndex, archetype: BehaviorArchetype) {
        self.class.insert(class_id, archetype);
    }

    /// 查一条职业绑定的行为原型；未绑定返回 `None`。
    pub fn archetype(&self, class_id: ContentIndex) -> Option<BehaviorArchetype> {
        self.class.get(&class_id).copied()
    }

    /// 当前登记了多少条绑定——集成测试用来核对「本体内容真的绑了这么
    /// 多条」。
    pub fn len(&self) -> usize {
        self.class.len()
    }

    /// 一条绑定都没有。
    pub fn is_empty(&self) -> bool {
        self.class.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use ll_core::ident::{Interner, NamespacedId};

    use super::*;

    /// 造一批彼此不同的测试用索引——`ContentIndex` 没有公开的裸整数
    /// 构造函数，照抄 [`crate::xp_curve`] 测试里的同名帮手。
    fn distinct_indices(count: usize) -> Vec<ContentIndex> {
        let mut interner = Interner::new();
        (0..count)
            .map(|i| {
                interner.intern(
                    NamespacedId::parse(&format!("test:slot_{i}")).expect("测试标识符恒合法"),
                )
            })
            .collect()
    }

    #[test]
    fn 三个原型的字面量互不相同且能往返解析() {
        for archetype in BehaviorArchetype::ALL {
            assert_eq!(
                BehaviorArchetype::parse(archetype.as_str()),
                Some(archetype),
                "{archetype} 的字面量应当能解析回它自己"
            );
        }
        assert_eq!(BehaviorArchetype::ALL.len(), 3);
    }

    #[test]
    fn 不认识的行为原型写法解析失败而不是落回默认值() {
        // 写成职业名是最容易犯的那个错——它必须失败，不能静默变成守卫。
        assert_eq!(BehaviorArchetype::parse("farmer"), None);
        assert_eq!(BehaviorArchetype::parse(""), None);
        assert_eq!(BehaviorArchetype::parse("Townsfolk"), None);
    }

    #[test]
    fn 未绑定的职业查不到原型而重复绑定取最后一次() {
        let mut bindings = ClassBehaviorBindings::new();
        assert!(bindings.is_empty());
        let [farmer, guard] = distinct_indices(2)[..] else {
            unreachable!("distinct_indices(2) 恒返回两条")
        };
        assert_eq!(bindings.archetype(farmer), None);

        bindings.bind_class(farmer, BehaviorArchetype::Sentry);
        bindings.bind_class(farmer, BehaviorArchetype::Townsfolk);
        assert_eq!(
            bindings.archetype(farmer),
            Some(BehaviorArchetype::Townsfolk)
        );
        assert_eq!(bindings.archetype(guard), None);
        assert_eq!(bindings.len(), 1);
    }
}
