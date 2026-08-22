//! 角色六项主属性的字段布局。
//!
//! 完整的属性系统（调整值公式、三系攻防、穿透、幸运、次级属性）冻结在
//! `knowledge/design/attribute-system.md`，实现阶段是 P3（战斗结算）与
//! P5（职业技能树）。本任务只建 P3 建 [`crate::entity::Agent`] 时必须
//! 已经存在的字段布局——具体的伤害/判定公式属于后续批次。

use ll_core::time::Tick;

/// 六项主属性 + 幸运。全部整数，理由见 `attribute-system.md` 开篇「所有
/// 数值一律整数」。幸运并入批次（见 [`Self::luck`] 文档）之前本类型只有
/// 六个字段，字段名 `BaseStats` 沿用未改——七项数值仍然是"这个实体的
/// 基础数值分别是多少"这一件事，改名不会让这件事更清楚。
///
/// 基础属性硬上限 30（装备与临时效果可以突破，见该文档「成长上限」
/// 一节），但那是 P5 装备系统要执行的规则，本类型自身不做范围校验——
/// 校验属于装备结算的职责，不是字段布局本身的不变式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BaseStats {
    /// 力量：物理攻击、负重上限。
    pub strength: i32,
    /// 敏捷：时间轴速度、闪避、命中。
    pub dexterity: i32,
    /// 体质：生命上限、抗性、耐力。
    pub constitution: i32,
    /// 智力：魔法攻击、法力、学习速度。
    pub intelligence: i32,
    /// 意志：精神攻防、抵抗、视野半径。
    pub willpower: i32,
    /// 魅力：招募随从、交易议价、随从士气。
    pub charisma: i32,
    /// 幸运：暴击率（每点 +5‰，`ll_sim::combat::crit_chance_permille`）
    /// 已接线，见 `ll_sim::resolve::resolve_attack`「暴击」一节。仍未
    /// 落地的（均详见 `knowledge/design/attribute-system.md` 「五、
    /// 幸运」）：优势掷骰（每满 20 点多掷一次取较优）、掉落品质权重、
    /// 稀有事件触发权重——本字段只保证一个消费者（暴击率）真的读它，
    /// 其余三项各自需要一套目前还不存在的机制（判定系统本身/掉落表/
    /// 随机事件表），留给各自的系统落地批次。
    ///
    /// # 为什么现在并入 `BaseStats`（推翻旧裁定）
    ///
    /// 曾经刻意放在 `Agent` 而非 `BaseStats`，理由是幸运走「每点 +5‰」
    /// 的原始值语义，与六项主属性统一的 `(属性 − 10) / 2` 调整值公式
    /// 形状不同——但这条调整值公式当前在本仓库任何一处结算代码里都
    /// **没有真正实现**（`derive_stats`/`resolve_attack` 至今直接使用
    /// 六项主属性的裸整数值，没有任何地方对它们做 `(v − 10) / 2` 换算，
    /// 见 `crates/ll-sim/src/resolve.rs`/`combat.rs`），因此「幸运的换算
    /// 公式与其余六项不同」在当前代码里不成立：全部七项属性此刻都是
    /// 原样传递给各自的消费者，幸运与其余六项在这一点上并无二致。
    ///
    /// 项目所有者裁定「幸运我希望也能被并入 `AttributeKind`」——并入后
    /// 幸运戒指、祝福术、诅咒（降低幸运）这类装备/技能加成能复用
    /// [`crate::entity::Agent::active_stat_modifiers`]（按 `AttributeKind`
    /// 索引）与装备静态加成（`ll_sim::item::StatBonus`）这两条现成通道
    /// ——不并入的话两条通道都碰不到幸运，装备/buff 永远无法影响它。
    /// `AttributeKind` 因此新增 [`AttributeKind::Luck`] 变体，本字段随之
    /// 从 `Agent` 挪进 `BaseStats`，与其余六项同一处存储、同一套
    /// `derive_stats` 聚合路径——不再是「同一个概念两处存储」的隐患。
    pub luck: i32,
}

