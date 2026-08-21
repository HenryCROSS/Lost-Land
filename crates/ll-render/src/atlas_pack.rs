//! 运行期图集打包：把一批松散贴图（本体自带的 + mod 带来的 + 已经
//! 应用完覆盖规则的）合成一张图集画布，取代此前 `include_bytes!` 在
//! 编译期烧死的单一 `assets/atlas/placeholder.png`。
//!
//! # 为什么本体资产也要走这条路径
//!
//! 资产覆盖要求「同路径覆盖」在打包前解析生效——如果本体贴图是独立
//! 烧录的图集、mod 贴图是独立纹理，根本没有「覆盖」可言，两者会同时
//! 画出来。本模块因此不区分「本体贴图」与「mod 贴图」，只认调用方
//! 已经按 `ll_mod::asset_vfs` 解析出的最终生效来源（[`SpriteSource`]）
//! ——本体只是这批来源里 `namespace == "lostland"` 的那一部分，不是
//! 单独走另一条打包路径。这也是每个 mod 不需要各开一张独立纹理、
//! 不会给渲染层带来额外纹理切换的原因（`atlas.rs` 模块文档「图集」
//! 设计初衷本就是靠一张纹理省掉这些切换）。
//!
//! # 确定性（约束 C5）
//!
//! [`pack_atlas`] 打包前先按 [`SpriteSource::name`] 字符串排序，矩形
//! 布局算法（[`shelf_pack`]）本身也是纯函数、只依赖排序后的输入——
//! 因此同一批贴图内容（无论调用方传入的 `Vec` 顺序如何）恒定打出
//! 逐位相同的图集画布与元数据。见本模块测试
//! `sources_乱序传入仍打出逐位相同的图集`。
//!
//! # 打包失败必须优雅
//!
//! 单张贴图损坏或缺失（读不到文件、不是合法图片）只跳过那一条并记
//! 警告日志，不让整批打包失败——与 `ll_render::anim` 模块文档「降级
//! 而非崩溃」同一条纪律。极端情况下全部贴图都打包失败，退化成一张
//! 1×1 透明画布、空条目列表，而不是返回错误让调用方无从处理——一张
//! 空图集仍然是一个合法的 [`crate::atlas::Atlas`]，只是查不到任何
//! 条目（[`crate::atlas::AtlasMetadata::lookup`] 恒返回 `None`），下游
//! 已有的「查不到条目就跳过绘制」降级路径天然接得住这种情况。

use image::RgbaImage;

use crate::atlas::{AtlasEntry, AtlasMetadata, FrameRect};
use crate::sprite::{Footprint, Pivot};

/// 图集打包前的一份贴图来源：条目名、图片文件的字节内容、摆放参数。
///
/// 这是 `ll-mod` 的 `asset_vfs::ResolvedSprite` 与本 crate 之间的转换
/// 终点——调用方（`ll-game`）负责读出 `ResolvedSprite::source_file`
/// 对应的字节并转换成本结构体，本 crate 不直接依赖 `ll-mod`（依赖
/// 方向不允许，见 `ll_mod` crate 文档）。
#[derive(Debug, Clone)]
pub struct SpriteSource {
    /// 图集条目名，最终写进 [`AtlasEntry::name`]。
    pub name: String,
    /// 图片文件的原始字节（PNG）。
    pub image_bytes: Vec<u8>,
    /// 锚点。
    pub pivot: Pivot,
    /// 逻辑占地格数。
    pub footprint: Footprint,
}

/// 打包结果：可直接喂给 [`crate::atlas::Atlas::from_rgba`] 的元数据
/// 与画布。
pub struct PackedAtlas {
    /// 图集元数据——`image` 字段固定填 `"packed"`（运行期打包没有实际
    /// 磁盘文件名可填，这个字段只在旧的「元数据描述磁盘文件」场景下
    /// 才有意义，见 [`AtlasMetadata::image`] 字段文档；打包路径下这个
    /// 字符串不会被任何调用方读取，只是满足类型形状）。
    pub metadata: AtlasMetadata,
    /// 合成好的图集画布，可直接传给 [`crate::atlas::Atlas::from_rgba`]。
    pub canvas: RgbaImage,
}

/// 相邻贴图之间的间隔像素——避免半 texel 内缩（`atlas.rs` 里
/// `Atlas::uv_rect` 的既有机制）在极端缩放下仍采样到邻居贴图，留一圈
/// 完全空白的缓冲像素。
const PADDING: u32 = 1;

/// 打包失败时退化用的画布尺寸——1×1 透明像素，见模块文档「打包失败
/// 必须优雅」一节。
const EMPTY_CANVAS_SIZE: u32 = 1;

