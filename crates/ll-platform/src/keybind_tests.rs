//! `super::keybind` 的测试——用 `#[path]` 挂成它的 `tests` 子模块。
//!
//! 从 `keybind.rs` 原样搬出来的，一个字没改。理由见挂载点的注释：那个
//! 文件在行数棘轮的快照里，而它承担的四件事（键位词汇表、三张默认绑定
//! 表、`KeyBindings` 类型、默认值合并算法）没有一件是「八百行测试」。

use super::*;

#[test]
fn 文本输入态下游戏按键解析不出任何动作() {
    // 「打字打出个 W 不该让角色往上走」这条要求的直接断言。它靠的
    // 不是某处 `if` 记得跳过，而是 `DEFAULT_TEXT_ENTRY_BINDINGS`
    // 里根本没有这些键——路径上产不出动作。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act & Assert
    for key in [
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::ArrowUp,
        KeyCode::KeyI,
        KeyCode::KeyG,
        // 空格在菜单里是确认，在文本框里必须是一个字符。
        KeyCode::Space,
    ] {
        assert_eq!(
            table.resolve(key, Modifiers::NONE, InputContext::TextEntry),
            None,
            "{key:?} 在文本输入态下不该解析出任何动作"
        );
    }
}

#[test]
fn 文本输入态下确认与取消仍然解析得到() {
    // 上一条只证明了「文本输入态不是游戏内/菜单上下文」；这一条
    // 证明那张表真的被查到了，而不是整个解析失效——玩家得能提交
    // 和退出。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act & Assert
    assert_eq!(
        table.resolve(KeyCode::Enter, Modifiers::NONE, InputContext::TextEntry),
        Some(GameKey::Confirm)
    );
    assert_eq!(
        table.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::TextEntry),
        Some(GameKey::Cancel)
    );
}

#[test]
fn 已有配置文件的玩家也能补到文本输入态那两条默认绑定() {
    // 这是 `crate::config` 模块文档里那条缺陷的预防：新增一张默认
    // 表却忘了接进 `fill_missing_defaults`，写过配置文件的玩家就
    // **永久静默地**拿不到它。
    // Arrange：一份只含游戏内绑定的老配置。
    let 老配置 = KeyBindings::from_bindings([KeyBinding::gameplay(KeyCode::ArrowUp, GameKey::Up)])
        .expect("单条绑定不会冲突");
    assert_eq!(
        老配置.resolve(KeyCode::Enter, Modifiers::NONE, InputContext::TextEntry),
        None,
        "前置条件：老配置里本来就没有这两条"
    );

    // Act
    let 补齐后 = 老配置.fill_missing_defaults(&[]);

    // Assert
    assert_eq!(
        补齐后.resolve(KeyCode::Enter, Modifiers::NONE, InputContext::TextEntry),
        Some(GameKey::Confirm)
    );
    assert_eq!(
        补齐后.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::TextEntry),
        Some(GameKey::Cancel)
    );
}

#[test]
fn 默认绑定表能解析方向键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::Up));
}

#[test]
fn 默认绑定表里字母键位也映射到同一方向() {
    // 传统 Roguelike 玩家习惯 WASD，方向键与字母键应解析到同一动作。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::KeyW, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::Up));
}

#[test]
fn 默认绑定表能解析截图键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::F2, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::Screenshot));
}

#[test]
fn 菜单与文本输入上下文下截图键同样解析得出来() {
    // 规格 N13 / D9：`GameKey::Screenshot` 此前**只绑 `Gameplay`**，
    // 一开模态屏上下文就切到 `Menu`，F2 再也解析不出任何动作。而
    // `crate::input::GameKey::Screenshot` 的文档明说这个键是「冻结
    // 视觉回归基准的入口，不是调试功能」——结果所有 UI 屏都进不了
    // 视觉回归基准。
    //
    // 反例验证（已实跑，且**这条在落地前就是红的**，符合规格标注）：
    // 把 `DEFAULT_MENU_BINDINGS` 里那条 F2 删掉，本条红在菜单上下文
    // 那一行（`None` 而不是 `Some(Screenshot)`）。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act & Assert：三个上下文都要解析得出来——截图是「任何时候都
    // 能按」的那一类，不是某个上下文的功能。
    for context in [
        InputContext::Gameplay,
        InputContext::Menu,
        InputContext::TextEntry,
    ] {
        assert_eq!(
            table.resolve(KeyCode::F2, Modifiers::NONE, context),
            Some(GameKey::Screenshot),
            "{context:?} 上下文下 F2 解析不出截图键"
        );
    }
}

