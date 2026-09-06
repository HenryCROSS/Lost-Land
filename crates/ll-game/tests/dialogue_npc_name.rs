//! NPC 姓名的**端到端**验收（ADR 0031：真实 `mods/` 内容、生产路径、
//! 端到端）——批次 34（对话批次 6）。
//!
//! 本文件咬住计划文档
//! `docs/superpowers/plans/2026-09-01-batch34-npc-names.md` 里那几条
//! 「本批必须能证明真的成立」的能力：
//!
//! | 能力 | 本文件里对应的断言 |
//! |---|---|
//! | 姓名真的**确定**（同一个 NPC 两次同名） | `同一个人两次派生出同一个名字` |
//! | 姓名真的**因实体而异**（否则等于没做） | `一座据点里的名字互不相同` |
//! | 切语言只换字形、**不换人** | `两种语言的名字取的是同一串下标` |
//! | 会话屏标题里真的填进了名字 | `会话屏标题里真的是说话人的名字` |
//! | 派生不出来时**诚实回落**，不伪造 | `没有文化归属的人回落到职业显示名` |
//! | 参数化那条键的**拼好的整行**真的排得下 | `会话屏标题在最长的名字下也排得进两行` |
//!
//! 「姓名不进世界状态」那一条**不在本文件**：它的判据是三条黄金基准
//! 一条都不动，由那三条自己守着（本批 A 部分一个字都没重冻它们）。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `populated_determinism.rs`/`npc_materialization.rs` 同一条纪律：
//! 全程不碰 GPU、不模拟键盘，直接调生产路径上的那几个函数。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ll_game::content::{BASE_NAMESPACE, LoadedContent, load_content};
use ll_game::dialogue_screen::{NPC_NAME_ARG, speaker_name};
use ll_game::world::{GameWorld, STREAM_RADIUS_ZONES, build_new_world};
use ll_game::{GamePaths, locale_sources};
use ll_i18n::{Catalog, FluentArgs};
use ll_mod::roster::SettlementRoles;
use ll_world::entity::{Arena, EntityId};
use ll_world::generate::GenParams;
use ll_world::settlement::SettlementStatus;

/// 两种语言都要测——`ll-ui` 的溢出门禁模块文档那条实测结论
/// （英文才是每一条散文型字符串的最坏情况）在这里同样适用。
const LANGUAGES: [&str; 2] = ["zh-CN", "en"];

/// 本文件的世界种子。与 `populated_determinism.rs` 取同一颗，理由相同：
/// 「这个世界里有一座还有人住的据点」这条前提要可复现。
const SEED: u64 = 20260831;

/// 仓库根——`ll-game` 位于 `crates/ll-game`，向上两级。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// 按生产路径装一次真实内容。
fn 真实内容() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功")
}

/// 按生产路径装出 `Catalog`：本体 + 全部带 `locales/` 的 mod。
fn 真实文案() -> Catalog {
    let paths = GamePaths::under(&repo_root());
    Catalog::load(BASE_NAMESPACE, &locale_sources(&paths))
}

/// 造一个**真的住着人**的世界：每一步都是生产路径上的那一个函数，
/// 写法与 `populated_determinism.rs` 的 `populated_world` 一致。
fn 真实世界(content: &LoadedContent) -> GameWorld {
    let mut game_world = build_new_world(
        content,
        GenParams {
            seed: SEED,
            ..GenParams::default()
        },
    )
    .expect("建世界应当成功");
    let roles = SettlementRoles::resolve(
        &content.registry,
        &content.class_table,
        &content.resource_table,
        &content.culture_table,
    );
    let anchor = {
        let chronicle = game_world
            .world
            .terrain
            .chronicle_handle()
            .expect("新游戏必然装了编年史");
        chronicle
            .sites()
            .iter()
            .find(|site| site.status == SettlementStatus::Inhabited && site.population > 0)
            .expect("三百年历史必然留下至少一座还有人住的据点")
            .anchor
    };
    let clock = game_world.world.clock;
    game_world.world.terrain.stream_neighborhood(
        &game_world.noise,
        &game_world.params,
        &content.terrain_ids,
        anchor,
        STREAM_RADIUS_ZONES,
        clock,
    );
    ll_game::world::materialize_nearby_settlements(&mut game_world.world, content, &roles);
    game_world
}