/// [`BaseStats`] 七项字段的枚举形式——供职业「主属性倾向」、技能「临时
/// 属性修正」等需要「指定某一项属性」而非「持有一份完整 [`BaseStats`]」
/// 的场景使用（P5-B `knowledge/design/class-skill-quest-system.md` 第一节
/// `ClassDef::primary_attribute`、第五节 `SkillEffect::TemporaryStatModifier`
/// 的落点）。
///
/// [`BaseStats`] 回答「这个实体的七项数值分别是多少」，`AttributeKind`
/// 回答「指的是七项里的哪一项」——两者服务不同的场景，并存不冲突。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AttributeKind {
    /// 力量：物理攻击、负重上限。
    Strength,
    /// 敏捷：时间轴速度、闪避、命中。
    Dexterity,
    /// 体质：生命上限、抗性、耐力。
    Constitution,
    /// 智力：魔法攻击、法力、学习速度。
    Intelligence,
    /// 意志：精神攻防、抵抗、视野半径。
    Willpower,
    /// 魅力：招募随从、交易议价、随从士气。
    Charisma,
    /// 幸运：暴击率（每点 +5‰），见 [`BaseStats::luck`] 文档「为什么
    /// 现在并入 `BaseStats`」一节——并入批次新增变体，让幸运戒指/祝福术/
    /// 诅咒这类装备/技能加成能复用 `active_stat_modifiers`/`StatBonus`
    /// 两条现成通道。
    Luck,
}

/// 一条正在生效的临时属性修正——技能效果
/// （`SkillEffect::TemporaryStatModifier`，见
/// `knowledge/design/class-skill-quest-system.md` 第五节）落到具体实体
/// 上的实例状态，P5-B 任务 5 新增。
///
/// # 惰性到期判定，不存「当前是否生效」
///
/// 只存「到期时刻」与「修正量」这两个静态量，不存一个可以现算出来的
/// 布尔值——与 [`crate::entity::Agent::skill_cooldowns`] 同一条纪律
/// （见其字段文档），也是 `buffs-and-triggers.md` 一、惰性到期判定的
/// 直接落点：真正要读「这个属性当前的有效修正量」的调用方（衍生属性
/// 计算，P3/P5 之后落地）在读取的那一刻自行比对世界时钟与 `expires_at`
/// ，本类型自身不做任何判断，也不主动清理过期条目（同一条「有意留给
/// 后续阶段的缺口」，见 `Agent::skill_cooldowns` 文档）。
///
/// # 堆叠策略：同源刷新、异源叠加（`buffs-and-triggers.md` 六节）
///
/// `Agent::active_stat_modifiers` 按 `(属性, 来源)` 两层键做索引——外层
/// [`AttributeKind`]，内层「来源」（施加这条修正的技能/载具等内容自身
/// 的 [`ll_core::ident::ContentIndex`]）。这不再是本类型早期版本那种
/// 「同一项属性同一时刻只能有一条生效修正」的形状：**不同来源的修正
/// 各自独立存在、聚合时求和**（`resolve_attack` 一类的读取路径逐条过滤
/// 未过期条目再求和，见 `crates/ll-sim/src/resolve.rs` 的
/// `effective_attribute`）；**同一来源再次施加时才合并**，合并规则见
/// [`Self::merge_same_source`]。项目所有者原话「不同效果能叠加，同效果
/// 只刷新时间」——「来源」就是这句话里「效果」的准确定义，见
/// `buffs-and-triggers.md` 六、①。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveStatModifier {
    /// 增减量，可为负——与技能效果 `SkillEffect::TemporaryStatModifier`
    /// 里的 `amount` 同一个数值,技能释放那一刻原样抄进来（完整形状见
    /// `knowledge/design/class-skill-quest-system.md` 第五节；本 crate
    /// 不依赖 `ll-mod`（依赖方向 `ll-world` ← `ll-sim` ← `ll-script` ←
    /// `ll-mod`，规格 §5），这里只是引用文档说明来源,不是可解析的代码
    /// 内链接）。
    pub delta: i32,
    /// 到期时刻——世界时钟达到或超过这个值时，这条修正视为已失效。
    pub expires_at: Tick,
}

impl ActiveStatModifier {
    /// 同一个 `(属性, 来源)` 再次被施加时的合并规则——
    /// `buffs-and-triggers.md` 六、②③的具体落地，两个维度独立比较，
    /// 互不牵连：
    ///
    /// - **强度**（③）：取 `delta.abs()` 更大的那一个 `delta`。防止一次
    ///   弱化的重复施放（低等级重复施放同名技能、或较弱施法者补了一刀）
    ///   悄悄冲淡已经生效的强化版本。绝对值相等时退化成取新值——两者
    ///   本就等价，谁赢都不改变结果。
    /// - **到期时刻**（②）：恒取 `existing`/`incoming` 两者中较晚的
    ///   `expires_at`（`.max()`），**不是把两段剩余时长相加**——时长
    ///   相加会把「连续快速重复施放」这个漏洞从「数值无限叠加」原样
    ///   平移成「持续时间无限叠加」，是同一个漏洞换了个维度发作。
    ///   这一步与强度谁赢无关：哪怕弱化版本没能刷新强度，它依然应该把
    ///   到期时刻续到自己本该持续到的那一刻。
    pub fn merge_same_source(self, incoming: ActiveStatModifier) -> ActiveStatModifier {
        ActiveStatModifier {
            delta: if incoming.delta.abs() >= self.delta.abs() {
                incoming.delta
            } else {
                self.delta
            },
            expires_at: self.expires_at.max(incoming.expires_at),
        }
    }
}

