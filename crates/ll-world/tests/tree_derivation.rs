//! 树木**派生层**的回归：确定性、O(1)、不消耗随机流、环面、气候分布。
//!
//! # 这个文件为什么不装载 `mods/`
//!
//! [ADR 0031] 的「已知的合法例外」一节：**引擎层的纯算法单元**（噪声、
//! 哈希、几何、环面换算）没有内容侧输入，不需要经 `mods/`。
//! `ll_world::tree::derived_tree_at` 正是这一类——它的输入是
//! `(种子, 位置, 地形, 世界高度, 条带宽度)`，一个内容 id 都没有。
//!
//! **树这个能力本身仍然要有端到端证据**，那部分在
//! `crates/ll-game/tests/tree_end_to_end.rs`（真实 `mods/` + 生产路径）。
//! 本文件只管这一层的算法性质。
//!
//! [ADR 0031]: ../../../knowledge/decisions/0031-end-to-end-evidence-through-real-content.md

use ll_core::torus::TorusSize;
use ll_world::climate::ClimateBand;
use ll_world::terrain::{TerrainKind, base_terrain_fixture};
use ll_world::tree::{TreeSpecies, derived_species_at, derived_tree_at};

/// 本文件统一的世界尺寸——高度取 512，`climate::warmth_at` 的周期因此是
/// 256，一整张图里恰有两条赤道与两条极圈（气候条带批次那条设计要求）。
const W: u32 = 512;
const H: u32 = 512;

/// 与 `ll_world::terrain_shape::TerrainShape::default()` 的
/// `climate_band_width` 同值——**不硬编码一个自己编的宽度**：本文件要验的
/// 是「气候真的在起作用」，用一个生产路径上不存在的宽度会让结论对不上。
fn band_width() -> i32 {
    ll_world::terrain_shape::TerrainShape::default().climate_band_width
}

fn size() -> TorusSize {
    TorusSize::new(W, H).expect("512x512 是合法尺寸")
}

fn forest_and_grass() -> (TerrainKind, TerrainKind) {
    let (ids, _) = base_terrain_fixture();
    (ids.forest, ids.grass)
}

/// 把整张 `W×H` 图上的森林格全部派生一遍，统计树种。
fn census(seed: u64, forest: TerrainKind) -> (usize, [usize; 3], usize) {
    let size = size();
    let mut trees = 0usize;
    let mut by_species = [0usize; 3];
    let mut tiles = 0usize;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            tiles += 1;
            if let Some(species) =
                derived_tree_at(seed, size.wrap(x, y), forest, forest, H, band_width())
            {
                trees += 1;
                let index = TreeSpecies::ALL
                    .iter()
                    .position(|s| *s == species)
                    .expect("派生出的树种必在 ALL 里");
                by_species[index] += 1;
            }
        }
    }
    (trees, by_species, tiles)
}

#[test]
fn 同一格连算两次结果相同() {
    // 反例验证（已实跑，见计划文档十节）：给 `derived_tree_at` 混入一个
    // 全局计数器，本条当场红。
    let (forest, _) = forest_and_grass();
    let size = size();
    let pos = size.wrap(123, 456);

    let 第一次 = derived_tree_at(7, pos, forest, forest, H, band_width());

    // **中间穿插一万次别的格子**：若派生层藏着可变状态或消耗着某条随机
    // 流，穿插会改变第二次的结果。只连着调两次是测不出这件事的——那正是
    // 「断言恒绿因为它咬不到该咬的东西」那个形状。
    for i in 0..10_000i32 {
        let _ = derived_tree_at(
            7,
            size.wrap(i % W as i32, i / W as i32),
            forest,
            forest,
            H,
            band_width(),
        );
    }

    let 第二次 = derived_tree_at(7, pos, forest, forest, H, band_width());
    assert_eq!(
        第一次, 第二次,
        "穿插一万次别的格子之后，同一格必须算出同一个结果"
    );
}

#[test]
fn 换一个种子整张图的树就换一批() {
    // 对照组：没有这一条，「同一格连算两次相同」可以被一个恒返回 `None`
    // 的实现满足——那是 ADR 0022 点名的「判据退化成恒真」。
    let (forest, _) = forest_and_grass();
    let size = size();
    let 不同 = (0..H as i32)
        .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let pos = size.wrap(*x, *y);
            derived_tree_at(1, pos, forest, forest, H, band_width())
                != derived_tree_at(2, pos, forest, forest, H, band_width())
        })
        .count();
    assert!(
        不同 > (W * H) as usize / 10,
        "换种子只改变了 {不同} 格（共 {} 格）——种子多半没有真的进哈希",
        W * H
    );
}