#[test]
fn 菜单上下文下菜单键同样解析得出来() {
    // 规格 N13 / D8：Tab 此前**只绑 `Gameplay`**——能开菜单，开完
    // 上下文就变成 `Menu`，Tab 再也解析不出任何动作。开关键不对称。
    //
    // **文本输入上下文刻意不补 Tab**：规格原文「它将来是『跳到下一个
    // 输入框』」。这里连带断言它今天确实**没有**绑——不留「到底是
    // 漏了还是刻意的」这种歧义。
    //
    // 反例验证（已实跑，且**这条在落地前就是红的**）：把
    // `DEFAULT_MENU_BINDINGS` 里那条 Tab 删掉，本条红在菜单上下文。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act & Assert
    assert_eq!(
        table.resolve(KeyCode::Tab, Modifiers::NONE, InputContext::Gameplay),
        Some(GameKey::Menu)
    );
    assert_eq!(
        table.resolve(KeyCode::Tab, Modifiers::NONE, InputContext::Menu),
        Some(GameKey::Menu),
        "菜单开着按 Tab 解析不出菜单键——开关键不对称（规格 D8）"
    );
    assert_eq!(
        table.resolve(KeyCode::Tab, Modifiers::NONE, InputContext::TextEntry),
        None,
        "文本输入态刻意不绑 Tab：它将来是「跳到下一个输入框」（规格 N13）"
    );
}

#[test]
fn 默认绑定表能解析取消键() {
    // demo 与后续的「退出游戏」菜单都依赖这条映射，它曾经是全项目
    // 唯一映射却无人消费的死映射（见 `window.rs` 此前的同名测试）。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::Cancel));
}

#[test]
fn 未绑定的键解析为空值() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::F13, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, None);
}

#[test]
fn 不同上下文下未注册的组合解析为空值() {
    // 目前只有一个上下文取值，这里验证「上下文不匹配」本身确实会
    // 让 resolve 查不到——防止未来新增上下文变体时，判重逻辑悄悄
    // 退化成只比较键位而忽略上下文字段。
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind(KeyBinding {
            key: KeyCode::KeyQ,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Menu,
        })
        .expect("首次绑定不冲突");

    // Act：同一个键，但没有为之注册过的假想上下文用不同修饰键代替
    // 以制造一个确定查不到的组合。
    let action = table.resolve(
        KeyCode::KeyQ,
        Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        },
        InputContext::Gameplay,
    );

    // Assert
    assert_eq!(action, None);
}

#[test]
fn 同一个键绑给两个不同动作时注册被拒绝() {
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind(KeyBinding {
            key: KeyCode::KeyQ,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Menu,
        })
        .expect("首次绑定不冲突");

    // Act
    let result = table.try_bind(KeyBinding {
        key: KeyCode::KeyQ,
        modifiers: Modifiers::NONE,
        context: InputContext::Gameplay,
        action: GameKey::Map,
    });

    // Assert
    assert!(result.is_err());
}

#[test]
fn 冲突被拒绝后表内容不变() {
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind(KeyBinding {
            key: KeyCode::KeyQ,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Menu,
        })
        .expect("首次绑定不冲突");

    // Act
    let _ = table.try_bind(KeyBinding {
        key: KeyCode::KeyQ,
        modifiers: Modifiers::NONE,
        context: InputContext::Gameplay,
        action: GameKey::Map,
    });

    // Assert
    assert_eq!(table.bindings().len(), 1);
}