/// 把一批贴图来源打包成一张图集。
///
/// 解码失败（文件读不到、不是合法图片）的条目会被跳过并记警告日志，
/// 不影响其余条目——见模块文档「打包失败必须优雅」一节。
pub fn pack_atlas(sources: &[SpriteSource]) -> PackedAtlas {
    // 按名字排序而非直接用调用方传入的顺序——保证同一批贴图内容打出
    // 逐位相同的画布，不受调用方构造 `Vec` 时的迭代/装载顺序影响
    // （见模块文档「确定性」一节）。
    let mut sorted: Vec<&SpriteSource> = sources.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut decoded: Vec<(&SpriteSource, RgbaImage)> = Vec::with_capacity(sorted.len());
    for source in sorted {
        match image::load_from_memory(&source.image_bytes) {
            Ok(image) => decoded.push((source, image.to_rgba8())),
            Err(error) => {
                tracing::warn!(
                    name = %source.name,
                    %error,
                    "精灵图片解码失败，已跳过该条目并降级"
                );
            }
        }
    }

    if decoded.is_empty() {
        return PackedAtlas {
            metadata: AtlasMetadata {
                image: "packed".to_string(),
                entries: Vec::new(),
            },
            canvas: RgbaImage::new(EMPTY_CANVAS_SIZE, EMPTY_CANVAS_SIZE),
        };
    }

    let sizes: Vec<(u32, u32)> = decoded.iter().map(|(_, img)| img.dimensions()).collect();
    let placements = shelf_pack(&sizes);
    let (canvas_width, canvas_height) = placements
        .iter()
        .zip(&sizes)
        .map(|(&(x, y), &(w, h))| (x + w, y + h))
        .fold((0u32, 0u32), |(mw, mh), (w, h)| (mw.max(w), mh.max(h)));

    let mut canvas = RgbaImage::new(canvas_width.max(1), canvas_height.max(1));
    let mut entries = Vec::with_capacity(decoded.len());
    for ((source, image), &(x, y)) in decoded.iter().zip(&placements) {
        let (width, height) = image.dimensions();
        image::imageops::replace(&mut canvas, image, x as i64, y as i64);
        entries.push(AtlasEntry {
            name: source.name.clone(),
            rect: FrameRect {
                x: x as u16,
                y: y as u16,
                width: width as u16,
                height: height as u16,
            },
            pivot: source.pivot,
            footprint: source.footprint,
        });
    }

    PackedAtlas {
        metadata: AtlasMetadata {
            image: "packed".to_string(),
            entries,
        },
        canvas,
    }
}

