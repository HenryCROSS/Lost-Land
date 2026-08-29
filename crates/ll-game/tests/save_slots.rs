//! 多槽位存档的端到端证据：命名、并存、覆盖、列表、老存档收编、模式。
//!
//! 全部用仓库真实的 `mods/lostland/` 内容装载、真实的
//! [`ll_game::world::build_new_world`]、真实的存档写出与读入管线——不造
//! 任何简化夹具（ADR 0018：新能力要有经真实 `mods/` 内容的端到端证据）。
//!
//! ADR 0025 禁止用合成按键做验收：这里的每一条都是程序化驱动同一条公开
//! 路径，不模拟任何键盘事件。

use ll_content::mode::SaveMode;
use ll_game::content::{LoadedContent, load_content};
use ll_game::save::{LoadedGame, load_game, save_game};
use ll_game::save_slot::{
    SaveTarget, SlotId, adopt_legacy_save, format_saved_at, legacy_slot_stem, list_slots,
};
use ll_game::world::{GameWorld, build_new_world, build_new_world_with_mode};
use ll_world::generate::GenParams;

/// 装载仓库真实的本体内容——与其余端到端测试同一条路径。
fn real_content() -> LoadedContent {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crates/ll-game 上溯两级就是仓库根")
        .to_path_buf();
    let scratch = 临时目录("content");
    std::fs::create_dir_all(&scratch).expect("创建测试目录应当成功");
    let content = load_content(&repo_root.join("mods"), &scratch.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&scratch);
    content
}

