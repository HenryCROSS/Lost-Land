//! 设置界面与菜单屏的**排版**：把一行设置变成一句可以直接画出来的字。
//!
//! # 为什么与 [`crate::menu_screen`] 分开
//!
//! 两件事的形状完全不同：那个模块回答「按下这一行会发生什么」（读
//! 输入、改配置、切状态），本模块回答「这一行现在该显示成什么字」
//! （只读，纯函数，一个字节的状态都不改）。合在一个文件里，那个文件
//! 会越过本仓库 800 行的上限，而且「改配置」与「显示配置」两类函数
//! 混排会让评审很难一眼看出哪些函数有副作用。
//!
//! # 全部走 i18n，一个用户可见字符串都不硬编
//!
//! 规格 §11.3 与 `scripts/ci/check_i18n_strings.py` 那道门禁。唯一的
//! 例外是**物理键名**（`KeyW`/`Space`），理由见 [`binding_summary`]。

use ll_i18n::{Catalog, FluentArgs};
use ll_platform::config::{GameConfig, ScaleFilter};
use ll_platform::input::GameKey;
use ll_platform::keybind::KeyBindings;

use crate::menu_screen::{EDITABLE_CONTEXT, MENU_ITEM_KEYS, SettingsRow};

/// 某个动作在 [`EDITABLE_CONTEXT`] 下当前绑着哪些键，排好版成一行。
///
/// 键名用 `KeyCode` 的 `Debug` 形态（`KeyW`/`Space`/`ArrowUp`）——如实
/// 记录这是一处**临时取舍**：给两百多个 `KeyCode` 变体各配一条 i18n
/// 键是一笔与本批次目标无关的大工程，而物理键名在绝大多数游戏里本来
/// 就不翻译（键帽上印的就是这些字母）。真要本地化，加法是在
/// `ll-platform` 给 `KeyCode` 配一张 `display_name_key` 表，本函数改成
/// 查那张表，其余一行不动。
pub fn binding_summary(
    bindings: &KeyBindings,
    action: GameKey,
    catalog: &Catalog,
    language: &str,
) -> String {
    let keys: Vec<String> = bindings
        .bindings_for(action)
        .filter(|binding| binding.context == EDITABLE_CONTEXT)
        .map(|binding| format!("{:?}", binding.key))
        .collect();
    if keys.is_empty() {
        return catalog.resolve(language, "screen-settings-unbound");
    }
    keys.join(" / ")
}

/// 某种语言在**它自己的语言**里叫什么（endonym）。
///
/// 查的是 `language-name` 这条键在**那一份** `.ftl` 里的取值，不是在
/// 当前显示语言里的取值——语言选单上每一项都用自己的文字写，是这类
/// 界面的通行做法（玩家看不懂当前语言时，恰恰要靠这一列找回自己的
/// 语言）。查不到时退回语言标签本身（`Catalog::resolve` 找不到键会
/// 原样返回键名，那个键名对玩家毫无意义，退回标签更诚实）。
pub fn language_display_name(catalog: &Catalog, tag: &str) -> String {
    let name = catalog.resolve(tag, "language-name");
    if name == "language-name" {
        tag.to_string()
    } else {
        name
    }
}

/// 把一行排成「标签：取值」——分隔符走 i18n 模板（`screen-settings-row`）
/// 而不是在代码里拼一个冒号：中文用全角「：」、英文用半角「: 」，写死
/// 任何一种都会在另一种语言下看起来是错的。
fn labeled_row(catalog: &Catalog, language: &str, label_key: &str, value: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("label", catalog.resolve(language, label_key));
    args.set("value", value.to_string());
    catalog.resolve_with_args(language, "screen-settings-row", Some(&args))
}

/// 把设置界面这一帧的每一行排好版。
pub fn settings_row_texts(
    rows: &[SettingsRow],
    config: &GameConfig,
    catalog: &Catalog,
    capturing: bool,
    capture_row: usize,
) -> Vec<String> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            settings_row_text(*row, config, catalog, capturing && index == capture_row)
        })
        .collect()
}