#[test]
fn 同一个键在不同修饰键下绑给不同动作不算冲突() {
    // 修饰键是判重维度之一：Ctrl+S 与 S 完全可以各自独立绑定。
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind(KeyBinding {
            key: KeyCode::KeyS,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Down,
        })
        .expect("首次绑定不冲突");

    // Act
    let result = table.try_bind(KeyBinding {
        key: KeyCode::KeyS,
        modifiers: Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
        },
        context: InputContext::Gameplay,
        action: GameKey::Screenshot,
    });

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 同一个动作绑定两个键属于多绑不算冲突() {
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind(KeyBinding {
            key: KeyCode::ArrowUp,
            modifiers: Modifiers::NONE,
            context: InputContext::Gameplay,
            action: GameKey::Up,
        })
        .expect("首次绑定不冲突");

    // Act
    let result = table.try_bind(KeyBinding {
        key: KeyCode::KeyW,
        modifiers: Modifiers::NONE,
        context: InputContext::Gameplay,
        action: GameKey::Up,
    });

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 多绑后两个键都能解析出同一动作() {
    // Arrange
    let table = KeyBindings::from_bindings([KeyBinding::gameplay(KeyCode::ArrowUp, GameKey::Up)])
        .expect("单条绑定不冲突");
    let mut table = table;
    table
        .try_bind(KeyBinding::gameplay(KeyCode::KeyW, GameKey::Up))
        .expect("多绑同一动作不冲突");

    // Act
    let via_arrow = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);
    let via_letter = table.resolve(KeyCode::KeyW, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(
        (via_arrow, via_letter),
        (Some(GameKey::Up), Some(GameKey::Up))
    );
}

#[test]
fn bindings_for只返回指定动作的绑定() {
    // `bindings_for` 按 `action` 过滤，不按 `context` 过滤——
    // `GameKey::Up` 现在同时被 `DEFAULT_BINDINGS`（Gameplay）与
    // `DEFAULT_MENU_BINDINGS`（Menu）各绑了 `ArrowUp`/`KeyW` 两个
    // 物理键，因此这里只筛出 Gameplay 上下文那一半，理由见
    // `bindings_for` 文档「设置界面展示这个动作当前绑了哪些键」——
    // 展示界面天然是按上下文分别展示的，不会把两个上下文的绑定
    // 混在一起呈现。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let up_keys: Vec<KeyCode> = table
        .bindings_for(GameKey::Up)
        .filter(|binding| binding.context == InputContext::Gameplay)
        .map(|binding| binding.key)
        .collect();

    // Assert
    assert_eq!(up_keys, vec![KeyCode::ArrowUp, KeyCode::KeyW]);
}

#[test]
fn bindings_for涵盖菜单上下文下的绑定() {
    // 上一条测试只看 Gameplay 那一半，这条测试核实 Menu 那一半也
    // 确实被 `bindings_for` 看到（防止未来有人误以为 Menu 绑定表
    // 是另一套没有接进同一张 `bindings` 表的平行数据）。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let menu_up_keys: Vec<KeyCode> = table
        .bindings_for(GameKey::Up)
        .filter(|binding| binding.context == InputContext::Menu)
        .map(|binding| binding.key)
        .collect();

    // Assert
    assert_eq!(menu_up_keys, vec![KeyCode::ArrowUp, KeyCode::KeyW]);
}

#[test]
fn 修饰键状态从winit状态换算正确() {
    // Arrange
    let mut state = winit::keyboard::ModifiersState::empty();
    state.insert(winit::keyboard::ModifiersState::SHIFT);

    // Act
    let modifiers = Modifiers::from(state);

    // Assert
    assert_eq!(
        modifiers,
        Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        }
    );
}