#[test]
fn 非森林地形上一棵树都长不出来() {
    // 「`forest` 地形保留当底图」是项目所有者的要求原话。
    let (forest, grass) = forest_and_grass();
    let size = size();
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            assert_eq!(
                derived_tree_at(9, size.wrap(x, y), grass, forest, H, band_width()),
                None,
                "({x},{y}) 是草地却派生出了树"
            );
        }
    }
}

#[test]
fn 森林里真的有树而且不是每一格都有() {
    // **先断言对象存在**（ADR 0022 的「断言恒绿因为被断言的对象根本不
    // 存在」那个形状）：下面几条分布断言全部建立在「森林里真的数得出树」
    // 之上，这一条是它们的前提。
    //
    // 「不是每一格都有」同样是硬要求：满格都是树等于一堵墙，玩家只会
    // 绕着走。密度旋钮见 `TREE_DENSITY_PERMILLE` 文档。
    let (forest, _) = forest_and_grass();
    let (trees, _, tiles) = census(4242, forest);
    assert!(trees > 0, "整张森林图一棵树都没有");
    assert!(trees < tiles, "整张森林图每一格都是树");
    let permille = trees * 1000 / tiles;
    assert!(
        (500..=750).contains(&permille),
        "森林树木密度 {permille}‰ 落在设计区间之外（`TREE_DENSITY_PERMILLE` 是 620‰）"
    );
}

#[test]
fn 三条气候带的树种分布互不相同() {
    // **本批点名要验的第四条。** 反例验证（已实跑，见计划文档十节）：
    // 把 `species_weights` 三条带的权重表改成完全相同，本条当场红。
    //
    // 判据取「每条带各自的树种直方图」而不是「全图有多于一种树」——后者
    // 是一条会被「随便掷一个不看气候的骰子」满足的假判据。
    let (forest, _) = forest_and_grass();
    let size = size();
    let mut by_band: [[usize; 3]; 3] = [[0; 3]; 3];
    for y in 0..H as i32 {
        let band = ll_world::climate::band_at(y, H, band_width());
        let bi = match band {
            ClimateBand::Hot => 0,
            ClimateBand::Temperate => 1,
            ClimateBand::Polar => 2,
        };
        for x in 0..W as i32 {
            let Some(species) =
                derived_tree_at(4242, size.wrap(x, y), forest, forest, H, band_width())
            else {
                continue;
            };
            let si = TreeSpecies::ALL.iter().position(|s| *s == species).unwrap();
            by_band[bi][si] += 1;
        }
    }

    // 前提：三条带都真的有树可数（否则下面的比较是在比两个零向量）。
    for (bi, counts) in by_band.iter().enumerate() {
        assert!(
            counts.iter().sum::<usize>() > 0,
            "第 {bi} 条气候带一棵树都没数出来，下面的分布比较将毫无意义"
        );
    }

    // 三条带两两不同——**比的是比例不是绝对数**（三条带的面积不一样）。
    let ratio = |c: [usize; 3]| {
        let total: usize = c.iter().sum();
        [
            c[0] * 1000 / total,
            c[1] * 1000 / total,
            c[2] * 1000 / total,
        ]
    };
    let r: Vec<[usize; 3]> = by_band.iter().map(|c| ratio(*c)).collect();
    // 判据是「差得**足够多**」，不是「不相等」。
    //
    // **第一版写的是 `assert_ne!(r[i], r[j])`，那是一条近似恒真的断言。**
    // 三条带的树数量不同，抽样噪声本来就会让千分比整数彼此不等——哪怕
    // 权重表被改成三行完全一样。反例验证时它**真的没红**：红的是下面那
    // 两条零权重断言（「干热带不该有松树」）。把那两条临时关掉再验一次，
    // 三行相同权重下三条带的比例实测是 `[688,249,62]` / `[687,249,62]` /
    // `[687,249,62]`——**最大差只有 1‰，`assert_ne!` 照样过**。
    //
    // 这正是 ADR 0022 点名的「生产数据恰好让判据退化成恒真」。改成下面
    // 这条之后，同一个改坏动作当场红在这一行上。
    //
    // 门槛 100‰：真实权重下两两之间最大差值都在 500‰ 以上（Hot 的 Oak
    // 187‰ vs Temperate 的 687‰、Polar 的 Pine 876‰ vs Temperate 的
    // 249‰）；三行权重相同时只剩抽样噪声，实测 1‰。两者之间空得很宽。
    const 分布差门槛: usize = 100;
    for i in 0..3 {
        for j in (i + 1)..3 {
            let 最大差 = (0..3)
                .map(|k| r[i][k].abs_diff(r[j][k]))
                .max()
                .expect("三种树");
            assert!(
                最大差 >= 分布差门槛,
                "第 {i} 条与第 {j} 条气候带的树种比例几乎一样（{:?} vs {:?}，最大差 {最大差}‰                  < 门槛 {分布差门槛}‰）——气候多半没有真的接上",
                r[i],
                r[j]
            );
        }
    }

    // 权重表里的 0 是刻意的（热带无松、极地无棕榈），它让「某条带真的
    // 一棵某种树都没有」成为可断言的事实，而不是碰巧没抽到。
    assert_eq!(by_band[0][1], 0, "干热带不该有松树");
    assert_eq!(by_band[2][2], 0, "极地带不该有棕榈");

    println!("树种分布实测（种子 4242，512×512 全森林）：");
    for (bi, name) in ["Hot", "Temperate", "Polar"].iter().enumerate() {
        println!(
            "  {name:<10} Oak {:>4}‰  Pine {:>4}‰  Palm {:>4}‰  （共 {} 棵）",
            r[bi][0],
            r[bi][1],
            r[bi][2],
            by_band[bi].iter().sum::<usize>()
        );
    }
}

