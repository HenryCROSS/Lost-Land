//! 存档写出/读入的接线：把 [`ll_content::save_file`] 的存档主体管线
//! 与 [`crate::content::LoadedContent`]/[`crate::world::GameWorld`]
//! 串起来，供本体二进制在退出前存一次、启动时读一次。
//!
//! # 生成期 mod 集合只被搬运，永不在这里重算
//!
//! 本模块曾经在 [`save_game`] 里每次都调用一次
//! `GenerationModSet::capture(&content.registry, &content.manifests)`——
//! 也就是拿「玩家现在开着哪些 mod」当成「这个世界当初是用哪些 mod
//! 生成的」。玩家中途新装一个 mod，那个 mod 不在存档头的生成期名单
//! 里，两道校验（`ll_content::load_error::check_mod_set` 与
//! `check_mod_content`，都只遍历生成期名单）都不会看它一眼，读档放行；
//! 再存一次档，它就永久混进了这个世界的生成期名单，而原始记录已经被
//! 覆盖，追不回来——种子分享、缺陷复现、回归测试全部失效
//! （`knowledge/handoff/p4-to-p5.md` 二节原话）。
//!
//! 现在 [`save_game`] 只从 `GameWorld::identity` 里把那一份**原样
//! 搬运**出来，本模块不再 `use` `GenerationModSet`——而且即便有人把它
//! `use` 回来也没有用：`SaveHeader` 的四个世界身份字段是 `pub(crate)`
//! 的，crate 外唯一的写出入口 [`SaveHeader::new`] 只接受一份已经绑定
//! 好的 [`WorldIdentity`]，见 `ll_content::world_identity` 模块文档
//! 「单一真相源」一节。
//!
//! 本模块不重新实现任何存档格式细节——`SaveHeader` 的构造、schema
//! 迁移、`ContentIndex` 重映射、VM 强制重建全部是 `ll_content`/
//! `ll_mod` 已经交付并测试过的部件（见 `ll_content::save_file` 模块
//! 文档），这里只负责把「本体二进制手里现成的这些值」摆进正确的参数
//! 位置。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ll_content::content_index_map::snapshot_for_header;
use ll_content::degrade::{LoadOutcome, ReadOnlySave};
use ll_content::header::{ModHeaderEntry, SaveHeader, SaveHeaderMeta};
use ll_content::load_error::LoadError;
use ll_content::save_file::{
    CURRENT_SCHEMA_VERSION, SaveError, load_from_header_only, load_full, save_to_file,
};
use ll_content::world_identity::WorldIdentity;
use ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION;
use ll_mod::mod_set::{CurrentModSet, ModSetEntry};
use ll_world::state::WorldState;

use crate::content::LoadedContent;
use crate::world::GameWorld;

/// 把 [`ModSetEntry`] 列表原样搬成 [`ModHeaderEntry`] 列表——与
/// `ll_content::world_identity::generation_mods_to_header_entries` 做的
/// 是同一件事，但那个函数的签名特意只接受
/// [`ll_mod::mod_set::GenerationModSet`]（见其文档「为什么这一环值得
/// 单独一个函数」），存档头 `current_mods` 字段需要对 [`CurrentModSet`]
/// 做同样的搬运，两个类型在编译期就无法互相替代（`mod_set` 模块文档的
/// `compile_fail` 示例），故此处单独写一份三字段搬运，不强行复用那个
/// 类型受限的函数。
///
/// 生成期那一侧现在根本不经过本模块：它由
/// `ll_content::header::SaveHeader::new` 直接从世界身份搬进头部，见本
/// 模块文档第一节。
fn current_mods_to_header_entries(entries: &[ModSetEntry]) -> Vec<ModHeaderEntry> {
    entries
        .iter()
        .map(|entry| ModHeaderEntry {
            namespace: entry.id.namespace().to_string(),
            version: entry.version.clone(),
            content_hash: entry.content_hash,
        })
        .collect()
}

