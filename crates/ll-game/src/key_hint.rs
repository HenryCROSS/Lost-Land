//! 世界里那一行常驻的按键提示——**玩家怎么知道能按什么**。
//!
//! # 规格 F6 与它补的那个洞
//!
//! `knowledge/design/ui-and-navigation.md` §9.3 F6 原文：
//!
//! > 今天玩家进世界之后，屏幕上**没有任何一处**告诉他 I/C/空格/M/Tab
//! > 是干什么的。`hud-inventory-menu-hint` 那一行提示只在弹窗**开着**时
//! > 才显示——玩家得先猜对一个键，才看得到其余键的说明。
//!
//! 九块模态屏各有一行 `hint_key`（批次 19 之后两种语言下都完整可见，
//! 见 F5），**世界里一行都没有**。这个洞的形状是「可发现性依赖于已经
//! 发现过」，它自己堵不上自己。
//!
//! # 键名从当前键位表现查，不写死字面量
//!
//! 规格明文：「键名走 `GameKey::display_name_key` 从当前键位表现查，
//! 玩家重绑之后提示跟着变，不写死字面量」。本模块因此**不自己排键名**
//! ——它调 [`crate::settings_view::binding_summary`]，那是「某个动作在
//! `Gameplay` 上下文下绑着哪些键」这条查询在本仓库里唯一的产出点
//! （设置屏那一列键位用的就是它）。写第二份等于让设置屏显示的键名与
//! 提示行显示的键名有两个来源，而两个来源迟早分叉（ADR 0021）。
//!
//! # 文案走 i18n，键名走 Fluent 变量
//!
//! 两条键 `hud-key-hint-world` / `hud-key-hint-map` 各带若干个变量，
//! 变量的值就是现查出来的键名。这也是它们在溢出门禁
//! （`crates/ll-ui/tests/i18n_text_width.rs`）里归「参数化」一类的原因
//! ——拿模板本身去量宽度量出来的是 `{ $inventory }` 这串占位符的宽度，
//! 没有意义。**拼好的整行**的宽度另有断言，见本模块测试。
//!
//! # 地图那一条为什么只有三样，且「方向键」是写死在文案里的
//!
//! 规格给的两行内容是「I 背包　C 制作　空格 交互　M 地图　Tab 菜单」与
//! 「方向键 平移　+/- 缩放　M 关闭」。平移用的是四个方向键，**四个键名
//! 全排出来**（`KeyW / ArrowUp　KeyS / ArrowDown　…`）会让这一行长过
//! 世界那一行两倍，而它要说的其实只有「方向键」这一个概念。因此
//! 「方向键」三个字写在 `.ftl` 文案里（它是一句会被翻译的话，不是硬编码
//! 的键名），缩放与关闭两样仍然现查。**这是规格没裁定的一处取舍**，
//! 记在批次 23 计划文档第八节。

use ll_i18n::{Catalog, FluentArgs};
use ll_platform::input::GameKey;
use ll_platform::keybind::KeyBindings;

use crate::settings_view::binding_summary;

/// 这一刻该显示哪一行提示。
///
/// **只有两种**，因为今天世界层只有两种「玩家在看什么」的形态：世界
/// 本身，和盖在它上面的世界地图。玩家菜单（背包/制作/交互）刻意不单列
/// 一种——那三块弹窗自己底部就有一行 `hud-*-menu-hint`（规格 §9.3 引用
/// 的那一行），再叠一行是重复。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyHintContext {
    /// 玩家在世界里走动。
    World,
    /// 世界地图浮层开着。
    WorldMap,
}

impl KeyHintContext {
    /// 这一行提示的 Fluent 键。
    fn i18n_key(self) -> &'static str {
        match self {
            KeyHintContext::World => "hud-key-hint-world",
            KeyHintContext::WorldMap => "hud-key-hint-map",
        }
    }
}

