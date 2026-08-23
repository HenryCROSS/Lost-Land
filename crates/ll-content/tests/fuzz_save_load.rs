//! 存档反序列化模糊测试（任务 11）。
//!
//! # 为什么是 `proptest`，不是本次会话里真正跑起来的 `cargo-fuzz`
//!
//! 规格 §14.3、`p4-to-p5.md` 五、3 把「存档反序列化」列为 L5 模糊测试
//! 新的最高优先级目标，判定标准是「任何输入都不得 panic、不得 OOM、
//! 不得无限循环，只允许返回 `Err`」。`cargo-fuzz`（`libFuzzer`）需要
//! nightly 工具链 + `clang`（libFuzzer 的覆盖率插桩由 clang 完成）——
//! 本次实施的开发环境（Windows，`rustup toolchain list` 只有 stable
//! 工具链，`clang` 不在 `PATH` 上）核实过不满足这两个前提，如实记录，
//! 不假装装上了。
//!
//! `crates/ll-content/fuzz/` 已经按 `cargo-fuzz` 的标准目录结构落地了
//! target（`fuzz_targets/save_load.rs`），供工具链齐备的环境（CI、
//! Linux/macOS 开发机）直接 `cargo fuzz run save_load` 使用——本文件
//! 是**本次会话实际能跑起来、且已经跑过**的等价覆盖：用 `proptest`
//! 生成随机字节喂给同一个入口（[`ll_content::save_file::load_full_from_bytes`]），
//! 断言不 panic；额外针对规格点名的四类损坏（截断、篡改字段、畸形
//! 头部、声明长度与实际不符）各写一条定向用例，而不是只依赖纯随机
//! 输入侥幸撞上这些边界。
//!
//! # 为什么直接调用 `load_full_from_bytes` 而不是 `load_full`
//!
//! `load_full` 接受 `&Path`——模糊测试的每一次迭代都要先把生成的字节
//! 写成临时文件再读回，多一层与「输入 ↔ 被测代码」无关的磁盘 I/O。
//! `load_full_from_bytes`（任务 11 从 `load_full` 里拆出的字节级核心
//! 实现，两者共享同一条完整调用链）是 `cargo-fuzz` target 与本文件
//! 共同的真正入口。

use ll_mod::registry::Registry;
use ll_world::generate::GenParams;
use ll_world::terrain::materialize_base_terrain;
use ll_world::zone::ZoneLayout;
use proptest::prelude::*;

use ll_content::header::{ModHeaderEntry, SaveHeader};
use ll_content::mode::SaveMode;
use ll_content::save_file::{load_full_from_bytes, save_to_file};

/// 建一份结构完全合法、写出到临时文件后再读回原始字节的存档——供各条
/// 定向用例在此基础上做局部损坏，而不是每条用例各自手搓一份存档字节
/// （手搓格式细节容易漂移，且不是本文件要验证的东西——本文件验证的是
/// 「损坏的输入不会让读档崩溃」，不是「构造合法存档的能力」，那已经由
/// `save_file.rs`/`remap.rs` 自己的测试覆盖）。
fn valid_save_bytes() -> Vec<u8> {
    let mut registry = Registry::new();
    let (terrain_ids, terrain_table) =
        materialize_base_terrain(&mut |id| registry.intern(id)).expect("本体地形声明表内部一致");
    let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 合法");
    let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐约束");
    let spawn = layout.tile_size().wrap(0, 0);
    let world = ll_world::state::WorldState::new(
        layout,
        &GenParams::default(),
        &terrain_ids,
        terrain_table,
        spawn,
    )
    .expect("测试布局满足全部构造前置条件");

    let header = SaveHeader {
        schema_version: ll_content::save_file::CURRENT_SCHEMA_VERSION,
        saved_at: 1_755_000_000,
        character_name: "旅人".to_string(),
        current_region: "初始村落".to_string(),
        playtime_ticks: 0,
        generation_mods: vec![ModHeaderEntry {
            namespace: "lostland".to_string(),
            version: "0.1.0".to_string(),
            content_hash: registry.content_hash_of("lostland"),
        }],
        current_mods: Vec::new(),
        content_hash_algorithm_version: ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION,
        content_index_map: registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect(),
        world_size: (1, 1),
        world_seed: 0,
        mode: SaveMode::Permadeath,
    };

    // 文件名必须对每次调用唯一——proptest 的属性测试函数在同一进程内
    // 会被反复调用,且不同 `#[test]` 函数之间 Rust 测试框架默认并行
    // 执行,只用进程号做文件名会导致多个调用互相踩到同一个文件（一个
    // 调用删除了另一个调用刚写出、还没读完的文件）。原子计数器保证
    // 同进程内每次调用都拿到一个全新的文件名。
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "ll-content-fuzz-baseline-{}-{unique}.llsave",
        std::process::id()
    ));
    save_to_file(&path, &header, &world).expect("写出合法存档不应失败");
    let bytes = std::fs::read(&path).expect("刚写出的文件必然可读");
    let _ = std::fs::remove_file(&path);
    bytes
}

