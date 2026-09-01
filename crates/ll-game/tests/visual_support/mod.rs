//! 三张纯 CPU 视觉基准共用的机制：把真实图集上的矩形拷进一张画布、整数
//! 放大、**与仓库里那张基准 PNG 逐像素比对**，以及有意更新时的环境变量
//! 覆盖。
//!
//! 使用者是 `crates/ll-game/tests/visual_baselines.rs` 里的三条测试；基准
//! 图与「每张图应当能一眼看见什么」写在
//! `crates/ll-game/tests/visual/README.md`。
//!
//! # 为什么判据是逐像素相等
//!
//! 这三张图的每一个像素都来自 `PackedAtlas::canvas` 的**整数拷贝**：没有
//! 浮点、没有插值、没有抗锯齿，透明像素是**跳过**而不是做 alpha 混合运算。
//! 上游的世界生成有黄金基准盯着确定性，图集打包不经任何哈希容器（约束
//! C5）。也就是说这条路径**应当**逐位确定，逐像素相等是它能达到的最强判据。
//!
//! 反过来说，任何更宽松的判据——只比尺寸、只比哈希前缀、给颜色留容差——
//! 都要先回答「多大的差异算差异」，而那个数字没有任何依据可推。ADR 0022
//! 的主张「覆盖不全的守护等于没有守护」在这里的具体形状是：**容差写宽一
//! 点，图画坏了也绿**。所以这里不设容差；将来真的观察到无害抖动，那时带着
//! 实测证据放宽，比现在预防性放宽安全。
//!
//! # 比的是解码后的像素，不是 PNG 文件字节
//!
//! `image` crate 换一个小版本就可能改压缩参数，同一份像素编出来的 PNG 字节
//! 不同。比文件字节会在与画面完全无关的地方变红，那是假信号。
//!
//! # 红了之后怎么看差在哪
//!
//! 比对失败时往 `target/visual-baselines/` 写两份产物，并把路径写进断言
//! 消息：`<名字>.actual.png`（本次实际产出，可与仓库里那张并排看）与
//! `<名字>.diff.png`（不同的像素涂成洋红、相同的像素压暗）。消息里另带不同
//! 像素数与前几个不同点的坐标和两侧 RGBA。只说「不一样」不够——README
//! 「比对失败时的处置规矩」要人判断「是有意的视觉调整还是缺陷」，判断需要
//! 这些输入。
//!
//! # 有意更新：`LL_BLESS_VISUAL=1`
//!
//! 置 `1` 时把本次产出写回基准路径并通过。规矩仍按 README：**绝不允许
//! 「看着不一样就重新跑一遍覆盖」**，只有在人已经判断过「这是有意的视觉
//! 调整」之后才用，并在提交信息里写清改了什么、为什么。

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use ll_render::atlas_pack::PackedAtlas;

/// 覆盖基准的环境变量名。置 `1` 生效，其余值（含未设置）一律走比对。
pub const BLESS_ENV: &str = "LL_BLESS_VISUAL";

/// 失败产物写到哪个目录（相对仓库根）。放在 `target/` 下，不进 git。
const ARTIFACT_DIR: &str = "target/visual-baselines";

/// 断言消息里最多逐个列出几个不同点——列全了没人看得完，列 0 个等于没说。
const MAX_REPORTED_DIFFS: usize = 8;

/// 仓库根：`ll-game` 到仓库根固定隔两级 `../..`，与
/// `tests/surface_render.rs` 里的 `repo_mods` 同一条推导。
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 某张基准图在仓库里的路径。
pub fn baseline_path(name: &str) -> PathBuf {
    repo_root()
        .join("crates/ll-game/tests/visual")
        .join(format!("{name}.png"))
}

/// 把图集条目按 source-over 叠到画布上：全透明的源像素**跳过**，让底下
/// 那一层透出来。画布外的目标像素直接丢弃（`dst` 允许为负）。
///
/// 「跳过全透明像素」不是省事，而是「地面物品堆的背景留空」在画面上生效
/// 的地方——下面那格地形要透得出来。
pub fn blit_over(canvas: &mut RgbaImage, atlas: &PackedAtlas, key: &str, dst_x: i32, dst_y: i32) {
    blit(canvas, atlas, key, dst_x, dst_y, true);
}

