//! 端到端验证：本体的职业/技能/副职/任务真的由 `mods/lostland/` 下的
//! 脚本注册，且**逐字段**与迁移前硬编码在 `ll_mod::{class,skill,
//! subclass,quest}::materialize_base_*` 里的那份完全相同。
//!
//! # 这份测试为什么必须存在
//!
//! 理由与 `base_mod_races.rs` 逐字相同：本体内容从 Rust 字面量搬进 mod
//! 脚本之后，「战士的主属性是什么、strike 打多少伤害」这件事在 Rust
//! 侧**一行代码都不剩**——各模块的单元测试因此只能验注册表机制，验不了
//! 内容本身。若没有本文件，把 `skills.json5` 里 strike 的伤害从 5 改成
//! 50 不会让任何一条测试变红。
//!
//! 本文件把迁移前那份数值逐条钉在这里，充当迁移忠实性的**冻结基准**。
//!
//! # 与本批次「哈希会变」的关系
//!
//! 种族/卫兵那几批迁移是纯搬家，内容值哈希逐位不变。本批次不是：这
//! 十四条内容（职业 3 + 技能 5 + 副职 2 + 任务 4）此前**从来没有进过
//! 生产装载路径**（`materialize_base_*` 四个函数的唯一调用方是一个
//! 验收 demo 与各模块自己的单元测试，见 `ll_mod::class` 模块文档同名
//! 一节），因此它们第一次被装载进游戏，`lostland` 命名空间的内容值
//! 哈希必然改变。那不是「迁移不忠实」的信号——恰恰相反，本文件逐字段
//! 钉住的数值就是「忠实」这件事的直接证据，哈希的变化只说明这批内容
//! 此前根本不在游戏里。
//!
//! # 与 `example_mod_*.rs` 同一套手法
//!
//! 装载**整个** `mods/` 目录（不是只挑 `mods/lostland/`），理由同
//! `base_mod_races.rs` 模块文档。

use std::path::Path;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_mod::class::{ClassTable, resolve_base_classes};
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::quest::{QuestCondition, QuestTable, resolve_base_quests};
use ll_mod::registry::Registry;
use ll_mod::skill::{
    ResourceCost, ResourceKind, SkillEffect, SkillTable, resolve_base_skills, validate_no_cycles,
};
use ll_mod::subclass::{SubclassTable, resolve_base_subclasses};
use ll_world::entity::AttributeKind;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_races.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回注册表与四张相关内容表。
struct Loaded {
    registry: Registry,
    class: ClassTable,
    skill: SkillTable,
    subclass: SubclassTable,
    quest: QuestTable,
}

fn load_real_mods() -> Loaded {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry,
        class,
        skill,
        subclass,
        quest,
        ..
    } = session;

    let lostland_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == lostland_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的断言毫无意义"
    );

    Loaded {
        registry,
        class,
        skill,
        subclass,
        quest,
    }
}

fn index_of(registry: &Registry, raw: &str) -> ContentIndex {
    registry
        .get(&NamespacedId::parse(raw).expect("合法标识符"))
        .unwrap_or_else(|| panic!("{raw} 必须已被本体内容数据文件注册"))
}

#[test]
fn 本体三个基础职业由本体mod的内容数据文件注册而不是任何rust函数() {
    // 「本体即 Mod」在职业上第一次字面意义成立：把 mods/lostland/ 从
    // 磁盘上拿掉，本体就没有职业——本 crate 里已经不存在任何能凭空造出
    // 这几条内容的 Rust 函数（`resolve_base_classes` 只查询、不注册）。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let ids = resolve_base_classes(&loaded.registry, &loaded.class)
        .expect("本体 mod 装载后职业契约必须解析成功");

    // Assert
    for (index, expected) in [
        (ids.warrior, "lostland:warrior"),
        (ids.mage, "lostland:mage"),
        (ids.ranger, "lostland:ranger"),
    ] {
        assert_eq!(
            loaded.registry.resolve(index).map(|id| id.to_string()),
            Some(expected.to_string())
        );
    }
}

#[test]
fn 本体职业的主属性倾向与显示名键逐条与迁移前一致() {
    // 迁移忠实性的冻结基准，见模块文档。
    // Arrange
    let loaded = load_real_mods();
    let ids = resolve_base_classes(&loaded.registry, &loaded.class).expect("契约解析必须成功");

    // Act & Assert
    for (index, attribute, key) in [
        (
            ids.warrior,
            AttributeKind::Strength,
            "lostland:class.warrior.display_name",
        ),
        (
            ids.mage,
            AttributeKind::Intelligence,
            "lostland:class.mage.display_name",
        ),
        (
            ids.ranger,
            AttributeKind::Dexterity,
            "lostland:class.ranger.display_name",
        ),
    ] {
        let view = loaded.class.get(index).expect("已注册");
        assert_eq!(view.primary_attribute, attribute);
        assert_eq!(
            view.display_name_key,
            &NamespacedId::parse(key).expect("合法标识符")
        );
        assert!(
            view.traits.is_empty(),
            "本体职业不授予任何职业天赋，见 classes.json5 文件头"
        );
    }
}

