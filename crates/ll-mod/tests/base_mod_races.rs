//! 端到端验证：本体三个种族真的由 `mods/lostland/races.json5` 注册，
//! 且**逐字段**与迁移前硬编码在 `ll_mod::race::materialize_base_races`
//! 里的那份完全相同。
//!
//! # 这份测试为什么必须存在
//!
//! 本体内容从 Rust 字面量搬进 mod 脚本之后，「这三个种族的数值是多少」
//! 这件事在 Rust 侧**一行代码都不剩**——`ll_mod::race` 的单元测试因此
//! 只能验 `RaceTable` 这套机制，验不了内容本身。若没有本文件，把
//! `races.json5` 里矮人的体质从 2 改成 20 不会让任何一条测试变红（内容
//! 值哈希会变，但那是一个「变了」的信号，不是一个「应该是多少」的
//! 断言）。
//!
//! 本文件把迁移前那份数值逐条钉在这里，充当迁移忠实性的**冻结基准**：
//! 迁移是否忠实由「内容值哈希逐位不变」证明，而这些数值今后是否被
//! 无意改动，由本文件证明。
//!
//! # 与 `example_mod_*.rs` 同一套手法
//!
//! 装载**整个** `mods/` 目录（不是只挑 `mods/lostland/`），理由同
//! `example_mod_starting_items.rs` 模块文档：真实装载路径就是整目录
//! 装载，只挑一个 mod 装会绕过拓扑排序与命名空间共存这两件真实存在的
//! 事。ADR 0018「玩法层内容必须能从 mod 脚本注册，且要有真实 mod 脚本
//! 为证」——本文件是这条判据在**本体自己的内容**上的证据，比
//! `example_mod` 更强：它证明的不是「mod 能注册种族」，而是「本体的
//! 种族除了 mod 脚本之外没有别的来源」。