/// 这个世界里全部**物化出来的 NPC**（玩家除外）。
///
/// 「玩家除外」的判据是 `GameWorld::player` 那个 id，不是「有没有文化
/// 归属」——后者恰好是本文件要验的东西，拿它当筛子会把判据变成恒真。
fn 世界里的居民(game_world: &GameWorld) -> Vec<(EntityId, &ll_world::entity::Agent)> {
    game_world
        .world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != game_world.player)
        .collect()
}

/// 造出 `count` 个真实的 `EntityId`。
///
/// `EntityId::new` 是 `pub(crate)`（刻意的：外部只该从 `spawn` 拿到
/// 实体号），因此这里走公开入口 `Arena::spawn`——与生产路径拿到实体号
/// 的方式完全相同。
fn 实体号(count: u32) -> Vec<EntityId> {
    let mut arena: Arena<u32> = Arena::new();
    (0..count).map(|value| arena.spawn(value)).collect()
}

#[test]
fn 本体每一份文化在两种语言下都声明了对齐的音素表() {
    // 这一条排在最前面，因为它是下面几条的**前提**：被断言的对象必须
    // 先存在（本会话反复出现的假绿形状之一是「断言恒绿，因为被断言的
    // 对象根本不存在」）。
    // Arrange
    let content = 真实内容();
    let cultures = &content.culture_table;
    let 样本 = 实体号(1)[0];

    // Assert
    assert!(
        !cultures.registered().is_empty(),
        "本体必须真的注册了文化——空表会让下面每一条断言退化成恒真"
    );
    for kind in cultures.registered() {
        let naming = cultures
            .naming(*kind)
            .unwrap_or_else(|| panic!("文化 {kind:?} 必须声明 naming"));
        for language in LANGUAGES {
            assert!(
                naming.phonemes.contains_key(language),
                "文化 {kind:?} 必须声明 {language} 的音素表——缺一种语言时\
                 rules_for 会静默回落到另一种，玩家会看到一串外文名"
            );
        }
        // 表长对齐由注册期校验保证（装载能走到这里就说明过了那道关），
        // 这里再钉一次「两种语言都真的拼得出名字」。
        for language in LANGUAGES {
            let name = naming
                .rules_for(language)
                .map(|rules| ll_world::naming::given_name(&rules, SEED, 样本))
                .expect("已声明的语言必然取得到规则");
            assert!(
                !name.is_empty(),
                "文化 {kind:?} 在 {language} 下拼出了空名字"
            );
            assert_ne!(
                name, "无名氏",
                "文化 {kind:?} 在 {language} 下拼出了占位名——音素表配错了"
            );
        }
    }
}

#[test]
fn 同一个人两次派生出同一个名字() {
    // Arrange
    let content = 真实内容();
    let catalog = 真实文案();
    let game_world = 真实世界(&content);
    let npcs = 世界里的居民(&game_world);
    assert!(
        !npcs.is_empty(),
        "这条断言的世界里必须真的物化出 NPC——没有就说明物化路径断了，\
         此时下面的比较即使绿也毫无意义"
    );

    // Act & Assert
    for (id, _) in &npcs {
        for language in LANGUAGES {
            let first = speaker_name(*id, &game_world.world, &content, &catalog, language);
            let second = speaker_name(*id, &game_world.world, &content, &catalog, language);
            assert_eq!(
                first, second,
                "{id:?} 在 {language} 下两次算出了不同的名字——姓名必须是纯函数"
            );
        }
    }
}

#[test]
fn 一座据点里的名字互不相同() {
    // 「不同 NPC 不同名，否则等于没做」。判据不是「全部两两不同」——
    // 同一份文化的音素表有限，撞名是可能的、也是可接受的；判据是
    // **不同的名字够多**，也就是实体号真的参与了派生。
    // Arrange
    let content = 真实内容();
    let catalog = 真实文案();
    let game_world = 真实世界(&content);
    let npcs = 世界里的居民(&game_world);
    assert!(npcs.len() >= 4, "一座还有人住的据点至少物化出四个人");

    // Act
    let names: BTreeSet<String> = npcs
        .iter()
        .map(|(id, _)| speaker_name(*id, &game_world.world, &content, &catalog, "zh-CN"))
        .collect();

    // Assert：把「实体号参与派生」这件事量化——若 `given_name` 里那个
    // `entity.as_u64()` 被换成常量，这一整座据点会只剩一个名字。
    assert!(
        names.len() * 2 > npcs.len(),
        "{} 个 NPC 只有 {} 个不同的名字——实体号多半没有参与派生：{names:?}",
        npcs.len(),
        names.len()
    );
}

