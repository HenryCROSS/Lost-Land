//! 存档写出/读入的接线：把 [`ll_content::save_file`] 的存档主体管线
//! 与 [`crate::content::LoadedContent`]/[`crate::world::GameWorld`]
//! 串起来，供本体二进制在退出前存一次、启动时读一次。
//!
//! 本模块不重新实现任何存档格式细节——`SaveHeader` 的构造、schema
//! 迁移、`ContentIndex` 重映射、VM 强制重建全部是 `ll_content`/
//! `ll_mod` 已经交付并测试过的部件（见 `ll_content::save_file` 模块
//! 文档），这里只负责把「本体二进制手里现成的这些值」摆进正确的参数
//! 位置。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ll_content::content_index_map::snapshot_for_header;
use ll_content::degrade::LoadOutcome;
use ll_content::header::{ModHeaderEntry, SaveHeader};
use ll_content::mode::SaveMode;
use ll_content::save_file::{CURRENT_SCHEMA_VERSION, SaveError, load_full, save_to_file};
use ll_content::world_identity::generation_mods_to_header_entries;
use ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION;
use ll_mod::mod_set::{CurrentModSet, GenerationModSet, ModSetEntry};

use crate::content::LoadedContent;
use crate::world::GameWorld;

/// 把 [`ModSetEntry`] 列表原样搬成 [`ModHeaderEntry`] 列表——与
/// [`generation_mods_to_header_entries`] 做的是同一件事，但那个函数的
/// 签名特意只接受 [`ll_mod::mod_set::GenerationModSet`]（见其文档
/// 「为什么这一环值得单独一个函数」），存档头 `current_mods` 字段需要
/// 对 [`CurrentModSet`] 做同样的搬运，两个类型在编译期就无法互相
/// 替代（`mod_set` 模块文档的 `compile_fail` 示例），故此处单独写一份
/// 三字段搬运，不强行复用那个类型受限的函数。
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
fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// 把当前世界写出到 `path`。
///
/// 完整调用链：`Registry`/`manifests` → [`GenerationModSet::capture`]/
/// [`CurrentModSet::derive_from`] → 两组头部 mod 条目 →
/// [`snapshot_for_header`] 产出 `content_index_map` → 拼出
/// [`SaveHeader`] → [`save_to_file`]。
pub fn save_game(
    path: &Path,
    content: &LoadedContent,
    game_world: &GameWorld,
    character_name: &str,
    current_region: &str,
    mode: SaveMode,
) -> Result<(), SaveError> {
    let content_index_map = snapshot_for_header(&content.registry);
    let generation = GenerationModSet::capture(&content.registry, &content.manifests);
    let generation_mods = generation_mods_to_header_entries(&generation);
    let CurrentModSet(current_entries) =
        CurrentModSet::derive_from(&content.registry, &content.manifests);
    let current_mods = current_mods_to_header_entries(&current_entries);

    let layout = game_world.world.terrain.layout();
    let zone_count = layout.zone_count();

    let header = SaveHeader {
        schema_version: CURRENT_SCHEMA_VERSION,
        saved_at: now_unix_seconds(),
        character_name: character_name.to_string(),
        current_region: current_region.to_string(),
        playtime_ticks: game_world.world.clock.0,
        generation_mods,
        current_mods,
        content_hash_algorithm_version: CONTENT_HASH_ALGORITHM_VERSION,
        content_index_map,
        world_size: (zone_count.width(), zone_count.height()),
        world_seed: game_world.world.seed,
        mode,
    };

    save_to_file(path, &header, &game_world.world)
}

/// 从 `path` 读入存档：把 `content` 手里现成的「当前会话装载结果」
/// 转交给 [`load_full`]，见其文档「完整调用链」——本函数不做任何额外
/// 处理，只是把参数从 [`LoadedContent`] 的字段位置搬到 `load_full` 的
/// 参数位置。
pub fn load_game(path: &Path, content: &LoadedContent) -> LoadOutcome {
    load_full(
        path,
        &content.registry,
        &content.manifests,
        content.terrain_table.clone(),
        &content.script_sources,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::load_content;
    use crate::world::build_new_world;

    fn test_content() -> LoadedContent {
        let dir =
            std::env::temp_dir().join(format!("ll-game-save-test-content-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
        let content = load_content(&dir, &dir.join("assets"));
        let _ = std::fs::remove_dir_all(&dir);
        content
    }

    fn temp_save_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ll-game-save-roundtrip-{name}-{}.llsave",
            std::process::id()
        ));
        path
    }

    #[test]
    fn 存档退出后重进读回的世界哈希与存档前一致() {
        // 硬约束的核心验证：不是「有存档代码」，是存了退出、重进能读回
        // 同一个世界——用 WorldState::hash() 逐位比对,这是规格要求的
        // 判据（与 ll_content::save_file 自己的同名测试同一个理由）。
        // Arrange
        let content = test_content();
        let game_world = build_new_world(&content, 7).expect("测试用布局满足全部前置条件");
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
            SaveMode::Permadeath,
        )
        .expect("写出应当成功");
        drop(game_world);
        let outcome = load_game(&path, &content);

        // Assert
        match outcome {
            LoadOutcome::Playable(loaded_world) => {
                assert_eq!(loaded_world.hash(), hash_before);
            }
            other => panic!("期望 Playable，实际读到 {other:?}"),
        }

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
        let game_world = build_new_world(&content, 123).expect("测试用布局满足全部前置条件");
        let path = temp_save_path("seed-in-header");
        save_game(
            &path,
            &content,
            &game_world,
            "测试旅人",
            "出生地",
            SaveMode::Permadeath,
        )
        .expect("写出应当成功");

        // Act
        let header = ll_content::save_file::load_from_header_only(&path).expect("读头部应当成功");

        // Assert
        assert_eq!(header.world_seed, 123);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
