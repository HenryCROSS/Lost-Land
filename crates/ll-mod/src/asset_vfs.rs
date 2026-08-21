//! 资产 VFS：把 mod（与本体自身）声明的精灵图片解析成一份已经应用完
//! 覆盖规则的资产清单，供 `ll-render` 在运行期打包图集。
//!
//! # 为什么资产 VFS 落在 `ll-mod` 而不是 `ll-render`
//!
//! 覆盖解析（同路径覆盖、`topo_sort` 决定的确定性总序、冲突产出
//! `LoadStatus::Warning`）与 mod 发现/清单/拓扑排序是同一套机制的
//! 直接延伸——本模块复用 [`crate::discover::discover_mods`]/
//! [`crate::manifest::parse_manifest`]/[`crate::topo::topo_sort`]，
//! 不重新发明一套 mod 遍历逻辑。`ll-render` 完全不知道「mod」「覆盖」
//! 这些概念，只知道「给我一批已经排好序、内容已经确定的精灵图片，我
//! 打包成一张图集」——两层职责边界与 `knowledge/design/mod-package-structure.md`
//! 四节的设计完全一致。
//!
//! # 目录约定
//!
//! ```text
//! <mod 目录>/assets/sprites/manifest.json   本 mod（或本体）自带的精灵声明
//! <mod 目录>/assets/sprites/<相对路径>.png  声明里 `file` 字段指向的图片
//! <mod 目录>/assets/overrides/<目标命名空间>/sprites/<相对路径>.png
//!                                            覆盖目标命名空间下同相对路径的资产
//! ```
//!
//! `manifest.json` 的形状：
//!
//! ```json
//! { "entries": [
//!     { "name": "lava_floor", "file": "lava_floor.png",
//!       "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }
//! ] }
//! ```
//!
//! **没有 `rect` 字段**——与本体旧的 `assets/atlas/placeholder.json`
//! 不同，这里的精灵是运行期打包的松散贴图，摆放到图集里哪个矩形完全
//! 由打包器（`ll_render::atlas_pack`）决定，声明方只需要给出图片本身、
//! 锚点与占地格数。
//!
//! # 覆盖只换字节，不换摆放参数
//!
//! 覆盖文件只替换被覆盖资产的**图片来源**（[`ResolvedSprite::source_file`]），
//! `pivot`/`footprint` 恒定沿用原始声明方的值——这正是
//! `mod-package-structure.md` 四节「换贴图这个需求的本质是把这份资产的
//! 字节内容换掉，逻辑意义完全不变」的直接实现。
//!
//! # 路径安全
//!
//! [`validate_relative_asset_path`] 是路径穿越的唯一防线：`manifest.json`
//! 里的 `file` 字段是外部不可信输入（mod 作者可以写任何字符串），必须
//! 拒绝 `..`、绝对路径、Windows 盘符前缀（`C:`）、UNC 路径
//! （`\\server\share`）——这里用 [`std::path::Component`] 逐段判断而不是
//! 字符串匹配，`Path::components()` 在 Windows 目标上原生同时识别 `/`
//! 与 `\` 两种分隔符、原生识别盘符/UNC 前缀，不需要手写平台相关的
//! 字符串规则去覆盖这些坑。覆盖目录（`overrides/`）本身不解析任何
//! 声明字符串，是直接遍历真实文件系统条目，因此不存在同一类字符串
//! 穿越风险；但递归遍历使用 [`std::fs::DirEntry::file_type`]（不追踪
//! 符号链接，与 [`std::path::Path::is_dir`] 会跟随符号链接不同）作为
//! 额外的纵深防御，避免恶意 mod 用指向 mod 目录之外的符号链接/junction
//! 把无关文件伪装成「覆盖资产」带进来。
//!
//! # 确定性（约束 C5）
//!
//! 覆盖处理严格按 [`crate::topo::topo_sort`] 产出的确定性总序进行——
//! 同一个命名空间目录下发现的多个覆盖文件按路径字符串排序后处理，
//! `HashMap` 只用于 O(1) 查找，从不被遍历产出顺序（与 `crate::topo`
//! 同一条纪律）。[`AssetVfs::sprites`] 最终按 [`ResolvedSprite::id`]
//! 的字符串表示排序，保证同样的 mod 集合恒定打出同一份有序精灵列表，
//! 不受 mod 发现顺序、覆盖目录遍历顺序影响。

use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use ll_core::ident::NamespacedId;

use crate::manifest::{ModManifest, mod_self_id, parse_manifest};
use crate::{discover, topo};