/// 排一行。每种行各自一个小函数，不把七个分支的取数逻辑摞在一个
/// `match` 里——那正好会越过本仓库 50 行的函数上限。
pub fn settings_row_text(
    row: SettingsRow,
    config: &GameConfig,
    catalog: &Catalog,
    capturing_this_row: bool,
) -> String {
    let language = config.language.as_str();
    match row {
        SettingsRow::Language => labeled_row(
            catalog,
            language,
            "screen-settings-language",
            &language_display_name(catalog, language),
        ),
        SettingsRow::Vsync => vsync_row(config, catalog, language),
        SettingsRow::ScaleFilter => scale_filter_row(config, catalog, language),
        SettingsRow::KeybindsHeader => catalog.resolve(language, "screen-settings-keybinds-header"),
        SettingsRow::Keybind(action) => {
            keybind_row(action, config, catalog, language, capturing_this_row)
        }
        SettingsRow::Save => catalog.resolve(language, "screen-settings-save"),
        SettingsRow::Back => catalog.resolve(language, "screen-settings-back"),
    }
}

/// 垂直同步那一行——取值后面**恒跟一句「重启后生效」**：这一项与另外
/// 两项不同，改完不会当场变化（`vsync` 只在 `GpuContext::new` 时决定
/// 呈现模式）。不说这一句，玩家会以为设置没生效。
fn vsync_row(config: &GameConfig, catalog: &Catalog, language: &str) -> String {
    let value = catalog.resolve(
        language,
        if config.display.vsync {
            "screen-settings-on"
        } else {
            "screen-settings-off"
        },
    );
    let restart = catalog.resolve(language, "screen-settings-restart-required");
    labeled_row(
        catalog,
        language,
        "screen-settings-vsync",
        &format!("{value} {restart}"),
    )
}

/// 缩放滤波那一行——两档，改完当场生效。
fn scale_filter_row(config: &GameConfig, catalog: &Catalog, language: &str) -> String {
    let value = catalog.resolve(
        language,
        match config.display.scale_filter {
            ScaleFilter::Nearest => "screen-settings-filter-nearest",
            ScaleFilter::SharpBilinear => "screen-settings-filter-sharp-bilinear",
        },
    );
    labeled_row(catalog, language, "screen-settings-scale-filter", &value)
}

/// 一个动作的键位那一行；正处于捕获模式的那一行显示「……请按键……」
/// 而不是当前绑定——玩家必须能看出「现在这一行在等我按键」。
fn keybind_row(
    action: GameKey,
    config: &GameConfig,
    catalog: &Catalog,
    language: &str,
    capturing_this_row: bool,
) -> String {
    let value = if capturing_this_row {
        catalog.resolve(language, "screen-settings-capturing")
    } else {
        binding_summary(&config.bindings, action, catalog, language)
    };
    labeled_row(
        catalog,
        language,
        &action.display_name_key().to_string(),
        &value,
    )
}

/// 菜单屏这一帧的三行文字。
pub fn menu_row_texts(catalog: &Catalog, language: &str) -> Vec<String> {
    MENU_ITEM_KEYS
        .iter()
        .map(|key| catalog.resolve(language, key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_screen::clear_bindings;
    use std::path::Path;

    fn 测试目录() -> Catalog {
        Catalog::load_dir(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/locales"
        )))
    }

    #[test]
    fn 未绑定的动作在设置界面显示成未绑定而不是空白() {
        // 空白会让玩家分不清「没绑」与「这一行坏了」。
        // Arrange
        let mut config = GameConfig::default();
        clear_bindings(&mut config, GameKey::Map);
        let catalog = 测试目录();

        // Act
        let text = binding_summary(&config.bindings, GameKey::Map, &catalog, &config.language);

        // Assert
        assert_eq!(
            text,
            catalog.resolve(&config.language, "screen-settings-unbound")
        );
    }

    #[test]
    fn 键位行同时列出一个动作的多个绑定() {
        // Up 默认同时绑着 ArrowUp 与 KeyW，只显示一个会让玩家以为丢了。
        // Arrange
        let config = GameConfig::default();
        let catalog = 测试目录();

        // Act
        let text = binding_summary(&config.bindings, GameKey::Up, &catalog, &config.language);

        // Assert
        assert!(text.contains("ArrowUp"), "实际是：{text}");
        assert!(text.contains("KeyW"), "实际是：{text}");
    }
}