impl BaseStats {
    /// 基础属性硬上限——`knowledge/design/attribute-system.md`「成长
    /// 上限」一节原文「基础属性**硬上限 30**；装备与临时效果**可以
    /// 突破**」的落点。
    ///
    /// # 为什么常量在这里，校验不在这里
    ///
    /// 本类型自身刻意不做范围校验（见类型文档最后一段）——`Agent.stats`
    /// 会被种族修正烘焙、被存档读回、被测试夹具直接构造，任何一处
    /// 自动裁剪都会把「这个值本来是多少」悄悄改掉。但「上限是 30」
    /// 这个**数字**必须有唯一出处，否则每个要执行这条规则的结算点
    /// （当前是 `ll_sim::resolve` 的升级加点闸门，未来是装备/药水一类
    /// 能否再堆基础值的判定）都会各自写一个字面 30，一次改动就地漂移
    /// ——这正是「魔法数字」要防的那件事。常量放在数值的定义方，
    /// 执行放在规则的定义方。
    ///
    /// **只约束基础值**：装备静态加成（`ll_sim::item::StatBonus`）与
    /// 限时修正（[`crate::entity::Agent::active_stat_modifiers`]）都
    /// 走 `ll_sim::resolve::derive_stats` 那条聚合路径，不写回
    /// `Agent.stats`，因此天然不受本常量约束——设计文档要的「装备可以
    /// 突破」不需要任何额外分支就成立。
    pub const HARD_CAP: i32 = 30;

    /// 六项主属性均取「调整值为零」的基准点（10）——`(10 − 10) / 2 = 0`，
    /// 见 `attribute-system.md` 的调整值公式。用作背景 NPC 升格
    /// （[`crate::entity::ThinPopulation::promote`]）时的默认属性：薄层
    /// 本就不追踪逐项属性，升格时给一个不偏不倚的起点，好过任意选一个
    /// 具体数值却假装它有出处。
    ///
    /// **幸运取零，不是十**：幸运走「每点 +5‰」的原始值语义（见
    /// [`Self::luck`] 文档），没有六项主属性那套调整值公式的「基准点」
    /// 概念可言——`ll_sim::combat::crit_chance_permille`（定义在下游的
    /// `ll-sim`，`ll-world` 不能反过来依赖它，见依赖方向 `ll-world` ←
    /// `ll-sim`，这里只能用反引号纯文本指向，不能用 intra-doc link）
    /// 文档「没有独立的『基础暴击率』常量」一节已经论证零幸运对应零
    /// 暴击率是唯一
    /// 选择，此处的零基准与之呼应，也保持了本仓库全部现存测试夹具「幸运
    /// 恒为零」这条既有假设不被打破。
    pub const BASELINE: BaseStats = BaseStats {
        strength: 10,
        dexterity: 10,
        constitution: 10,
        intelligence: 10,
        willpower: 10,
        charisma: 10,
        luck: 0,
    };

    /// 逐项相加，把一份固定增减量叠加到当前值上——种族属性修正的烘焙
    /// 语义在这里落地：角色/NPC 创建那一刻调用一次
    /// （见 `knowledge/design/race-system.md`「二、属性修正」一节与
    /// `ll_sim::character::bake_race_stat_modifiers` 文档），产出的值
    /// 直接写死进 `Agent.stats`，此后不再持有对修正来源的引用。
    ///
    /// 幸运与其余六项同一条加法路径——种族对幸运的修正因此自动成立：
    /// `RaceDef.stat_modifiers` 复用本类型，一旦某个种族声明非零
    /// `luck`，本函数会像处理其余六项一样把它加进结果，调用方
    /// （`ll_sim::character::bake_race_stat_modifiers`）不需要为幸运
    /// 另写一条分支。
    ///
    /// 不做上下限裁剪——同本类型既有纪律（见类型文档「基础属性硬上限
    /// 30」一节）：范围校验属于装备结算的职责，不是字段布局本身的
    /// 不变式。
    pub fn add_modifiers(self, modifiers: BaseStats) -> BaseStats {
        BaseStats {
            strength: self.strength + modifiers.strength,
            dexterity: self.dexterity + modifiers.dexterity,
            constitution: self.constitution + modifiers.constitution,
            intelligence: self.intelligence + modifiers.intelligence,
            willpower: self.willpower + modifiers.willpower,
            charisma: self.charisma + modifiers.charisma,
            luck: self.luck + modifiers.luck,
        }
    }

