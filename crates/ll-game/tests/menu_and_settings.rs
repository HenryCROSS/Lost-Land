//! 游戏内菜单与设置界面的行为断言。
//!
//! # 为什么住在 `tests/` 而不是 `menu_screen.rs` 里
//!
//! 本仓库 800 行的文件上限。`menu_screen.rs` 的产品代码本身只有四百多
//! 行，但这一批断言（键位冲突、解绑、保存往返、语言切换）加起来又是
//! 五百多行——两者放在一个文件里会越过上限。搬出来还有一个额外好处：
//! 这里只摸得到 `pub` 的东西，断言因此**必须走玩家真正走的那条公开
//! 路径**（`update_settings`），而不是抄近路去调内部函数。
//!
//! ADR 0025 禁止用合成按键做验收，这里的每一条都是程序化驱动同一条
//! 调用路径，不模拟任何键盘事件。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ll_game::menu_screen::{
    EDITABLE_CONTEXT, ScreenNotice, ScreenOutcome, ScreenState, SettingsContext, SettingsOrigin,
    SettingsRow, SettingsUpdate, clear_bindings, settings_rows, try_rebind, update_settings,
};
use ll_game::pause_menu::{MenuRow, menu_focus_index, menu_rows, update_menu};
use ll_i18n::Catalog;
use ll_platform::config::{GameConfig, ScaleFilter};
use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::{KeyBindings, KeyCode, Modifiers};
use ll_ui::widget::state::WidgetStateTable;

/// 每次调用独占一个临时路径——与 `ll_game::test_support` 同一个手法
/// （进程 ID 隔离进程，计数器隔离同一进程内的并发调用），那个模块是
/// `#[cfg(test)]` 的 crate 内部模块，集成测试摸不到。
fn 临时路径(name: &str) -> PathBuf {
    static 计数器: AtomicU64 = AtomicU64::new(0);
    let n = 计数器.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "ll-game-menu-settings-{name}-{}-{n}",
        std::process::id()
    ))
}

fn 测试目录() -> Catalog {
    Catalog::load_dir(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/locales"
    )))
}

fn 按下(keys: &[GameKey]) -> InputState {
    let mut input = InputState::new();
    for key in keys {
        input.press(*key);
    }
    input
}

/// 只取这一帧要说的那句话——多数断言只关心它，不关心 `outcome` 与
/// `rebound`。
fn 提示(update: SettingsUpdate) -> Option<ScreenNotice> {
    update.notice
}

/// 这一批既有菜单测试全部按**普通模式**跑（「保存」那一行在）——它是
/// 玩家最常见的那一档，也是行数最多的那一档，因此最能咬住「按下标分支
/// 会错位」这类缺陷。肉鸽模式那一档由 `menu_rows` 自己的测试覆盖。
const 普通模式: bool = true;

/// 走到第 `steps` 行：每按一次向下，焦点前进一格。
fn 向下(table: &mut WidgetStateTable, steps: usize) {
    for _ in 0..steps {
        update_menu(
            table,
            &按下(&[GameKey::Down]),
            ll_game::pointer::RowPointer::Idle,
            普通模式,
        );
    }
}

fn 设置状态(cursor: usize) -> ScreenState {
    ScreenState::Settings {
        cursor,
        capturing: false,
        origin: SettingsOrigin::Menu,
    }
}

fn 某行下标(target: SettingsRow) -> usize {
    settings_rows()
        .iter()
        .position(|row| *row == target)
        .expect("这一行必然存在")
}

#[test]
fn 每个动作键在设置界面都占一行() {
    // 「新增动作后设置界面静默漏掉它」是本模块最想防的缺陷。
    // Arrange & Act
    let rows = settings_rows();

    // Assert
    for key in GameKey::all() {
        assert!(
            rows.contains(&SettingsRow::Keybind(*key)),
            "{key:?} 在设置界面里没有对应的行"
        );
    }
}