#[test]
fn 卫兵职业仍然由本体mod的内容数据文件注册且主属性是体质() {
    // 卫兵不进 `BaseClassIds`（Rust 侧没有任何使用点，见
    // `ll_mod::class::BaseClassIds` 文档「哪些内容进」一节），因此它的
    // 存在性只能靠本条按字符串核对——`ll_mod::native_behavior`
    // 的 `(self-has-profession? "lostland:guard")` 依赖它真的在注册表里。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let guard = index_of(&loaded.registry, "lostland:guard");
    let view = loaded.class.get(guard).expect("卫兵必须已被定义");

    // Assert
    assert_eq!(view.primary_attribute, AttributeKind::Constitution);
}

#[test]
fn 本体五条技能的树形结构与全部字段逐条与迁移前一致() {
    // Arrange
    let loaded = load_real_mods();
    let ids =
        resolve_base_skills(&loaded.registry, &loaded.skill).expect("本体技能契约必须解析成功");
    let warrior = index_of(&loaded.registry, "lostland:warrior");

    // Act & Assert：strike——起点，无前置、无冷却、无消耗。
    let strike = loaded.skill.get(ids.strike).expect("已注册");
    assert_eq!(strike.owning_class, Some(warrior));
    assert!(strike.prerequisites.is_empty());
    assert_eq!(strike.cooldown_ticks, 0);
    assert_eq!(strike.resource_cost, ResourceCost::None);
    assert_eq!(strike.effect, SkillEffect::DealDamage { base: 5 });

    // power_strike——分支之一。
    let power_strike = loaded.skill.get(ids.power_strike).expect("已注册");
    assert_eq!(power_strike.owning_class, Some(warrior));
    assert_eq!(power_strike.prerequisites, &[ids.strike]);
    assert_eq!(power_strike.cooldown_ticks, 20);
    assert_eq!(
        power_strike.resource_cost,
        ResourceCost::Amount(ResourceKind::Stamina, 10)
    );
    assert_eq!(power_strike.effect, SkillEffect::DealDamage { base: 12 });

    // brace——分支之二，临时属性修正。
    let brace = loaded.skill.get(ids.brace).expect("已注册");
    assert_eq!(brace.owning_class, Some(warrior));
    assert_eq!(brace.prerequisites, &[ids.strike]);
    assert_eq!(brace.cooldown_ticks, 15);
    assert_eq!(
        brace.resource_cost,
        ResourceCost::Amount(ResourceKind::Stamina, 5)
    );
    assert_eq!(
        brace.effect,
        SkillEffect::TemporaryStatModifier {
            attribute: AttributeKind::Constitution,
            amount: 3,
            duration_ticks: 10,
        }
    );

    // focus——分支之三，**通用技能**（不专属任何职业）。
    let focus = loaded.skill.get(ids.focus).expect("已注册");
    assert_eq!(
        focus.owning_class, None,
        "focus 刻意是通用技能，owning-class 在脚本里传的是空串"
    );
    assert_eq!(focus.prerequisites, &[ids.strike]);
    assert_eq!(focus.cooldown_ticks, 10);
    assert_eq!(focus.resource_cost, ResourceCost::None);
    assert_eq!(
        focus.effect,
        SkillEffect::RestoreResource {
            resource: ResourceKind::Mana,
            base: 8,
        }
    );

    // combo——汇聚，两个前置。
    let combo = loaded.skill.get(ids.combo).expect("已注册");
    assert_eq!(combo.owning_class, Some(warrior));
    assert_eq!(
        combo.prerequisites,
        &[ids.power_strike, ids.brace],
        "combo 要求两个前置同时满足——这一点单靠「树」表达不了"
    );
    assert_eq!(combo.cooldown_ticks, 30);
    assert_eq!(
        combo.resource_cost,
        ResourceCost::Amount(ResourceKind::Stamina, 15)
    );
    assert_eq!(combo.effect, SkillEffect::DealDamage { base: 20 });
}

#[test]
fn 真实mods目录装载出来的技能表无环() {
    // 这条检查现在真的会在生产装载路径上跑（`ll_game::content::
    // load_content`），本条是它在真实内容上的正面证据；反面证据（把
    // 一个成环的 mod 放进来会让装载整批失败）在
    // `crates/ll-game/tests/prerequisite_graph_gate.rs`。
    // Arrange
    let loaded = load_real_mods();

    // Act & Assert
    assert!(validate_no_cycles(&loaded.skill).is_ok());
}

#[test]
fn 本体两条基础副职由本体mod的内容数据文件注册() {
    // Arrange
    let loaded = load_real_mods();

    // Act
    let ids = resolve_base_subclasses(&loaded.registry, &loaded.subclass)
        .expect("本体副职契约必须解析成功");

    // Assert
    for (index, expected_id, expected_key) in [
        (
            ids.duelist,
            "lostland:duelist",
            "lostland:subclass.duelist.display_name",
        ),
        (
            ids.apprentice,
            "lostland:apprentice",
            "lostland:subclass.apprentice.display_name",
        ),
    ] {
        assert_eq!(
            loaded.registry.resolve(index).map(|id| id.to_string()),
            Some(expected_id.to_string())
        );
        assert_eq!(
            loaded.subclass.get(index).expect("已注册").display_name_key,
            &NamespacedId::parse(expected_key).expect("合法标识符")
        );
    }
}