/// mod（或本体）资产目录下固定的子目录/文件名约定，见模块文档「目录
/// 约定」一节。
pub const ASSETS_DIR: &str = "assets";
/// 精灵资产子目录名。
pub const SPRITES_DIR: &str = "sprites";
/// 覆盖资产子目录名。
pub const OVERRIDES_DIR: &str = "overrides";
/// 精灵清单文件的固定名。
pub const SPRITE_MANIFEST_FILENAME: &str = "manifest.json";

/// 精灵图像内的锚点，字段形状与 `ll_render::sprite::Pivot`（`i16` x/y）
/// 逐一对应——本 crate 不依赖 `ll-render`（依赖方向见 `crate` 模块
/// 文档），由调用方（`ll-game`）在两者之间转换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct SpritePivot {
    /// 锚点横向偏移。
    pub x: i16,
    /// 锚点纵向偏移。
    pub y: i16,
}

/// 精灵的逻辑占地格数，字段形状与 `ll_render::sprite::Footprint`
/// （`u8` width/height）逐一对应，理由同 [`SpritePivot`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct SpriteFootprint {
    /// 横向占几格。
    pub width: u8,
    /// 纵向占几格。
    pub height: u8,
}

/// `manifest.json` 里的一条精灵声明。
#[derive(Debug, Clone, serde::Deserialize)]
struct SpriteManifestEntry {
    name: String,
    file: String,
    pivot: SpritePivot,
    footprint: SpriteFootprint,
}

/// `manifest.json` 的顶层结构。
#[derive(Debug, Default, serde::Deserialize)]
struct SpriteManifestFile {
    #[serde(default)]
    entries: Vec<SpriteManifestEntry>,
}

/// 资产 VFS 相关的、值得作为独立类型区分的错误。
///
/// 只有路径校验会产出结构化错误——`build`（本模块的主入口）本身遵循
/// 「打包失败必须优雅」：单条精灵声明有问题只跳过那一条并记日志，不让
/// 整个资产 VFS 构建失败，因此不需要一个笼罩全局的错误类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetVfsError {
    /// 相对路径不合法：可能包含 `..`、是绝对路径、带 Windows 盘符或
    /// UNC 前缀。附带原始输入文本，供日志/测试断言。
    PathTraversal(String),
}

impl fmt::Display for AssetVfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetVfsError::PathTraversal(raw) => {
                write!(f, "资产相对路径不合法（可能是路径穿越）：{raw:?}")
            }
        }
    }
}

impl core::error::Error for AssetVfsError {}

/// 校验并规范化一段「资产相对路径」（`manifest.json` 里 `file` 字段的
/// 原始文本），拒绝任何可能逃出 mod 自己资产目录的输入。
///
/// # 为什么用 [`Component`] 逐段判断而不是字符串匹配
///
/// 只允许 [`Component::Normal`] 段通过；[`Component::ParentDir`]（`..`）、
/// [`Component::RootDir`]（前导分隔符）、[`Component::Prefix`]（Windows
/// 盘符 `C:` 或 UNC `\\server\share`）、[`Component::CurDir`]（`.`）
/// 一律拒绝。`std::path::Path` 在 Windows 目标上原生同时识别 `/` 与 `\`
/// 两种分隔符——`Path::new("..\\..\\secret.png").components()` 与
/// `Path::new("../../secret.png").components()` 产出的都是两个
/// [`Component::ParentDir`]，不需要手写字符串替换去统一分隔符，也不
/// 存在「校验只认斜杠、漏了反斜杠」这类平台相关的疏漏。
pub fn validate_relative_asset_path(raw: &str) -> Result<PathBuf, AssetVfsError> {
    if raw.is_empty() {
        return Err(AssetVfsError::PathTraversal(raw.to_string()));
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_)
            | Component::CurDir => {
                return Err(AssetVfsError::PathTraversal(raw.to_string()));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(AssetVfsError::PathTraversal(raw.to_string()));
    }

    Ok(normalized)
}