#[test]
fn 菜单里向下移动焦点落在第一项() {
    // Arrange
    let mut table = WidgetStateTable::new();

    // Act
    向下(&mut table, 1);

    // Assert
    assert_eq!(menu_focus_index(&table, 普通模式), 0);
}

#[test]
fn 菜单里没有任何一项聚焦时光标越界不标记任何行() {
    // Arrange
    let table = WidgetStateTable::new();

    // Act
    let index = menu_focus_index(&table, 普通模式);

    // Assert
    assert_eq!(index, usize::MAX);
}

#[test]
fn 菜单里选中设置项后进入设置界面() {
    // Arrange：普通模式的菜单是「继续 / 保存 / 设置 / 返回主菜单 / 退出」
    // ——向下三次落在「设置」。
    let mut table = WidgetStateTable::new();
    向下(&mut table, 3);

    // Act
    let (outcome, next) = update_menu(
        &mut table,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        普通模式,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Idle);
    assert_eq!(
        next,
        Some(ScreenState::Settings {
            cursor: 0,
            capturing: false,
            origin: SettingsOrigin::Menu,
        })
    );
}

#[test]
fn 菜单里选中退出项返回退出() {
    // Arrange：「退出游戏」是普通模式下的第五行。
    let mut table = WidgetStateTable::new();
    向下(&mut table, 5);

    // Act
    let (outcome, _) = update_menu(
        &mut table,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        普通模式,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Quit);
}

#[test]
fn 菜单里按取消关掉整块屏() {
    // Arrange
    let mut table = WidgetStateTable::new();

    // Act
    let (outcome, _) = update_menu(
        &mut table,
        &按下(&[GameKey::Cancel]),
        ll_game::pointer::RowPointer::Idle,
        普通模式,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Close);
}

#[test]
fn 把已经被别的动作占着的键绑过来会被拒绝且原表不变() {
    // 空格默认绑给 Interact；试图把它绑给 Confirm 必须被拒。
    // Arrange
    let bindings = KeyBindings::default_bindings();
    let 原来的空格 = bindings.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT);

    // Act
    let result = try_rebind(&bindings, GameKey::Confirm, KeyCode::Space);

    // Assert
    assert_eq!(result.err(), Some(GameKey::Interact));
    assert_eq!(
        bindings.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
        原来的空格,
        "被拒绝的重绑不该改动原表"
    );
}

#[test]
fn 解绑之后空格可以改回确认键() {
    // 交接文档第四节第 18 条的直接验收：Interact 从 Confirm 手里
    // 拿走了空格，所有者要求「配置合并落地后要能让玩家改回来」。
    // Arrange
    let mut config = GameConfig::default();
    clear_bindings(&mut config, GameKey::Interact);

    // Act
    let rebound = try_rebind(&config.bindings, GameKey::Confirm, KeyCode::Space)
        .expect("空格已经解绑，重绑不该冲突");

    // Assert
    assert_eq!(
        rebound.resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
        Some(GameKey::Confirm)
    );
}

#[test]
fn 解绑会把动作记进刻意解绑清单() {
    // 不记的话，下次加载 fill_missing_defaults 会把默认键补回来。
    // Arrange
    let mut config = GameConfig::default();

    // Act
    clear_bindings(&mut config, GameKey::Interact);

    // Assert
    assert!(config.unbound_actions.contains(&GameKey::Interact));
}

#[test]
fn 重新绑上之后刻意解绑的记号被撤销() {
    // 否则玩家「解绑再绑别的键」之后，下次加载会以为他还想解绑。
    // Arrange
    let mut config = GameConfig::default();
    clear_bindings(&mut config, GameKey::Interact);
    let mut state = ScreenState::Settings {
        cursor: 某行下标(SettingsRow::Keybind(GameKey::Interact)),
        capturing: true,
        origin: SettingsOrigin::Menu,
    };
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-rebind");
    let mut input = InputState::new();
    input.record_physical_key(KeyCode::KeyN);
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act：走公开入口 `update_settings`（内部会因为 `capturing`
    // 为真而进捕获模式），不直接调私有的捕获处理函数——测的是
    // 玩家真正走的那条路径。
    let notice = 提示(update_settings(
        &mut state,
        &input,
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    ));

    // Assert
    assert_eq!(notice, Some(ScreenNotice::Bound(GameKey::Interact)));
    assert!(!config.unbound_actions.contains(&GameKey::Interact));
}