/// 简单的「货架」矩形装箱：把已排序的尺寸序列逐个铺进有限宽度的行，
/// 行放不下就另起一行。返回每个输入尺寸对应的左上角坐标，与输入
/// 顺序一一对应。
///
/// 不追求最优装箱率（那是离线工具该做的优化，见
/// `knowledge/design/mod-package-structure.md` 对 `tools/ll-artgen`
/// 定位变化的讨论）——运行期打包器要的是「够快、结果确定」，货架
/// 算法是这两条约束下最简单可靠的选择：只依赖输入顺序（调用方已经
/// 排好序）与固定的行宽策略，不含任何随机或哈希迭代。
fn shelf_pack(sizes: &[(u32, u32)]) -> Vec<(u32, u32)> {
    // 行宽取全部条目里最宽的一个乘以一个小的横向条目数，让图集大致
    // 呈方形而不是一整条极窄的长条——纯粹是打包效率的取舍，不影响
    // 正确性：任意正数的行宽都能产出合法（哪怕效率很差）的装箱结果。
    const ROW_ITEMS_TARGET: u32 = 8;
    let max_width = sizes.iter().map(|&(w, _)| w).max().unwrap_or(1);
    let row_width = max_width.saturating_mul(ROW_ITEMS_TARGET).max(max_width);

    let mut placements = Vec::with_capacity(sizes.len());
    let mut cursor_x = 0u32;
    let mut cursor_y = 0u32;
    let mut row_height = 0u32;

    for &(width, height) in sizes {
        if cursor_x != 0 && cursor_x + width > row_width {
            cursor_x = 0;
            cursor_y += row_height + PADDING;
            row_height = 0;
        }
        placements.push((cursor_x, cursor_y));
        cursor_x += width + PADDING;
        row_height = row_height.max(height);
    }

    placements
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 编码一张纯色 `width`×`height` PNG，供测试构造贴图来源。
    fn solid_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut image = RgbaImage::new(width, height);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba(color);
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("内存编码 PNG 不应失败");
        bytes
    }

    fn source(name: &str, width: u32, height: u32) -> SpriteSource {
        SpriteSource {
            name: name.to_string(),
            image_bytes: solid_png(width, height, [255, 0, 0, 255]),
            pivot: Pivot { x: 0, y: 0 },
            footprint: Footprint {
                width: 1,
                height: 1,
            },
        }
    }

    #[test]
    fn 打包结果的条目数与输入贴图数一致() {
        // Arrange
        let sources = vec![source("a", 4, 4), source("b", 8, 8)];

        // Act
        let packed = pack_atlas(&sources);

        // Assert
        assert_eq!(packed.metadata.entries.len(), 2);
    }

    #[test]
    fn 打包后的条目能按名字查到() {
        // Arrange
        let sources = vec![source("hero_idle_0", 16, 24)];

        // Act
        let packed = pack_atlas(&sources);

        // Assert
        assert!(packed.metadata.lookup("hero_idle_0").is_some());
    }

    #[test]
    fn 打包后的条目帧矩形不超出画布边界() {
        // Arrange
        let sources = vec![source("a", 16, 16), source("b", 32, 8), source("c", 4, 40)];

        // Act
        let packed = pack_atlas(&sources);
        let (canvas_w, canvas_h) = packed.canvas.dimensions();

        // Assert
        for entry in &packed.metadata.entries {
            assert!(entry.rect.x as u32 + entry.rect.width as u32 <= canvas_w);
            assert!(entry.rect.y as u32 + entry.rect.height as u32 <= canvas_h);
        }
    }

    #[test]
    fn 打包后的锚点与占地格数原样保留() {
        // Arrange
        let mut src = source("a", 16, 16);
        src.pivot = Pivot { x: 8, y: 16 };
        src.footprint = Footprint {
            width: 2,
            height: 3,
        };

        // Act
        let packed = pack_atlas(&[src]);

        // Assert
        let entry = packed.metadata.lookup("a").expect("条目应存在");
        assert_eq!(entry.pivot, Pivot { x: 8, y: 16 });
        assert_eq!(
            entry.footprint,
            Footprint {
                width: 2,
                height: 3
            }
        );
    }

    #[test]
    fn 损坏的贴图被跳过而不影响其它条目() {
        // Arrange
        let mut broken = source("broken", 16, 16);
        broken.image_bytes = b"not a png".to_vec();
        let good = source("good", 8, 8);

        // Act
        let packed = pack_atlas(&[broken, good]);

        // Assert
        assert_eq!(packed.metadata.entries.len(), 1);
        assert!(packed.metadata.lookup("good").is_some());
        assert!(packed.metadata.lookup("broken").is_none());
    }

    #[test]
    fn 全部贴图都损坏时退化为空图集而不是崩溃() {
        // Arrange
        let mut broken = source("broken", 16, 16);
        broken.image_bytes = b"not a png".to_vec();

        // Act
        let packed = pack_atlas(&[broken]);

        // Assert
        assert!(packed.metadata.entries.is_empty());
    }

    #[test]
    fn 空输入产出空图集而不是崩溃() {
        // Arrange & Act
        let packed = pack_atlas(&[]);

        // Assert
        assert!(packed.metadata.entries.is_empty());
        assert!(packed.canvas.dimensions().0 >= 1);
    }

    #[test]
    fn sources乱序传入仍打出逐位相同的图集() {
        // 「同样的 mod 集合必须打出逐位相同的图集」的核心断言：同一批
        // 贴图内容，仅调换 `Vec` 里的传入顺序，画布像素与元数据必须
        // 完全相同——见模块文档「确定性」一节。
        // Arrange
        let forward = vec![
            source("aaa", 16, 16),
            source("bbb", 8, 8),
            source("ccc", 32, 4),
        ];
        let shuffled = vec![forward[2].clone(), forward[0].clone(), forward[1].clone()];

        // Act
        let packed_forward = pack_atlas(&forward);
        let packed_shuffled = pack_atlas(&shuffled);

        // Assert
        assert_eq!(
            packed_forward.canvas.as_raw(),
            packed_shuffled.canvas.as_raw()
        );
        assert_eq!(
            packed_forward.metadata.entries,
            packed_shuffled.metadata.entries
        );
    }

    #[test]
    fn 重复打包同一批贴图产出逐位相同的图集() {
        // Arrange
        let sources = vec![source("a", 16, 16), source("b", 8, 8)];

        // Act
        let first = pack_atlas(&sources);
        let second = pack_atlas(&sources);

        // Assert
        assert_eq!(first.canvas.as_raw(), second.canvas.as_raw());
        assert_eq!(first.metadata.entries, second.metadata.entries);
    }
}