/// 把已校验的相对路径规范化成用于跨模块比对的字符串键（正斜杠拼接）。
///
/// 不直接用 [`PathBuf`] 当键——同一逻辑路径在 Windows 上可能因为
/// 大小写或分隔符差异得到不同的 [`PathBuf`] 表示（`PathBuf` 的
/// `PartialEq`/`Hash` 按平台原生语义比较，Windows 上恰好不区分大小写
/// 但仍可能因分隔符风格不同而不相等），用字符串键统一成一种表示，
/// 覆盖匹配才不会因为大小写或分隔符写法不同而失配。
fn normalize_path_key(path: &Path) -> String {
    path.iter()
        .map(|segment| segment.to_string_lossy().to_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

/// 已解析完覆盖规则的一个精灵声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSprite {
    /// 完整命名空间 ID（`namespace:name`）。
    pub id: NamespacedId,
    /// 声明这份资产的命名空间（mod 自己的命名空间，或本体命名空间）。
    pub namespace: String,
    /// 供图集打包使用的条目名。
    ///
    /// 本体资产与 mod 资产统一用完整 `"namespace:name"`（等价于
    /// [`ResolvedSprite::id`] 的字符串表示）——此前本体资产曾特例用
    /// 裸名字（例如 `"hero_idle_0"`）打包，导致本体与 mod 的图集键形式
    /// 不对称：mod 作者想引用本体贴图，最自然的写法
    /// `lostland:hero_idle_0` 反而查不到，必须知道「本体是裸名字」这条
    /// 不成文的例外。项目所有者裁定本体也加前缀，消掉这条不对称——
    /// 现在任何调用方（无论是本体二进制自己的硬编码查找，还是 mod 注册
    /// 的地形回退查图集）统一用完整命名空间 ID 字符串当查找键。
    pub atlas_name: String,
    /// 锚点，沿用声明方原始值——覆盖不改变它，见模块文档「覆盖只换
    /// 字节」一节。
    pub pivot: SpritePivot,
    /// 逻辑占地格数，理由同 `pivot`。
    pub footprint: SpriteFootprint,
    /// 最终生效的图片文件绝对路径：初始是声明方自己的文件，若被
    /// （拓扑序上最后生效的）覆盖顶替，则替换成覆盖文件的路径。
    pub source_file: PathBuf,
}

/// 全部已解析精灵声明的集合。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetVfs {
    /// 按 [`ResolvedSprite::id`] 的字符串表示升序排列——保证「同样的
    /// mod 集合打出逐位相同的图集」（约束 C5），见模块文档「确定性」
    /// 一节。
    pub sprites: Vec<ResolvedSprite>,
}

/// [`build`] 的完整产出：解析好的资产 VFS，与资产覆盖冲突告警。
pub struct AssetVfsBuildResult {
    /// 已解析完覆盖规则的资产清单。
    pub vfs: AssetVfs,
    /// 资产覆盖冲突：`(触发冲突、当前生效的那个 mod, 面向 mod 作者的
    /// 说明文本)`。供调用方并入 [`crate::load_report::LoadReport`]
    /// （[`crate::load_report::LoadStatus::Warning`]）——这正是该字段
    /// 此前「声明了但从没被构造过」的产出路径，见模块文档与
    /// `mod-package-structure.md` 四节「必须可见，不能是静默覆盖」。
    pub conflicts: Vec<(NamespacedId, String)>,
}