fn fresh_registry_and_terrain() -> (Registry, ll_world::terrain::TerrainTable) {
    let mut registry = Registry::new();
    let (_ids, terrain_table) =
        materialize_base_terrain(&mut |id| registry.intern(id)).expect("本体地形声明表内部一致");
    (registry, terrain_table)
}

/// 对任意字节调用 `load_full_from_bytes`，只断言「不 panic」——不关心
/// 结果是 `Playable`/`ReadOnly`/`Rejected` 中的哪一种,那是各自逻辑
/// 的正确性,不是模糊测试要判定的范围（模糊测试的判据是规格 §14.3：
/// 不 panic、不 OOM、不死循环）。
fn assert_does_not_panic(data: &[u8]) {
    let (registry, terrain_table) = fresh_registry_and_terrain();
    let result = std::panic::catch_unwind(|| {
        load_full_from_bytes(data, &registry, &[], terrain_table.clone())
    });
    assert!(
        result.is_ok(),
        "load_full_from_bytes 在以下输入上 panic 了（长度 {}）：{:?}",
        data.len(),
        &data[..data.len().min(64)]
    );
}

proptest! {
    /// 完全随机的字节流——覆盖规格点名的「畸形头部」「声明长度与实际
    /// 不符」这两类损坏里数量最庞大的一部分（绝大多数随机字节根本不是
    /// 合法 JSON,或长度前缀本身就是垃圾值）。
    #[test]
    fn 任意随机字节都不会让读档崩溃(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        assert_does_not_panic(&data);
    }

    /// 截断合法存档：从一份结构完全合法的存档里截取任意长度的前缀。
    /// 覆盖规格点名的「被截断的字节流」——不止头部长度前缀那 4 个字节
    /// 可能被截断，头部 JSON 内部、主体压缩数据内部的任意截断点都要
    /// 覆盖。
    #[test]
    fn 截断合法存档的任意前缀都不会让读档崩溃(cut_ratio in 0.0f64..1.0) {
        let full = valid_save_bytes();
        let cut_len = ((full.len() as f64) * cut_ratio) as usize;
        assert_does_not_panic(&full[..cut_len]);
    }

    /// 篡改合法存档里的任意单个字节——覆盖规格点名的「被篡改的字段」。
    /// 篡改点随机落在整个文件的任意位置（头部 JSON 内部的字段值、
    /// 压缩后的主体二进制内部都有可能被选中）。
    #[test]
    fn 篡改合法存档中的任意单字节都不会让读档崩溃(
        index_ratio in 0.0f64..1.0,
        new_byte in any::<u8>(),
    ) {
        let mut tampered = valid_save_bytes();
        if !tampered.is_empty() {
            let index = ((tampered.len() as f64) * index_ratio) as usize % tampered.len();
            tampered[index] = new_byte;
        }
        assert_does_not_panic(&tampered);
    }
}

/// 畸形头部：头部长度前缀合法，但头部字节本身不是合法 JSON。
#[test]
fn 头部长度前缀合法但内容不是合法json时不会崩溃() {
    let mut data = 5u32.to_le_bytes().to_vec(); // 声称头部长度 5 字节
    data.extend_from_slice(b"nope!"); // 5 字节,但不是合法 JSON
    assert_does_not_panic(&data);
}

