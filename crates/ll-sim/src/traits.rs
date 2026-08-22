//! `resolve` 侧需要的天赋聚合接口（`knowledge/design/trait-system.md`
//! 三、四、六节）——本批次只接线该文档三节①「授予技能」，見模块文档
//! 「为什么这里只有两个小类型」一节。
//!
//! # 为什么这里只有 `TraitGrant`/`TraitRule` 两个小类型，不是完整
//! `TraitDef`
//!
//! 依赖方向 `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格 §5）：
//! 完整的 `TraitDef`（四类效果——授予技能/属性修正/规则修正/资源池,
//! 连同 `RuleModifier`/`ResourcePoolGrant` 这类当前没有任何 `resolve`
//! 侧消费者的数据形状）留在 `ll-mod::trait_def`，因为它服务的是"mod
//! 作者怎么声明一个天赋"这个注册期问题,不是"resolve 结算时需要读什么"
//! 这个运行期问题——`ll_sim::skill::SkillRule` 只收敛 `resolve_use_skill`
//! 真正要读的字段、不是完整 `ll_mod::skill::SkillDef`，是同一个理由的
//! 先例（见其模块文档「本任务选择的解法」一节）。[`TraitRule`] 同样
//! 只声明本批次唯一有消费者的字段（`granted_skills`）——不是"偷懒少
//! 写三个字段"，是"没有 resolve 侧消费者的字段不该在 resolve 侧的类型
//! 里出现"（YAGNI），等 ②③④ 批次真正把 `resolve`/伤害公式/资源池
//! 查询接上对应效果时，各自在这里追加需要的字段。
//!
//! # 天赋归谁所有：种族这一路先接，其余四路留白
//!
//! `trait-system.md` 三节①的完整公式是「有效技能 = 已学会的 ∪
//! 种族天赋 ∪ 职业天赋 ∪ 副职天赋 ∪ 载具天赋 ∪ buff 天赋」五路来源的
//! 并集。本批次范围只做种族这一路（见 `crate::resolve` 模块文档
//! 「本批次范围」一节的完整裁定）——[`TraitGrantSource`] 因此只有一个
//! 真实实现（`ll_mod::race::RaceTable`），但 trait 本身的形状（「给一个
//! 所有者索引，还我它授予哪些天赋」）与所有者是种族、职业、副职、载具
//! 还是 buff 无关，未来接入其余四路时不需要改这个 trait 的签名,只需要
//! 让对应的表也实现它,再在调用点多传一个来源（见
//! `resolve::resolve_use_skill`——模块私有，无法作为 rustdoc 链接目标——
//! 目前只读一路来源的诚实标注）。
//!
//! # 聚合顺序为什么确定（约束 C5）
//!
//! [`effective_traits`]/[`granted_skills`] 全程只遍历 `Vec`——
//! `TraitGrantSource::granted_traits` 返回的顺序由实现方决定，真实实现
//! （`RaceTable`）内部存的是 `Vec<TraitGrant>`（保留 `register-race-trait`
//! 的调用顺序，见 `ll_mod::race` 模块文档），`TraitRule::granted_skills`
//! 同理源自 `TraitDef.granted_skills: Vec<ContentIndex>`（保留
//! `register-trait` 参数列表里的书写顺序）——两处都不触碰任何
//! `HashMap`/`HashSet`，聚合结果的顺序完全由注册期写死的静态顺序决定,
//! 不随进程/平台变化。这条纪律现在看起来只是"不重要的技术细节"（本
//! 批次的效果集合只求并集,顺序不影响判断结果——同一个技能不管进
//! 并集几次,"能不能放"的答案不变),但设计文档二节已经指出：未来
//! "替换型"效果（例如载具设计里速度是替换语义,不是叠加）落地到天赋
//! 系统后,顺序会直接决定"最后谁的效果生效",现在就把顺序钉死成确定
//! 的、有文档记录的规则，比将来再回头补一条纪律更省事。

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::resource_pool::ResourcePoolGrant;