    /// 读出七项中指定的那一项——[`AttributeKind`] 回答「哪一项」，本
    /// 方法把那个回答兑换成真正的数值。
    ///
    /// 与 [`Self::add_modifiers`]「整份叠加」互补：加点、单项上限校验
    /// 这类只关心一项的调用方不需要为了读一个数字而手写一个七分支
    /// `match`（`ll_sim::resolve` 侧原本就要写一次，`ll_sim::apply` 侧
    /// 还要再写一次，两处各写一遍正是同一段 `match` 漂移的经典成因）。
    pub fn value(self, kind: AttributeKind) -> i32 {
        match kind {
            AttributeKind::Strength => self.strength,
            AttributeKind::Dexterity => self.dexterity,
            AttributeKind::Constitution => self.constitution,
            AttributeKind::Intelligence => self.intelligence,
            AttributeKind::Willpower => self.willpower,
            AttributeKind::Charisma => self.charisma,
            AttributeKind::Luck => self.luck,
        }
    }

    /// 只把指定的那一项加上 `delta`，其余六项原样保留，返回**新值**
    /// （不原地改写，与 [`Self::add_modifiers`] 同一条不可变纪律）。
    ///
    /// 与 [`Self::add_modifiers`] 的区别不是「少写几个零」：调用方
    /// （玩家升级加点）手里只有一个 [`AttributeKind`]，构造一份「除
    /// 这一项外全零」的 `BaseStats` 需要它自己写那个七分支 `match`
    /// ——那正是 [`Self::value`] 文档说的漂移成因。
    ///
    /// 同样不做上下限裁剪，理由见 [`Self::add_modifiers`]：范围规则
    /// 属于结算层（`ll_sim::resolve` 的加点闸门会在产出效果之前拒绝
    /// 越过 [`Self::HARD_CAP`] 的请求），不是字段布局的不变式。
    pub fn with_added(self, kind: AttributeKind, delta: i32) -> BaseStats {
        match kind {
            AttributeKind::Strength => BaseStats {
                strength: self.strength + delta,
                ..self
            },
            AttributeKind::Dexterity => BaseStats {
                dexterity: self.dexterity + delta,
                ..self
            },
            AttributeKind::Constitution => BaseStats {
                constitution: self.constitution + delta,
                ..self
            },
            AttributeKind::Intelligence => BaseStats {
                intelligence: self.intelligence + delta,
                ..self
            },
            AttributeKind::Willpower => BaseStats {
                willpower: self.willpower + delta,
                ..self
            },
            AttributeKind::Charisma => BaseStats {
                charisma: self.charisma + delta,
                ..self
            },
            AttributeKind::Luck => BaseStats {
                luck: self.luck + delta,
                ..self
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_modifiers叠加非零增减量后各项分别加上对应修正() {
        // Arrange：矮人式修正——体质与力量各有增减，其余项恒零。
        let modifiers = BaseStats {
            strength: 1,
            dexterity: 0,
            constitution: 2,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 0,
        };

        // Act
        let baked = BaseStats::BASELINE.add_modifiers(modifiers);

        // Assert
        assert_eq!(
            baked,
            BaseStats {
                strength: 11,
                dexterity: 10,
                constitution: 12,
                intelligence: 10,
                willpower: 10,
                charisma: 10,
                luck: 0,
            }
        );
    }

    #[test]
    fn add_modifiers叠加非零幸运增减量后幸运随其余六项同一条路径相加() {
        // Arrange：半身人式修正——只有幸运非零，验证种族幸运加成
        // （knowledge/design/race-system.md「二、属性修正」一节）能通过
        // 本函数自动成立，不需要为幸运单开一条分支。
        let modifiers = BaseStats {
            strength: 0,
            dexterity: 0,
            constitution: 0,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 3,
        };

        // Act
        let baked = BaseStats::BASELINE.add_modifiers(modifiers);

        // Assert
        assert_eq!(baked.luck, 3);
    }

    #[test]
    fn add_modifiers叠加全零增减量后结果与基线相等() {
        // Arrange & Act
        let baked = BaseStats::BASELINE.add_modifiers(BaseStats {
            strength: 0,
            dexterity: 0,
            constitution: 0,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 0,
        });

        // Assert：反例——零修正必须原样等于基线，不是「无论如何都加点
        // 什么」。
        assert_eq!(baked, BaseStats::BASELINE);
    }

    #[test]
    fn 基准属性的六项均为十() {
        // Arrange & Act
        let stats = BaseStats::BASELINE;

        // Assert
        assert_eq!(
            [
                stats.strength,
                stats.dexterity,
                stats.constitution,
                stats.intelligence,
                stats.willpower,
                stats.charisma,
            ],
            [10; 6]
        );
    }

    #[test]
    fn 基准幸运为零() {
        // Arrange & Act
        let stats = BaseStats::BASELINE;

        // Assert：幸运不遵循六项主属性「调整值为零→基准 10」的换算，
        // 基准直接是原始值零，见 BASELINE 文档「幸运取零，不是十」一节。
        assert_eq!(stats.luck, 0);
    }

    #[test]
    fn 序列化往返后属性值不变() {
        // Arrange
        let original = BaseStats {
            strength: 14,
            dexterity: 12,
            constitution: 16,
            intelligence: 8,
            willpower: 11,
            charisma: 9,
            luck: 7,
        };

        // Act
        let json = serde_json::to_string(&original).expect("BaseStats 全字段均为整数，必可序列化");
        let decoded: BaseStats = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn 属性种类序列化往返后不变() {
        // Arrange
        let original = AttributeKind::Willpower;

        // Act
        let json = serde_json::to_string(&original).expect("枚举变体必可序列化");
        let decoded: AttributeKind = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded, original);
    }

    #[test]
    fn 不同属性种类不相等() {
        // Arrange & Act & Assert
        assert_ne!(AttributeKind::Strength, AttributeKind::Dexterity);
    }

    #[test]
    fn 合并同源修正时到期时刻取较晚者而非两段时长相加() {
        // Arrange：existing 剩余到 Tick(30)，incoming（同一来源再次施加）
        // 到期于 Tick(90)——若退化成「时长相加」会得到远超两者中较晚者
        // 的结果，这里断言的是 `.max()`，不是加法。
        let existing = ActiveStatModifier {
            delta: 2,
            expires_at: Tick(30),
        };
        let incoming = ActiveStatModifier {
            delta: 2,
            expires_at: Tick(90),
        };

        // Act
        let merged = existing.merge_same_source(incoming);

        // Assert
        assert_eq!(merged.expires_at, Tick(90));
    }

    #[test]
    fn 合并同源修正时更弱的一次施加不冲淡已生效的强度() {
        // Arrange：existing 是较强的修正（|delta| = 5），incoming 是同一
        // 来源较弱的一次重复施放（|delta| = 2）——③要求强度保持较强值。
        let existing = ActiveStatModifier {
            delta: 5,
            expires_at: Tick(10),
        };
        let incoming = ActiveStatModifier {
            delta: 2,
            expires_at: Tick(50),
        };

        // Act
        let merged = existing.merge_same_source(incoming);

        // Assert：强度不变（仍是较强的 5），到期时刻仍然取较晚者（50）
        // ——两个维度独立比较，弱化的重复施放没能刷新强度，但依然续了
        // 到期时刻。
        assert_eq!(merged.delta, 5);
        assert_eq!(merged.expires_at, Tick(50));
    }

    #[test]
    fn 合并同源修正时更强的一次施加覆盖强度并取较晚到期时刻() {
        // Arrange：existing 较弱，incoming 是同一来源更强的一次施放。
        let existing = ActiveStatModifier {
            delta: 2,
            expires_at: Tick(50),
        };
        let incoming = ActiveStatModifier {
            delta: 7,
            expires_at: Tick(10),
        };

        // Act
        let merged = existing.merge_same_source(incoming);

        // Assert：强度更新为较强值（7），到期时刻取两者中较晚者（existing
        // 的 50，尽管这次施加本身更强，但它自己的到期时刻更早）。
        assert_eq!(merged.delta, 7);
        assert_eq!(merged.expires_at, Tick(50));
    }
}