#[test]
fn 两种语言的名字取的是同一串下标() {
    // `naming-and-localization.md` 三节那条：切语言只换字形、不换人。
    // 判据用一份**构造的**文化（真实内容的两张表不是彼此的大小写变体，
    // 没法直接比对），但走的是生产函数 `CultureNaming::rules_for` +
    // `given_name`。
    // Arrange
    let mut phonemes = std::collections::BTreeMap::new();
    let en = ll_world::naming::PhonemeTables {
        onsets: vec!["thr".into(), "k".into(), "br".into()],
        nuclei: vec!["a".into(), "o".into(), "ai".into()],
        codas: vec!["n".into(), "r".into(), String::new()],
    };
    // 「中文版」逐项取英文版的大写——**同一下标同一个音**这条约束的
    // 一个可逆编码，于是「取的是不是同一串下标」变成一个可判定的问题。
    let 大写 = ll_world::naming::PhonemeTables {
        onsets: en.onsets.iter().map(|p| p.to_uppercase()).collect(),
        nuclei: en.nuclei.iter().map(|p| p.to_uppercase()).collect(),
        codas: en.codas.iter().map(|p| p.to_uppercase()).collect(),
    };
    phonemes.insert("en".to_string(), en);
    phonemes.insert("zh-CN".to_string(), 大写);
    let naming = ll_world::naming::CultureNaming {
        syllables: (2, 3),
        phonemes,
    };
    assert!(naming.problem().is_none(), "这份构造的声明本身必须合法");

    // Act & Assert
    for entity in 实体号(32) {
        let 英文 =
            ll_world::naming::given_name(&naming.rules_for("en").expect("声明了 en"), SEED, entity);
        let 中文 = ll_world::naming::given_name(
            &naming.rules_for("zh-CN").expect("声明了 zh-CN"),
            SEED,
            entity,
        );
        assert_eq!(
            中文.to_lowercase(),
            英文,
            "{entity:?} 在两种语言下取到了不同的下标串——切语言把人换了"
        );
    }
}

#[test]
fn 没有文化归属的人回落到职业显示名() {
    // 玩家就是这一档（`build_player_agent` 刻意不挂任何归属）。
    // Arrange
    let content = 真实内容();
    let catalog = 真实文案();
    let game_world = 真实世界(&content);
    let player = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("新游戏必然有玩家");
    assert!(
        player.affiliations.is_empty(),
        "玩家不挂任何归属是一条裁定（见 build_player_agent）——它若变了，\
         本条测试验的东西就不再是「没有文化归属时会怎样」"
    );

    // Act
    let 名字 = speaker_name(
        game_world.player,
        &game_world.world,
        &content,
        &catalog,
        "zh-CN",
    );
    let 职业名 = catalog.resolve(
        "zh-CN",
        &content
            .class_table
            .get(player.profession)
            .expect("玩家的职业必须已注册")
            .display_name_key
            .to_string(),
    );

    // Assert：回落到职业显示名，而不是占位符、也不是一个凭空捏的名字。
    assert_eq!(名字, 职业名);
}