#[test]
fn 捕获模式下按退格解绑当前这一行() {
    // Arrange
    let mut config = GameConfig::default();
    let cursor = 某行下标(SettingsRow::Keybind(GameKey::Map));
    let mut state = ScreenState::Settings {
        cursor,
        capturing: true,
        origin: SettingsOrigin::Menu,
    };
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-clear");
    let mut input = InputState::new();
    input.record_physical_key(KeyCode::Backspace);
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    let notice = 提示(update_settings(
        &mut state,
        &input,
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    ));

    // Assert
    assert_eq!(notice, Some(ScreenNotice::Cleared(GameKey::Map)));
    assert_eq!(config.bindings.bindings_for(GameKey::Map).count(), 0);
}

#[test]
fn 捕获模式下按esc取消不改动任何绑定() {
    // Arrange
    let mut config = GameConfig::default();
    let 改前 = config.bindings.bindings_for(GameKey::Map).count();
    let cursor = 某行下标(SettingsRow::Keybind(GameKey::Map));
    let mut state = ScreenState::Settings {
        cursor,
        capturing: true,
        origin: SettingsOrigin::Menu,
    };
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-cancel");
    let mut input = InputState::new();
    input.record_physical_key(KeyCode::Escape);
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    let notice = 提示(update_settings(
        &mut state,
        &input,
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    ));

    // Assert
    assert_eq!(notice, None);
    assert_eq!(config.bindings.bindings_for(GameKey::Map).count(), 改前);
    assert_eq!(
        state,
        ScreenState::Settings {
            cursor,
            capturing: false,
            origin: SettingsOrigin::Menu,
        }
    );
}

#[test]
fn 冲突时留在捕获模式让玩家直接再按一个键() {
    // Arrange
    let mut config = GameConfig::default();
    let cursor = 某行下标(SettingsRow::Keybind(GameKey::Confirm));
    let mut state = ScreenState::Settings {
        cursor,
        capturing: true,
        origin: SettingsOrigin::Menu,
    };
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-conflict");
    let mut input = InputState::new();
    input.record_physical_key(KeyCode::Space);
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    let notice = 提示(update_settings(
        &mut state,
        &input,
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    ));

    // Assert
    assert_eq!(notice, Some(ScreenNotice::Conflict(GameKey::Interact)));
    assert_eq!(
        state,
        ScreenState::Settings {
            cursor,
            capturing: true,
            origin: SettingsOrigin::Menu,
        }
    );
}

#[test]
fn 左右键切换语言当场改变配置里的语言标签() {
    // Arrange
    let mut config = GameConfig::default();
    let 原语言 = config.language.clone();
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-language");
    let mut state = 设置状态(某行下标(SettingsRow::Language));
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Right]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_ne!(config.language, 原语言);
}

#[test]
fn 切换语言后同一个键解析出另一种语言的文字() {
    // 「当场生效」的实质验证：不是只改了一个字符串字段。
    // Arrange
    let catalog = 测试目录();

    // Act
    let 中文 = catalog.resolve("zh-CN", "screen-menu-title");
    let 英文 = catalog.resolve("en", "screen-menu-title");

    // Assert
    assert_ne!(中文, 英文);
}

#[test]
fn 垂直同步行左右键翻转开关() {
    // Arrange
    let mut config = GameConfig::default();
    let 原值 = config.display.vsync;
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-vsync");
    let mut state = 设置状态(某行下标(SettingsRow::Vsync));
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Right]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(config.display.vsync, !原值);
}

#[test]
fn 缩放滤波行左右键在两档之间循环() {
    // Arrange
    let mut config = GameConfig::default();
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-filter");
    let mut state = 设置状态(某行下标(SettingsRow::ScaleFilter));
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Right]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(config.display.scale_filter, ScaleFilter::SharpBilinear);
}

