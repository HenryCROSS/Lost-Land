//! mod 自带 `.ftl` 的**端到端**验收：走生产装载路径
//! （`ll_game::locale_sources` → `ll_i18n::Catalog::load`），读仓库里
//! 真实的 `mods/` 与 `assets/locales/`，不用任何临时夹具。
//!
//! 本文件咬住的是 `knowledge/design/dialogue-system.md` 三节 3.2 点名的
//! 两个**互相独立**的致命缺口：
//!
//! | | 缺口 | 本文件里对应的断言 |
//! |---|---|---|
//! | ① | mod 的 `.ftl` 根本没有被读过 | `示例模组的键解析出它自己的文案` |
//! | ② | 命名空间被剥掉，跨 mod 撞键静默覆盖 | `同名键在两个命名空间下互不覆盖`、`裸键恒定落到本体命名空间` |
//!
//! 反例验证（ADR 0022）：把 `locale_sources` 里遍历 `mods/*/locales/`
//! 那一段去掉 ⇒ 第一条红；把 `ll_i18n::split_key` 的命名空间分流改回
//! 「剥掉前缀」⇒ 后两条红。两条都实测过。

use std::path::{Path, PathBuf};

use ll_game::content::BASE_NAMESPACE;
use ll_game::{GamePaths, locale_sources};
use ll_i18n::Catalog;

/// 仓库根——`ll-game` 位于 `crates/ll-game`，向上两级。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// 按生产路径装出 `Catalog`：本体 + 全部带 `locales/` 的 mod。
fn 真实装载() -> Catalog {
    let paths = GamePaths::under(&repo_root());
    Catalog::load(BASE_NAMESPACE, &locale_sources(&paths))
}

#[test]
fn 示例模组的键解析出它自己的文案而不是键名() {
    // 缺口 ① 的端到端断言。`mods/example_mod/` 是「本体即 Mod」唯一的
    // 活证据；在本批之前它的每一条 display_name_key 都退化成键名，因为
    // 全仓库没有任何代码会去读它的 locales/（那个目录当时也不存在）。
    // Arrange
    let catalog = 真实装载();

    // Act
    let zh = catalog.resolve("zh-CN", "examplemod:half_elf_display_name");
    let en = catalog.resolve("en", "examplemod:half_elf_display_name");

    // Assert
    assert_eq!(zh, "半精灵");
    assert_eq!(en, "Half-Elf");
    assert_ne!(zh, "examplemod:half_elf_display_name", "退化成键名了");
    assert_ne!(zh, en);
}

#[test]
fn 示例模组各类内容的键都真的有译文() {
    // 上一条只证明「有一条通了」。这一条铺开到六类内容（职业/物品/
    // 配方/种族/天赋/天气），其中天气那条的键路径带点号
    // （`weather.ashfall.display_name`），顺带咬住「点号换连字符」这一步
    // 在 mod 的键上同样生效。
    // Arrange
    let catalog = 真实装载();
    let keys = [
        "examplemod:necromancer_display_name",
        "examplemod:iron_sword_display_name",
        "examplemod:herb_stew_recipe_display_name",
        "examplemod:dragonborn_display_name",
        "examplemod:shadow_dance_display_name",
        "examplemod:weather.ashfall.display_name",
    ];

    // Act & Assert：try_resolve 精确查找，缺译文就是 None——不走语言
    // 回退链，否则「只缺了中文」会被一句英文糊过去。
    for key in keys {
        for language in ["zh-CN", "en"] {
            assert!(
                catalog.try_resolve(language, key).is_some(),
                "键 {key} 在 example_mod 的 {language}.ftl 里没有译文"
            );
        }
    }
}

#[test]
fn 同名键在两个命名空间下互不覆盖() {
    // 缺口 ② 的端到端断言。`race-elf-display_name` 这个消息 id 在本体
    // 与示例 mod 的 .ftl 里**同时存在**（示例 mod 那一条是刻意留的撞键
    // 回归夹具，见 mods/example_mod/locales/zh-CN.ftl 末尾的注释）。
    //
    // 命名空间维度落地之前：两条折成同一个 Fluent id，而 mod 恒在本体
    // 之后装载——一个第三方 mod 可以**不声不响**地改掉游戏本体的文案，
    // 没有任何东西会报错。
    // Arrange
    let catalog = 真实装载();

    // Act
    let 本体 = catalog.resolve("zh-CN", "lostland:race.elf.display_name");
    let 模组 = catalog.resolve("zh-CN", "examplemod:race.elf.display_name");

    // Assert
    assert_eq!(本体, "精灵", "本体的文案被 mod 覆盖了");
    assert_eq!(模组, "示例模组的精灵", "mod 的文案被本体覆盖了");
    assert_ne!(本体, 模组);
}

#[test]
fn 裸键恒定落到本体命名空间() {
    // 撞键的另一半，也是更危险的那一半：`hud-inventory-empty` 是**引擎
    // 自己**的 HUD 文案，调用方用的是不带命名空间前缀的裸键。示例 mod
    // 的 .ftl 里同样有一条同 id 的条目（同一份撞键夹具）。裸键必须恒定
    // 落到本体命名空间，任何 mod 都不该能劫持它。
    // Arrange
    let catalog = 真实装载();

    // Act
    let zh = catalog.resolve("zh-CN", "hud-inventory-empty");
    let en = catalog.resolve("en", "hud-inventory-empty");

    // Assert
    assert_eq!(zh, "（空）");
    assert_eq!(en, "(empty)");
}

#[test]
fn 本体的键在装了模组之后仍然逐条正确() {
    // 「加了一维之后本体自己还对不对」——本体现在与任何 mod 走同一条
    // 装载路径（`locale_sources` 的第一条），这条是它没走坏的证据。
    // Arrange
    let catalog = 真实装载();

    // Act & Assert
    assert_eq!(catalog.resolve("zh-CN", "window.title"), "迷途大陆");
    assert_eq!(
        catalog.resolve("zh-CN", "lostland:race.human.display_name"),
        "人类"
    );
    assert_eq!(
        catalog.languages(),
        vec!["en".to_string(), "zh-CN".to_string()]
    );
}

#[test]
fn 真实装载里示例模组的本地化确实被装进来了() {
    // 直接对装载来源本身断言：不经查表，先证明那条来源真的在列表里。
    // 这样「一条查表断言红了」时能一眼分清是「没装」还是「装了但查错」。
    // Arrange
    let paths = GamePaths::under(&repo_root());

    // Act
    let sources = locale_sources(&paths);
    let namespaces: Vec<&str> = sources.iter().map(|s| s.namespace.as_str()).collect();

    // Assert
    assert_eq!(
        namespaces.first(),
        Some(&BASE_NAMESPACE),
        "本体应当排在第一条（可读性约定，不是查表规则）"
    );
    assert!(
        namespaces.contains(&"examplemod"),
        "示例 mod 的 locales/ 没有被发现，实测 {namespaces:?}"
    );
}