#[test]
fn 会话屏标题里真的是说话人的名字() {
    // **端到端**：真实内容 + 真实 `.ftl` + 生产的行组装函数
    // （`ll_ui::screen::screen_text_lines` 就是渲染侧与输入侧共用的
    // 那一个）。
    // Arrange
    let content = 真实内容();
    let catalog = 真实文案();
    let game_world = 真实世界(&content);
    let (speaker, _) = *世界里的居民(&game_world)
        .first()
        .expect("这个世界里必须有 NPC");
    let rows: Vec<String> = vec!["（告辞）".to_string()];

    for language in LANGUAGES {
        let 名字 = speaker_name(speaker, &game_world.world, &content, &catalog, language);
        let mut args = FluentArgs::new();
        args.set(NPC_NAME_ARG, 名字.as_str());
        let data = ll_ui::screen::ScreenData {
            title_key: "lostland:dialogue.steward.root",
            title_args: Some(&args),
            rows: &rows,
            cursor: 0,
            empty_key: "screen-dialogue-empty",
            hint_key: "screen-dialogue-hint",
            notice: None,
            hovered: None,
        };

        // Act
        let (lines, _) = ll_ui::screen::screen_text_lines(&data, &catalog, language);

        // Assert：三条，缺一条这条测试就会漏掉一整类回潮。
        let 标题 = &lines[0];
        assert!(
            标题.contains(&名字),
            "{language} 的标题里没有说话人的名字「{名字}」：{标题}"
        );
        assert!(
            !标题.contains(NPC_NAME_ARG),
            "{language} 的标题里还留着参数名——插值根本没有发生：{标题}"
        );
        assert_ne!(
            标题, "lostland:dialogue.steward.root",
            "{language} 的标题回退成了键名——这条键在这种语言下没有文案"
        );
    }
}

#[test]
fn 会话屏标题在最长的名字下也排得进两行() {
    // 溢出门禁那一侧把 `dialogue-steward-root` / `dialogue-guard-root`
    // 归了 `宽度判据::参数化`（拿模式串去量量到的是占位符的宽度）。
    // **参数化必须另有一条对「拼好的整行」的实测断言**，这就是那一条，
    // 分工与 `ll_game::key_hint` 的 `两行提示在两种语言下都排得进一行`
    // 逐字相同。
    //
    // 它比取样更强：名字不是随手挑的，是**上界**——
    // `CultureNaming::longest_name` 按音节数上限、每个位置取最长音素
    // 拼出这份文化在这种语言下能出现的最长名字。
    //
    // 反例（已实测，见计划文档）：把 `dialogue-steward-root` 的 en 文案
    // 加长到溢出，本条当场红。
    // Arrange
    let content = 真实内容();
    let catalog = 真实文案();
    let mut measurer = ll_text::TextMeasurer::new().expect("内置字体资产应能正常解析");
    let 内容宽 = ll_ui::screen::SCREEN_WIDTH - ll_ui::screen::SCREEN_PADDING * 2.0;
    // `dialogue-` 那一类在溢出门禁里的预算：散文两行。这里不重新拍一个
    // 数，直接与那一侧写死同一个 2——两处对不上时，改的人两边都会看到。
    const 行数预算: usize = 2;
    let 台词 = [
        "lostland:dialogue.steward.root",
        "lostland:dialogue.guard.root",
    ];

    let mut 量过 = 0_usize;
    for kind in content.culture_table.registered() {
        let naming = content
            .culture_table
            .naming(*kind)
            .expect("本体每份文化都声明了 naming");
        for language in LANGUAGES {
            let 最长名字 = naming
                .longest_name(language)
                .expect("音素表非空时必然拼得出名字");
            let mut args = FluentArgs::new();
            args.set(NPC_NAME_ARG, 最长名字.as_str());
            for key in 台词 {
                let 整行 = catalog.resolve_with_args(language, key, Some(&args));
                let metrics = ll_text::MeasureText::measure_text(
                    &mut measurer,
                    &整行,
                    ll_ui::screen::SCREEN_FONT_SIZE,
                    ll_ui::screen::SCREEN_LINE_HEIGHT,
                    内容宽,
                );

                // Assert
                assert!(
                    metrics.line_count <= 行数预算,
                    "{key} / {language} 在最长的名字「{最长名字}」（文化 {kind:?}）下排成了 \
                     {} 行，预算 {行数预算} 行：「{整行}」（最宽一行 {}，可用 {内容宽}）",
                    metrics.line_count,
                    metrics.max_line_width
                );
                量过 += 1;
            }
        }
    }

    // 「断言恒绿因为循环一次都没进」这条假绿形状的钉子。
    assert!(
        量过 >= 台词.len() * LANGUAGES.len(),
        "一条都没量到——culture_table.registered() 是空的？"
    );
}