/// 当前墙钟时间的 Unix 秒数；系统时钟异常（早于 1970 年）时退回 0——
/// 存档时间戳只用于展示，不值得因为这种几乎不可能出现的情况让存档
/// 失败。
///
/// # 全 crate 唯一一处墙钟读取，因此是 `pub`
///
/// 除了写进存档头的 `saved_at`，建档那一刻还要给
/// [`crate::save_slot::SaveTarget::create_in`] 一个时刻（玩家的名字被
/// 白名单滤空时拿它当文件名主干，见 `save_slot` 模块文档）。两处要的
/// 是同一个「现在」，**不该各读一次** `SystemTime::now()`——那样同一次
/// 建档的头部时间与文件名时间可以差一秒，而这种偏差只会在跨秒的那一
/// 瞬间出现，是最难复现的一类不一致。
///
/// **读墙钟在这里是对的，不违反约束 C3/C4**：这个值不进 `WorldState`、
/// 不参与结算、不喂 `DetRng`，只用于展示与命名。完整论证见
/// `crate::save_slot` 模块文档「建档时间戳读墙钟，这不违反约束
/// C3/C4」一节。
pub fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// 把当前世界写出到 `path`。
///
/// 完整调用链：`GameWorld::identity`（**建档那一刻绑定、此后只被搬运**
/// 的世界身份）+ `Registry`/`manifests` → [`CurrentModSet::derive_from`]
/// 产出「玩家现在开着哪些 mod」→ [`snapshot_for_header`] 产出
/// `content_index_map` → [`SaveHeader::new`] → [`save_to_file`]。
///
/// # `content` 只允许影响「当前」那一半
///
/// 参数里的 `content` 是**当前会话**的装载结果，它只影响存档头里那些
/// 本来就该随每次存档变化的字段（`current_mods`、`content_index_map`）。
/// 世界身份的四个要素一个字节都不来自它——那是 `game_world.identity`
/// 的职责，见本模块文档第一节。
///
/// # 存档模式也不再是参数
///
/// 本函数曾经收一个 `mode: SaveMode` 参数，而全仓库 7 处调用点**全部
/// 硬编码 `SaveMode::Permadeath`**——也就是说「这局是不是肉鸽」这条
/// 事实，此前在每次存档那一刻由调用方现填一个字面量。那与「存档时重算
/// 生成期 mod 集合」是同一种形状的缺陷，而模式的单向不可逆
/// （[`ll_content::mode::SaveMode`]）让它更严重：现填一个值等于随时
/// 可以把降级抹掉。
///
/// 现在模式住在 [`ll_content::world_identity::WorldIdentity`] 里，与另外
/// 四个身份要素同一条通路：建档时绑定、读档时从存档头搬回来、存档时原样
/// 写回。`SaveHeaderMeta` 里已经没有这个字段，本函数因此**写不出**
/// 「存档那一刻现填一个模式」这行代码。
pub fn save_game(
    path: &Path,
    content: &LoadedContent,
    game_world: &GameWorld,
    character_name: &str,
    current_region: &str,
    save_name: &str,
) -> Result<(), SaveError> {
    let CurrentModSet(current_entries) =
        CurrentModSet::derive_from(&content.registry, &content.manifests);

    let header = SaveHeader::new(
        &game_world.identity,
        SaveHeaderMeta {
            schema_version: CURRENT_SCHEMA_VERSION,
            saved_at: now_unix_seconds(),
            character_name: character_name.to_string(),
            current_region: current_region.to_string(),
            playtime_ticks: game_world.world.clock.0,
            current_mods: current_mods_to_header_entries(&current_entries),
            content_hash_algorithm_version: CONTENT_HASH_ALGORITHM_VERSION,
            content_index_map: snapshot_for_header(&content.registry),
            save_name: save_name.to_string(),
        },
    );

    save_to_file(path, &header, &game_world.world)
}

