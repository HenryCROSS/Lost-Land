//! 前置关系图校验接进生产装载路径的**反面证据**。
//!
//! # 补的是哪个缺口
//!
//! `ll_mod::skill::validate_no_cycles`/`ll_mod::quest::validate_no_cycles`
//! 从落地那天起就只有一个调用点：`materialize_base_skills`/
//! `materialize_base_quests` 的函数体内部。而那两个函数**从来不在生产
//! 装载路径上**（`ll_game::content::load_content` 交给装载管线的是两张
//! 空表，见 `ll_mod::class` 模块文档同名一节）——也就是说 ADR 0017
//! 「注册期完整校验」在这两张表上事实落空：**任何 mod 注册的技能/任务，
//! 前置成环也好、指向一条谁都没注册过的条目也好，一次都没有被检查过**。
//!
//! 本体技能/任务迁进脚本的批次把这两条检查接到了 `load_content` 上。
//! 正面证据（真实 `mods/` 目录装载出来的两张表无环）在
//! `crates/ll-mod/tests/base_mod_class_skill_quest.rs`；本文件是 ADR
//! 0018 要求的那一半反面证据：**造一个真的成环的 mod，装载必须整批
//! 失败**。把 `load_content` 里那两行摘掉，本文件两条测试立刻变红。
//!
//! # 为什么要把真实 `mods/` 整个拷进临时目录
//!
//! `load_content` 在跑图校验之前先跑本体内容契约解析（缺任何一条本体
//! 内容就整批失败）。一个只装着「成环 mod」的临时目录会在**更早**的
//! 那一步失败，于是这条测试根本走不到图校验，变成一条恒绿的假证据。
//! 把真实 `mods/` 拷过去再补一个成环 mod，失败原因才真的是图校验。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ll_game::content::{ContentLoadError, load_content};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 本进程内独占的临时目录路径，手法同 `ll_game::test_support`
/// （进程 ID 隔离进程、计数器隔离同进程内的并发调用）。
fn unique_temp_dir(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()))
}

fn copy_dir_recursive(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("创建临时目录应当成功");
    for entry in fs::read_dir(from).expect("读取源目录应当成功") {
        let entry = entry.expect("目录项应当可读");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("文件类型应当可读").is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("拷贝文件应当成功");
        }
    }
}

/// 把真实 `mods/` 拷进一个独占临时目录，再往里加一个 mod。
///
/// 返回临时 mods 目录路径；调用方用完自行删除（失败时刻意保留，方便
/// 人工排查——与本仓库其余临时目录测试同一条惯例）。
fn mods_root_with_extra(prefix: &str, namespace: &str, script: &str) -> PathBuf {
    let root = unique_temp_dir(prefix);
    copy_dir_recursive(&repo_root().join("mods"), &root);

    let extra = root.join(namespace);
    fs::create_dir_all(&extra).expect("创建额外 mod 目录应当成功");
    fs::write(
        extra.join("mod.json5"),
        format!(
            "{{\n  namespace: \"{namespace}\",\n  version: \"0.1.0\",\n  \
             entry_points: [\"content.scm\"],\n}}\n"
        ),
    )
    .expect("写 mod.json5 应当成功");
    fs::write(extra.join("content.scm"), script).expect("写脚本应当成功");
    root
}

#[test]
fn 成环的技能前置让整批装载失败而不是静默进到游戏里() {
    // Arrange：cyclicskills:a 需要 b，b 需要 a——二节点环。
    let mods_root = mods_root_with_extra(
        "ll-game-cyclic-skill",
        "cyclicskills",
        "(register-skill \"cyclicskills:a\" \"\" '(\"cyclicskills:b\") \
         0 \"none\" 0 \"deal-damage\" \"\" 1 0)\n\
         (register-skill \"cyclicskills:b\" \"\" '(\"cyclicskills:a\") \
         0 \"none\" 0 \"deal-damage\" \"\" 1 0)\n",
    );

    // Act
    let result = load_content(&mods_root, &repo_root().join("assets"));

    // Assert
    match &result {
        Err(error @ ContentLoadError::SkillGraph { .. }) => {
            let text = error.to_string();
            assert!(
                text.contains("cyclicskills:a") || text.contains("cyclicskills:b"),
                "错误必须点名构成环的具体技能，实际是：{text}"
            );
        }
        Err(other) => panic!("失败原因必须是技能图校验，实际是 {other}"),
        Ok(_) => panic!("成环的技能前置必须让装载整批失败，实际却装载成功了"),
    }

    let _ = fs::remove_dir_all(&mods_root);
}

#[test]
fn 成环的任务前置让整批装载失败而不是静默进到游戏里() {
    // Arrange：cyclicquests:a 需要 b，b 需要 a。
    let mods_root = mods_root_with_extra(
        "ll-game-cyclic-quest",
        "cyclicquests",
        "(register-quest \"cyclicquests:a\" '(\"cyclicquests:b\") \
         \"kill-count\" \"cyclicquests:target\" 1)\n\
         (register-quest \"cyclicquests:b\" '(\"cyclicquests:a\") \
         \"kill-count\" \"cyclicquests:target\" 1)\n",
    );

    // Act
    let result = load_content(&mods_root, &repo_root().join("assets"));

    // Assert
    match &result {
        Err(error @ ContentLoadError::QuestGraph { .. }) => {
            let text = error.to_string();
            assert!(
                text.contains("cyclicquests:a") || text.contains("cyclicquests:b"),
                "错误必须点名构成环的具体任务节点，实际是：{text}"
            );
        }
        Err(other) => panic!("失败原因必须是任务图校验，实际是 {other}"),
        Ok(_) => panic!("成环的任务前置必须让装载整批失败，实际却装载成功了"),
    }

    let _ = fs::remove_dir_all(&mods_root);
}

#[test]
fn 前置指向一条谁都没注册过的技能同样让整批装载失败() {
    // UnregisteredPrerequisite 那一档——与成环是同一个校验的两种失败，
    // 但 mod 作者要做的事完全不同（一个是解开环，一个是补上那条内容）。
    // Arrange
    let mods_root = mods_root_with_extra(
        "ll-game-ghost-prereq",
        "ghostprereq",
        "(register-skill \"ghostprereq:a\" \"\" '(\"ghostprereq:never_registered\") \
         0 \"none\" 0 \"deal-damage\" \"\" 1 0)\n",
    );

    // Act
    let result = load_content(&mods_root, &repo_root().join("assets"));

    // Assert
    match &result {
        Err(error @ ContentLoadError::SkillGraph { .. }) => {
            let text = error.to_string();
            assert!(
                text.contains("ghostprereq:never_registered"),
                "错误必须点名那条不存在的前置，实际是：{text}"
            );
        }
        Err(other) => panic!("失败原因必须是技能图校验，实际是 {other}"),
        Ok(_) => panic!("悬空前置必须让装载整批失败，实际却装载成功了"),
    }

    let _ = fs::remove_dir_all(&mods_root);
}

#[test]
fn 仓库真实mods目录装载成功证明上面三条不是靠一个恒失败的路径变绿() {
    // 防「空转通过」的另一头：若 `load_content` 因为别的原因（例如
    // 临时目录拷贝不完整）恒失败，上面三条会因为 match 分支不符而 panic
    // ——但若哪天有人把图校验改成「恒失败」，上面三条仍会绿。本条钉住
    // 「正常内容必须装载成功」这一半。
    // Arrange & Act
    let result = load_content(&repo_root().join("mods"), &repo_root().join("assets"));

    // Assert
    if let Err(error) = result {
        panic!("仓库真实 mods/ 目录必须装载成功，实际失败：{error}");
    }
}