/// 每次调用独占一个临时目录——进程 ID 隔离进程，线程 ID 隔离同一进程内
/// 并行跑的测试（Rust 测试框架默认多线程）。
fn 临时目录(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ll-game-save-slots-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn 建一局(content: &LoadedContent, seed: u64) -> GameWorld {
    build_new_world(
        content,
        GenParams {
            seed,
            ..GenParams::default()
        },
    )
    .expect("默认布局满足全部构造前置条件")
}

/// 把一局世界写进 `dir` 下叫 `name` 的槽位，返回那个槽位目标。
fn 存一份(
    dir: &std::path::Path,
    content: &LoadedContent,
    world: &GameWorld,
    name: &str,
) -> SaveTarget {
    std::fs::create_dir_all(dir).expect("创建存档目录应当成功");
    let target = SaveTarget::create_in(dir, name);
    save_game(
        &target.path,
        content,
        world,
        "测试旅人",
        "出生地",
        &target.name,
    )
    .expect("写出应当成功");
    target
}

#[test]
fn 两个名字建出两份并存的存档且各自读回自己的世界() {
    // C1。多槽位的全部意义：两份存档同时存在、互不覆盖、各是各的世界。
    // Arrange
    let content = real_content();
    let dir = 临时目录("coexist");
    let 甲 = 建一局(&content, 1);
    let 乙 = 建一局(&content, 2);
    let 甲种子 = 甲.world.seed;
    let 乙种子 = 乙.world.seed;
    assert_ne!(甲种子, 乙种子, "Arrange：两局必须真的是不同的世界");

    // Act
    let 甲槽 = 存一份(&dir, &content, &甲, "alpha");
    let 乙槽 = 存一份(&dir, &content, &乙, "beta");

    // Assert：列表里两条都在。
    let slots = list_slots(&dir);
    assert_eq!(slots.len(), 2, "两份存档必须并存：{slots:?}");
    let names: Vec<String> = slots.iter().map(|slot| slot.display_name()).collect();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));

    // Assert：各自读回各自的世界。
    for (槽, 期望种子) in [(&甲槽, 甲种子), (&乙槽, 乙种子)] {
        match load_game(&槽.path, &content) {
            LoadedGame::Playable { world, .. } => assert_eq!(
                world.seed, 期望种子,
                "{} 读回来的应当是它自己的世界",
                槽.name
            ),
            other => panic!("{} 应当可游玩，实际 {other:?}", 槽.name),
        }
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 同名的第二份存档不覆盖第一份() {
    // C3 的一半：两个都叫「测试」的世界是两份存档，后一份不该悄悄覆盖
    // 前一份。区分它们的是文件名后缀与时间戳，展示名保持玩家输入的原样。
    // Arrange
    let content = real_content();
    let dir = 临时目录("same-name");
    let 甲 = 建一局(&content, 11);
    let 乙 = 建一局(&content, 22);

    // Act
    let 甲槽 = 存一份(&dir, &content, &甲, "world");
    let 乙槽 = 存一份(&dir, &content, &乙, "world");

    // Assert
    assert_ne!(甲槽.path, 乙槽.path, "两份存档必须落在不同的文件上");
    assert_eq!(乙槽.id.as_str(), "world-2", "重名应当追加数字后缀");
    assert_eq!(
        乙槽.name, "world",
        "展示名保持玩家输入的原样——`-2` 是文件系统的实现细节"
    );
    assert_eq!(list_slots(&dir).len(), 2);

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 往同一个槽位存第二次是覆盖不是新建() {
    // C2。槽位在建档那一刻定死，此后永远写同一个文件——否则玩家每存一次
    // 就多一份档。
    // Arrange
    let content = real_content();
    let dir = 临时目录("overwrite");
    let world = 建一局(&content, 33);
    let target = 存一份(&dir, &content, &world, "same-slot");

    // Act：对着**同一个** target 再存一次。
    save_game(
        &target.path,
        &content,
        &world,
        "测试旅人",
        "出生地",
        &target.name,
    )
    .expect("重新写出应当成功");

    // Assert
    assert_eq!(list_slots(&dir).len(), 1, "同一个槽位存两次只该有一份文件");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 列表按最近存过的排在最前且跳过读不出来的那一份() {
    // C4。一份坏档不该让整个列表打不开——玩家在界面上没有任何办法把那份
    // 坏的挑出来删掉。
    // Arrange
    let content = real_content();
    let dir = 临时目录("ordering");
    let world = 建一局(&content, 44);
    let 旧 = 存一份(&dir, &content, &world, "older");
    let 新 = 存一份(&dir, &content, &world, "newer");
    // 把两份的时间戳拉开——同一秒写出来的两份靠 `saved_at` 分不出先后。
    重写时间戳(&旧.path, 1_700_000_000);
    重写时间戳(&新.path, 1_800_000_000);
    // 再丢一份根本不是存档的文件进去。
    std::fs::write(dir.join("broken.llsave"), b"this is not a save file")
        .expect("写占位文件应当成功");

    // Act
    let slots = list_slots(&dir);

    // Assert
    assert_eq!(slots.len(), 2, "坏档被跳过，其余两份照常列出：{slots:?}");
    assert_eq!(
        slots[0].display_name(),
        "newer",
        "最近存过的排在最前：{slots:?}"
    );
    assert_eq!(slots[1].display_name(), "older");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

/// 把一份存档头里的 `saved_at` 改成 `value`——直接改头部 JSON，主体一个
/// 字节不动。
///
/// 存档的物理布局是「4 字节小端长度前缀 + 头部 JSON + 压缩主体」（见
/// `ll_content::save_file` 模块文档），本帮手按同一份布局改写头部。
fn 重写时间戳(path: &std::path::Path, value: i64) {
    let bytes = std::fs::read(path).expect("存档应当读得出来");
    let len = u32::from_le_bytes(bytes[..4].try_into().expect("前四个字节必然够")) as usize;
    let header: serde_json::Value =
        serde_json::from_slice(&bytes[4..4 + len]).expect("头部应当是合法 JSON");
    let mut header = header;
    header["saved_at"] = serde_json::json!(value);
    let new_header = serde_json::to_vec(&header).expect("改完仍然可序列化");
    let mut out = (new_header.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&new_header);
    out.extend_from_slice(&bytes[4 + len..]);
    std::fs::write(path, out).expect("回写应当成功");
}

/// 把一份存档头里的 `schema_version` 改成 `value`——直接改头部 JSON，
/// 主体一个字节不动，用来模拟「上一版 schema 写出的存档」。
fn 重写schema版本(path: &std::path::Path, value: u32) {
    let bytes = std::fs::read(path).expect("存档应当读得出来");
    let len = u32::from_le_bytes(bytes[..4].try_into().expect("前四个字节必然够")) as usize;
    let mut header: serde_json::Value =
        serde_json::from_slice(&bytes[4..4 + len]).expect("头部应当是合法 JSON");
    header["schema_version"] = serde_json::json!(value);
    let new_header = serde_json::to_vec(&header).expect("改完仍然可序列化");
    let mut out = (new_header.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&new_header);
    out.extend_from_slice(&bytes[4 + len..]);
    std::fs::write(path, out).expect("回写应当成功");
}

/// 把一份存档头里的某个键**整个删掉**——模拟本批次之前写出的老存档。
fn 删掉头部的键(path: &std::path::Path, key: &str) {
    let bytes = std::fs::read(path).expect("存档应当读得出来");
    let len = u32::from_le_bytes(bytes[..4].try_into().expect("前四个字节必然够")) as usize;
    let mut header: serde_json::Value =
        serde_json::from_slice(&bytes[4..4 + len]).expect("头部应当是合法 JSON");
    header
        .as_object_mut()
        .expect("头部是一个 JSON 对象")
        .remove(key)
        .unwrap_or_else(|| panic!("Arrange：头部本来就该有 {key} 这个键"));
    let new_header = serde_json::to_vec(&header).expect("改完仍然可序列化");
    let mut out = (new_header.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&new_header);
    out.extend_from_slice(&bytes[4 + len..]);
    std::fs::write(path, out).expect("回写应当成功");
}

#[test]
fn 老存档被收编成槽位且读得回同一个世界() {
    // C5，端到端。所有者手上有一份迁移前写出的 `save.llsave`：
    //
    // - 它躺在**旧路径**（数据目录下的单个文件），不在 `saves/` 里；
    // - 它的头部**没有 `save_name` 这个键**（本批次才加的）。
    //
    // 两件事都不许让它读崩。
    //
    // 反例验证（已实跑）：把 `SaveHeader::save_name` 的 `#[serde(default)]`
    // 摘掉，本条当场变红（头部反序列化失败 ⇒ 列表里一份都没有）。
    // Arrange：造一份「旧格式」的存档。
    let content = real_content();
    let base = 临时目录("legacy");
    std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
    let legacy = base.join("save.llsave");
    let saves_dir = base.join("saves");
    let world = 建一局(&content, 55);
    let 玩家位置 = world
        .world
        .actors
        .get(world.player)
        .expect("刚生成必然存在")
        .pos;
    save_game(&legacy, &content, &world, "老旅人", "出生地", "").expect("写出应当成功");
    删掉头部的键(&legacy, "save_name");

    // Act：走真实的收编路径。
    let adopted = adopt_legacy_save(&legacy, &saves_dir).expect("有老存档就该收编出一份");

    // Assert：老文件**原样还在**（复制不是移动）。
    assert!(
        legacy.exists(),
        "收编必须是复制——移动会让原始存档在收编有缺陷时彻底消失"
    );

    // Assert：它出现在列表里，名字退回文件名主干（老存档没有名字）。
    let slots = list_slots(&saves_dir);
    assert_eq!(slots.len(), 1, "收编出来的槽位应当出现在列表里：{slots:?}");
    assert_eq!(slots[0].save_name, "", "老存档头里本来就没有名字");
    assert_eq!(
        slots[0].display_name(),
        legacy_slot_stem(),
        "没有名字时退回文件名主干，绝不显示空白行"
    );

    // Assert：真的读得回来，而且是同一个世界、同一个玩家位置。
    match load_game(&adopted, &content) {
        LoadedGame::Playable { world: 读回, .. } => {
            assert_eq!(读回.seed, world.world.seed);
            let 读回位置 = 读回
                .actors
                .get(读回.player_entity.expect("存档里带着玩家实体号"))
                .expect("玩家必然在")
                .pos;
            assert_eq!(读回位置, 玩家位置, "老存档读回来的玩家应当站在原地");
        }
        other => panic!("老存档必须可游玩，实际 {other:?}"),
    }

    // Assert：再收编一次不会重复复制（否则每次启动都会把老档盖回去，
    // 玩家收编后的进度会被反复抹掉）。
    assert!(
        adopt_legacy_save(&legacy, &saves_dir).is_none(),
        "已经收编过就不该再来一次"
    );
    assert_eq!(list_slots(&saves_dir).len(), 1);

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn 没有老存档时收编什么都不做() {
    // Arrange
    let base = 临时目录("no-legacy");

    // Act & Assert
    assert!(adopt_legacy_save(&base.join("save.llsave"), &base.join("saves")).is_none());
    assert!(!base.join("saves").exists(), "没东西可收编时连目录都不该建");
}

#[test]
fn 肉鸽档存档往返之后仍然是肉鸽档() {
    // 模式跟着世界身份走：建档时绑定，读档时从存档头原样接回来。中间
    // 没有任何一处「按当前偏好重新推导」。
    // Arrange
    let content = real_content();
    let dir = 临时目录("roguelike-roundtrip");
    let world = build_new_world_with_mode(
        &content,
        GenParams {
            seed: 66,
            ..GenParams::default()
        },
        SaveMode::Permadeath,
    )
    .expect("默认布局满足全部构造前置条件");
    assert!(
        !world.identity.allows_manual_save(),
        "Arrange：肉鸽档不该允许手动存档"
    );

    // Act
    let target = 存一份(&dir, &content, &world, "rogue");
    let LoadedGame::Playable { identity, .. } = load_game(&target.path, &content) else {
        panic!("刚写出来的存档必须可游玩");
    };

    // Assert
    assert_eq!(identity.mode(), SaveMode::Permadeath);
    assert!(!identity.allows_manual_save());
    // 列表也只读头部就看得到模式——玩家有权在读档之前知道这是肉鸽档。
    assert_eq!(list_slots(&dir)[0].mode, SaveMode::Permadeath);
    assert!(!list_slots(&dir)[0].allows_manual_save());

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 肉鸽档降级为普通之后存档仍在且再读回来还是普通() {
    // C7。所有者的修正原话：「死亡后变成一般模式，可以再创建角色然后选择
    // 在某个地方出生。」——**不删档**，模式单向转普通。
    //
    // 反例验证（已实跑）：把 `WorldIdentity::downgrade_mode` 改成永远返回
    // 假且不改模式，本条当场变红。
    // Arrange
    let content = real_content();
    let dir = 临时目录("downgrade");
    let mut world = build_new_world_with_mode(
        &content,
        GenParams {
            seed: 77,
            ..GenParams::default()
        },
        SaveMode::Permadeath,
    )
    .expect("默认布局满足全部构造前置条件");
    let target = 存一份(&dir, &content, &world, "doomed");

    // Act：玩家死了。
    let 真的降级了 = world.identity.downgrade_mode();
    save_game(
        &target.path,
        &content,
        &world,
        "测试旅人",
        "出生地",
        &target.name,
    )
    .expect("死后存档应当成功");

    // Assert
    assert!(真的降级了);
    let slots = list_slots(&dir);
    assert_eq!(slots.len(), 1, "死亡**不删档**——世界比角色活得长");
    assert!(
        slots[0].allows_manual_save(),
        "降级之后这个世界应当允许手动存档"
    );
    assert!(
        slots[0].mode.was_downgraded_from_permadeath(),
        "「曾经是肉鸽」这条记录必须永久留下"
    );

    // Assert：读回来还是普通，而且**没有任何路径能把它改回肉鸽**。
    let LoadedGame::Playable { identity, .. } = load_game(&target.path, &content) else {
        panic!("降级之后仍然必须可游玩");
    };
    assert!(identity.allows_manual_save());
    assert_eq!(
        identity.mode().downgrade(),
        None,
        "普通档没有任何「再降一次」或「升回去」的路径"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 没明说模式时建出来的是普通档() {
    // 迁移前 `save_game` 的 7 处调用点全部硬编码 `Permadeath`：每一局都
    // 被记成肉鸽档，而玩家从来没做过这个选择。
    // Arrange & Act
    let content = real_content();
    let world = 建一局(&content, 88);

    // Assert
    assert!(world.identity.allows_manual_save());
    assert!(matches!(world.identity.mode(), SaveMode::FreeSave { .. }));
    assert!(
        !world.identity.mode().was_downgraded_from_permadeath(),
        "从一开始就是普通档，不是降级来的"
    );
}

#[test]
fn 槽位标识过滤之后仍然落在存档目录里() {
    // 白名单过滤同时是一道路径穿越闸门，见 `SlotId::from_name` 文档。
    // Arrange
    let dir = std::path::Path::new("saves");

    // Act & Assert
    for evil in ["../../etc/passwd", "..\\..\\windows", "a/b/c", "C:evil"] {
        let path = SlotId::from_name(evil).path_in(dir);
        assert_eq!(
            path.parent(),
            Some(dir),
            "{evil} 过滤之后应当仍然落在存档目录里，实际 {}",
            path.display()
        );
    }
}

#[test]
fn 时间戳显示成人类读得懂的样子() {
    // 存档列表要回答「哪一份更新」，一串 Unix 秒回答不了。
    assert_eq!(format_saved_at(1_800_000_000), "2027-01-15 08:00");
}

#[test]
fn 上一版schema的老存档被明确拒绝而不是静默误解析() {
    // 归属批次的存档兼容证据（端到端，走真实 mods/ 与真实读写管线）。
    //
    // 背景：本批次给 ItemStack 加了 owner 字段——那是存档**主体**的结构
    // 变化。主体走 postcard，而 postcard 是 non-self-describing 的二进制
    // 格式：字节流里没有字段名，反序列化按声明顺序逐字段吃字节，
    // `#[serde(default)]` 在那条路径上是**空操作**（既有的
    // `Agent::gender`/`GroundItemStack::placed` 两批都误以为它管用，
    // 完整论证见 ll_content::save_file::CURRENT_SCHEMA_VERSION 文档）。
    //
    // 因此本批次把 CURRENT_SCHEMA_VERSION 从 2 加到 3。本条钉住那个决定
    // 的可观察后果：一份自称 schema 2 的存档必须被**明确拒绝**
    // （LoadedGame::Rejected），不许被当前的字段布局静默解析成一份看似
    // 合法实则损坏的世界，也不许 panic。所有者手上有真实存档，这一条
    // 决定的正是它撞上新版本时的表现。
    //
    // 反例验证（已实跑）：把 CURRENT_SCHEMA_VERSION 改回 2，本条当场变红
    // ——那份存档会被当成当前版本直接解析。
    // Arrange
    let content = real_content();
    let dir = 临时目录("schema-bump");
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let path = dir.join("old.llsave");
    let world = 建一局(&content, 77);
    save_game(&path, &content, &world, "旧旅人", "出生地", "").expect("写出应当成功");
    重写schema版本(&path, 2);

    // Act
    let loaded = load_game(&path, &content);

    // Assert：明确拒绝，不是 Playable、也不是 ReadOnly。
    match loaded {
        LoadedGame::Rejected(_) => {}
        LoadedGame::Playable { .. } => {
            panic!("上一版 schema 的存档不许被当前字段布局静默解析成可游玩的世界")
        }
        LoadedGame::ReadOnly(_) => panic!("这不是「内容缺失」那类降级，应当是明确拒绝"),
    }

    // Assert：当前版本写出的存档照常读得回来——上面那条拒绝不是把读档
    // 整个弄坏了。
    let fresh = dir.join("fresh.llsave");
    save_game(&fresh, &content, &world, "新旅人", "出生地", "").expect("写出应当成功");
    assert!(
        matches!(load_game(&fresh, &content), LoadedGame::Playable { .. }),
        "当前版本自己写出的存档必须照常可游玩"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