/// 构建一份完整的资产 VFS：本体资产 + `mods_root` 下按拓扑序装载的
/// 全部 mod 资产，应用完覆盖规则。
///
/// # 打包失败必须优雅
///
/// 单条精灵声明的问题（路径穿越、名字不合法、JSON 语法错误、清单文件
/// 缺失）一律跳过那一条（或那个 mod 的全部自带精灵）并记日志，不让
/// 整个资产 VFS 构建失败——「降级而非崩溃」，与 `ll_render::anim`
/// 模块文档的既有先例同一条纪律。真正会让整个 mod 批次的资产都拿不到
/// 的唯一情形是 `topo_sort` 本身失败（依赖成环/缺失/版本不兼容/重复
/// 命名空间）——那种情况下退化为「只有本体资产」，与
/// `crate::pipeline::load_all` 「整批中止」的既有语义一致：既然连
/// 装载顺序都定不出来，也没有任何单一「正确」的资产覆盖结果可以给。
///
/// 单个精灵源文件是否真的能读到、能解码，本函数不检查——那是
/// `ll_render::atlas_pack` 打包阶段的职责（需要真的打开文件才知道
/// 是否损坏），本函数只负责路径解析。
pub fn build(
    mods_root: &Path,
    base_assets_dir: &Path,
    base_namespace: &str,
) -> AssetVfsBuildResult {
    let mut sprites: Vec<ResolvedSprite> = Vec::new();
    let mut index_by_id: HashMap<NamespacedId, usize> = HashMap::new();
    let mut index_by_path: HashMap<(String, String), usize> = HashMap::new();

    register_own_sprites(
        base_assets_dir,
        base_namespace,
        &mut sprites,
        &mut index_by_id,
        &mut index_by_path,
    );

    let mut conflicts: Vec<(NamespacedId, String)> = Vec::new();

    let candidates = discover::discover_mods(mods_root);
    let mut parsed: Vec<(PathBuf, ModManifest)> = Vec::new();
    for path in &candidates {
        // 解析失败的候选已经在 `pipeline::load_all` 产出的报告里有一条
        // Failed 记录——资产 VFS 只关心「已经能解析出命名空间」的 mod，
        // 不重复报告解析错误，理由与 `ll_game::content::successfully_parsed_manifests`
        // 同一条纪律。
        if let Ok(manifest) = parse_manifest(path) {
            parsed.push((path.clone(), manifest));
        }
    }

    let manifests_only: Vec<ModManifest> = parsed.iter().map(|(_, m)| m.clone()).collect();
    let Ok(order) = topo::topo_sort(&manifests_only) else {
        // 拓扑排序失败即整批中止，见本函数文档「打包失败必须优雅」
        // 一节。
        sprites.sort_by_key(|s| s.id.to_string());
        return AssetVfsBuildResult {
            vfs: AssetVfs { sprites },
            conflicts,
        };
    };

    let mut override_history: HashMap<(String, String), Vec<String>> = HashMap::new();
    for idx in order {
        let (mod_path, manifest) = &parsed[idx];
        let mod_dir = mod_path.parent().unwrap_or_else(|| Path::new("."));
        let namespace = manifest.id.namespace();
        let assets_dir = mod_dir.join(ASSETS_DIR);

        register_own_sprites(
            &assets_dir,
            namespace,
            &mut sprites,
            &mut index_by_id,
            &mut index_by_path,
        );

        apply_overrides(
            &assets_dir,
            namespace,
            &mut sprites,
            &index_by_path,
            &mut override_history,
            &mut conflicts,
        );
    }

    sprites.sort_by_key(|s| s.id.to_string());
    AssetVfsBuildResult {
        vfs: AssetVfs { sprites },
        conflicts,
    }
}

/// 解析 `assets_dir/sprites/manifest.json`，把每条声明注册进 `sprites`。
///
/// 没有清单文件是完全合法的（多数 mod 不带美术资产）——静默跳过，
/// 不是错误。清单存在但解析失败、单条声明的名字/路径不合法，都只
/// 跳过对应的那一份并记警告日志，不让其它声明也跟着报废。
fn register_own_sprites(
    assets_dir: &Path,
    owner_namespace: &str,
    sprites: &mut Vec<ResolvedSprite>,
    index_by_id: &mut HashMap<NamespacedId, usize>,
    index_by_path: &mut HashMap<(String, String), usize>,
) {
    let manifest_path = assets_dir.join(SPRITES_DIR).join(SPRITE_MANIFEST_FILENAME);
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return;
    };
    let manifest: SpriteManifestFile = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            tracing::warn!(
                path = %manifest_path.display(),
                %error,
                "精灵清单解析失败，已跳过该命名空间的全部自带精灵"
            );
            return;
        }
    };

    for entry in manifest.entries {
        let Ok(id) = NamespacedId::parse(&format!("{owner_namespace}:{}", entry.name)) else {
            tracing::warn!(
                name = %entry.name,
                namespace = owner_namespace,
                "精灵条目名不是合法的命名空间路径，已跳过"
            );
            continue;
        };
        if index_by_id.contains_key(&id) {
            tracing::warn!(id = %id, "重复的精灵条目名，已跳过后出现的一份声明");
            continue;
        }
        let relative = match validate_relative_asset_path(&entry.file) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(%error, "精灵条目的文件路径校验失败，已跳过");
                continue;
            }
        };

        let source_file = assets_dir.join(SPRITES_DIR).join(&relative);
        // 本体与 mod 统一用完整命名空间字符串当图集条目名，见
        // `ResolvedSprite::atlas_name` 文档——不再有「本体是裸名字」
        // 这条不对称的例外。
        let atlas_name = id.to_string();
        let path_key = (owner_namespace.to_string(), normalize_path_key(&relative));

        let index = sprites.len();
        sprites.push(ResolvedSprite {
            id: id.clone(),
            namespace: owner_namespace.to_string(),
            atlas_name,
            pivot: entry.pivot,
            footprint: entry.footprint,
            source_file,
        });
        index_by_id.insert(id, index);
        index_by_path.insert(path_key, index);
    }
}

