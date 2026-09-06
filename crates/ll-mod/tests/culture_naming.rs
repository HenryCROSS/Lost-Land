//! 命名规则走**真实解析路径**的验收：一份 `cultures.json5` 文本
//! （第三方 mod 的形状）经 `json5` → `apply_cultures` → `CultureTable`
//! → `given_name`，产出一个真名；对不齐的音素表在**装载期当场被拒**。
//!
//! 写法与 `culture_town_shape.rs` 逐字同构（那一份验的是建筑类型）：
//! 不用任何 Rust 侧夹具直接构造 `CultureAttrs`——那样验不到 schema 与
//! `apply_cultures` 这一段，而 mod 作者写的正是这一段。

use ll_mod::content_schema_world::{CultureFile, apply_cultures};
use ll_mod::registry::Registry;
use ll_world::culture::{CultureKind, CultureTable};
use ll_world::entity::Arena;

/// 把一份 `cultures.json5` 文本走完整路径装进表里。
fn load(source: &str) -> Result<(Registry, CultureTable, Vec<CultureKind>), String> {
    let file: CultureFile = json5::from_str(source).expect("测试文本必须是合法 JSON5");
    let mut registry = Registry::new();
    let mut table = CultureTable::new();
    apply_cultures(&mut registry, &mut table, &file.cultures)?;
    let order = table.registered().to_vec();
    Ok((registry, table, order))
}

/// 一份两种语言按下标对齐的声明——本文件的「正例」。
const ALIGNED: &str = r#"{
  cultures: [
    {
      id: "mymod:skalds",
      display_name_key: "mymod:culture.skalds.display_name",
      economy: "stone",
      home_terrain: "lostland:hill",
      wall_terrain: "lostland:wall_stone",
      founder_races: [ { race: "lostland:human", weight: 1 } ],
      buildings: [ { weight: 1, furniture: [] } ],
      naming: {
        syllables: [2, 3],
        phonemes: {
          "en":    { onsets: ["thr", "k", "br"], nuclei: ["a", "o", "ai"], codas: ["n", "r", ""] },
          "zh-CN": { onsets: ["斯", "克", "布"], nuclei: ["阿", "奥", "艾"], codas: ["恩", "尔", ""] },
        },
      },
    },
  ],
}"#;

fn 实体号(count: u32) -> Vec<ll_world::entity::EntityId> {
    let mut arena: Arena<u32> = Arena::new();
    (0..count).map(|value| arena.spawn(value)).collect()
}

#[test]
fn 加一份文化文本就有自己的取名方式() {
    // Arrange
    let (_, table, order) = load(ALIGNED).expect("这份声明自洽");
    assert_eq!(order.len(), 1, "这份文本声明了一条文化");
    let naming = table.naming(order[0]).expect("naming 必须被装进表里");

    // Act
    let 名字: Vec<String> = 实体号(16)
        .into_iter()
        .map(|entity| {
            ll_world::naming::given_name(&naming.rules_for("en").expect("声明了 en"), 4242, entity)
        })
        .collect();

    // Assert：真的拼出了名字（不是占位名），而且不止一个。
    assert!(
        名字.iter().all(|name| !name.is_empty() && name != "无名氏"),
        "音素表非空却拼出了空名/占位名：{名字:?}"
    );
    assert!(
        名字.iter().collect::<std::collections::BTreeSet<_>>().len() > 1,
        "十六个实体只有一个名字——实体号没有参与派生：{名字:?}"
    );
}

#[test]
fn 两种语言的音素表长度不同时装载期当场拒绝() {
    // `naming-and-localization.md` 三节点名要做成门禁的那条。做在**注册
    // 期**而不是 shell 脚本里：shell 只看得见本仓库的 `mods/`，第三方
    // mod 装载时照样能塞一张对不齐的表。
    // Arrange：中文版比英文版多一个声母。
    let source = ALIGNED.replace(r#"["斯", "克", "布"]"#, r#"["斯", "克", "布", "德"]"#);
    assert_ne!(source, ALIGNED, "改坏必须真的改到了那一行");

    // Act & Assert
    let err = load(&source).expect_err("表长对不齐必须当场拒绝");
    assert!(
        err.contains("en") && err.contains("zh-CN") && err.contains("onsets"),
        "错误信息要说清是哪两种语言的哪一张表对不齐，实际是：{err}"
    );
}

#[test]
fn 一种语言都不声明的命名规则当场拒绝() {
    // Arrange
    let source = ALIGNED
        .replace(
            r#""en":    { onsets: ["thr", "k", "br"], nuclei: ["a", "o", "ai"], codas: ["n", "r", ""] },"#,
            "",
        )
        .replace(
            r#""zh-CN": { onsets: ["斯", "克", "布"], nuclei: ["阿", "奥", "艾"], codas: ["恩", "尔", ""] },"#,
            "",
        );

    // Act & Assert
    let err = load(&source).expect_err("一种语言都没有的命名规则必须当场拒绝");
    assert!(err.contains("语言"), "错误信息应当点名语言：{err}");
}

#[test]
fn 声母表为空的命名规则当场拒绝() {
    // 空表拼不出任何音节，`build_name` 会退回占位名——那是给「mod 配错
    // 了」准备的兜底，不该是装载期静默放行的结果（ADR 0017）。
    // Arrange
    let source = ALIGNED
        .replace(r#"onsets: ["thr", "k", "br"]"#, "onsets: []")
        .replace(r#"onsets: ["斯", "克", "布"]"#, "onsets: []");

    // Act & Assert
    let err = load(&source).expect_err("空声母表必须当场拒绝");
    assert!(
        err.contains("声母") || err.contains("韵腹"),
        "错误信息应当点名是哪一张表空了：{err}"
    );
}

#[test]
fn 本体每一条文化的取名方式互不相同() {
    // 「一份文化一套音素表」这件事在**本体真实内容**里真的成立：
    // 七条文化两两之间至少有一张表不同。判据取「同一个实体在两份文化
    // 下算出不同的名字」——那是玩家真的看得见的差别。
    // Arrange
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/lostland");
    let text = std::fs::read_to_string(root.join("cultures.json5")).expect("本体文化文件必须存在");
    let (_, table, order) = load(&text).expect("本体文化文件必须装载成功");
    assert!(order.len() >= 6, "本体至少六条文化，实际 {}", order.len());
    let entity = 实体号(1)[0];

    // Act
    let 名字: Vec<String> = order
        .iter()
        .map(|kind| {
            let naming = table.naming(*kind).expect("本体每条文化都声明了 naming");
            ll_world::naming::given_name(
                &naming.rules_for("en").expect("本体每条文化都声明了 en"),
                4242,
                entity,
            )
        })
        .collect();

    // Assert：同一个实体在不同文化下叫不同的名字。
    let 去重: std::collections::BTreeSet<&String> = 名字.iter().collect();
    assert_eq!(
        去重.len(),
        名字.len(),
        "有两条文化给同一个人算出了同一个名字，音素表多半抄重了：{名字:?}"
    );
}