#[test]
fn 绑定表能序列化后再反序列化出等价内容() {
    // 验证的是「经过一种真实的 serde 格式往返」而不只是 derive 能
    // 编译，见 ADR 0011「验证方式」一节的同款要求。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let json = serde_json::to_string(&table).expect("默认绑定表应能序列化");
    let restored: KeyBindings = serde_json::from_str(&json).expect("刚序列化的数据应能读回");

    // Assert
    assert_eq!(restored.bindings(), table.bindings());
}

#[test]
fn 默认绑定表能解析放大键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::Equal, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::ZoomIn));
}

#[test]
fn 默认绑定表能解析缩小键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::Minus, Modifiers::NONE, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::ZoomOut));
}

#[test]
fn 默认滚轮绑定能解析出放大动作() {
    // 滚轮与按键绑给同一对抽象动作,是「同一个抽象动作可由滚轮或
    // 按键触发」的直接验证。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve_wheel(WheelDirection::Away, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::ZoomIn));
}

#[test]
fn 默认滚轮绑定能解析出缩小动作() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve_wheel(WheelDirection::Toward, InputContext::Gameplay);

    // Assert
    assert_eq!(action, Some(GameKey::ZoomOut));
}

#[test]
fn 同一个滚动方向绑给两个不同动作时注册被拒绝() {
    // Arrange
    let mut table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");
    table
        .try_bind_wheel(WheelBinding {
            direction: WheelDirection::Away,
            context: InputContext::Gameplay,
            action: GameKey::ZoomIn,
        })
        .expect("首次绑定不冲突");

    // Act
    let result = table.try_bind_wheel(WheelBinding {
        direction: WheelDirection::Away,
        context: InputContext::Gameplay,
        action: GameKey::ZoomOut,
    });

    // Assert
    assert!(result.is_err());
}

#[test]
fn 滚轮与按键分属不同判重维度互不冲突() {
    // 一个动作同时被按键与滚轮绑定,不该被判定为冲突——两者是完全
    // 独立的判重表,见 WheelDirection 文档「为什么滚轮是独立的一套
    // 抽象」一节。
    // Arrange
    let mut table =
        KeyBindings::from_bindings([KeyBinding::gameplay(KeyCode::Equal, GameKey::ZoomIn)])
            .expect("单条绑定不冲突");

    // Act
    let result = table.try_bind_wheel(WheelBinding {
        direction: WheelDirection::Away,
        context: InputContext::Gameplay,
        action: GameKey::ZoomIn,
    });

    // Assert
    assert!(result.is_ok());
}

#[test]
fn 未绑定的滚动方向解析为空值() {
    // Arrange
    let table = KeyBindings::from_bindings(std::iter::empty()).expect("空表不冲突");

    // Act
    let action = table.resolve_wheel(WheelDirection::Away, InputContext::Gameplay);

    // Assert
    assert_eq!(action, None);
}

#[test]
fn 绑定表含滚轮绑定时仍能序列化后再反序列化出等价内容() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let json = serde_json::to_string(&table).expect("默认绑定表应能序列化");
    let restored: KeyBindings = serde_json::from_str(&json).expect("刚序列化的数据应能读回");

    // Assert
    assert_eq!(restored.wheel_bindings(), table.wheel_bindings());
}

#[test]
fn 旧版本不含滚轮字段的配置文件仍能反序列化() {
    // 兜底旧配置文件——本字段引入之前写出的 JSON 不含 wheel_bindings
    // 键,`#[serde(default)]` 应当把它当成空列表处理,而不是解析失败。
    // Arrange
    let json = r#"{"bindings":[
        {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Menu"}
    ]}"#;

    // Act
    let table: KeyBindings = serde_json::from_str(json).expect("缺失 wheel_bindings 字段应当兜底");

    // Assert
    assert!(table.wheel_bindings().is_empty());
}