/// 千分比乘数的分母——`RuleModifier::Resistance::multiplier_permille`
/// 与 `resistance_multiplier_permille` 返回值共用同一把刻度尺：`1000`
/// 表示「无抗性」（乘数 1.0），`0` 表示免疫，`500` 表示半伤，`2000`
/// 表示双倍，与 `crate::combat` 模块「所有数值一律整数，百分比一律用
/// 千分比」的既有惯例同一条纪律。
pub const RESISTANCE_MULTIPLIER_SCALE: i32 = 1000;

/// 天赋效果③「改变规则本身」——`knowledge/design/trait-system.md` 三节
/// 定形的封闭枚举，走注册表第一档（声明式）。
///
/// 类型定义现居 `ll-sim`（伤害类别/抗性接线批次从 `ll_mod::trait_def`
/// 挪过来）——`crate::resolve` 需要在决策层直接 `match`/引用
/// [`RuleModifier::Resistance`] 才能把它接进伤害管线，而依赖方向
/// （`ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`，规格 §5）不允许
/// `ll-sim` 反过来依赖 `ll-mod`——与 [`ResourcePoolGrant`] 当初「挪到
/// `ll-sim`，`ll_mod::trait_def` 改为 `pub use` 复用同一份声明」是同一条
/// 先例（见该类型在 `ll_mod::trait_def` 里的文档「类型定义现移居
/// `ll_sim::resource_pool`」一节），本次是它在 `RuleModifier` 上的第二次
/// 应用。
///
/// # 本批次接线状态
///
/// [`RuleModifier::Resistance`] 现在有真实的 `resolve` 侧消费者——见
/// [`resistance_multiplier_permille`] 与
/// `crate::resolve::resolve_attack` 文档「抗性接线」一节。其余三个变体
/// 仍是纯声明,无消费者：
/// - [`RuleModifier::RerollOnce`] 需要 `roll_one_die` 钩子（伤害公式
///   引擎求值器内部的骰子取数原语），本批次不改写该求值器签名，见
///   `trait-system.md` 三节③「重骰」一节「代价诚实标注」段落。
/// - [`RuleModifier::Advantage`]/[`RuleModifier::Disadvantage`] 需要
///   本项目当前不存在的判定/检定系统,见同节「占位变体」说明。
///
/// # 与 10% 下限的关系
///
/// `damage-formula-mod-api.md` 二十节「与 10% 下限的关系：不冲突，但要
/// 如实说明『免疫』能让下限之后的结果归零」——`crate::combat::damage_after_defense`
/// 内部的 10% 下限保证的是**减伤链路本身**不会因为防御过高被压到零以
/// 下，抗性乘数作用在这条链路**之后**（见 `crate::resolve::resolve_attack`
/// 文档「抗性接线」一节），下限的保证只覆盖到减伤链路那一步：免疫
/// （乘数 0）会合法地把抗性这一步的结果打成 0，即使上一步的减后伤害
/// 满足了 10% 下限——两者不冲突，因为它们从来不覆盖同一个问题：10%
/// 下限回答「打不打得穿盔甲」，抗性回答「这种伤害对这个目标有没有
/// 意义」。免疫因此产出 0 伤害，不是钳到下限伤害——那样"免疫"这个词
/// 就名不副实了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleModifier {
    /// 抗性：该伤害类别的伤害，在既有减伤链路算完之后再打一个千分比
    /// 折扣——挂载点见 `damage-formula-mod-api.md` 二十节（减伤之后、
    /// 乘数形式）。`0`=免疫，`500`=半伤，`2000`=双倍。
    Resistance {
        /// 伤害类别，走 `damage-formula-mod-api.md` 十七节的开放
        /// `register-damage-category` 集合。
        damage_category: ContentIndex,
        /// 千分比乘数，见 [`RESISTANCE_MULTIPLIER_SCALE`]。
        multiplier_permille: i32,
    },
    /// 重骰：该实体掷骰抽出 `value` 时,立即重抽一次,取新值（不再检查
    /// 新值是否又是 `value`）。
    RerollOnce {
        /// 触发重骰的点数。
        value: i32,
    },
    /// 优势：该实体在 `check_context` 这类判定上默认套用优势——占位
    /// 变体，当前无消费者（本项目没有判定/检定系统）。
    Advantage {
        /// 判定种类的开放标识符,具体值域留给判定系统落地时定案。
        check_context: NamespacedId,
    },
    /// 劣势，语义同 [`RuleModifier::Advantage`]，方向相反。
    Disadvantage {
        /// 判定种类的开放标识符。
        check_context: NamespacedId,
    },
}