/// 扫描 `assets_dir/overrides/`，把每个目标命名空间下的覆盖文件应用到
/// `sprites` 上，并在同一个目标被多个 mod 覆盖时追加冲突告警。
fn apply_overrides(
    assets_dir: &Path,
    overriding_namespace: &str,
    sprites: &mut [ResolvedSprite],
    index_by_path: &HashMap<(String, String), usize>,
    override_history: &mut HashMap<(String, String), Vec<String>>,
    conflicts: &mut Vec<(NamespacedId, String)>,
) {
    let overrides_root = assets_dir.join(OVERRIDES_DIR);
    let Ok(entries) = std::fs::read_dir(&overrides_root) else {
        return;
    };

    // 按目标命名空间字典序处理，保证多个目标命名空间同时存在覆盖时，
    // 处理顺序不依赖 `read_dir` 的文件系统遍历顺序（规格 C5）。
    let mut target_namespaces: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| matches!(entry.file_type(), Ok(ft) if ft.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    target_namespaces.sort();

    for target_namespace in target_namespaces {
        let sprites_root = overrides_root.join(&target_namespace).join(SPRITES_DIR);
        let mut files = Vec::new();
        collect_png_files(&sprites_root, &sprites_root, &mut files);
        files.sort();

        for relative in files {
            let path_key = (target_namespace.clone(), normalize_path_key(&relative));
            let Some(&idx) = index_by_path.get(&path_key) else {
                tracing::warn!(
                    target_namespace = %target_namespace,
                    file = %relative.display(),
                    overriding = overriding_namespace,
                    "覆盖了一个不存在的资产路径，已忽略"
                );
                continue;
            };

            sprites[idx].source_file = sprites_root.join(&relative);

            let history = override_history.entry(path_key).or_default();
            history.push(overriding_namespace.to_string());
            if history.len() > 1
                && let Ok(mod_id) = mod_self_id(overriding_namespace)
            {
                let target_id = &sprites[idx].id;
                let message = format!(
                    "资源 {target_id} 被多个 mod 覆盖：{}，当前生效：{overriding_namespace}",
                    history.join("、")
                );
                conflicts.push((mod_id, message));
            }
        }
    }
}

/// 递归收集 `current` 目录（相对 `root`）下的全部 `.png` 文件，返回
/// 相对 `root` 的路径。
///
/// 用 [`std::fs::DirEntry::file_type`] 而非 [`Path::is_dir`] 判断是否
/// 递归进入某个条目——前者不跟踪符号链接，后者会。恶意 mod 若在
/// `overrides/` 目录树里放一个指向 mod 目录之外的符号链接/junction，
/// 用 `file_type` 判断会因为符号链接本身的类型不是「目录」而拒绝
/// 递归进入，天然不会把外部文件当成覆盖资产读进来——见模块文档
/// 「路径安全」一节。
fn collect_png_files(root: &Path, current: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_png_files(root, &entry.path(), out);
        } else if file_type.is_file() {
            let path = entry.path();
            let is_png = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
            if is_png && let Ok(relative) = path.strip_prefix(root) {
                out.push(relative.to_path_buf());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tempdir;
    use std::fs;

    fn write_file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("测试路径恒有父目录")).expect("创建目录");
        fs::write(path, content).expect("写入测试文件");
    }

    /// 写一个最小的合法 PNG 字节序列，供需要「文件确实存在」但不关心
    /// 图片内容的测试使用（图片解码是 `ll_render::atlas_pack` 的职责，
    /// 本模块只关心路径是否解析正确）。
    fn write_placeholder_png(path: &Path) {
        fs::create_dir_all(path.parent().expect("测试路径恒有父目录")).expect("创建目录");
        // 1x1 透明 PNG 的固定字节序列。
        const ONE_PIXEL_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        fs::write(path, ONE_PIXEL_PNG).expect("写入占位 PNG");
    }

    fn write_mod_manifest(root: &Path, namespace: &str) {
        write_file(
            &root.join(namespace).join("mod.toml"),
            &format!("namespace = \"{namespace}\"\nversion = \"0.1.0\"\n"),
        );
    }

    fn write_sprite_manifest(assets_dir: &Path, entries_json: &str) {
        write_file(
            &assets_dir.join(SPRITES_DIR).join(SPRITE_MANIFEST_FILENAME),
            &format!(r#"{{ "entries": [{entries_json}] }}"#),
        );
    }

    // ---- 路径安全 ----

    #[test]
    fn 含双点的相对路径声明被拒绝() {
        // Arrange & Act
        let result = validate_relative_asset_path("../../../../etc/passwd");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn 含反斜杠双点的相对路径声明被拒绝() {
        // Windows 上路径分隔符可能是反斜杠——校验必须同样识别。
        // Arrange & Act
        let result = validate_relative_asset_path("..\\..\\secret.png");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn 带盘符的绝对路径声明被拒绝() {
        // Arrange & Act
        let result = validate_relative_asset_path("C:\\secret.png");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn unc路径声明被拒绝() {
        // Arrange & Act
        let result = validate_relative_asset_path("\\\\server\\share\\secret.png");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn 前导斜杠的绝对路径声明被拒绝() {
        // Arrange & Act
        let result = validate_relative_asset_path("/etc/passwd");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn 空字符串路径声明被拒绝() {
        // Arrange & Act
        let result = validate_relative_asset_path("");

        // Assert
        assert!(matches!(result, Err(AssetVfsError::PathTraversal(_))));
    }

    #[test]
    fn 合法相对路径通过校验() {
        // Arrange & Act
        let result = validate_relative_asset_path("characters/hero/idle_0.png");

        // Assert
        assert_eq!(
            result,
            Ok(PathBuf::from("characters").join("hero").join("idle_0.png"))
        );
    }

    #[test]
    fn 错误信息包含原始输入文本() {
        // Arrange & Act
        let result = validate_relative_asset_path("../secret.png");

        // Assert
        match result {
            Err(AssetVfsError::PathTraversal(raw)) => assert_eq!(raw, "../secret.png"),
            other => panic!("期望 PathTraversal，实际是 {other:?}"),
        }
    }

    #[test]
    fn 带路径穿越的精灵声明在构建时被拒绝而不影响其它条目() {
        // 端到端场景：一个 mod 的 manifest.json 里混了一条路径穿越声明
        // 与一条合法声明——穿越的那条必须被拒绝，不能让整个 mod 的资产
        // VFS 构建崩溃或把其它合法声明也一并丢弃。
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_mod_manifest(root.path().join("mods").as_path(), "evilmod");
        let evil_assets = root.path().join("mods").join("evilmod").join("assets");
        write_sprite_manifest(
            &evil_assets,
            r#"
            { "name": "escape", "file": "../../../../etc/passwd",
              "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } },
            { "name": "legit", "file": "legit.png",
              "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }
            "#,
        );
        write_placeholder_png(&evil_assets.join(SPRITES_DIR).join("legit.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert：只有合法条目进了资产 VFS，穿越的那条被拒绝。
        let names: Vec<String> = result
            .vfs
            .sprites
            .iter()
            .map(|s| s.id.to_string())
            .collect();
        assert_eq!(names, vec!["evilmod:legit".to_string()]);
    }

    // ---- 基本装载 ----

    #[test]
    fn 本体精灵使用完整命名空间字符串作为图集条目名() {
        // 本体不再是「裸名字」的特例——与 mod 精灵统一用
        // `namespace:name` 打包图集，消除本体/mod 之间键形式的不对称。
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 8, "y": 24 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(result.vfs.sprites.len(), 1);
        assert_eq!(result.vfs.sprites[0].atlas_name, "lostland:hero_idle_0");
    }

    #[test]
    fn mod精灵使用完整命名空间字符串作为图集条目名() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_mod_manifest(root.path().join("mods").as_path(), "examplemod");
        let mod_assets = root.path().join("mods").join("examplemod").join("assets");
        write_sprite_manifest(
            &mod_assets,
            r#"{ "name": "lava_floor", "file": "lava_floor.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&mod_assets.join(SPRITES_DIR).join("lava_floor.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(result.vfs.sprites.len(), 1);
        assert_eq!(result.vfs.sprites[0].atlas_name, "examplemod:lava_floor");
    }

    #[test]
    fn 没有精灵清单的mod不产出任何条目也不报错() {
        // Arrange：mod 存在，但完全没有 assets/sprites/manifest.json。
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_mod_manifest(root.path().join("mods").as_path(), "puredata");

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert!(result.vfs.sprites.is_empty());
    }

    #[test]
    fn 语法错误的精灵清单被跳过而不影响其它mod() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_mod_manifest(root.path().join("mods").as_path(), "broken");
        write_file(
            &root
                .path()
                .join("mods")
                .join("broken")
                .join("assets")
                .join(SPRITES_DIR)
                .join(SPRITE_MANIFEST_FILENAME),
            "{ this is not json",
        );
        write_mod_manifest(root.path().join("mods").as_path(), "good");
        let good_assets = root.path().join("mods").join("good").join("assets");
        write_sprite_manifest(
            &good_assets,
            r#"{ "name": "ok", "file": "ok.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&good_assets.join(SPRITES_DIR).join("ok.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(result.vfs.sprites.len(), 1);
        assert_eq!(result.vfs.sprites[0].atlas_name, "good:ok");
    }

    // ---- 覆盖 ----

    #[test]
    fn mod覆盖本体资产后源文件指向覆盖文件() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "terrain_grass", "file": "terrain_grass.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        let base_png = base_assets.join(SPRITES_DIR).join("terrain_grass.png");
        write_placeholder_png(&base_png);

        write_mod_manifest(root.path().join("mods").as_path(), "reskin");
        let reskin_override = root
            .path()
            .join("mods")
            .join("reskin")
            .join("assets")
            .join(OVERRIDES_DIR)
            .join("lostland")
            .join(SPRITES_DIR)
            .join("terrain_grass.png");
        write_placeholder_png(&reskin_override);

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        let sprite = result
            .vfs
            .sprites
            .iter()
            .find(|s| s.atlas_name == "lostland:terrain_grass")
            .expect("本体地形条目应仍存在");
        assert_eq!(sprite.source_file, reskin_override);
    }

    #[test]
    fn 覆盖不改变原始声明的锚点与占地格数() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 8, "y": 24 }, "footprint": { "width": 2, "height": 2 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));

        write_mod_manifest(root.path().join("mods").as_path(), "reskin");
        write_placeholder_png(
            &root
                .path()
                .join("mods")
                .join("reskin")
                .join("assets")
                .join(OVERRIDES_DIR)
                .join("lostland")
                .join(SPRITES_DIR)
                .join("hero_idle_0.png"),
        );

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        let sprite = &result.vfs.sprites[0];
        assert_eq!(sprite.pivot, SpritePivot { x: 8, y: 24 });
        assert_eq!(
            sprite.footprint,
            SpriteFootprint {
                width: 2,
                height: 2
            }
        );
    }

    #[test]
    fn 两个mod覆盖同一份资产时产出冲突告警() {
        // Arrange：aoverride 与 boverride 互不依赖，topo_sort 决胜规则
        // 按命名空间字典序——"aoverride" 先加载、"boverride" 后加载，
        // 后者的覆盖最终生效。
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "terrain_grass", "file": "terrain_grass.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("terrain_grass.png"));

        for namespace in ["aoverride", "boverride"] {
            write_mod_manifest(root.path().join("mods").as_path(), namespace);
            write_placeholder_png(
                &root
                    .path()
                    .join("mods")
                    .join(namespace)
                    .join("assets")
                    .join(OVERRIDES_DIR)
                    .join("lostland")
                    .join(SPRITES_DIR)
                    .join("terrain_grass.png"),
            );
        }

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert：冲突确实被构造出来，且归给后生效的 boverride。
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].0, mod_self_id("boverride").unwrap());
    }

    #[test]
    fn 冲突告警消息包含全部涉及的mod命名空间() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "terrain_grass", "file": "terrain_grass.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("terrain_grass.png"));
        for namespace in ["aoverride", "boverride"] {
            write_mod_manifest(root.path().join("mods").as_path(), namespace);
            write_placeholder_png(
                &root
                    .path()
                    .join("mods")
                    .join(namespace)
                    .join("assets")
                    .join(OVERRIDES_DIR)
                    .join("lostland")
                    .join(SPRITES_DIR)
                    .join("terrain_grass.png"),
            );
        }

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        let message = &result.conflicts[0].1;
        assert!(message.contains("aoverride") && message.contains("boverride"));
    }

    #[test]
    fn 只有单个mod覆盖资产时不产出冲突告警() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "terrain_grass", "file": "terrain_grass.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("terrain_grass.png"));
        write_mod_manifest(root.path().join("mods").as_path(), "reskin");
        write_placeholder_png(
            &root
                .path()
                .join("mods")
                .join("reskin")
                .join("assets")
                .join(OVERRIDES_DIR)
                .join("lostland")
                .join(SPRITES_DIR)
                .join("terrain_grass.png"),
        );

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn 覆盖了不存在的资产路径时被忽略且不产出幽灵条目() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_mod_manifest(root.path().join("mods").as_path(), "reskin");
        write_placeholder_png(
            &root
                .path()
                .join("mods")
                .join("reskin")
                .join("assets")
                .join(OVERRIDES_DIR)
                .join("lostland")
                .join(SPRITES_DIR)
                .join("does_not_exist.png"),
        );

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert!(result.vfs.sprites.is_empty());
    }

    // ---- 确定性 ----

    #[test]
    fn 输出按id字符串升序排列() {
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"
            { "name": "zzz", "file": "zzz.png",
              "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } },
            { "name": "aaa", "file": "aaa.png",
              "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }
            "#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("zzz.png"));
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("aaa.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        let ids: Vec<String> = result
            .vfs
            .sprites
            .iter()
            .map(|s| s.id.to_string())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn 同一份mod目录重复构建产出逐位相同的结果() {
        // 「同样的 mod 集合必须打出逐位相同的图集」在资产 VFS 这一层的
        // 落点：对同一份目录状态重复调用 build，结果必须完全相等——
        // 不依赖任何进程内的隐藏状态或非确定性来源。
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 8, "y": 24 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));
        write_mod_manifest(root.path().join("mods").as_path(), "examplemod");
        let mod_assets = root.path().join("mods").join("examplemod").join("assets");
        write_sprite_manifest(
            &mod_assets,
            r#"{ "name": "lava_floor", "file": "lava_floor.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&mod_assets.join(SPRITES_DIR).join("lava_floor.png"));

        // Act
        let first = build(&root.path().join("mods"), &base_assets, "lostland");
        let second = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(first.vfs.sprites, second.vfs.sprites);
    }

    #[test]
    fn 拓扑排序失败时退化为只有本体资产() {
        // Arrange：needs_ghost 依赖一个从未被发现的 mod，topo_sort 整批
        // 中止——资产 VFS 应当优雅退化，而不是 panic。
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));
        write_file(
            &root
                .path()
                .join("mods")
                .join("needs_ghost")
                .join("mod.toml"),
            "namespace = \"needs_ghost\"\nversion = \"0.1.0\"\ndependencies = [\"ghost\"]\n",
        );

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(result.vfs.sprites.len(), 1);
        assert_eq!(result.vfs.sprites[0].atlas_name, "lostland:hero_idle_0");
    }

    // ---- 命名空间不变式 ----

    #[test]
    fn 图集里全部条目的键都带非空命名空间前缀() {
        // 失败是静默的（查不到图集条目就跳过绘制，不报错，见模块所在
        // crate 的既有降级纪律）——本体/mod 键形式对称这条不变式必须
        // 有机器守着，不能靠人肉保证「改全了」。这里同时装载本体精灵与
        // mod 精灵，断言 build() 产出的每一条 atlas_name 都形如
        // `namespace:name`，任何将来重新引入「本体裸名字」特例的改动
        // 都会被这里挡住。
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 8, "y": 24 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));
        write_mod_manifest(root.path().join("mods").as_path(), "examplemod");
        let mod_assets = root.path().join("mods").join("examplemod").join("assets");
        write_sprite_manifest(
            &mod_assets,
            r#"{ "name": "lava_floor", "file": "lava_floor.png",
                 "pivot": { "x": 0, "y": 0 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&mod_assets.join(SPRITES_DIR).join("lava_floor.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        assert_eq!(
            result.vfs.sprites.len(),
            2,
            "前置条件：本体与 mod 各贡献一条精灵"
        );
        let all_namespaced = result.vfs.sprites.iter().all(|sprite| {
            sprite
                .atlas_name
                .split_once(':')
                .is_some_and(|(namespace, _)| !namespace.is_empty())
        });
        assert!(
            all_namespaced,
            "存在缺少命名空间前缀的图集键：{:?}",
            result
                .vfs
                .sprites
                .iter()
                .map(|s| &s.atlas_name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mod能按完整命名空间id查到本体精灵的图集条目() {
        // 这正是本次要解决的问题的正面证据：mod 作者想引用本体的英雄
        // 贴图，最自然的写法是完整命名空间 id `lostland:hero_idle_0`
        // ——此前只能用裸名字 `hero_idle_0` 查到，`lostland:hero_idle_0`
        // 查不到；现在两者统一，必须能按后者查到。
        // Arrange
        let root = tempdir();
        let base_assets = root.path().join("base_assets");
        write_sprite_manifest(
            &base_assets,
            r#"{ "name": "hero_idle_0", "file": "hero_idle_0.png",
                 "pivot": { "x": 8, "y": 24 }, "footprint": { "width": 1, "height": 1 } }"#,
        );
        write_placeholder_png(&base_assets.join(SPRITES_DIR).join("hero_idle_0.png"));

        // Act
        let result = build(&root.path().join("mods"), &base_assets, "lostland");

        // Assert
        let found = result
            .vfs
            .sprites
            .iter()
            .any(|sprite| sprite.atlas_name == "lostland:hero_idle_0");
        assert!(
            found,
            "mod 应能按 lostland:hero_idle_0 查到本体精灵的图集条目"
        );
    }
}