/// 把图集条目**原样拷贝**到画布上，透明像素照拷（会覆盖底下那一层）。
/// 地形那一层用它：地形贴图铺满整格且不透明，没有「透出来」这回事。
pub fn blit_copy(canvas: &mut RgbaImage, atlas: &PackedAtlas, key: &str, dst_x: i32, dst_y: i32) {
    blit(canvas, atlas, key, dst_x, dst_y, false);
}

fn blit(
    canvas: &mut RgbaImage,
    atlas: &PackedAtlas,
    key: &str,
    dst_x: i32,
    dst_y: i32,
    skip_transparent: bool,
) {
    let entry = atlas
        .metadata
        .lookup(key)
        .unwrap_or_else(|| panic!("图集里查不到条目 {key}"));
    let rect = entry.rect;
    for row in 0..i32::from(rect.height) {
        for col in 0..i32::from(rect.width) {
            let (tx, ty) = (dst_x + col, dst_y + row);
            if tx < 0 || ty < 0 || tx >= canvas.width() as i32 || ty >= canvas.height() as i32 {
                continue;
            }
            let src = *atlas.canvas.get_pixel(
                u32::from(rect.x) + col as u32,
                u32::from(rect.y) + row as u32,
            );
            if skip_transparent && src.0[3] == 0 {
                continue;
            }
            canvas.put_pixel(tx as u32, ty as u32, src);
        }
    }
}

/// 最近邻整数放大。16 像素的瓦片在看图工具里太小，看不出门把手、窗棂、
/// 胸口 6×6 徽记的形状；放大不产生新信息，只是让人看得见。
pub fn upscale(image: &RgbaImage, factor: u32) -> RgbaImage {
    let mut out = RgbaImage::new(image.width() * factor, image.height() * factor);
    for y in 0..out.height() {
        for x in 0..out.width() {
            out.put_pixel(x, y, *image.get_pixel(x / factor, y / factor));
        }
    }
    out
}

/// 与仓库里那张同名基准 PNG 比对；`LL_BLESS_VISUAL=1` 时改为覆盖基准。
///
/// 失败时 panic，消息里带失败产物的路径与前几个不同点的坐标。
pub fn assert_matches_baseline(name: &str, actual: &RgbaImage) {
    let baseline = baseline_path(name);
    if std::env::var(BLESS_ENV).ok().as_deref() == Some("1") {
        bless(name, actual, &baseline);
        return;
    }

    let expected = image::open(&baseline)
        .unwrap_or_else(|err| {
            panic!(
                "读不出基准图 {}：{err}。\n\
                 若这张基准确实还不存在，用 {BLESS_ENV}=1 跑一遍本条测试生成它，\n\
                 并按 crates/ll-game/tests/visual/README.md 的规矩在提交信息里说明。",
                baseline.display()
            )
        })
        .to_rgba8();

    let report = compare(&expected, actual);
    if report.is_match() {
        return;
    }

    let (actual_path, diff_path) = write_failure_artifacts(name, &expected, actual);
    panic!(
        "视觉基准 {name} 与仓库里那张对不上。\n\
         {}\n\
         实际产出：{}\n\
         差异图（洋红＝不同）：{}\n\
         基准：{}\n\
         \n\
         按 crates/ll-game/tests/visual/README.md 的规矩：**先判断**这是有意的\n\
         视觉调整还是缺陷，绝不允许「看着不一样就重新跑一遍覆盖」。确认是有意\n\
         调整之后，用 {BLESS_ENV}=1 覆盖基准，并在提交信息里写清改了什么、为什么。",
        report.describe(),
        actual_path.display(),
        diff_path.display(),
        baseline.display(),
    );
}