/// 一条"某个所有者在什么等级授予某个天赋"的引用——种族/职业/副职/
/// 装备/buff 的"这个所有者授予哪些天赋"字段统一用这个类型的列表，
/// 不是裸 `Vec<ContentIndex>`（`trait-system.md` 六节）。
///
/// `unlock_level` 直接比较 [`ll_world::entity::Agent::level`]（`i32`，
/// 该项目选择单一整数等级,不拆分逐职业等级,见其字段文档），因此本
/// 字段也是 `i32` 而不是设计文档草图里的 `u32`——两边比较不需要任何
/// 符号转换,是"载荷共享,消费者决定具体类型"这条设计纪律的直接推论
/// （trait-system.md 六节「消费者需要 `Agent.level`」一节：字段语义
/// 不变,读取路径的具体类型由消费方定案）。种族/副职/装备/buff 恒填
/// `1`（"拥有即生效",这些来源本身不随等级变化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitGrant {
    /// 指向 `ll_mod::trait_def::TraitTable` 的天赋索引。
    pub trait_id: ContentIndex,
    /// 解锁所需等级——`agent.level >= unlock_level` 才算这条引用关系
    /// 生效，见 [`effective_traits`]。
    pub unlock_level: i32,
}

/// `resolve` 侧需要的一条天赋定义的最小只读视图——本批次接线①授予
/// 技能与④授予资源池容量两项，见模块文档「为什么这里只有两个小类型」
/// 一节与 [`crate::resource_pool`] 模块文档「本批次范围」一节。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraitRule {
    /// 这个天赋授予的技能——三节①"有效技能=并集"公式里天赋这一路
    /// 贡献的技能集合。
    pub granted_skills: Vec<ContentIndex>,
    /// 这个天赋授予的资源池容量——`trait-system.md` 三节④，
    /// `crate::resource_pool::effective_scalar_capacity` 聚合这一路
    /// 来源，见其文档。
    pub granted_resource_pools: Vec<ResourcePoolGrant>,
    /// 这个天赋声明的规则修正——`trait-system.md` 三节③，
    /// [`resistance_multiplier_permille`] 聚合这一路来源查抗性,见其
    /// 文档。
    pub rule_modifiers: Vec<RuleModifier>,
}

/// `resolve_use_skill` 依赖的最小「天赋定义来源」接口——把结算算法
/// 本身与「天赋定义具体存在哪个 crate、用什么容器存」解耦，与
/// [`crate::skill::SkillCatalog`] 同一套依赖倒置手法（见其模块文档）。
pub trait TraitCatalog {
    /// 查询一条天赋定义；未注册的索引返回 `None`（对齐 ADR 0015：查
    /// 不到就是查不到）。
    fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule>;
}

/// 空天赋目录：查询任何索引恒返回 `None`——理由同
/// [`crate::skill::NoSkills`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTraits;

impl TraitCatalog for NoTraits {
    fn trait_rule(&self, _trait_id: ContentIndex) -> Option<TraitRule> {
        None
    }
}

/// "给一个所有者索引（本批次唯一的真实调用点是种族），还我它授予
/// 哪些天赋"——`effective_traits` 依赖的另一半来源，见模块文档「天赋
/// 归谁所有」一节。
pub trait TraitGrantSource {
    /// 查询 `owner`（种族/职业/副职/载具/buff 的 `ContentIndex`,本
    /// 批次只有种族会被真实传入）授予的全部天赋引用；`owner` 未注册
    /// 时返回空列表——与 [`TraitCatalog::trait_rule`] 返回 `None` 的
    /// 「查不到就是查不到」是同一条纪律,只是空列表与空并集在语义上
    /// 等价,不需要 `Option` 包一层。
    fn granted_traits(&self, owner: ContentIndex) -> Vec<TraitGrant>;
}

/// 空天赋授予来源：查询任何索引恒返回空列表。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTraitGrants;

impl TraitGrantSource for NoTraitGrants {
    fn granted_traits(&self, _owner: ContentIndex) -> Vec<TraitGrant> {
        Vec::new()
    }
}