#[test]
fn 剑舞者与学徒不声明获得条件这是一条写下来的已知缺口() {
    // 见 `mods/lostland/subclasses.json5` 文件头：`Effect::GrantSubclass`
    // 在整个 `ll-sim` 里只有制作计数达标那一个产出点，而
    // `register-subclass-unlock` 的 trigger-kind 至今只接受
    // "items-crafted"——给一个近战/魔法副职配「做满 N 件东西」的获得
    // 条件是荒唐的，因此本体刻意不给它们编造获得条件。
    //
    // 本条测试把这个**已知缺口**钉成一条可执行的断言，而不是只写在
    // 注释里：哪天有人给它们补上了获得条件（或者新增了合适的触发器
    // 种类），本条会变红，逼人回来更新那段文档，而不是让一段过时的
    // 「已知缺口」说明无声地留在仓库里。
    // Arrange
    let loaded = load_real_mods();
    let ids =
        resolve_base_subclasses(&loaded.registry, &loaded.subclass).expect("契约解析必须成功");

    // Act & Assert
    for index in [ids.duelist, ids.apprentice] {
        assert!(
            loaded.subclass.craft_unlock(index).is_none(),
            "本体这两条副职当前不声明任何获得条件，见 subclasses.json5 文件头"
        );
    }
}

#[test]
fn 本体四条任务的网状结构与两档完成条件逐条与迁移前一致() {
    // Arrange
    let loaded = load_real_mods();
    let ids =
        resolve_base_quests(&loaded.registry, &loaded.quest).expect("本体任务契约必须解析成功");
    let goblin = index_of(&loaded.registry, "lostland:goblin");

    // Act & Assert：起点，无前置，击杀 3。
    let main = loaded.quest.get(ids.main_quest_1).expect("已注册");
    assert!(main.prerequisites.is_empty());
    assert_eq!(
        main.condition,
        &QuestCondition::KillCount {
            target_kind: goblin,
            count: 3,
        }
    );

    // branch_a：一档条件，击杀 5。
    let branch_a = loaded.quest.get(ids.branch_a).expect("已注册");
    assert_eq!(branch_a.prerequisites, &[ids.main_quest_1]);
    assert_eq!(
        branch_a.condition,
        &QuestCondition::KillCount {
            target_kind: goblin,
            count: 5,
        }
    );

    // branch_b：三档条件（脚本回调标识符）。
    let branch_b = loaded.quest.get(ids.branch_b).expect("已注册");
    assert_eq!(branch_b.prerequisites, &[ids.main_quest_1]);
    assert_eq!(
        branch_b.condition,
        &QuestCondition::Script(
            NamespacedId::parse("lostland:branch_b_condition").expect("合法标识符")
        )
    );

    // finale：两个前置同时满足——这张图因此不是树。
    let finale = loaded.quest.get(ids.finale).expect("已注册");
    assert_eq!(finale.prerequisites, &[ids.branch_a, ids.branch_b]);
    assert_eq!(
        finale.condition,
        &QuestCondition::KillCount {
            target_kind: goblin,
            count: 1,
        }
    );
}

#[test]
fn 本体技能与任务的id清单不多不少就是脚本里注册的那几条() {
    // 防「多注册了一条没人知道的内容」：契约解析只保证**至少**有那
    // 几条，本条守另一头——lostland 命名空间下的技能/任务条目数恰好
    // 等于脚本里写的条数。多写一条 register-skill 忘了更新句柄结构体，
    // 本条会红。
    // Arrange
    let loaded = load_real_mods();

    // Act
    let count_in_namespace = |is_defined: &dyn Fn(ContentIndex) -> bool| {
        loaded
            .registry
            .snapshot()
            .iter()
            .filter(|id| id.namespace() == "lostland")
            .filter(|id| loaded.registry.get(id).is_some_and(is_defined))
            .count()
    };

    // Assert
    assert_eq!(
        count_in_namespace(&|index| loaded.skill.is_defined(index)),
        5,
        "mods/lostland/skills.json5 注册五条技能"
    );
    assert_eq!(
        count_in_namespace(&|index| loaded.quest.is_defined(index)),
        4,
        "mods/lostland/quests.json5 注册四条任务"
    );
    assert_eq!(
        count_in_namespace(&|index| loaded.class.is_defined(index)),
        13,
        "mods/lostland/classes.json5 注册十三条职业（战士/法师/游侠/卫兵 +          据点管理者/民兵/农夫/猎户/屠夫/铁匠/渔夫/牧羊人/石匠）"
    );
    assert_eq!(
        count_in_namespace(&|index| loaded.subclass.is_defined(index)),
        6,
        "mods/lostland/subclasses.json5 注册六条副职（四条制作类 + 剑舞者/学徒）"
    );
}
