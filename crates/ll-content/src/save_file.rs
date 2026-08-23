//! 存档主体读写管线：把任务 1–8 各自独立测试过的组件真正串成一条
//! 可用的存档 → 读档路径。
//!
//! 本模块是本批次改动面最大的一步——把已经分别验证过的部件接线起来，
//! 不重新发明任何一部件自己的逻辑（`SaveHeader`/`MigrationChain`/
//! `content_index_map`/`degrade`/`load_error`/[`crate::remap`] 各自的
//! 正确性已由各自模块的测试锁住,这里只验证串联本身）。
//!
//! # 物理布局：单文件两段，头部不需要解压主体就能读到
//!
//! 规格 §11.2「存档列表界面只读头部」要求 [`load_from_header_only`]
//! 不得触发主体解压。本模块选**单文件、两段**的布局（不是两个独立
//! 文件）——头部是明文 JSON,主体是 `postcard` 编码后再经 `lz4_flex`
//! 压缩的二进制，两者用一个 4 字节小端长度前缀分隔：
//!
//! ```text
//! [4 字节：头部 JSON 长度，小端 u32] [头部 JSON] [lz4_flex 压缩后的主体]
//! ```
//!
//! 单文件的理由：存档在文件系统里是一个天然的原子单位（复制、删除、
//! 云同步都按文件走），拆成两个文件要求调用方自己保证两者不会不同步
//! 出现（例如复制了头部文件却漏了主体文件），单文件从物理上排除了
//! 这类问题；代价是「只读头部」必须靠精确控制读取字节数做到——本文件
//! [`load_from_header_only`] 只读取 `4 + 头部长度` 字节，不 `read_to_end`，
//! 因此不会触发主体解压，这条性质由代码结构直接保证，不依赖约定。
//!
//! # 关于 VM 强制重建与 `terrain_table` 重新灌入
//!
//! [`load_full`] 的签名比计划文档「概念形状」多两个参数
//! （`current_terrain_table`/`current_script_sources`）——这是本任务
//! 落地过程中发现的真实缺口，如实记录：`ll-content` 不知道如何从一个
//! `Registry` 推出一张 `TerrainTable`（那需要具体的地形定义与
//! `materialize_base_terrain` 之类的注册期函数，属于 `ll-mod` 装载
//! 管线的职责，不是 `ll-content` 该重新实现的事），也不持有已装载 mod
//! 的脚本源码文本（那同样是装载管线读文件的产物）。调用方（已经跑完
//! 一次 mod 装载、手里同时有 `Registry`/`ModManifest`/`TerrainTable`/
//! 脚本源码的一方）把这两样一并传入，比让 `load_full` 试图自己重新
//! 装载一遍 mod 更诚实——那不是「读档」这一步该做的事。

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION;
use ll_mod::manifest::ModManifest;
use ll_mod::registry::Registry;
use ll_world::state::WorldState;
use ll_world::terrain::TerrainTable;

use crate::degrade::{LoadOutcome, summarize_load_outcome};
use crate::header::SaveHeader;
use crate::load_error::{
    LoadError, check_content_hash_algorithm, check_mod_content, check_mod_set, check_schema_version,
};
use crate::migration::MigrationChain;
use crate::remap::remap_world;

/// 当前游戏认识的唯一存档 schema 版本。
///
/// # 为什么重置为 1，而不是继续从发布前累计到的 4 往上加
///
/// 发布前的开发过程中，这个常量曾经从 1 一路加到 4——落地探索记忆
/// 批次（`Interior::origin`/`WorldState::exploration`）、击杀与死亡
/// 记录批次（`WorldState::history`/`next_world_id`）、无名单位击杀
/// 计数批次（`WorldState::kill_counts`）各贡献一次破坏性存档结构变化，
/// 配套的迁移函数（`Migration1To2`/`Migration2To3`/`Migration3To4`，
/// 曾经落在 `crate::migrations`）与「形状变了」的镜像类型逐批注册进
/// [`migration_chain`]。项目所有者复核后裁定「老存档去掉就好了」：
/// 项目尚未发布，此前累计的全部存档都是开发期产物，没有任何一份值得
/// 维护迁移路径去兼容。继续背着这条越来越长的链，意味着往后
/// `WorldState`/`Agent`/`Interior` 每新增一个字段都要多付一份镜像
/// 类型与手写字节测试的维护成本，而它保护的对象在发布后根本不存在
/// ——把常量重置为 1、清空 [`crate::migrations`] 里的具体迁移函数
/// （见其模块文档），是「项目尚未发布」这个阶段独有的清理窗口。
///
/// 重置后任何 `schema_version != 1` 的存档都会被明确拒绝
/// （[`crate::load_error::LoadError::SchemaTooNew`]/
/// [`crate::load_error::LoadError::SchemaMigrationGap`]，见
/// [`migration_chain`] 文档与本模块测试「旧版本号存档被拒绝」一节），
/// 不会被静默按当前字段布局误解析。
///
/// # 迁移框架本身没有被删除
///
/// [`crate::migration::MigrationChain`] 依旧是读档管线的一环（见
/// [`migration_chain`]），只是目前注册的迁移函数集合为空——发布之后
/// 真的需要升级 schema 时，新的迁移函数照常实现
/// [`crate::migration::Migration`] 并注册进 `migration_chain`，这个
/// 常量再次成为「往上加一」的那一个，不需要重新设计版本判定或
/// 迁移执行的机制。
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// 头部 JSON 长度声明的安全上限——防御「声明长度与实际不符」类畸形
/// 存档（规格 §14.3 fuzz 要求之一）：一个只有几十字节的文件却在长度
/// 前缀里写了一个天文数字，若不设上限，`vec![0u8; header_len]` 会在
/// 读到任何真实数据之前就尝试分配巨量内存。真实存档头部是明文 JSON，
/// 远小于这个上限（`SaveHeader` 测试固件不到 1KB）。
const MAX_HEADER_LEN: u32 = 16 * 1024 * 1024;