/// 聚合一个实体当前有效的天赋 id 集合——`trait-system.md` 三节①公式
/// 里"种族天赋"这一路来源，按 [`TraitGrant::unlock_level`] 过滤
/// （`level >= unlock_level` 才算生效,见 `TraitGrant` 文档），并按
/// 声明顺序去重（模块文档「聚合顺序为什么确定」一节）。
///
/// # 为什么参数是 `race: ContentIndex, level: i32`，不是 `&Agent`
///
/// 本函数只需要 `Agent` 两个字段（`race`/`level`），拆成两个原始
/// 参数而不是借一个 `&Agent`：一是调用方（`resolve_use_skill`）本来
/// 就已经从这两个字段各自取值，不需要额外借用整个结构体；二是
/// `Agent` 的构造在测试里成本不低（`pos: TorusPos`/`current_space:
/// Space` 都要求一个真实世界上下文才能造出合法值，见
/// `crates/ll-sim/src/resolve.rs` 测试模块 `spawn_agent` 帮手），本
/// 模块的单元测试因此不需要为了验证一条纯粹的整数比较逻辑而搭一个
/// 完整世界——与 [`crate::timeline::action_cost`]/
/// `effective_speed_from_dexterity` 只取 `Agent` 的某个具体字段作为
/// 参数，而不是整个 `&Agent`，是同一种取舍。
///
/// # 为什么不缓存
///
/// 天赋是纯派生（`trait-system.md` 八节：`RaceDef.traits` 是注册表
/// 数据的一部分,不进 `WorldState::hash()`,`race`/`level` 已经是存过、
/// 已 hash 的字段）——每次要用时现算，不缓存进 `WorldState`。调用
/// 频率见 `resolve::resolve_use_skill`（模块私有，无法作为 rustdoc
/// 链接目标）文档「性能」一节：与技能释放同频率,不是逐 tick 热路径。
pub fn effective_traits(
    race: ContentIndex,
    level: i32,
    race_traits: &dyn TraitGrantSource,
) -> Vec<ContentIndex> {
    let mut result = Vec::new();
    for grant in race_traits.granted_traits(race) {
        if level >= grant.unlock_level && !result.contains(&grant.trait_id) {
            result.push(grant.trait_id);
        }
    }
    result
}

/// `trait-system.md` 三节①"有效技能=并集"公式里天赋这一路来源——遍历
/// [`effective_traits`] 的结果，收集每条天赋授予的技能，按声明顺序
/// 去重（理由同 [`effective_traits`]）。
pub fn granted_skills(
    race: ContentIndex,
    level: i32,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
) -> Vec<ContentIndex> {
    let mut result = Vec::new();
    for trait_id in effective_traits(race, level, race_traits) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for skill in rule.granted_skills {
            if !result.contains(&skill) {
                result.push(skill);
            }
        }
    }
    result
}