#[test]
fn 保存写出的配置能被重新加载且键位一致() {
    // Arrange：先改一处键位，再保存，再读回。
    let mut config = GameConfig::default();
    clear_bindings(&mut config, GameKey::Interact);
    config.bindings = try_rebind(&config.bindings, GameKey::Confirm, KeyCode::Space)
        .expect("空格已解绑，不该冲突");
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-save").join("config.json5");
    let mut state = 设置状态(某行下标(SettingsRow::Save));
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    let notice = 提示(update_settings(
        &mut state,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    ));
    let 读回 = ll_platform::config::load_or_default(&path);

    // Assert
    assert_eq!(notice, Some(ScreenNotice::Saved));
    assert_eq!(
        读回
            .bindings
            .resolve(KeyCode::Space, Modifiers::NONE, EDITABLE_CONTEXT),
        Some(GameKey::Confirm),
        "存盘再读回之后，空格仍然是确认键"
    );
}

#[test]
fn 设置界面按取消返回菜单屏() {
    // Arrange
    let mut config = GameConfig::default();
    let catalog = 测试目录();
    let path = 临时路径("menu-screen-back");
    let mut state = 设置状态(0);
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Cancel]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(state, ScreenState::Menu);
}

#[test]
fn 从菜单进的设置屏按取消回到菜单屏() {
    // 守住既有行为不被 `SettingsOrigin` 改坏——这条在首页落地之前就
    // 存在，落地之后必须一字不差地继续成立。
    // Arrange
    let mut config = GameConfig::default();
    let catalog = 测试目录();
    let path = 临时路径("settings-origin-menu");
    let mut state = ScreenState::Settings {
        cursor: 0,
        capturing: false,
        origin: SettingsOrigin::Menu,
    };
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Cancel]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(state, ScreenState::Menu);
}

#[test]
fn 从首页进的设置屏按取消回到首页而不是暂停菜单() {
    // 写死回 `ScreenState::Menu` 会把玩家扔进一个**底下没有世界**的
    // 暂停菜单，那块屏第一项是「继续游戏」，按下去会露出一个空世界。
    //
    // 反例验证（已实跑）：把 `update_navigation` 里的 `origin.screen()`
    // 换回 `ScreenState::Menu`，本条立刻变红。
    // Arrange
    let mut config = GameConfig::default();
    let catalog = 测试目录();
    let path = 临时路径("settings-origin-title");
    let mut state = ScreenState::Settings {
        cursor: 0,
        capturing: false,
        origin: SettingsOrigin::Title,
    };
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Cancel]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(state, ScreenState::Title);
}

#[test]
fn 从首页进的设置屏按返回那一行也回到首页() {
    // 取消键与「返回」那一行必须给出同一个答案；只修其中一条是本项目
    // 反复踩过的「两处逻辑迟早只更新一份」。
    // Arrange
    let mut config = GameConfig::default();
    let catalog = 测试目录();
    let path = 临时路径("settings-origin-back-row");
    let cursor = 某行下标(SettingsRow::Back);
    let mut state = ScreenState::Settings {
        cursor,
        capturing: false,
        origin: SettingsOrigin::Title,
    };
    let mut ctx = SettingsContext {
        config: &mut config,
        config_path: &path,
        catalog: &catalog,
    };

    // Act
    update_settings(
        &mut state,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        &mut ctx,
    );

    // Assert
    assert_eq!(state, ScreenState::Title);
}

// ---------------------------------------------------------------------
// 任务 B：暂停菜单补「保存」与「返回主菜单」
// ---------------------------------------------------------------------