#[test]
fn 滚轮反序列化遇到冲突时拒绝而不是绕过校验() {
    // 与按键版本的 ADR 0011 测试同一类攻击面。
    // Arrange
    let json = r#"{"bindings":[],"wheel_bindings":[
        {"direction":"Away","context":"Gameplay","action":"ZoomIn"},
        {"direction":"Away","context":"Gameplay","action":"ZoomOut"}
    ]}"#;

    // Act
    let result: Result<KeyBindings, _> = serde_json::from_str(json);

    // Assert
    assert!(result.is_err());
}

#[test]
fn 竖直负增量换算成远离方向() {
    // Arrange & Act
    let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, -1.0));

    // Assert
    assert_eq!(direction, Some(WheelDirection::Away));
}

#[test]
fn 竖直正增量换算成靠近方向() {
    // Arrange & Act
    let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, 1.0));

    // Assert
    assert_eq!(direction, Some(WheelDirection::Toward));
}

#[test]
fn 零增量换算为空值() {
    // Arrange & Act
    let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::LineDelta(0.0, 0.0));

    // Assert
    assert_eq!(direction, None);
}

#[test]
fn 触控板像素增量同样按竖直分量符号判定() {
    // Arrange & Act
    let direction = WheelDirection::from_scroll_delta(MouseScrollDelta::PixelDelta(
        winit::dpi::PhysicalPosition::new(0.0, -5.0),
    ));

    // Assert
    assert_eq!(direction, Some(WheelDirection::Away));
}

#[test]
fn 默认绑定表在菜单上下文下能解析方向键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Menu);

    // Assert
    assert_eq!(action, Some(GameKey::Up));
}

#[test]
fn 默认绑定表在菜单上下文下能解析确认键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::Enter, Modifiers::NONE, InputContext::Menu);

    // Assert
    assert_eq!(action, Some(GameKey::Confirm));
}

#[test]
fn 默认绑定表在菜单上下文下能解析取消键() {
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let action = table.resolve(KeyCode::Escape, Modifiers::NONE, InputContext::Menu);

    // Assert
    assert_eq!(action, Some(GameKey::Cancel));
}

#[test]
fn 只在游戏内上下文绑定的键在菜单上下文下解析为空值() {
    // 这条核实两张表确实是**按上下文隔离**的，不是不小心共用了同一
    // 份判重逻辑而让所有键都跨上下文生效。
    //
    // # 探针从 F2 换成了 I（规格 N13）
    //
    // 本条原先拿截图键（F2）当探针，理由是「F2 只出现在
    // `DEFAULT_BINDINGS`（Gameplay），不在 `DEFAULT_MENU_BINDINGS`
    // 里」。规格 N13 / D9 把 F2 **补进了**菜单表（截图要在任何上下文
    // 下都能按），那个前提不再成立。
    //
    // 换探针，**不是**放宽或删掉这条断言：`Inventory`（I）今天仍然
    // 只绑 `Gameplay`，而且它天然就该只绑那里——菜单开着时按 I 不该
    // 打开背包。断言本身一个字没改。
    // Arrange
    let table = KeyBindings::default_bindings();

    // 先自证探针本身有效：I 在游戏内**确实**绑着东西，否则
    // 「在菜单里解析为空」对一个哪儿都没绑的键恒绿。
    assert_eq!(
        table.resolve(KeyCode::KeyI, Modifiers::NONE, InputContext::Gameplay),
        Some(GameKey::Inventory),
        "探针失效：I 在游戏内上下文下也没绑东西"
    );

    // Act
    let action = table.resolve(KeyCode::KeyI, Modifiers::NONE, InputContext::Menu);

    // Assert
    assert_eq!(action, None);
}

#[test]
fn 反序列化遇到冲突绑定时拒绝而不是绕过校验() {
    // 这是 ADR 0011 要防的事：手改的配置文件可能包含冲突绑定，
    // 若直接派生 Deserialize 会绕开 try_bind 的校验，把非法状态
    // 直接怼进私有字段。
    // Arrange：手写一份把同一个键绑给两个不同动作的 JSON。
    let json = r#"{"bindings":[
        {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Menu"},
        {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Map"}
    ]}"#;

    // Act
    let result: Result<KeyBindings, _> = serde_json::from_str(json);

    // Assert
    assert!(result.is_err());
}