/// 存档主体解压后长度声明的安全上限，理由与 [`MAX_HEADER_LEN`] 相同
/// ——`lz4_flex::decompress_size_prepended` 内部按声明的长度整个预分配
/// 一个 `Vec`（[`lz4_flex`] 0.11.6 `decompress` 实现），不会自己核对
/// 这个声明是否合理，必须在调用它之前由本模块自己拦一道。
const MAX_BODY_DECOMPRESSED_LEN: usize = 512 * 1024 * 1024;

/// 存档写出失败的原因。
#[derive(Debug)]
pub enum SaveError {
    /// 存档头部编码为 JSON 失败——`SaveHeader` 全部字段都是 serde
    /// 标准可派生类型，正常情况下不会发生。
    HeaderEncode(serde_json::Error),
    /// 存档主体编码为 `postcard` 二进制失败。
    BodyEncode(postcard::Error),
    /// 头部编码后的长度超出 `u32` 能表达的范围——不应该在真实场景发生
    /// （头部是人类可读的元数据,不会有几个 GB），列出这个变体只是为了
    /// 不让 `as u32` 转换悄悄截断。
    HeaderTooLarge(usize),
    /// 文件系统 I/O 失败。
    Io(std::io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::HeaderEncode(err) => write!(f, "存档头部编码失败：{err}"),
            SaveError::BodyEncode(err) => write!(f, "存档主体编码失败：{err}"),
            SaveError::HeaderTooLarge(len) => write!(f, "存档头部编码后长度 {len} 超出可表达范围"),
            SaveError::Io(err) => write!(f, "存档写入失败：{err}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// 当前认识的 schema 迁移链——目前是空的，见
/// [`CURRENT_SCHEMA_VERSION`] 文档「为什么重置为 1」一节与
/// [`crate::migrations`] 模块文档：发布前累计过的三步迁移
/// （v1→v2→v3→v4）已随「老存档去掉就好了」的裁定一并清空。
///
/// # 为什么这个函数还在，没有跟着一起删掉
///
/// [`load_full_from_bytes`] 仍然在 `header.schema_version <
/// CURRENT_SCHEMA_VERSION` 时调用这条链——保留这次调用，是保留「迁移
/// 框架接在真实读档路径上」这件事本身：[`crate::migration::MigrationChain`]
/// 空载时的行为（[`MigrationChain::apply`] 找不到已知版本就报
/// [`crate::migration::MigrationError::NoPathFrom`]，经
/// [`crate::load_error::LoadError`] 的 `From` 实现转换成
/// [`crate::load_error::LoadError::SchemaMigrationGap`]）本身就是「老
/// 存档被明确拒绝」这条要求的一部分，不是需要绕开的死代码。发布之后
/// 真的需要升级 schema 时，新的迁移函数只需要加进这里的 `vec!`，
/// [`load_full_from_bytes`] 调用这条链的方式不需要跟着变。
fn migration_chain() -> MigrationChain {
    MigrationChain::new(Vec::new())
}

/// 把 `world` 与 `header` 写出到 `path`，见模块文档「物理布局」。
pub fn save_to_file(path: &Path, header: &SaveHeader, world: &WorldState) -> Result<(), SaveError> {
    let header_json = serde_json::to_vec(header).map_err(SaveError::HeaderEncode)?;
    let header_len = u32::try_from(header_json.len())
        .map_err(|_| SaveError::HeaderTooLarge(header_json.len()))?;
    let body_raw = postcard::to_allocvec(world).map_err(SaveError::BodyEncode)?;
    let body_compressed = lz4_flex::compress_prepend_size(&body_raw);

    let mut file = File::create(path).map_err(SaveError::Io)?;
    file.write_all(&header_len.to_le_bytes())
        .map_err(SaveError::Io)?;
    file.write_all(&header_json).map_err(SaveError::Io)?;
    file.write_all(&body_compressed).map_err(SaveError::Io)?;
    Ok(())
}

/// 只读取存档头部，不触碰主体（不解压、不反序列化）——供存档列表界面
/// 使用，见模块文档「物理布局」。
pub fn load_from_header_only(path: &Path) -> Result<SaveHeader, LoadError> {
    let mut file =
        File::open(path).map_err(|err| LoadError::Corrupted(format!("无法打开存档文件：{err}")))?;
    read_header_prefix(&mut file)
}

/// 从一个已打开的文件句柄读取「长度前缀 + 头部 JSON」这一段，读完后
/// 文件游标恰好停在主体第一个字节——[`load_from_header_only`] 与
/// `load_full` 内部共享这段逻辑，避免两处各写一份等价的截断/解析
/// 校验代码。
fn read_header_prefix(file: &mut File) -> Result<SaveHeader, LoadError> {
    let mut len_bytes = [0u8; 4];
    file.read_exact(&mut len_bytes)
        .map_err(|_| LoadError::Corrupted("存档文件被截断（读取头部长度前缀失败）".to_string()))?;
    let header_len = u32::from_le_bytes(len_bytes);
    if header_len > MAX_HEADER_LEN {
        return Err(LoadError::Corrupted(format!(
            "存档头部声明长度 {header_len} 超出安全上限 {MAX_HEADER_LEN}，疑似被篡改"
        )));
    }

    let mut header_bytes = vec![0u8; header_len as usize];
    file.read_exact(&mut header_bytes)
        .map_err(|_| LoadError::Corrupted("存档文件被截断（读取头部内容失败）".to_string()))?;
    serde_json::from_slice(&header_bytes)
        .map_err(|err| LoadError::Corrupted(format!("存档头部 JSON 解析失败：{err}")))
}

/// [`read_header_prefix`] 的字节切片版本：从 `cursor` 开头读取「长度
/// 前缀 + 头部 JSON」，成功时把 `cursor` 前移到主体第一个字节——供
/// [`load_full_from_bytes`]/模糊测试 target 使用。
///
/// 与文件版本共享同一套截断/长度上限校验逻辑，只是数据源从 `Read`
/// 换成对切片做手工下标——`&[u8]` 没有 `Read::read_exact` 天然可用的
///「不够字节就报错」语义（`&[u8]` 确实实现了 `Read`，但这里手写更直接：
/// 不需要为了复用 `Read` trait 而先把切片包一层 `std::io::Cursor`）。
fn read_header_prefix_from_slice(cursor: &mut &[u8]) -> Result<SaveHeader, LoadError> {
    if cursor.len() < 4 {
        return Err(LoadError::Corrupted(
            "存档文件被截断（读取头部长度前缀失败）".to_string(),
        ));
    }
    let (len_bytes, rest) = cursor.split_at(4);
    let header_len = u32::from_le_bytes(len_bytes.try_into().expect("上面已确认切片长度恰为 4"));
    if header_len > MAX_HEADER_LEN {
        return Err(LoadError::Corrupted(format!(
            "存档头部声明长度 {header_len} 超出安全上限 {MAX_HEADER_LEN}，疑似被篡改"
        )));
    }

    if rest.len() < header_len as usize {
        return Err(LoadError::Corrupted(
            "存档文件被截断（读取头部内容失败）".to_string(),
        ));
    }
    let (header_bytes, rest) = rest.split_at(header_len as usize);
    let header = serde_json::from_slice(header_bytes)
        .map_err(|err| LoadError::Corrupted(format!("存档头部 JSON 解析失败：{err}")))?;
    *cursor = rest;
    Ok(header)
}

/// 解压存档主体，先核实声明的解压后长度在安全上限内——见
/// [`MAX_BODY_DECOMPRESSED_LEN`] 文档。
fn decompress_body(compressed: &[u8]) -> Result<Vec<u8>, LoadError> {
    let size_prefix = compressed
        .get(..4)
        .ok_or_else(|| LoadError::Corrupted("存档主体被截断（缺少压缩长度前缀）".to_string()))?;
    let declared_size =
        u32::from_le_bytes(size_prefix.try_into().expect("上面已确认切片长度恰为 4"));
    if declared_size as usize > MAX_BODY_DECOMPRESSED_LEN {
        return Err(LoadError::Corrupted(format!(
            "存档主体声明的解压后长度 {declared_size} 超出安全上限 {MAX_BODY_DECOMPRESSED_LEN}，疑似被篡改"
        )));
    }
    lz4_flex::decompress_size_prepended(compressed)
        .map_err(|err| LoadError::Corrupted(format!("存档主体解压失败：{err}")))
}

/// 完整读档：串联本计划任务 1–8 的全部组件，产出 [`LoadOutcome`]。
///
/// 参数：
/// - `current_registry`/`current_manifests`：当前会话已完成的 mod
///   装载结果——`current_manifests` 除了未来展示「当前装了哪些 mod」，
///   现在也被 [`check_mod_content`] 用来分清「mod 仍在场但内容变了」
///   与「mod 完全不在场」（见其文档，P5-A 任务 14 断链二修复）。
/// - `current_terrain_table`：当前会话按同一次装载重新注册出的地形
///   属性表——本函数负责把它灌回读出的 `WorldState` 并调用
///   [`WorldState::assert_terrain_table_loaded`] 校验，但不负责生成它
///   （见模块文档「关于 VM 强制重建与 terrain_table 重新灌入」）。
/// - `current_script_sources`：当前会话已装载的 `(mod 命名空间, 脚本
///   源码)` 列表，转交
///   [`ll_script::host::rebuild_all_engines_after_load`]——存档读入
///   意味着世界状态被替换成另一个时间点的快照，VM 必须强制从零重建
///   （约束 C1 修订版，见该函数文档）。
///
/// 完整调用链：读头部 → schema 版本判定（必要时走迁移链）→ **mod 集合
/// 硬门禁**（[`check_mod_set`]，决策二：mod 缺失或版本不对直接拒绝，
/// 见其文档）→ 解压 + 反序列化主体 → **VM 强制重建**（世界即将被
/// 替换）→ mod 内容哈希校验 → `ContentIndex` 重映射
/// （[`crate::remap::remap_world`]）→ 灌回 `terrain_table` 并**显式
/// 校验** → 汇总降级决策产出 [`LoadOutcome`]。任何一步失败都提前返回
/// [`LoadOutcome::Rejected`]，不会把一个已知不自洽的 `WorldState`
/// 交给调用方。
///
/// # 为什么 `check_mod_set` 排在解压主体之前
///
/// 决策二是一条硬门禁：mod 缺失或版本不对根本不允许进入这份存档,不需要
/// 先解压、迁移、反序列化一整个 `WorldState` 才能知道这件事——`header`
/// 已经带着判定所需的全部信息（`generation_mods`）。提前到这里既能让
/// 判定失败时更快返回,也让「决策二命中」与「存档主体本身是否完整」这
/// 两件事在阅读代码时不需要靠先后顺序猜测谁先谁后。
pub fn load_full(
    path: &Path,
    current_registry: &Registry,
    current_manifests: &[ModManifest],
    current_terrain_table: TerrainTable,
    current_script_sources: &[(String, String)],
) -> LoadOutcome {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) => {
            return LoadOutcome::Rejected(LoadError::Corrupted(format!("无法打开存档文件：{err}")));
        }
    };
    let mut whole_file = Vec::new();
    if let Err(err) = file.read_to_end(&mut whole_file) {
        return LoadOutcome::Rejected(LoadError::Corrupted(format!("读取存档文件失败：{err}")));
    }
    load_full_from_bytes(
        &whole_file,
        current_registry,
        current_manifests,
        current_terrain_table,
        current_script_sources,
    )
}

/// [`load_full`] 的字节级核心实现：接受一段完整的存档文件内容（等价于
/// 直接把整份文件读进内存），不接触文件系统。
///
/// 拆出这个版本有两个理由：
///
/// 1. **可测试性**——[`load_full`] 各条测试都要先落盘再读回，这个版本
///    可以直接构造字节数组,省掉临时文件的开销与清理代码。
/// 2. **可模糊测试性（任务 11）**——`cargo-fuzz` 的 fuzz target 签名是
///    `fn(data: &[u8])`,若唯一入口是接受 `&Path` 的 [`load_full`],
///    每次模糊测试迭代都要先把 fuzzer 产出的字节写成一个临时文件再
///    读回,徒增一次磁盘 I/O 且让「输入」与「被测函数实际处理的数据」
///    隔着一层文件系统——本函数是模糊测试 target
///    （`fuzz/fuzz_targets/save_load.rs`）真正调用的入口。
///
/// 参数与完整调用链见 [`load_full`] 文档，两者唯一的区别是数据来源。
pub fn load_full_from_bytes(
    data: &[u8],
    current_registry: &Registry,
    current_manifests: &[ModManifest],
    current_terrain_table: TerrainTable,
    current_script_sources: &[(String, String)],
) -> LoadOutcome {
    let mut cursor = data;
    let header = match read_header_prefix_from_slice(&mut cursor) {
        Ok(header) => header,
        Err(err) => return LoadOutcome::Rejected(err),
    };

    if let Err(err) = check_schema_version(header.schema_version, CURRENT_SCHEMA_VERSION) {
        return LoadOutcome::Rejected(err);
    }

    // 决策二硬门禁：见本函数文档「为什么 check_mod_set 排在解压主体
    // 之前」一节——不需要先解压/迁移/反序列化整个存档主体就能判定。
    if let Err(err) = check_mod_set(&header.generation_mods, current_manifests) {
        return LoadOutcome::Rejected(err);
    }

    let body_compressed = cursor;
    let body_raw = match decompress_body(body_compressed) {
        Ok(body) => body,
        Err(err) => return LoadOutcome::Rejected(err),
    };

    let body_ready = if header.schema_version < CURRENT_SCHEMA_VERSION {
        match migration_chain().apply(header.schema_version, body_raw) {
            Ok(body) => body,
            Err(err) => return LoadOutcome::Rejected(err.into()),
        }
    } else {
        body_raw
    };

    let mut world: WorldState = match postcard::from_bytes(&body_ready) {
        Ok(world) => world,
        Err(err) => {
            return LoadOutcome::Rejected(LoadError::Corrupted(format!("存档主体解码失败：{err}")));
        }
    };

    // VM 强制重建：世界状态从这一刻起就是「另一个时间点的快照」，见
    // 本函数文档「完整调用链」。不检查返回值——单个 mod 引擎重建失败
    // （脚本源码本身有问题）不是本函数要处理的失败类别，那属于 mod
    // 装载管线自身的诊断范围；这里只保证「重建确实被触发」这条动作本
    // 身发生。
    //
    // # 为什么要另开一根线程
    //
    // 读档发生在 mod 装载**之后**，调用线程早就编译过脚本了；直接在
    // 这里构造引擎正是 ADR 0028 定位到的「先编译、后构造」相邻关系，
    // 也会被 `ll_script::host` 的构造阶段断言当场拦下。新线程天生处在
    // 自己的构造阶段（`thread_local!` 初值为 `false`），而且 steel-core
    // 的内核镜像本身就是线程局部的——换线程等于拿到一份全新的、没有
    // 被任何编译动作污染过的内核。
    //
    // 返回的引擎在这根线程上就地丢弃：`ScriptEngine` 内部是 `Rc`，
    // 不是 `Send`，本来也搬不回来；而本调用点原本就没有使用它们
    // （上面那个 `let _ =`），重建计数器 `REBUILD_COUNT` 是进程级
    // 原子量，跨线程照样可见，「重建确实发生」这条断言不受影响。
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let _ = ll_script::host::rebuild_all_engines_after_load(current_script_sources);
        });
    });

    // 必须排在 check_mod_content 之前：算法版本不一致时,内容哈希数值
    // 本身就不可比较,继续跑 check_mod_content 只会把"存档写于算法升级
    // 之前"误判成"mod 内容真的变了"，见 check_content_hash_algorithm
    // 文档「为什么必须与 check_mod_content 分开」一节。
    if let Err(err) = check_content_hash_algorithm(
        header.content_hash_algorithm_version,
        CONTENT_HASH_ALGORITHM_VERSION,
    ) {
        return LoadOutcome::Rejected(err);
    }

    if let Err(err) =
        check_mod_content(&header.generation_mods, current_registry, current_manifests)
    {
        return LoadOutcome::Rejected(err);
    }

    // 占位索引：从当前会话的 registry 里查询本体占位内容（P5-A 任务 14
    // 断链一修复，见 ll_mod::base_placeholder 模块文档）——不在这里
    // 注册（`current_registry` 只是 `&Registry`，读档这一刻没有能力
    // 也不应该反过来往注册表里塞新内容，注册是启动时装载阶段的职责）。
    // 若调用方传入的 registry 从未注册过占位内容（例如某些测试特意
    // 构造的最小注册表），这里诚实地拿到 None，`remap_world` 会按
    // crate::degrade 模块文档「ContentIndex 缺占位值的既知债务」退化为
    // Reject，不会伪造一个可能指向错误内容的索引。
    let placeholder = ll_mod::base_placeholder::base_placeholder_index(current_registry);
    let degrade_actions = match remap_world(
        &mut world,
        &header.content_index_map,
        current_registry,
        placeholder,
    ) {
        Ok(actions) => actions,
        Err(err) => return LoadOutcome::Rejected(err),
    };

    world.terrain_table = current_terrain_table;
    if world.assert_terrain_table_loaded().is_err() {
        return LoadOutcome::Rejected(LoadError::Corrupted(
            "terrain_table 读档后校验未通过（调用方传入的表为空）".to_string(),
        ));
    }

    summarize_load_outcome(world, &degrade_actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{ContentIndex, NamespacedId};
    use ll_core::torus::TorusSize;
    use ll_mod::registry::Registry;
    use ll_world::generate::GenParams;
    use ll_world::terrain::{base_terrain_fixture, materialize_base_terrain};
    use ll_world::zone::ZoneLayout;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    /// 建一个带真实地形数据的测试世界，连同「写出存档那一刻」对应的
    /// `Registry`（其 `snapshot()` 即写入头部的 `content_index_map`）
    /// ——凡是需要 `load_full` 真正走到 `Playable`/`ReadOnly` 分支的
    /// 测试都必须用这个而不是裸 [`test_world`]：`WorldState::new` 恒
    /// 预热出生点邻域,地形从不为空,`content_index_map` 必须覆盖它。
    fn test_world_with_save_registry() -> (WorldState, Registry) {
        let mut registry = Registry::new();
        let (terrain_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
            .expect("本体地形声明表内部一致，注册恒不失败");
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        (world, registry)
    }

    /// 与 [`test_world_with_save_registry`] 地形内容逐字符串一致的
    /// 「当前会话」注册表 + 配套的 `TerrainTable`（供 `load_full` 的
    /// `current_terrain_table` 参数使用），供各测试在此基础上叠加各自
    /// 需要的角色内容。
    fn current_session_registry_with_terrain() -> (Registry, TerrainTable) {
        let mut registry = Registry::new();
        let (_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
            .expect("本体地形声明表内部一致，注册恒不失败");
        (registry, terrain_table)
    }

    /// 与 [`current_session_registry_with_terrain`] 相同，另外注册了
    /// 本体占位内容（[`ll_mod::base_placeholder::register_base_placeholder_content`]）
    /// ——供需要验证 `load_full` 真的能走到
    /// `DegradeAction::FallbackToPlaceholder` 分支的测试使用（P5-A 任务
    /// 14 断链一修复：这条分支此前在完整读档管线里永远不可达）。
    fn current_session_registry_with_terrain_and_placeholder() -> (Registry, TerrainTable) {
        let mut registry = Registry::new();
        let (_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
            .expect("本体地形声明表内部一致，注册恒不失败");
        ll_mod::base_placeholder::register_base_placeholder_content(&mut registry);
        (registry, terrain_table)
    }

    fn sample_header(content_index_map: Vec<String>) -> SaveHeader {
        SaveHeader {
            schema_version: CURRENT_SCHEMA_VERSION,
            saved_at: 1_755_000_000,
            character_name: "旅人".to_string(),
            current_region: "初始村落".to_string(),
            playtime_ticks: 0,
            generation_mods: Vec::new(),
            current_mods: Vec::new(),
            content_hash_algorithm_version: CONTENT_HASH_ALGORITHM_VERSION,
            content_index_map,
            world_size: (1, 1),
            world_seed: 0,
            mode: crate::mode::SaveMode::Permadeath,
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ll-content-save-file-test-{name}-{}.llsave",
            std::process::id()
        ));
        path
    }

    #[test]
    fn 存档写出后读出的头部与原头部逐字段相等() {
        // Arrange
        let path = temp_path("header-roundtrip");
        let header = sample_header(Vec::new());
        let world = test_world();
        save_to_file(&path, &header, &world).expect("写出应当成功");

        // Act
        let loaded = load_from_header_only(&path).expect("读头部应当成功");

        // Assert
        assert_eq!(loaded, header);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 完整往返后世界哈希与存档前一致() {
        // 规格要求「存档 → 读档后世界逐位一致」——用 WorldState::hash()
        // 比对,这是本任务（也是本计划）最核心的一条正确性判据。
        // Arrange
        let path = temp_path("full-roundtrip-hash");
        let (mut world, mut registry) = test_world_with_save_registry();
        let profession = registry.intern(id("lostland:farmer"));
        let race = registry.intern(id("lostland:human"));
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let player = world.actors.spawn(ll_world::entity::Agent {
            pos: world.size.wrap(1, 1),
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: ll_world::entity::Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 7,
            profession,
            goals: Vec::new(),
            race,
            mana: ll_world::entity::Agent::STARTING_MANA,
            stamina: ll_world::entity::Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        world.player_entity = Some(player);
        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let header = sample_header(content_index_map.clone());
        let hash_before = world.hash();
        save_to_file(&path, &header, &world).expect("写出应当成功");

        // Act：读档用的是「mod 集合原样未变」这一最常见场景——当前会话
        // 的 registry 按存档头 content_index_map 同样的顺序重建
        // （`rebuild_from_header`），索引分配因此与写出时完全一致。
        // 「重映射按字符串对号、不按索引数值巧合」这条性质已经由
        // `crate::remap` 模块自己的测试用刻意打乱的顺序单独锁住,这里
        // 的重点是另一条独立的性质：往返后世界必须逐位不变,不能因为
        // 走了一趟重映射就悄悄漂移。
        let current_registry = crate::content_index_map::rebuild_from_header(&content_index_map)
            .expect("content_index_map 全部由本测试自己产出,恒合法");
        let (_ids, terrain_table) = base_terrain_fixture();
        let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

        // Assert
        match outcome {
            LoadOutcome::Playable(loaded_world) => {
                assert_eq!(loaded_world.hash(), hash_before);
            }
            other => panic!("期望 Playable，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 存档文件被截断时读档返回rejected而不panic() {
        // Arrange：只写入一个声称很长的头部长度前缀,后面什么都不跟。
        let path = temp_path("truncated");
        std::fs::write(&path, 100u32.to_le_bytes()).expect("写入测试文件应当成功");

        // Act
        let outcome = load_full(&path, &Registry::new(), &[], TerrainTable::default(), &[]);

        // Assert
        assert!(matches!(
            outcome,
            LoadOutcome::Rejected(LoadError::Corrupted(_))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn schema版本高于当前支持版本时读档返回rejected() {
        // Arrange
        let path = temp_path("schema-too-new");
        let mut header = sample_header(Vec::new());
        header.schema_version = CURRENT_SCHEMA_VERSION + 1;
        save_to_file(&path, &header, &test_world()).expect("写出应当成功");

        // Act
        let outcome = load_full(&path, &Registry::new(), &[], TerrainTable::default(), &[]);

        // Assert
        assert!(matches!(
            outcome,
            LoadOutcome::Rejected(LoadError::SchemaTooNew(_))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn schema版本低于当前版本且迁移链不认识该版本时返回迁移缺口错误() {
        // Arrange：伪造一份"更早版本"的存档——迁移链已被清空（老存档
        // 去掉就好了的裁定，见 migration_chain 文档），没有任何已注册
        // 的路径,版本 0 既不是任何一步的起点也不是终点,链条对它
        // 一无所知。
        let path = temp_path("schema-migration-gap");
        let mut header = sample_header(Vec::new());
        header.schema_version = 0;
        save_to_file(&path, &header, &test_world()).expect("写出应当成功");

        // Act
        let outcome = load_full(&path, &Registry::new(), &[], TerrainTable::default(), &[]);

        // Assert
        match outcome {
            LoadOutcome::Rejected(LoadError::SchemaMigrationGap(detail)) => {
                assert_eq!(detail.from, 0);
            }
            other => panic!("期望 Rejected(SchemaMigrationGap)，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 老存档的schema版本号不再被支持时读档明确拒绝而不静默解析() {
        // 项目所有者裁定「老存档去掉就好了」——发布前累计的迁移链已经
        // 清空（crate::migrations 模块文档），CURRENT_SCHEMA_VERSION
        // 重置为 1。这里模拟一份写于清空之前、版本号为 4（清空前的
        // CURRENT_SCHEMA_VERSION）的老存档：读档不应该把它当成当前版本
        // 静默塞进现在的 WorldState 类型解析，必须明确拒绝，且错误里
        // 能读到具体的版本号，不是笼统的"损坏"。
        // Arrange
        let path = temp_path("legacy-schema-version-rejected");
        let mut header = sample_header(Vec::new());
        header.schema_version = 4;
        save_to_file(&path, &header, &test_world()).expect("写出应当成功");

        // Act
        let outcome = load_full(&path, &Registry::new(), &[], TerrainTable::default(), &[]);

        // Assert
        match outcome {
            LoadOutcome::Rejected(LoadError::SchemaTooNew(detail)) => {
                assert_eq!(
                    detail,
                    crate::load_error::SchemaTooNew {
                        message_key: crate::load_error::SAVE_SCHEMA_TOO_NEW_MESSAGE_KEY,
                        save_version: 4,
                        max_supported: CURRENT_SCHEMA_VERSION,
                    }
                );
            }
            other => panic!("期望 Rejected(SchemaTooNew)，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn terrain_table未正确灌入时读档返回rejected() {
        // 直接对应任务 9 的核心要求之一：assert_terrain_table_loaded
        // 真的在读档流程里被调用——传入一张空表,读档必须拒绝而不是
        // 静默放行。
        // Arrange
        let path = temp_path("terrain-table-missing");
        let (world, save_registry) = test_world_with_save_registry();
        let content_index_map = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let header = sample_header(content_index_map);
        save_to_file(&path, &header, &world).expect("写出应当成功");
        let (current_registry, _terrain_table) = current_session_registry_with_terrain();

        // Act：故意传入 TerrainTable::default()（空表）,而不是当前会话
        // 真实注册出的表——重映射本身应当成功（地形内容都能对上号），
        // 卡住的必须是 terrain_table 校验点本身。
        let outcome = load_full(&path, &current_registry, &[], TerrainTable::default(), &[]);

        // Assert
        assert!(matches!(
            outcome,
            LoadOutcome::Rejected(LoadError::Corrupted(_))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 读档成功会触发脚本引擎强制重建计数增加() {
        // 直接对应任务 9 的核心要求之一：rebuild_all_engines_after_load
        // 真的接进了读档流程，用批次 D 特意留下的计数器断言，而不是
        // 「行为看起来正常」这种弱验证。
        // Arrange
        let path = temp_path("vm-rebuild-count");
        let (world, save_registry) = test_world_with_save_registry();
        let content_index_map = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let header = sample_header(content_index_map);
        save_to_file(&path, &header, &world).expect("写出应当成功");
        let (current_registry, terrain_table) = current_session_registry_with_terrain();
        let count_before = ll_script::host::rebuild_count();

        // Act
        let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

        // Assert
        assert!(matches!(outcome, LoadOutcome::Playable(_)));
        assert!(ll_script::host::rebuild_count() > count_before);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mod内容哈希不匹配时读档返回rejected() {
        // Arrange：mod 仍在场（current_manifests 里能找到 lostland），
        // 内容却变了——真正的不兼容，必须硬拒绝（区别于下面「完全卸载
        // 的 mod」那条测试，见 check_mod_content 文档「断链二修复」）。
        let path = temp_path("mod-content-mismatch");
        let mut header = sample_header(Vec::new());
        header.generation_mods.push(crate::header::ModHeaderEntry {
            namespace: "lostland".to_string(),
            version: "0.1.0".to_string(),
            content_hash: Some(999_999),
        });
        save_to_file(&path, &header, &test_world()).expect("写出应当成功");
        let mut current_registry = Registry::new();
        current_registry.intern(id("lostland:river")); // 与生成期记录的哈希对不上
        let current_manifests = vec![ModManifest {
            id: id("lostland:self"),
            version: "0.1.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
            event_scripts: Vec::new(),
        }];

        // Act
        let outcome = load_full(
            &path,
            &current_registry,
            &current_manifests,
            TerrainTable::default(),
            &[],
        );

        // Assert
        assert!(matches!(
            outcome,
            LoadOutcome::Rejected(LoadError::ModContentMismatch { .. })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 存档记录的内容哈希算法版本早于当前版本时返回contenthashalgorithmupgraded而非modcontentmismatch()
     {
        // 核心场景：mod 版本号对得上（check_mod_set 放行），内容哈希
        // 数值也刻意设成与当前会话完全一致（若真的走到
        // check_mod_content，理应判定为兼容）——唯一的差异是 header
        // 记录的算法版本比当前旧。这条差异必须在 check_mod_content 之前
        // 就被拦下,不能因为哈希数值凑巧一致就被放行,也不能被误判成
        // ModContentMismatch。
        // Arrange
        let path = temp_path("content-hash-algorithm-upgraded");
        let mut current_registry = Registry::new();
        current_registry.intern(id("lostland:mountain"));
        let matching_hash = current_registry.content_hash_of("lostland");
        let mut header = sample_header(Vec::new());
        header.content_hash_algorithm_version = 0; // 早于当前版本（哨兵值）
        header.generation_mods.push(crate::header::ModHeaderEntry {
            namespace: "lostland".to_string(),
            version: "0.1.0".to_string(),
            content_hash: matching_hash,
        });
        save_to_file(&path, &header, &test_world()).expect("写出应当成功");
        let current_manifests = vec![ModManifest {
            id: id("lostland:self"),
            version: "0.1.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
            event_scripts: Vec::new(),
        }];

        // Act
        let outcome = load_full(
            &path,
            &current_registry,
            &current_manifests,
            TerrainTable::default(),
            &[],
        );

        // Assert
        assert!(matches!(
            outcome,
            LoadOutcome::Rejected(LoadError::ContentHashAlgorithmUpgraded {
                save_algorithm_version: 0,
                current_algorithm_version: CONTENT_HASH_ALGORITHM_VERSION,
            })
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 完全卸载的mod读档后被硬门禁拒绝而非降级为只读() {
        // 决策二（项目所有者拍板，见 crate::load_error::check_mod_set
        // 文档）推翻了这条测试曾经验证的行为：header.generation_mods
        // 记录了一条真实的（非留空规避）「vanishedmod」条目；当前会话
        // 的 current_manifests 里完全没有这个命名空间（玩家把它整个
        // 卸载了）。P5-A 任务 14 断链二修复时，这种场景曾经被特意放行
        // 到 remap_world 按内容类型细粒度降级为只读——决策二明确要求
        // 「mod 不存在……就不能进入这个存档」，不再给这个场景细粒度
        // 降级的机会，check_mod_set 会在解压存档主体之前就直接拒绝。
        // Arrange
        let path = temp_path("mod-fully-uninstalled");
        let (mut world, mut registry) = test_world_with_save_registry();
        let vanished_race = registry.intern(id("vanishedmod:ghost_race"));
        let vanished_content_hash = registry
            .content_hash_of("vanishedmod")
            .expect("刚刚 intern 过，必有内容哈希");
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        world.actors.spawn(ll_world::entity::Agent {
            pos: world.size.wrap(1, 1),
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: ll_world::entity::Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: vanished_race,
            mana: ll_world::entity::Agent::STARTING_MANA,
            stamina: ll_world::entity::Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut header = sample_header(content_index_map);
        header.generation_mods.push(crate::header::ModHeaderEntry {
            namespace: "vanishedmod".to_string(),
            version: "0.1.0".to_string(),
            content_hash: Some(vanished_content_hash),
        });
        save_to_file(&path, &header, &world).expect("写出应当成功");

        // Act：当前会话的 manifests 里完全没有 vanishedmod（不是像旧
        // 测试那样靠留空 generation_mods 绕过检查点，是真的卸载）。
        let (current_registry, terrain_table) = current_session_registry_with_terrain();
        let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

        // Assert：硬门禁直接拒绝，错误信息指明了是哪个 mod、要什么
        // 版本、当前是什么版本（None——完全不在场）。
        match outcome {
            LoadOutcome::Rejected(LoadError::ModSetMismatch(detail)) => {
                assert_eq!(
                    detail,
                    crate::load_error::ModSetMismatch {
                        message_key: crate::load_error::SAVE_MOD_MISSING_MESSAGE_KEY,
                        namespace: "vanishedmod".to_string(),
                        required_version: "0.1.0".to_string(),
                        current_version: None,
                    }
                );
            }
            other => panic!("期望 Rejected(ModSetMismatch)，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 生成期mod版本号不一致时读档被硬门禁拒绝() {
        // 决策二第二条判据——mod 仍在场，但版本号跟存档记录的不一样。
        // 与「完全不在场」是两条独立触发路径，这里单独覆盖。
        // Arrange
        let path = temp_path("mod-version-mismatch");
        let (world, mut registry) = test_world_with_save_registry();
        registry.intern(id("lostland:farmer"));
        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut header = sample_header(content_index_map);
        header.generation_mods.push(crate::header::ModHeaderEntry {
            namespace: "lostland".to_string(),
            version: "0.1.0".to_string(),
            content_hash: registry.content_hash_of("lostland"),
        });
        save_to_file(&path, &header, &world).expect("写出应当成功");

        // Act：当前会话确实装载了 lostland，但版本号变成了 0.2.0。
        let (current_registry, terrain_table) = current_session_registry_with_terrain();
        let current_manifests = vec![ModManifest {
            id: id("lostland:self"),
            version: "0.2.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
            event_scripts: Vec::new(),
        }];
        let outcome = load_full(
            &path,
            &current_registry,
            &current_manifests,
            terrain_table,
            &[],
        );

        // Assert
        match outcome {
            LoadOutcome::Rejected(LoadError::ModSetMismatch(detail)) => {
                assert_eq!(
                    detail,
                    crate::load_error::ModSetMismatch {
                        message_key: crate::load_error::SAVE_MOD_VERSION_MISMATCH_MESSAGE_KEY,
                        namespace: "lostland".to_string(),
                        required_version: "0.1.0".to_string(),
                        current_version: Some("0.2.0".to_string()),
                    }
                );
            }
            other => panic!("期望 Rejected(ModSetMismatch)，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 缺失mod的npc种族在当前会话未注册占位内容时读档后降级为只读模式() {
        // 规格要求的三条最低验证之一：缺失 mod 时按类型正确降级且不
        // 崩溃——这里覆盖 NPC 种族缺失、且当前会话确实没有注册占位内容
        // 的场景（`crate::degrade` 模块文档「ContentIndex 缺占位值的
        // 既知债务」这一档诚实兜底仍然保留，不强制要求调用方必须提供
        // 占位内容）。与下一条「占位内容可用」的测试对照。
        // Arrange
        let path = temp_path("npc-race-missing-no-placeholder");
        let (mut world, mut registry) = test_world_with_save_registry();
        let vanished_race = registry.intern(id("vanishedmod:ghost_race"));
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        world.actors.spawn(ll_world::entity::Agent {
            pos: world.size.wrap(1, 1),
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: ll_world::entity::Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: vanished_race,
            mana: ll_world::entity::Agent::STARTING_MANA,
            stamina: ll_world::entity::Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let header = sample_header(content_index_map);
        save_to_file(&path, &header, &world).expect("写出应当成功");
        let (current_registry, terrain_table) = current_session_registry_with_terrain();

        // Act：当前会话完全没有装载 vanishedmod（但装载了地形，让重
        // 映射真正走到「找不到种族」这一步，而不是提前卡在地形上），
        // 也没有注册占位内容。
        let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

        // Assert
        assert!(matches!(outcome, LoadOutcome::ReadOnly(_)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 缺失mod的npc种族在当前会话注册占位内容时读档后降级为占位且仍可游玩() {
        // P5-A 任务 14 断链一修复的核心验证：`load_full` 的占位索引此前
        // 硬编码为 None，这条分支在完整读档管线里永远走不到——现在改为
        // 从 current_registry 里真的查询本体占位内容
        // （`ll_mod::base_placeholder::base_placeholder_index`），NPC
        // 种族缺失应当真正降级为占位而不是被拒绝，整体结果可继续游玩。
        // Arrange
        let path = temp_path("npc-race-missing-with-placeholder");
        let (mut world, mut registry) = test_world_with_save_registry();
        let vanished_race = registry.intern(id("vanishedmod:ghost_race"));
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let npc = world.actors.spawn(ll_world::entity::Agent {
            pos: world.size.wrap(1, 1),
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: ll_world::entity::Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: vanished_race,
            mana: ll_world::entity::Agent::STARTING_MANA,
            stamina: ll_world::entity::Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        });
        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let header = sample_header(content_index_map);
        save_to_file(&path, &header, &world).expect("写出应当成功");
        let (current_registry, terrain_table) =
            current_session_registry_with_terrain_and_placeholder();
        let expected_placeholder =
            ll_mod::base_placeholder::base_placeholder_index(&current_registry)
                .expect("刚刚注册过占位内容，必然能查到");

        // Act
        let outcome = load_full(&path, &current_registry, &[], terrain_table, &[]);

        // Assert：不是 ReadOnly，是 Playable——占位内容真的顶上了。
        match outcome {
            LoadOutcome::Playable(loaded_world) => {
                let race_after = loaded_world
                    .actors
                    .get(npc)
                    .expect("NPC 实体应当仍存在")
                    .race;
                assert_eq!(race_after, expected_placeholder);
            }
            other => panic!("期望 Playable，实际 {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn 建档流程真的调用了generation_mods_to_header_entries产出可读档的头部() {
        // P5-A 任务 14 断链三修复的端到端验证：不再手写
        // Vec<ModHeaderEntry> 或留空，而是真的走
        // Registry -> GenerationModSet::capture ->
        // crate::world_identity::generation_mods_to_header_entries ->
        // SaveHeader.generation_mods 这条完整链路，再存档、读档，证明
        // 这个转换函数产出的数据确实能撑起一次真实的 check_mod_content
        // 通过——不是又一个只在自己模块测试里被调用的孤立函数。
        // Arrange
        let path = temp_path("generation-mods-conversion-wired");
        let (world, registry) = test_world_with_save_registry();
        let manifests = vec![ModManifest {
            id: id("lostland:self"),
            version: "0.1.0".to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::new(),
            event_scripts: Vec::new(),
        }];
        let generation = ll_mod::mod_set::GenerationModSet::capture(&registry, &manifests);
        let generation_mods = crate::world_identity::generation_mods_to_header_entries(&generation);
        assert_eq!(
            generation_mods.len(),
            1,
            "本体地形已经贡献内容，生成期集合应当只有 lostland 这一条"
        );

        let content_index_map = registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut header = sample_header(content_index_map);
        header.generation_mods = generation_mods;
        save_to_file(&path, &header, &world).expect("写出应当成功");

        // Act：读档一侧同样真实装载了 lostland,哈希应当对得上。
        let (current_registry, terrain_table) = current_session_registry_with_terrain();
        let outcome = load_full(&path, &current_registry, &manifests, terrain_table, &[]);

        // Assert
        assert!(
            matches!(outcome, LoadOutcome::Playable(_)),
            "generation_mods_to_header_entries 产出的头部应当能通过 check_mod_content 走到 Playable"
        );
        let _ = std::fs::remove_file(&path);
    }
}