#[test]
fn 环面接缝上同一个物理格子派生出同一棵树() {
    // 「环面怎么处理」的判据。本函数**一行取模都没写**——`TorusSize::wrap`
    // 把绕回做在类型边界上，`wrap(x, y)` 与 `wrap(x + W, y + H)` 构造出的
    // 是**同一个 `TorusPos` 值**，因此派生结果必然相同。
    //
    // 这条断言因此验的是「没有人绕过 `TorusPos` 自己拿裸坐标去哈希」
    // ——那是本仓库禁止手写取模那条门禁同一条精神。
    let (forest, _) = forest_and_grass();
    let size = size();
    for (x, y) in [(0, 0), (1, 1), (W as i32 - 1, H as i32 - 1), (7, 500)] {
        let 直接 = size.wrap(x, y);
        let 绕一圈 = size.wrap(x + W as i32, y + H as i32);
        let 反向绕 = size.wrap(x - W as i32, y - H as i32);
        assert_eq!(直接, 绕一圈);
        assert_eq!(直接, 反向绕);
        assert_eq!(
            derived_tree_at(11, 直接, forest, forest, H, band_width()),
            derived_tree_at(11, 绕一圈, forest, forest, H, band_width()),
            "环面接缝两侧的同一个物理格子派生出了不同的树"
        );
    }
}

#[test]
fn 相邻格子长树与否的相关性接近相互独立() {
    // **这条是批次 28 那个坑的本批复检，不是抄结论。**
    //
    // FNV-1a 最后混进去的那几个字节没有足够轮数被摊开，而 `write_i64`
    // 写的是小端 8 字节、世界坐标那种小整数高 7 字节全是 0——最后混进去
    // 的那个维度会明显倾向「相邻格同结果」。批次 28 在地形变体上实测到
    // 的后果是画面上的纵向条纹（32% → 47%）。`position_digest` 因此把
    // 长字符串域名放在最后。
    //
    // # 判据必须是**双侧**的，这一条是反例验证逼出来的
    //
    // 第一版只写了上界（`比例 < 理想 + 8 个百分点`），照抄批次 28。
    // 把 `y` 挪到域名之后再跑，**它没红**：实测纵向 501‰，比理想的
    // 529‰ **低** 28‰，不是高。
    //
    // 原因是本函数与批次 28 验的不是同一件事：那里是「三张变体里挑一
    // 张」，弱雪崩表现为相邻格更常取到同一张；这里是「折算到 [0,1000)
    // 再与 620 比大小」的**二值阈值**，同样的弱雪崩表现成相邻格更常落
    // 在阈值两侧。**方向相反，偏离本身才是信号。** 上界那一版对这半边
    // 完全免疫——ADR 0022 意义上的一次真实覆盖缺口，如实登记在此。
    //
    // # 理想值现算，不写死
    //
    // 第一版把 529‰ 写成常量（由密度 620‰ 推出）。那样一改
    // `TREE_DENSITY_PERMILLE` 这条判据就自己失效了，而且**不会报错**，
    // 只会悄悄变成一条对着错误基准比大小的断言。这里改成从实测边际密度
    // 现算 `p² + (1-p)²`。
    //
    // 门槛 ±20‰：512×512 共 262144 个样本，比例的标准误约 1‰；本次序
    // 实测偏离 2/8/8‰，把 `y` 挪到最后偏离 28‰。两者之间空得下这条线。
    let (forest, _) = forest_and_grass();
    let size = size();
    let has = |x: i32, y: i32| {
        derived_tree_at(4242, size.wrap(x, y), forest, forest, H, band_width()).is_some()
    };
    let mut same = [0usize; 3]; // 横向 / 纵向 / 斜向
    let mut trees = 0usize;
    let mut total = 0usize;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let h = has(x, y);
            total += 1;
            if h {
                trees += 1;
            }
            if h == has(x + 1, y) {
                same[0] += 1;
            }
            if h == has(x, y + 1) {
                same[1] += 1;
            }
            if h == has(x + 1, y + 1) {
                same[2] += 1;
            }
        }
    }
    // 相互独立时「两格结果相同」的概率 = p² + (1-p)²，p 取实测边际密度。
    let p = trees as f64 / total as f64;
    let 理想 = ((p * p + (1.0 - p) * (1.0 - p)) * 1000.0).round() as i64;
    const 门槛: i64 = 20;
    let 方向 = ["横向", "纵向", "斜向"];
    for (i, name) in 方向.iter().enumerate() {
        let permille = (same[i] * 1000 / total) as i64;
        let 偏离 = (permille - 理想).abs();
        println!("{name}相邻同结果 {permille}‰（相互独立时 {理想}‰，偏离 {偏离}‰）");
        assert!(
            偏离 <= 门槛,
            "{name}相邻格有 {permille}‰ 取到同一结果，相互独立时应为 {理想}‰，             偏离 {偏离}‰ 超过门槛 {门槛}‰——哈希混入次序多半被人改了，             见 `position_digest` 文档「混入次序为什么是『域名放最后』」一节"
        );
    }
}