use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::race::{RaceTable, resolve_base_races};
use ll_mod::registry::Registry;
use ll_world::entity::BaseStats;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `example_mod_items.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 装载真实 `mods/` 目录一次，返回注册表与种族表。
fn load_real_mods() -> (Registry, RaceTable) {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession { registry, race, .. } = session;

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

    (registry, race)
}

#[test]
fn 本体三个种族由本体mod的内容数据文件注册而不是任何rust函数() {
    // 这是「本体即 Mod」在种族上字面意义成立的证据：把 mods/lostland/
    // 从磁盘上拿掉，本体就没有种族——本 crate 里已经不存在任何能凭空
    // 造出这三条内容的 Rust 函数（`resolve_base_races` 只查询、不注册）。
    //
    // **内容的来源换过一次**：三条种族此前由 `races.scm` 的
    // `register-race` 注册，现在由 `races.json5` 经
    // `ll_mod::content_data` 反序列化注册（项目所有者裁定「内容用数据
    // 文件（JSON5），行为用 Rust」）。换来源之后本文件一条断言都不用
    // 改——这本身就是「内容表是唯一真相、注册通道可替换」的证据，也是
    // 内容值哈希逐位不变的直接推论。
    // Arrange
    let (registry, race) = load_real_mods();

    // Act
    let ids = resolve_base_races(&registry, &race).expect("本体 mod 装载后契约必须解析成功");

    // Assert：三个句柄字段各自反查回对应的 id 字符串。
    let resolve = |index| {
        registry
            .resolve(index)
            .map(|id: &NamespacedId| id.to_string())
    };
    assert_eq!(resolve(ids.human), Some("lostland:human".to_string()));
    assert_eq!(resolve(ids.dwarf), Some("lostland:dwarf".to_string()));
    assert_eq!(resolve(ids.elf), Some("lostland:elf".to_string()));
}

/// 把一份 `starting_items`（`(ContentIndex, u32)` 列表）翻成
/// `("完整 id", 件数)` 列表，便于逐条钉住"本体三族各带什么"这条内容
/// 裁定——直接比 `ContentIndex` 会把断言绑死在注册顺序上，那正是
/// `ll_mod::content_hash` 模块文档「`ContentIndex` 字段」一节反复说
/// 的那件事：号本身没有内容含义，id 才有。
fn named_loadout(registry: &Registry, view: &ll_mod::race::RaceView<'_>) -> Vec<(String, u32)> {
    view.starting_items
        .iter()
        .map(|&(def, count)| {
            (
                registry
                    .resolve(def)
                    .expect("出生装备的物品索引必然已注册")
                    .to_string(),
                count,
            )
        })
        .collect()
}

#[test]
fn 人类逐字段与本体races脚本的声明一致() {
    // Arrange
    let (registry, race) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("契约解析");

    // Act
    let view = race.get(ids.human).expect("人类已注册");

    // Assert
    assert_eq!(
        view.display_name_key.to_string(),
        "lostland:race.human.display_name"
    );
    assert_eq!(
        view.stat_modifiers,
        BaseStats {
            strength: 0,
            dexterity: 0,
            constitution: 0,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 0,
        }
    );
    assert_eq!(view.darkvision_cells, 0);
    assert_eq!(view.footprint, (1, 1));
    assert_eq!(view.lifespan_years, 80);
    // 击杀基准经验值——**不再是 0**：项目所有者裁定「有个最低经验
    // 1xp，然后等级差越多给越多」，推翻了「本体三族是可玩种族不是
    // 猎物、刻意不声明」这条旧判断，`mods/lostland/races.json5` 因此
    // 为三族各自声明了基准值（人类 10，矮人/精灵各 12，依据见该文件
    // 末尾注释）。本文件的三条测试是那三个数字唯一的钉子。
    assert_eq!(view.xp_reward, 10);
    assert!(view.traits.is_empty());
    // 出生装备（内容裁定：续航型，合计 base_price 5700）——理由逐条
    // 写在 mods/lostland/races.json5 的「出生装备」一节。
    assert_eq!(
        named_loadout(&registry, &view),
        vec![
            ("lostland:linen_shirt".to_string(), 1),
            ("lostland:roast_meat".to_string(), 3),
        ]
    );
}

#[test]
fn 矮人逐字段与本体races脚本的声明一致() {
    // Arrange
    let (registry, race) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("契约解析");

    // Act
    let view = race.get(ids.dwarf).expect("矮人已注册");

    // Assert
    assert_eq!(
        view.display_name_key.to_string(),
        "lostland:race.dwarf.display_name"
    );
    assert_eq!(
        view.stat_modifiers,
        BaseStats {
            strength: 1,
            dexterity: 0,
            constitution: 2,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 0,
        }
    );
    assert_eq!(view.darkvision_cells, 7);
    assert_eq!(view.footprint, (1, 1));
    assert_eq!(view.lifespan_years, 250);
    // 见人类那条同一处注释。
    assert_eq!(view.xp_reward, 12);
    assert!(view.traits.is_empty());
    // 出生装备（内容裁定：抗寒型，合计 base_price 5600）。
    assert_eq!(
        named_loadout(&registry, &view),
        vec![
            ("lostland:wool_gloves".to_string(), 1),
            ("lostland:fur_pelt".to_string(), 1),
            ("lostland:roast_meat".to_string(), 1),
        ]
    );
}

#[test]
fn 精灵逐字段与本体races脚本的声明一致() {
    // Arrange
    let (registry, race) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("契约解析");

    // Act
    let view = race.get(ids.elf).expect("精灵已注册");

    // Assert
    assert_eq!(
        view.display_name_key.to_string(),
        "lostland:race.elf.display_name"
    );
    assert_eq!(
        view.stat_modifiers,
        BaseStats {
            strength: 0,
            dexterity: 2,
            constitution: 0,
            intelligence: 1,
            willpower: 0,
            charisma: 0,
            luck: 0,
        }
    );
    assert_eq!(view.darkvision_cells, 6);
    assert_eq!(view.footprint, (1, 1));
    assert_eq!(view.lifespan_years, 400);
    // 见人类那条同一处注释。
    assert_eq!(view.xp_reward, 12);
    assert!(view.traits.is_empty());
    // 出生装备（内容裁定：手上功夫型，合计 base_price 5250）。
    assert_eq!(
        named_loadout(&registry, &view),
        vec![
            ("lostland:bone_needle".to_string(), 1),
            ("lostland:linen_cloth".to_string(), 2),
            ("lostland:herbal_draught".to_string(), 1),
            ("lostland:herb_bundle".to_string(), 3),
        ]
    );
}

#[test]
fn 本体种族与mod种族在同一个注册表里共用同一段号段() {
    // 「本体即 Mod」的号段断言：本体内容不占任何预留区间，与
    // example_mod 的种族一样从同一个单调递增的计数器里取号。
    // Arrange
    let (registry, race) = load_real_mods();
    let ids = resolve_base_races(&registry, &race).expect("契约解析");

    // Act
    let half_elf = registry
        .get(&NamespacedId::parse("examplemod:half_elf").expect("合法标识符"))
        .expect("example_mod 应当已注册 half_elf");

    // Assert：两者互不相同，且都能在同一张种族表里查到定义。
    assert_ne!(half_elf, ids.human);
    assert!(race.is_defined(half_elf));
    assert!(race.is_defined(ids.human));
}