/// 三节③「改变规则本身」的抗性一路来源——遍历 [`effective_traits`] 的
/// 结果,收集全部 [`RuleModifier::Resistance`] 里 `damage_category` 与
/// 参数匹配的条目,取乘数;查不到任何匹配时返回
/// [`RESISTANCE_MULTIPLIER_SCALE`]（即乘数 1.0,等价于"没有抗性"）。
///
/// `crate::resolve::resolve_attack`（伤害类别/抗性接线批次）在减伤链路
/// 算完之后调用本函数,拿到的乘数按
/// `damage-formula-mod-api.md` 二十节挂在"减伤之后",见其文档「抗性
/// 接线」一节。
///
/// # 多条命中时取哪一条：按 `trait_id`（`ContentIndex`）升序取第一条
///
/// `trait-system.md` 三节③「抗性：挂载点已经现成」一节原文：「按
/// `TraitGrant` 的 `ContentIndex` 升序取第一条……不取乘积,理由是『免疫
/// 500‰ 又免疫一次』不应该变成 25% 而不是 0%」——`effective_traits`
/// 返回的顺序是声明顺序（去重后),不是按索引排序,因此这里显式取
/// `trait_id.get()` 最小的一条,不依赖 `effective_traits` 的遍历顺序
/// （约束 C5：结果必须与调用方传入的天赋声明顺序无关,只与
/// `ContentIndex` 数值有关)。
pub fn resistance_multiplier_permille(
    race: ContentIndex,
    level: i32,
    race_traits: &dyn TraitGrantSource,
    traits: &dyn TraitCatalog,
    damage_category: ContentIndex,
) -> i32 {
    let mut best: Option<(ContentIndex, i32)> = None;
    for trait_id in effective_traits(race, level, race_traits) {
        let Some(rule) = traits.trait_rule(trait_id) else {
            continue;
        };
        for modifier in &rule.rule_modifiers {
            let RuleModifier::Resistance {
                damage_category: candidate_category,
                multiplier_permille,
            } = modifier
            else {
                continue;
            };
            if *candidate_category != damage_category {
                continue;
            }
            best = match best {
                Some((best_id, _)) if best_id <= trait_id => best,
                _ => Some((trait_id, *multiplier_permille)),
            };
        }
    }
    best.map(|(_, multiplier)| multiplier)
        .unwrap_or(RESISTANCE_MULTIPLIER_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出第 N 个索引——
    /// 本模块的聚合逻辑只关心索引之间"相不相等"，不关心它们具体指向
    /// 哪条命名空间标识符，用固定字面量占位即可。
    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 测试用天赋授予来源：固定返回构造时传入的一份 `TraitGrant` 列表,
    /// 不关心传入的 `owner` 是谁——本模块的聚合函数只在乎"这个来源
    /// 给出了什么",不在乎它内部怎么按 owner 分流,那是真实实现
    /// （`ll_mod::race::RaceTable`）的职责,不是这里要验证的东西。
    struct FixedGrants(Vec<TraitGrant>);
    impl TraitGrantSource for FixedGrants {
        fn granted_traits(&self, _owner: ContentIndex) -> Vec<TraitGrant> {
            self.0.clone()
        }
    }

    /// 测试用天赋定义来源：固定的 `trait_id -> TraitRule` 映射。
    struct FixedTraits(Vec<(ContentIndex, TraitRule)>);
    impl TraitCatalog for FixedTraits {
        fn trait_rule(&self, trait_id: ContentIndex) -> Option<TraitRule> {
            self.0
                .iter()
                .find(|(id, _)| *id == trait_id)
                .map(|(_, rule)| rule.clone())
        }
    }

    #[test]
    fn 等级达到解锁要求时有效天赋包含该条引用() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let trait_id = index(&mut interner, "lostland:dwarven_resilience");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 5,
        }]);

        // Act
        let result = effective_traits(race, 5, &source);

        // Assert
        assert_eq!(result, vec![trait_id]);
    }

    #[test]
    fn 等级低于解锁要求时有效天赋不包含该条引用() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let trait_id = index(&mut interner, "lostland:dwarven_resilience");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 5,
        }]);

        // Act
        let result = effective_traits(race, 1, &source);

        // Assert
        assert!(result.is_empty());
    }

    #[test]
    fn 有效天赋授予的技能出现在granted_skills结果里() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dragonborn");
        let trait_id = index(&mut interner, "lostland:draconic_breath");
        let skill_id = index(&mut interner, "lostland:breath_weapon");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                granted_skills: vec![skill_id],
                ..Default::default()
            },
        )]);

        // Act
        let result = granted_skills(race, 1, &source, &traits);

        // Assert
        assert_eq!(result, vec![skill_id]);
    }

    #[test]
    fn 两个不同来源授予同一个技能时结果只出现一次() {
        // Arrange：两条不同的天赋引用都授予同一个技能索引——聚合结果
        // 应当去重,不是"并集"变成"多重集"。
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:half_elf");
        let trait_a = index(&mut interner, "lostland:trait_a");
        let trait_b = index(&mut interner, "lostland:trait_b");
        let skill_id = index(&mut interner, "lostland:shared_skill");
        let source = FixedGrants(vec![
            TraitGrant {
                trait_id: trait_a,
                unlock_level: 1,
            },
            TraitGrant {
                trait_id: trait_b,
                unlock_level: 1,
            },
        ]);
        let traits = FixedTraits(vec![
            (
                trait_a,
                TraitRule {
                    granted_skills: vec![skill_id],
                    ..Default::default()
                },
            ),
            (
                trait_b,
                TraitRule {
                    granted_skills: vec![skill_id],
                    ..Default::default()
                },
            ),
        ]);

        // Act
        let result = granted_skills(race, 1, &source, &traits);

        // Assert
        assert_eq!(result, vec![skill_id]);
    }

    #[test]
    fn 空天赋目录查询任意索引返回none() {
        // Arrange
        let mut interner = Interner::new();
        let trait_id = index(&mut interner, "lostland:never_registered");
        let catalog = NoTraits;

        // Act & Assert
        assert_eq!(catalog.trait_rule(trait_id), None);
    }

    #[test]
    fn 空天赋授予来源查询任意所有者返回空列表() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let source = NoTraitGrants;

        // Act & Assert
        assert!(source.granted_traits(race).is_empty());
    }

    #[test]
    fn 没有任何抗性天赋时乘数恒为无抗性刻度() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:human");
        let fire = index(&mut interner, "lostland:fire");
        let source = NoTraitGrants;
        let traits = NoTraits;

        // Act
        let multiplier = resistance_multiplier_permille(race, 1, &source, &traits, fire);

        // Assert
        assert_eq!(multiplier, RESISTANCE_MULTIPLIER_SCALE);
    }

    #[test]
    fn 有效天赋声明的抗性命中匹配类别时返回其乘数() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let fire = index(&mut interner, "lostland:fire");
        let trait_id = index(&mut interner, "lostland:fire_resistance");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                rule_modifiers: vec![RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                }],
                ..Default::default()
            },
        )]);

        // Act
        let multiplier = resistance_multiplier_permille(race, 1, &source, &traits, fire);

        // Assert
        assert_eq!(multiplier, 500);
    }

    #[test]
    fn 抗性声明只对匹配的伤害类别生效不对其它类别生效() {
        // Arrange：天赋只声明了火抗，查询冰类别应当查不到任何匹配。
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let fire = index(&mut interner, "lostland:fire");
        let cold = index(&mut interner, "lostland:cold");
        let trait_id = index(&mut interner, "lostland:fire_resistance");
        let source = FixedGrants(vec![TraitGrant {
            trait_id,
            unlock_level: 1,
        }]);
        let traits = FixedTraits(vec![(
            trait_id,
            TraitRule {
                rule_modifiers: vec![RuleModifier::Resistance {
                    damage_category: fire,
                    multiplier_permille: 500,
                }],
                ..Default::default()
            },
        )]);

        // Act
        let multiplier = resistance_multiplier_permille(race, 1, &source, &traits, cold);

        // Assert
        assert_eq!(multiplier, RESISTANCE_MULTIPLIER_SCALE);
    }

    #[test]
    fn 多条命中时按trait_id升序取第一条而不是取乘积() {
        // Arrange：两个天赋都声明了对同一类别的抗性，`trait_id` 更小的
        // 那一条（`low`）应当胜出——不是把两个千分比相乘。
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");
        let fire = index(&mut interner, "lostland:fire");
        let low = index(&mut interner, "lostland:fire_resistance");
        let high = index(&mut interner, "lostland:fire_immunity");
        assert!(
            low < high,
            "测试前提：low 先 intern，索引恒小于后 intern 的 high"
        );
        let source = FixedGrants(vec![
            TraitGrant {
                trait_id: low,
                unlock_level: 1,
            },
            TraitGrant {
                trait_id: high,
                unlock_level: 1,
            },
        ]);
        let traits = FixedTraits(vec![
            (
                low,
                TraitRule {
                    rule_modifiers: vec![RuleModifier::Resistance {
                        damage_category: fire,
                        multiplier_permille: 500,
                    }],
                    ..Default::default()
                },
            ),
            (
                high,
                TraitRule {
                    rule_modifiers: vec![RuleModifier::Resistance {
                        damage_category: fire,
                        multiplier_permille: 0,
                    }],
                    ..Default::default()
                },
            ),
        ]);

        // Act
        let multiplier = resistance_multiplier_permille(race, 1, &source, &traits, fire);

        // Assert：500（low 那条），不是 0（high）也不是 0（500*0/1000）。
        assert_eq!(multiplier, 500);
    }
}