#[test]
fn 派生层里没有循环也没有随机流() {
    // **「派生层没有消耗随机流」与「O(1)」两条性质的可执行版本。**
    //
    // 局限如实登记（计划文档三节写过同一条）：这条读的是源码文本，它证的
    // 是**没有回潮**（下一个人往里加一个「扫周围 8 格」的循环、或者顺手
    // 摸一次 `DetRng`），不是渐近复杂度本身——真正的 O(1) 论证是结构性的，
    // 写在 `derived_tree_at` 的文档里。
    //
    // 手法与仓库既有的 `scripts/ci/check_single_anchor_impl.sh` 同一条：
    // 判据放在能被执行的地方，不放在人的记性里。
    let 全文 = include_str!("../src/tree.rs");

    // **只扫代码行，不扫注释。** 第一版直接扫全文，当场红在自己的模块
    // 文档上（那段文档解释的正是「为什么不碰 `DetRng`」）——判据把讲
    // 这件事的散文当成了做这件事的代码。这不是一次小意外：一条**判据
    // 本身写错**的断言与一条**恒绿**的断言一样没有保护力，区别只是前者
    // 会当场吵。留这段注释是因为下一个往本文件加扫描规则的人会踩同一脚。
    let src: String = 全文
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let src = src.as_str();

    assert!(
        !src.contains("DetRng"),
        "`ll_world::tree` 里出现了 `DetRng`——渲染期每帧调用的派生层不许\
         消耗随机流（约束 C3，见模块文档「派生层为什么不碰 `DetRng`」）"
    );
    assert!(
        !src.contains("HashMap") && !src.contains("HashSet"),
        "`ll_world::tree` 里出现了哈希容器——偏差表要参与世界摘要遍历（约束 C5）"
    );
    assert!(
        !src.contains("rem_euclid") && !src.contains(" % "),
        "`ll_world::tree` 里出现了手写取模——环面绕回由 `TorusPos` 的类型\
         边界保证，见 `derived_tree_at` 文档「环面」一节"
    );

    // `while` 一律不许；`for` 只允许出现在 `#[cfg(test)]` 之后（本文件的
    // 单元测试要遍历权重表）与那一段定长的三项权重扫描里。
    let 生产段 = src.split("#[cfg(test)]").next().expect("总有第一段");
    assert!(
        !生产段.contains("while "),
        "`ll_world::tree` 的生产代码里出现了 `while`——派生层必须是常数时间"
    );
    let for_次数 = 生产段.matches("for ").count();
    assert!(
        for_次数 <= 2,
        "`ll_world::tree` 的生产代码里有 {for_次数} 处 `for`（允许至多 2：\
         `derived_species_at` 那段定长三项权重扫描、`write_hash` 那段按偏差\
         条数的遍历）——多出来的那一处很可能是一次会随世界规模增长的遍历"
    );
}

#[test]
fn 培植长出的树种与那块地的气候一致() {
    // 「种下去长出什么由那块地的气候决定，不由种子决定」——这条规则与
    // 「分布由气候决定」是同一个函数的两次应用（`derived_species_at`），
    // 不是两套逻辑。本条钉住那个「同一个函数」。
    let size = size();
    for y in [0, H as i32 / 8, H as i32 / 4, H as i32 / 2] {
        let pos = size.wrap(3, y);
        let 培植 = derived_species_at(4242, pos, H, band_width());
        let (forest, _) = forest_and_grass();
        // 那一格若本来就派生出树，两者必须是同一种——否则「种下去长出的
        // 树」与「原本长在那儿的树」会是两套规则。
        if let Some(原生) = derived_tree_at(4242, pos, forest, forest, H, band_width()) {
            assert_eq!(培植, 原生, "({},{y}) 的培植树种与原生树种不一致", 3);
        }
    }
}