fn bless(name: &str, actual: &RgbaImage, baseline: &Path) {
    let before = image::open(baseline).ok().map(|image| image.to_rgba8());
    std::fs::create_dir_all(baseline.parent().expect("基准图有父目录")).expect("建基准目录");
    actual.save(baseline).expect("写基准 PNG");
    match before {
        Some(before) => {
            let report = compare(&before, actual);
            println!("[{BLESS_ENV}] 已覆盖基准 {name}：{}", report.describe());
        }
        None => println!("[{BLESS_ENV}] 已新建基准 {name}（此前不存在）"),
    }
}

/// 一次比对的结果：尺寸是否一致、有多少像素不同、前几个不同点是什么。
struct DiffReport {
    expected_size: (u32, u32),
    actual_size: (u32, u32),
    differing: u64,
    compared: u64,
    samples: Vec<(u32, u32, Rgba<u8>, Rgba<u8>)>,
}

impl DiffReport {
    fn is_match(&self) -> bool {
        self.expected_size == self.actual_size && self.differing == 0
    }

    fn describe(&self) -> String {
        let mut out = String::new();
        if self.expected_size != self.actual_size {
            out.push_str(&format!(
                "尺寸不同：基准 {}×{}，实际 {}×{}。",
                self.expected_size.0, self.expected_size.1, self.actual_size.0, self.actual_size.1,
            ));
            if self.compared == 0 {
                return out;
            }
            out.push_str("重叠区域内 ");
        }
        out.push_str(&format!(
            "{} / {} 个像素不同。",
            self.differing, self.compared
        ));
        for (x, y, want, got) in &self.samples {
            out.push_str(&format!(
                "\n  ({x}, {y}) 基准 {:?} → 实际 {:?}",
                want.0, got.0
            ));
        }
        if self.differing as usize > self.samples.len() {
            out.push_str(&format!(
                "\n  …另有 {} 个不同点未列出",
                self.differing as usize - self.samples.len()
            ));
        }
        out
    }
}

fn compare(expected: &RgbaImage, actual: &RgbaImage) -> DiffReport {
    let width = expected.width().min(actual.width());
    let height = expected.height().min(actual.height());
    let mut differing = 0u64;
    let mut samples = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let want = *expected.get_pixel(x, y);
            let got = *actual.get_pixel(x, y);
            if want == got {
                continue;
            }
            differing += 1;
            if samples.len() < MAX_REPORTED_DIFFS {
                samples.push((x, y, want, got));
            }
        }
    }
    DiffReport {
        expected_size: (expected.width(), expected.height()),
        actual_size: (actual.width(), actual.height()),
        differing,
        compared: u64::from(width) * u64::from(height),
        samples,
    }
}

/// 把实际产出与差异图写进 `target/visual-baselines/`，返回两个路径。
fn write_failure_artifacts(
    name: &str,
    expected: &RgbaImage,
    actual: &RgbaImage,
) -> (PathBuf, PathBuf) {
    let dir = repo_root().join(ARTIFACT_DIR);
    std::fs::create_dir_all(&dir).expect("建失败产物目录");
    let actual_path = dir.join(format!("{name}.actual.png"));
    let diff_path = dir.join(format!("{name}.diff.png"));
    actual.save(&actual_path).expect("写实际产出 PNG");
    diff_image(expected, actual)
        .save(&diff_path)
        .expect("写差异 PNG");
    (actual_path, diff_path)
}

/// 差异图：不同的像素涂洋红，相同的像素压暗到四分之一亮度当底图——这样
/// 差异落在画面的哪个部位（哪一格、哪一层）一眼可见，而不是一片纯色。
fn diff_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let common_w = expected.width().min(actual.width());
    let common_h = expected.height().min(actual.height());
    let mut out = RgbaImage::from_pixel(width, height, Rgba([255, 0, 255, 255]));
    for y in 0..common_h {
        for x in 0..common_w {
            let want = *expected.get_pixel(x, y);
            let got = *actual.get_pixel(x, y);
            if want != got {
                continue;
            }
            out.put_pixel(x, y, Rgba([got.0[0] / 4, got.0[1] / 4, got.0[2] / 4, 255]));
        }
    }
    out
}