/// 声明长度与实际不符——头部长度前缀声称的长度远超实际剩余字节数。
#[test]
fn 头部声明长度超出实际剩余字节数时不会崩溃() {
    let data = u32::MAX.to_le_bytes().to_vec(); // 声称头部长度 40 亿字节
    assert_does_not_panic(&data);
}

/// 声明长度与实际不符——主体压缩数据前缀声称的解压后长度远超真实值,
/// 核实这不会触发 `lz4_flex` 尝试预分配一个天文数字大小的 `Vec`
/// （见 `save_file` 模块文档 `MAX_BODY_DECOMPRESSED_LEN` 的说明）。
#[test]
fn 主体声明的解压后长度超出安全上限时不会崩溃或试图分配巨量内存() {
    let mut full = valid_save_bytes();
    // 找到主体压缩数据的起始偏移：4 字节长度前缀 + 头部 JSON 长度。
    let header_len = u32::from_le_bytes(full[0..4].try_into().unwrap()) as usize;
    let body_start = 4 + header_len;
    // lz4_flex 的 `compress_prepend_size` 格式：压缩数据自己的前 4
    // 字节是解压后长度（小端）——篡改成一个天文数字。
    full[body_start..body_start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_does_not_panic(&full);
}

/// mod 内容哈希被篡改成一个不存在的命名空间数值——覆盖「篡改字段」
/// 里语义层面（不是字节层面）的一种：头部本身是合法 JSON，只是记录的
/// 内容与当前会话对不上。
///
/// # 断链二修复后，具体的 `LoadError` 变体变了（如实记录）
///
/// 这条用例把 `header_json` 里所有的 "lostland" 都替换成
/// "nonexist"——不只是 `generation_mods[0].namespace`，`content_index_map`
/// 里的每一条地形字符串（形如 `"lostland:mountain"`）也一并被换成了
/// `"nonexist:mountain"`。`current_manifests` 传的是 `&[]`（空），P5-A
/// 任务 14 断链二修复之后，`check_mod_content` 在 "nonexist" 命名空间
/// 完全不在 manifests 里时不再硬拒绝（那是留给 `remap_world` 的「mod
/// 不在了」放行分支，见其文档），判断因此推迟到 `remap_world`——它会
/// 在重映射地表地形（结构性内容,没有可用的降级语义）时发现
/// `"nonexist:mountain"` 在当前会话查不到，报
/// `LoadError::Corrupted`（不是 `ModContentMismatch`）。两者都是
/// `LoadOutcome::Rejected`，都不崩溃——这条用例的判据本来就是「不会
/// 崩溃而是拒绝」，不是「必须报出哪一个具体变体」，因此这里放宽到判定
/// 结果属于 `Rejected` 家族，不钉死具体是哪一个 `LoadError` 变体。
#[test]
fn 生成期mod集合被篡改成不存在的命名空间时不会崩溃而是拒绝() {
    let bytes = valid_save_bytes();
    let header_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let header_json =
        std::str::from_utf8(&bytes[4..4 + header_len]).expect("测试固件恒是合法 UTF-8");
    // 替换成同样 8 字节长度的字符串,不需要额外调整长度前缀——
    // "lostland" 与 "nonexist" 都是 8 个 ASCII 字符。
    let tampered_json = header_json.replace("lostland", "nonexist");
    assert_eq!(
        tampered_json.len(),
        header_json.len(),
        "替换前后长度必须一致,否则长度前缀也要跟着改,这不是本用例要测的东西"
    );

    let mut data = (tampered_json.len() as u32).to_le_bytes().to_vec();
    data.extend_from_slice(tampered_json.as_bytes());
    data.extend_from_slice(&bytes[4 + header_len..]);

    let (registry, terrain_table) = fresh_registry_and_terrain();
    let outcome = load_full_from_bytes(&data, &registry, &[], terrain_table);
    assert!(matches!(
        outcome,
        ll_content::degrade::LoadOutcome::Rejected(_)
    ));
}