/// 一份「只有旧动作」的绑定表，形状照抄项目所有者机器上那份
/// `config.json5`（写于「输入接线」批次之前）：12 个动作、17 条
/// 绑定、全部在 `Gameplay` 上下文下、空格绑给 `Confirm`。
fn 旧版本绑定表() -> KeyBindings {
    KeyBindings::from_bindings([
        KeyBinding::gameplay(KeyCode::ArrowUp, GameKey::Up),
        KeyBinding::gameplay(KeyCode::KeyW, GameKey::Up),
        KeyBinding::gameplay(KeyCode::ArrowDown, GameKey::Down),
        KeyBinding::gameplay(KeyCode::KeyS, GameKey::Down),
        KeyBinding::gameplay(KeyCode::ArrowLeft, GameKey::Left),
        KeyBinding::gameplay(KeyCode::KeyA, GameKey::Left),
        KeyBinding::gameplay(KeyCode::ArrowRight, GameKey::Right),
        KeyBinding::gameplay(KeyCode::KeyD, GameKey::Right),
        KeyBinding::gameplay(KeyCode::Enter, GameKey::Confirm),
        KeyBinding::gameplay(KeyCode::Space, GameKey::Confirm),
        KeyBinding::gameplay(KeyCode::Escape, GameKey::Cancel),
        KeyBinding::gameplay(KeyCode::Tab, GameKey::Menu),
        KeyBinding::gameplay(KeyCode::KeyM, GameKey::Map),
        KeyBinding::gameplay(KeyCode::Period, GameKey::Wait),
        KeyBinding::gameplay(KeyCode::F2, GameKey::Screenshot),
        KeyBinding::gameplay(KeyCode::Equal, GameKey::ZoomIn),
        KeyBinding::gameplay(KeyCode::Minus, GameKey::ZoomOut),
    ])
    .expect("夹具本身不该自相冲突")
}

#[test]
fn 合并默认键位不会让任何一个原有动作变成零键位() {
    // 抢占那条规则唯一的下限守卫，作为一条**性质**来验证（不是举
    // 一个例子）：合并前每个 `(动作, 上下文)` 只要有过键位，合并后
    // 就必须还有键位。
    //
    // 反例（已实跑验证会红）：把 `occupant_keys_left >= 2` 那条
    // 守卫拿掉改成无条件抢占，`(Confirm, Gameplay)` 那一项会在
    // 「原表里只有空格一个确认键」的夹具下掉到 0。
    // Arrange：把回车删掉，`Confirm` 在 Gameplay 下只剩空格——正是
    // 抢占会踩到的那一格。
    let before = KeyBindings::from_bindings(
        旧版本绑定表()
            .bindings()
            .iter()
            .copied()
            .filter(|binding| binding.key != KeyCode::Enter),
    )
    .expect("夹具本身不该自相冲突");

    // Act
    let after = before.fill_missing_defaults(&[]);

    // Assert
    for binding in before.bindings() {
        let 合并后还剩 = after
            .bindings()
            .iter()
            .filter(|b| b.action == binding.action && b.context == binding.context)
            .count();
        assert!(
            合并后还剩 >= 1,
            "动作 {:?}（上下文 {:?}）在合并前有键位，合并后却一个都不剩",
            binding.action,
            binding.context
        );
    }
}

#[test]
fn 对内置默认表做合并是恒等的() {
    // 幂等：默认表本身已经完整，合并不该多补也不该少一条，否则
    // 「每次加载都跑一遍合并」会让绑定表随加载次数漂移。
    // Arrange
    let table = KeyBindings::default_bindings();

    // Act
    let merged = table.fill_missing_defaults(&[]);

    // Assert
    assert_eq!(merged.bindings(), table.bindings());
    assert_eq!(merged.wheel_bindings(), table.wheel_bindings());
}