/// [`load_game`] 的结果：与 [`LoadOutcome`] 三个变体一一对应，区别只在
/// 可游玩那一支**额外带回一份世界身份**。
///
/// # 为什么不能直接复用 `LoadOutcome`
///
/// [`LoadOutcome::Playable`] 只装一个 [`WorldState`]，而生成期 mod 集合
/// **不在存档主体里**，只在存档头里——读档时若不把它接出来交给调用方
/// 保管，它就在读档那一刻丢失了，下一次存档只能重算，也就回到了本模块
/// 文档第一节描述的那条缺陷。本类型存在的全部理由就是让「存档头里那
/// 一份」有地方可去。
#[derive(Debug)]
pub enum LoadedGame {
    /// 完全正常，可以继续游玩：存档主体 + 从存档头原样接回来的世界身份。
    Playable {
        /// 存档主体。
        world: WorldState,
        /// 从存档头接回来的世界身份，直接交给 `GameWorld::identity`。
        identity: WorldIdentity,
    },
    /// 撞上不可降级的缺失内容——只读模式，见 [`LoadOutcome::ReadOnly`]。
    ReadOnly(ReadOnlySave),
    /// 存档损坏或不兼容，连一个 [`WorldState`] 都拿不到。
    Rejected(LoadError),
}