/// 排出这一刻要显示的那一行按键提示。
///
/// 键名现查（见模块文档），因此玩家在设置屏把「背包」重绑到别的键之后，
/// 下一帧这一行就跟着变——不需要任何缓存失效逻辑。
pub fn key_hint_line(
    context: KeyHintContext,
    bindings: &KeyBindings,
    catalog: &Catalog,
    language: &str,
) -> String {
    let 变量: &[(&str, GameKey)] = match context {
        KeyHintContext::World => &[
            ("inventory", GameKey::Inventory),
            ("craft", GameKey::Craft),
            ("interact", GameKey::Interact),
            ("map", GameKey::Map),
            ("menu", GameKey::Menu),
        ],
        KeyHintContext::WorldMap => &[
            ("zoom_in", GameKey::ZoomIn),
            ("zoom_out", GameKey::ZoomOut),
            ("close", GameKey::Map),
        ],
    };
    let mut args = FluentArgs::new();
    for (name, action) in 变量 {
        args.set(
            (*name).to_string(),
            binding_summary(bindings, *action, catalog, language),
        );
    }
    catalog.resolve_with_args(language, context.i18n_key(), Some(&args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_platform::keybind::{InputContext, KeyBinding, KeyBindings, KeyCode, Modifiers};

    /// 仓库真实的两份 `.ftl`——**不用空 `Catalog`**：空目录下查不到键会
    /// 回落，一切「文案 != 键名」的判据当场变成恒真。
    fn 真实文案() -> Catalog {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
        Catalog::load_one("lostland", &dir)
    }

    #[test]
    fn 世界那一行把五个动作的键名都排进去了() {
        // Arrange
        let catalog = 真实文案();
        let bindings = KeyBindings::default_bindings();

        // Act
        let line = key_hint_line(KeyHintContext::World, &bindings, &catalog, "zh-CN");

        // Assert：五个动作的键名逐个出现在这一行里。**不比整句字面量**
        // ——那会让改一个字的文案改动变成改测试。
        for action in [
            GameKey::Inventory,
            GameKey::Craft,
            GameKey::Interact,
            GameKey::Map,
            GameKey::Menu,
        ] {
            let key_name = binding_summary(&bindings, action, &catalog, "zh-CN");
            assert!(
                !key_name.is_empty(),
                "Arrange：{action:?} 在默认键位表里确实绑着键"
            );
            assert!(
                line.contains(&key_name),
                "提示行里应当出现 {action:?} 的键名 {key_name}，实际是「{line}」"
            );
        }
    }

    #[test]
    fn 玩家把背包重绑到别的键之后提示行跟着变() {
        // 规格 F6 明文点名的那条判据：「把 `Inventory` 重绑到别的键之后，
        // 这一行的文字跟着变」。
        //
        // 反例验证（已实跑）：把 `key_hint_line` 里那句
        // `binding_summary(...)` 换成写死的 `"I".to_string()`，本条当场
        // 红——重绑前后两行完全一样。
        // Arrange
        let catalog = 真实文案();
        let mut bindings = KeyBindings::default_bindings();
        let before = key_hint_line(KeyHintContext::World, &bindings, &catalog, "zh-CN");
        bindings.unbind_action(GameKey::Inventory, InputContext::Gameplay);
        bindings
            .try_bind(KeyBinding {
                key: KeyCode::F7,
                modifiers: Modifiers::NONE,
                context: InputContext::Gameplay,
                action: GameKey::Inventory,
            })
            .expect("F7 在默认表里没被占用");

        // Act
        let after = key_hint_line(KeyHintContext::World, &bindings, &catalog, "zh-CN");

        // Assert
        assert_ne!(before, after, "重绑之后提示行必须跟着变");
        assert!(after.contains("F7"), "新键名要出现在提示行里：{after}");
    }

    #[test]
    fn 两行提示在两种语言下各有各的文案() {
        // 同样防「回落到另一门语言」那条恒绿：断言 en 与 zh 互不相同。
        // Arrange
        let catalog = 真实文案();
        let bindings = KeyBindings::default_bindings();

        // Act & Assert
        for context in [KeyHintContext::World, KeyHintContext::WorldMap] {
            let zh = key_hint_line(context, &bindings, &catalog, "zh-CN");
            let en = key_hint_line(context, &bindings, &catalog, "en");
            assert!(
                !zh.contains(context.i18n_key()),
                "{context:?} 的中文文案必须真的存在，实际拿到「{zh}」"
            );
            assert!(
                !en.contains(context.i18n_key()),
                "{context:?} 的英文文案必须真的存在，实际拿到「{en}」"
            );
            assert_ne!(zh, en, "{context:?} 两种语言各写各的，不是其中一种回落");
        }
    }

    #[test]
    fn 两行提示在两种语言下都排得进一行() {
        // 规格 F6 的判据：「en 与 zh-CN 下都不溢出」。**整行**的宽度只能
        // 在这里量——溢出门禁那一侧按 `hud-key-hint-` 归「参数化」，
        // 它量不到拼好之后的这一行，见本模块文档最后一节。
        //
        // 反例验证（已实跑）：把 `KEY_HINT_WIDTH` 从 620 改成 300，本条
        // 当场红（英文那一行排成 2 行）。
        // Arrange
        let catalog = 真实文案();
        let bindings = KeyBindings::default_bindings();
        let mut measurer = ll_text::TextMeasurer::new().expect("内置字体资产应能正常解析");
        let 内容宽 = ll_ui::hud::content_width(ll_ui::hud::bottom_rows::KEY_HINT_WIDTH);

        // Act & Assert
        for context in [KeyHintContext::World, KeyHintContext::WorldMap] {
            for language in ["zh-CN", "en"] {
                let line = key_hint_line(context, &bindings, &catalog, language);
                let metrics = ll_text::MeasureText::measure_text(
                    &mut measurer,
                    &line,
                    ll_ui::hud::DEFAULT_FONT_SIZE,
                    ll_ui::hud::DEFAULT_LINE_HEIGHT,
                    内容宽,
                );
                assert_eq!(
                    metrics.line_count, 1,
                    "{context:?} / {language} 的提示行应当排成一行，实际 {} 行：「{line}」（宽 {}，可用 {内容宽}）",
                    metrics.line_count, metrics.max_line_width
                );
            }
        }
    }
}