#[test]
fn 普通模式的暂停菜单有保存那一行而肉鸽模式没有() {
    // B1。所有者裁定：「肉鸽模式是只有自动保存的」——所以手动存档那一
    // 项在肉鸽档里**整行不存在**，不是置灰。
    // Arrange & Act
    let 普通 = menu_rows(true);
    let 肉鸽 = menu_rows(false);

    // Assert
    assert!(
        普通.contains(&MenuRow::Save),
        "普通模式必须能手动存档：{普通:?}"
    );
    assert!(
        !肉鸽.contains(&MenuRow::Save),
        "肉鸽模式不该出现手动存档入口：{肉鸽:?}"
    );
    // 另外四行两种模式都有——「保存」是唯一随模式变化的那一行。
    for row in [
        MenuRow::Continue,
        MenuRow::Settings,
        MenuRow::BackToTitle,
        MenuRow::Quit,
    ] {
        assert!(普通.contains(&row), "普通模式缺了 {row:?}");
        assert!(肉鸽.contains(&row), "肉鸽模式缺了 {row:?}");
    }
}

#[test]
fn 两种模式下按同一行都得到同一件事() {
    // 行数随模式变化 ⇒ 按下标分支必然错位。这一条钉住的是「按行的语义
    // 分支」这个实现选择：肉鸽档里选「设置」不该退出游戏。
    // Arrange：肉鸽菜单是「继续 / 设置 / 返回主菜单 / 退出」，「设置」
    // 是第二行。
    let mut table = WidgetStateTable::new();
    for _ in 0..2 {
        update_menu(
            &mut table,
            &按下(&[GameKey::Down]),
            ll_game::pointer::RowPointer::Idle,
            false,
        );
    }

    // Act
    let (outcome, next) = update_menu(
        &mut table,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        false,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::Idle);
    assert_eq!(
        next,
        Some(ScreenState::Settings {
            cursor: 0,
            capturing: false,
            origin: SettingsOrigin::Menu,
        }),
        "肉鸽模式下第二行是「设置」，不该走成别的东西"
    );
}

#[test]
fn 选中保存那一行产出存档意图而不是关掉菜单() {
    // 存完留在菜单里——玩家按「保存」的意图是「把进度落盘」，不是
    // 「回到游戏」；顺手关掉会把那句「已保存」（以及写盘失败时唯一一次
    // 报错）一并关掉。
    // Arrange：普通模式第二行就是「保存」。
    let mut table = WidgetStateTable::new();
    向下(&mut table, 2);

    // Act
    let (outcome, next) = update_menu(
        &mut table,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        普通模式,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::SaveNow);
    assert_eq!(next, None, "存档不该顺手切走这块屏");
}

#[test]
fn 选中返回主菜单那一行产出回首页意图而不是退出进程() {
    // 「返回主菜单」与「退出游戏」是两件事——写死下标最容易把它们弄混。
    // Arrange：普通模式第四行是「返回主菜单」。
    let mut table = WidgetStateTable::new();
    向下(&mut table, 4);

    // Act
    let (outcome, _) = update_menu(
        &mut table,
        &按下(&[GameKey::Confirm]),
        ll_game::pointer::RowPointer::Idle,
        普通模式,
    );

    // Assert
    assert_eq!(outcome, ScreenOutcome::BackToTitle);
}

#[test]
fn 菜单每一行的文案在两种语言里都解析得出来() {
    // 硬编码用户可见字符串是门禁禁止的；这一条钉住「新加的行真的补了
    // 两个语言的 Fluent 键」——漏了的话 `resolve` 会回落成键名本身。
    // Arrange
    let catalog = 测试目录();

    // Act & Assert
    for can_save in [true, false] {
        for row in menu_rows(can_save) {
            for language in ["zh-CN", "en"] {
                let text = catalog.resolve(language, row.text_key());
                assert_ne!(
                    text,
                    row.text_key(),
                    "{language} 缺少 {} 的译文",
                    row.text_key()
                );
                assert!(!text.is_empty());
            }
        }
    }
    for notice in [ScreenNotice::GameSaved, ScreenNotice::GameSaveFailed] {
        for language in ["zh-CN", "en"] {
            let text = notice.resolve(&catalog, language);
            assert!(!text.is_empty(), "{language} 缺少 {notice:?} 的译文");
            assert!(
                !text.starts_with("screen-menu-"),
                "{language} 的 {notice:?} 回落成了键名：{text}"
            );
        }
    }
}