/// 从 `path` 读入存档：把 `content` 手里现成的「当前会话装载结果」
/// 转交给 [`load_full`]（见其文档「完整调用链」），再把**存档头记录的
/// 世界身份**一并接回来。
///
/// # 为什么要单独再读一次头部
///
/// [`load_full`] 只返回 [`LoadOutcome`]，头部被它在内部用完就丢了。
/// 这里用 [`load_from_header_only`] 再读一次——它按存档的物理布局只读
/// 「4 字节长度前缀 + 头部 JSON」这一段，**不触碰、更不解压主体**（见
/// `ll_content::save_file` 模块文档「物理布局」），代价是一次几百字节
/// 的读取，换来的是生成期 mod 集合不再在读档那一刻丢失。读的是同一个
/// 文件的同一段字节，不存在「两次读到不一致的头部」这种可能。
///
/// # 身份的四个要素分别取自哪里
///
/// 见 [`WorldIdentity::restore_from_header`]：生成期 mod 集合与种子取自
/// 存档头，尺寸与地形形态取自存档主体（主体那两份更完整，且本批次之前
/// 写出的老存档头部里根本没有形态）。
pub fn load_game(path: &Path, content: &LoadedContent) -> LoadedGame {
    let outcome = load_full(
        path,
        &content.registry,
        &content.manifests,
        content.terrain_table.clone(),
    );
    let world = match outcome {
        LoadOutcome::Playable(world) => world,
        LoadOutcome::ReadOnly(read_only) => return LoadedGame::ReadOnly(read_only),
        LoadOutcome::Rejected(error) => return LoadedGame::Rejected(error),
    };

    let header = match load_from_header_only(path) {
        Ok(header) => header,
        // 走到这里意味着主体刚刚读成功、头部却读不出来——同一个文件的
        // 同一段字节 `load_full` 刚读过一次。真发生了只可能是文件在两次
        // 读取之间被替换或损坏，如实按损坏处理，不猜。
        Err(error) => return LoadedGame::Rejected(error),
    };
    let identity = match WorldIdentity::restore_from_header(
        &header,
        *world.terrain.layout(),
        world.terrain_shape,
    ) {
        Ok(identity) => identity,
        Err(error) => {
            return LoadedGame::Rejected(LoadError::Corrupted(format!(
                "存档头记录的生成期 mod 命名空间非法：{error}"
            )));
        }
    };

    LoadedGame::Playable { world, identity }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::load_content;
    use crate::world::build_new_world;

    fn test_content() -> LoadedContent {
        let dir = crate::test_support::unique_temp_path("ll-game-save-test-content");
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        // 理由同 crate::app 的同名帮手：本体内容住在 mods/lostland/。
        let content = load_content(&crate::test_support::repo_mods_dir(), &dir.join("assets"))
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    fn temp_save_path(name: &str) -> std::path::PathBuf {
        crate::test_support::unique_temp_path(&format!("ll-game-save-roundtrip-{name}"))
            .with_extension("llsave")
    }

    /// 递归复制一整个目录树——[`mods_dir_with_extra_mod`] 需要一份可以
    /// 随意加 mod 的 `mods/` 副本，而仓库里那一份是只读输入，不能改。
    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).expect("创建目标目录应当成功");
        for entry in std::fs::read_dir(src).expect("源目录应当可读") {
            let entry = entry.expect("目录项应当可读");
            let target = dst.join(entry.file_name());
            if entry.file_type().expect("文件类型应当可读").is_dir() {
                copy_dir_all(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("复制文件应当成功");
            }
        }
    }

    /// 造一份 `mods/` 副本：仓库本体那几个 mod 原样搬过来，再按
    /// `extra_namespaces` 追加若干「只有清单、不贡献任何内容」的 mod。
    ///
    /// 「只有清单」是刻意的：本测试要复现的是**生成期 mod 名单被污染**，
    /// 与新 mod 贡献了什么内容无关，一个空 mod 已经足以让它出现在
    /// `CurrentModSet::derive_from` 的结果里（`content_hash` 为 `None`，
    /// 见 `ll_mod::mod_set::ModSetEntry::content_hash` 文档）。
    fn mods_dir_with_extra_mod(tag: &str, extra_namespaces: &[&str]) -> std::path::PathBuf {
        let root = crate::test_support::unique_temp_path(&format!("ll-game-save-mods-{tag}"));
        copy_dir_all(&crate::test_support::repo_mods_dir(), &root);
        for namespace in extra_namespaces {
            let dir = root.join(namespace);
            std::fs::create_dir_all(&dir).expect("创建 mod 目录应当成功");
            std::fs::write(
                dir.join("mod.json5"),
                format!(
                    "{{ namespace: \"{namespace}\", version: \"0.1.0\" }}
"
                ),
            )
            .expect("写出 mod 清单应当成功");
        }
        root
    }

    /// 从一个指定的 `mods/` 目录装载内容——[`test_content`] 只认仓库
    /// 那一份，本函数供「玩家中途新装了一个 mod」这类场景使用。
    fn content_from(mods_root: &std::path::Path, tag: &str) -> LoadedContent {
        let dir = crate::test_support::unique_temp_path(&format!("ll-game-save-assets-{tag}"));
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        let content =
            load_content(mods_root, &dir.join("assets")).expect("测试用内容契约必须解析成功");
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    /// 存档头里 `generation_mods` 记了哪几个命名空间（按记录顺序）。
    fn header_generation_namespaces(path: &std::path::Path) -> Vec<String> {
        ll_content::save_file::load_from_header_only(path)
            .expect("读头部应当成功")
            .generation_mods()
            .iter()
            .map(|entry| entry.namespace.clone())
            .collect()
    }

    /// 把一份读回来的 `WorldState` 重新装配成 [`GameWorld`]——与
    /// `crate::load_or_new_game` 读档分支做的是同一件事，测试里不便
    /// 复用那个私有函数（它还要读配置、找数据目录），这里按同一套
    /// 派生规则重建。
    fn reassemble(world: ll_world::state::WorldState, identity: WorldIdentity) -> GameWorld {
        let player = world.player_entity.expect("可游玩的存档必然记录了玩家实体");
        let params = world.gen_params();
        let layout = crate::world::build_zone_layout().expect("默认布局恒合法");
        let noise = ll_world::generate::build_zone_noise(&layout, &params).expect("默认布局恒合法");
        let timeline = crate::world::rebuild_timeline(&world);
        GameWorld {
            world,
            noise,
            params,
            player,
            timeline,
            identity,
        }
    }

    /// 读一份必然可游玩的存档，直接把主体与身份拆出来。
    fn load_playable(path: &std::path::Path, content: &LoadedContent) -> GameWorld {
        match load_game(path, content) {
            LoadedGame::Playable { world, identity } => reassemble(world, identity),
            other => panic!("期望 Playable，实际读到 {other:?}"),
        }
    }

    #[test]
    fn 中途新装的mod不会被再存一次档混进生成期集合() {
        // 本仓库记录的最严重缺陷（`knowledge/audit/2026-08-26-phase-reckoning-p6-p8.md`
        // 三节第 9 项）的直接复现：生成期 mod 集合是世界身份的一部分，
        // 只在建新世界那一刻确定一次，此后只被搬运、永不重算。存档时
        // 从「当前会话已装载的内容」重算，等于让玩家中途装的任何一个
        // mod 永久混进这个世界的生成期名单，而原始记录追不回来——
        // 种子分享、缺陷复现、回归测试全部失效（`knowledge/handoff/p4-to-p5.md` 二节）。
        //
        // Arrange：世界用 mod 集合 A 生成。
        let 集合甲根目录 = mods_dir_with_extra_mod("gen-a", &[]);
        let 内容甲 = content_from(&集合甲根目录, "gen-a");
        let game_world = build_new_world(
            &内容甲,
            ll_world::generate::GenParams {
                seed: 4242,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let path = temp_save_path("generation-mod-set-carry");
        save_game(
            &path,
            &内容甲,
            &game_world,
            "测试旅人",
            "出生地",
            "测试存档",
        )
        .expect("写出应当成功");
        let 建档时的生成期名单 = header_generation_namespaces(&path);
        assert!(
            !建档时的生成期名单.iter().any(|ns| ns == "extramod"),
            "前置条件：建档时 extramod 尚未存在，实际名单 {建档时的生成期名单:?}"
        );
        drop(game_world);

        // Act：玩家中途新装一个 mod（集合 A + B）→ 读档 → 再存一次档。
        let 集合乙根目录 = mods_dir_with_extra_mod("gen-b", &["extramod"]);
        let 内容乙 = content_from(&集合乙根目录, "gen-b");
        assert!(
            内容乙
                .manifests
                .iter()
                .any(|m| m.id.namespace() == "extramod"),
            "前置条件：第二次装载必须真的看到新装的 extramod"
        );
        let LoadedGame::Playable { world, identity } = load_game(&path, &内容乙) else {
            panic!("装了一个生成期之外的新 mod 之后，存档仍应能读开");
        };
        // 钉住两道校验在「新装了生成期之外的 mod」这一情形下的语义：
        // `check_mod_set`/`check_mod_content` 只遍历生成期名单，名单外
        // 多出来的 mod 它们一眼都不看——读档放行。本批次**不改**这条
        // 语义（那是所有者裁定的决策二的范围，只覆盖「缺 mod」与「版本
        // 对不上」两档），本批次只保证放行之后名单不被污染。
        assert!(
            identity
                .generation_mods()
                .0
                .iter()
                .all(|entry| entry.id.namespace() != "extramod"),
            "从存档头接回来的生成期集合里不该出现建档后才装的 mod"
        );
        let reloaded = reassemble(world, identity);
        save_game(&path, &内容乙, &reloaded, "测试旅人", "出生地", "测试存档")
            .expect("写出应当成功");

        // Assert：存档头的生成期名单仍然是 A，一个字都没变。
        let 再存一次之后 = header_generation_namespaces(&path);
        assert_eq!(
            再存一次之后, 建档时的生成期名单,
            "生成期 mod 集合必须原样搬运，不得按当前会话重算"
        );

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&集合甲根目录);
        let _ = std::fs::remove_dir_all(&集合乙根目录);
    }

    #[test]
    fn 存档读档再存档时生成期集合逐条不变() {
        // 「搬运不走样」：中间不装任何 mod 时，两次写出的生成期名单必须
        // 逐条（命名空间 + 版本号 + 内容哈希）完全相同——本条守的是修法
        // 本身不会在搬运途中丢字段或改顺序。
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 99,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let path = temp_save_path("generation-mod-set-stable");
        save_game(
            &path,
            &content,
            &game_world,
            "测试旅人",
            "出生地",
            "测试存档",
        )
        .expect("写出应当成功");
        let 第一次 = ll_content::save_file::load_from_header_only(&path)
            .expect("读头部应当成功")
            .generation_mods()
            .to_vec();
        drop(game_world);

        // Act
        let reloaded = load_playable(&path, &content);
        save_game(&path, &content, &reloaded, "测试旅人", "出生地", "测试存档")
            .expect("写出应当成功");

        // Assert
        let 第二次 = ll_content::save_file::load_from_header_only(&path)
            .expect("读头部应当成功")
            .generation_mods()
            .to_vec();
        assert_eq!(第二次, 第一次);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 存档退出后重进读回的世界哈希与存档前一致() {
        // 硬约束的核心验证：不是「有存档代码」，是存了退出、重进能读回
        // 同一个世界——用 WorldState::hash() 逐位比对,这是规格要求的
        // 判据（与 ll_content::save_file 自己的同名测试同一个理由）。
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 7,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let hash_before = game_world.world.hash();
        let path = temp_save_path("hash-roundtrip");

        // Act：存档 → 退出（本测试不真的退出进程，只是不再持有旧的
        // game_world）→ 重进（用同一份已装载内容读档）。
        save_game(
            &path,
            &content,
            &game_world,
            "测试旅人",
            "出生地",
            "测试存档",
        )
        .expect("写出应当成功");
        drop(game_world);
        let outcome = load_game(&path, &content);

        // Assert
        match outcome {
            LoadedGame::Playable { world, .. } => {
                assert_eq!(world.hash(), hash_before);
            }
            other => panic!("期望 Playable，实际读到 {other:?}"),
        }

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 地形形态参数进了世界身份也进了存档头并原样往返() {
        // 地形形态是世界身份的**第四个**要素：与种子、尺寸、生成期 mod
        // 集合完全同性质（建档那一刻的选择，事后无法反推，缺了就复现不
        // 出同一个世界），此前却只住在存档主体，既不在 `WorldIdentity`
        // 里、也不在存档头部——`knowledge/design/worldgen-parameters.md`
        // 五节把这记为「一处已知的不对齐」。本条钉住那处不对齐已经修好。
        // Arrange：一档明显不是默认值的形态（群岛）。
        let content = test_content();
        let 群岛 = ll_world::generate::TerrainShape {
            sea_level: 540,
            mountain_level: 780,
            octaves: 4,
            continent_shrink: 2,
            ..ll_world::generate::TerrainShape::default()
        };
        assert_ne!(
            群岛,
            ll_world::generate::TerrainShape::default(),
            "前置条件：这档形态必须与默认值不同，否则本测试没有区分力"
        );
        let game_world = build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 20_260_827,
                shape: 群岛,
            },
        )
        .expect("测试用布局满足全部前置条件");
        assert_eq!(
            game_world.identity.terrain_shape(),
            群岛,
            "建档时绑定的身份必须带着玩家选的那档形态"
        );
        let path = temp_save_path("terrain-shape-identity");

        // Act：存档 → 读档。
        save_game(
            &path,
            &content,
            &game_world,
            "测试旅人",
            "出生地",
            "测试存档",
        )
        .expect("写出应当成功");
        drop(game_world);
        let header = ll_content::save_file::load_from_header_only(&path).expect("读头部应当成功");
        let reloaded = load_playable(&path, &content);

        // Assert：头部单独读得到（不必解压主体），身份里也原样带回来。
        assert_eq!(header.terrain_shape(), Some(群岛));
        assert_eq!(reloaded.identity.terrain_shape(), 群岛);
        // 形态参与身份比对：只有这一项不同的两份身份不相等。
        let 另一档 = WorldIdentity::bind(
            reloaded.identity.seed(),
            *reloaded.identity.zone_layout(),
            ll_world::generate::TerrainShape::default(),
            reloaded.identity.generation_mods().clone(),
            reloaded.identity.mode(),
        );
        assert_ne!(reloaded.identity, 另一档);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 存档头记录的种子与世界自身种子一致() {
        // 世界身份三要素之一（种子）必须真的写进头部,不是留空占位——
        // ll_content::header 模块文档「为什么头部不能引用 ContentIndex」
        // 一节强调头部必须能独立于主体被读出,种子首当其冲。
        // Arrange
        let content = test_content();
        let game_world = build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 123,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部前置条件");
        let path = temp_save_path("seed-in-header");
        save_game(
            &path,
            &content,
            &game_world,
            "测试旅人",
            "出生地",
            "测试存档",
        )
        .expect("写出应当成功");

        // Act
        let header = ll_content::save_file::load_from_header_only(&path).expect("读头部应当成功");

        // Assert
        assert_eq!(header.world_seed(), 123);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
