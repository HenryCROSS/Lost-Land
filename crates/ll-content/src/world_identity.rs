//! 世界身份三要素：种子 + 尺寸 + 生成期 mod 集合。
//!
//! # 为什么尺寸也是身份的一部分
//!
//! 项目所有者已定：地图大小在开局建档前由玩家选择，世界可以是长方形
//! （区块与瓦片本身是正方形，世界总体不必是）。种子相同、mod 集合相同
//! 但尺寸不同，噪声场采样的周期（[`ll_world::noise::TileableNoise`]）
//! 跟着变，产出的不是同一张地形——尺寸因此和种子、生成期 mod 集合一样
//! 「缺一，世界都复现不出来」，见 `knowledge/design/identity-and-ids.md`
//! 六节与本模块最初的会话记录。
//!
//! # 绑定时机：世界创建时刻
//!
//! 三要素的绑定不等待任何生成器——`ll_mod::mod_set` 模块文档「绑定
//! 时机」一节已经更正了「留给 P6 世界生成器」这句过期注释：规格插入
//! 新 P6（物品与装备）后，真正的历史世界生成器现排到 P7,而世界创建
//! （地形本身，从 P2 起就存在）不需要等它。[`WorldIdentity::bind`] 是
//! 这个绑定时机在类型层面的落点——调用它的地方就是"世界创建"这一刻，
//! 不应该在任何更晚的时间点（例如每次读档）重新调用。
//!
//! # 本模块不设计开局 UI
//!
//! `ll-ui` 完整控件库在 P7，本模块只交付"给定一个尺寸候选，返回是否
//! 安全"的纯函数校验（[`validate_size_choice`]）与一份推荐预设表
//! （[`RECOMMENDED_PRESETS`]），供未来 P7 UI 直接引用。

use ll_core::torus::TorusSize;
use ll_mod::mod_set::GenerationModSet;
use ll_world::WorldError;
use ll_world::zone::ZoneLayout;

use crate::header::ModHeaderEntry;

/// 一档推荐的地图尺寸预设：区块边长（固定 48，与
/// [`ZoneLayout::default_config`] 一致）+ 世界区块数。
///
/// 四档预设全部选长方形（`zone_count` 的宽高不相等）——不是因为正方形
/// 必然踩雷（`safe_coarse_scale` 已经是通用算法级修复,见
/// `crates/ll-world/src/noise.rs` 模块文档「一个更隐蔽的退化」，任何
/// 尺寸修复后都安全），而是长方形天然远离「两轴周期相等」这个退化
/// 触发条件，不需要依赖减半分支就能确认安全,见
/// `crates/ll-world/tests/noise_presets.rs` 的多样性回归测试。
///
/// # 为什么区块边长从 128 改成 48
///
/// 见 [`ZoneLayout::default_config`] 文档：项目所有者裁定区块边长默认
/// 改为 48（`= CELL_SIZE * 3`，奇数倍数），这类取值下任何 `zone_count`
/// 都不会触发噪声大陆尺度层退化（同一份文档给出证明），比旧值 128
/// （`= CELL_SIZE * 8`，纯 2 的幂）更不容易踩雷——四档预设因此不需要
/// 重新论证一遍是否落在退化区间，`crates/ll-world/tests/noise_presets.rs`
/// 仍然保留实测多样性回归，作为独立于这条数学证明的经验性验证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizePreset {
    /// 供 UI 展示的标签，非最终文案（P7 UI 落地时按 Fluent 本地化）。
    pub label: &'static str,
    /// 区块边长（格）。
    pub zone_span: u32,
    /// 世界区块数 `(宽, 高)`。
    pub zone_count: (u32, u32),
}

/// 四档推荐预设：小/中/大/巨，区块边长固定 48。
///
/// 「标准」正是 [`ZoneLayout::default_config`] 给出的默认配置（96×64
/// 区块）——推荐预设表不是凭空另起一套数值，是把设计文档十一节的默认
/// 值纳入同一张表，与其余三档并列展示。其余三档在「标准」基础上按
/// 相同的宽高比例（4:3 / 3:2 交替，与旧版预设表同一种排布习惯）伸缩。
pub const RECOMMENDED_PRESETS: &[SizePreset] = &[
    SizePreset {
        label: "小陆地",
        zone_span: 48,
        zone_count: (64, 48),
    },
    SizePreset {
        label: "标准",
        zone_span: 48,
        zone_count: (96, 64),
    },
    SizePreset {
        label: "广阔",
        zone_span: 48,
        zone_count: (128, 96),
    },
    SizePreset {
        label: "浩瀚",
        zone_span: 48,
        zone_count: (192, 128),
    },
];

/// 校验一组尺寸选择是否能构造出合法的 [`ZoneLayout`]。
///
/// 两步校验：`zone_count` 本身必须是合法的 [`TorusSize`]（非零、不超过
/// [`TorusSize::MAX_EXTENT`]），再交给 [`ZoneLayout::new`] 校验区块边长
/// 的对齐约束——不重新实现任何一层校验规则，只是把两层串起来给一个
/// 统一的入口，供未来 P7 开局界面直接调用。
///
/// `zone_count` 不合法（零或溢出）时按 [`WorldError::WorldTooSmall`]
/// 报告——`TorusSize::new` 本身不携带失败原因，而「宽高任一维为零」与
/// 「尺寸小于视口所需跨度」在用户可见的意义上是同一类问题（尺寸选得
/// 不合理），复用这个既有变体不需要为一个不会被任何推荐预设触发的
/// 边界新增变体。
pub fn validate_size_choice(
    zone_span: u32,
    zone_count: (u32, u32),
) -> Result<ZoneLayout, WorldError> {
    let count = TorusSize::new(zone_count.0, zone_count.1).ok_or(WorldError::WorldTooSmall {
        width: zone_count.0,
        height: zone_count.1,
    })?;
    ZoneLayout::new(zone_span, count)
}

/// 世界身份三要素——种子、尺寸、生成期 mod 集合——捆绑在一起的类型，
/// 三者缺一，同一个世界都无法复现（见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldIdentity {
    /// 生成本世界地形所用的种子。
    pub seed: u64,
    /// 世界尺寸（区块边长 + 区块数）。
    pub zone_layout: ZoneLayout,
    /// 生成期 mod 集合快照，写入后永久不变。
    pub generation_mods: GenerationModSet,
}

impl WorldIdentity {
    /// 在世界创建时刻一次性捆绑三要素。
    ///
    /// 本方法本身就是"绑定时机"的落点：调用它的地方就是"世界创建"这
    /// 一刻——不应该在任何更晚的时间点（例如每次读档）重新调用，读档
    /// 应该直接从存档头读回三要素，不是重新推导。
    pub fn bind(seed: u64, zone_layout: ZoneLayout, generation_mods: GenerationModSet) -> Self {
        WorldIdentity {
            seed,
            zone_layout,
            generation_mods,
        }
    }
}

/// 把 [`GenerationModSet`]（`ll_mod::mod_set`）转换成
/// [`crate::header::SaveHeader::generation_mods`] 可以直接使用的
/// `Vec<ModHeaderEntry>`。
///
/// # 断链三修复（P5-A 任务 14）
///
/// `ll_mod::mod_set::ModSetEntry` 与 `crate::header::ModHeaderEntry`
/// 字段形状几乎相同（命名空间 + 版本号 + 内容哈希），但分属两个不同
/// crate 的类型，此前没有任何生产代码把两者接起来——`ll-content` 全部
/// 现存测试（含 P5 批次 E、L6 端到端脚手架）都是直接手写
/// `Vec<ModHeaderEntry>` 或干脆留空，验收 demo（任务 13）为了走通
/// `WorldIdentity::bind` 到「可以写进存档头」这一环，临时在 demo 自己
/// 的代码里补了一份等价的转换逻辑（不是生产代码），并如实记录了这处
/// 缺口。本函数是补上的那一环——`ModHeaderEntry` 只用 `String`/整数/
/// 枚举这类原始类型（见 [`crate::header`] 模块文档「为什么头部不能
/// 引用 `ContentIndex`」），转换本身只是把 `NamespacedId` 取出命名空间
/// 部分、版本号与内容哈希原样搬过来，不涉及任何需要额外校验或推导的
/// 逻辑。
///
/// 调用点：见 [`crate::save_file`] 的存档写出流程测试与
/// `crates/ll-content/examples/p5_save_acceptance.rs`——两处都已经改为
/// 调用这个函数，不再各自重新发明一份等价的搬运代码。
pub fn generation_mods_to_header_entries(set: &GenerationModSet) -> Vec<ModHeaderEntry> {
    set.0
        .iter()
        .map(|entry| ModHeaderEntry {
            namespace: entry.id.namespace().to_string(),
            version: entry.version.clone(),
            content_hash: entry.content_hash,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;
    use ll_mod::manifest::ModManifest;
    use ll_mod::registry::Registry;

    fn manifest(namespace: &str, version: &str) -> ModManifest {
        ModManifest {
            id: NamespacedId::parse(&format!("{namespace}:self")).expect("测试用命名空间恒合法"),
            version: version.to_string(),
            dependencies: Vec::new(),
        }
    }

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn generationmodset一旦封存后与后续currentmodset的变化无关() {
        // 类型上两者已经隔离（GenerationModSet/CurrentModSet 是不同
        // 类型，见 ll_mod::mod_set 模块文档的 compile_fail 示例），这里
        // 补运行期断言：捆绑进 WorldIdentity 之后，registry 继续变化
        // 不会让已经绑定的三要素跟着漂移。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);
        let identity = WorldIdentity::bind(42, ZoneLayout::default_config(), generation.clone());

        // Act：世界创建之后 registry 继续变化。
        registry.intern(id("lostland:river"));

        // Assert：已绑定的三要素原样不变。
        assert_eq!(identity.generation_mods, generation);
    }

    #[test]
    fn 清单里的版本号原样进存档头且改一个字符就打不开() {
        // 项目所有者裁决：「新版本不兼容旧版本存档就是了，版本不对就
        // 打不开。」这条测试把那句话钉成一条可执行的断言，钉的是整条
        // 链——`mod.json5` 的 `version` → `ModHeaderEntry.version` →
        // `check_mod_set` 硬门禁——而不是链上任何单独一环。
        //
        // 为什么值得单独钉：链上每一环各自都有测试，但「改 mod.json5
        // 的版本号会让此前的存档全部打不开」这个**后果**此前没有任何
        // 一条测试直说。它是一颗定时炸弹还是一条有意的策略，区别只在
        // 于有没有人把它写下来——策略见
        // knowledge/design/save-and-mod-version-policy.md。
        // Arrange：生成期的 mod 清单里版本是 0.1.0。
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let 生成期清单 = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &生成期清单);
        let 存档头条目 = generation_mods_to_header_entries(&generation);

        // Assert 其一：版本号原样搬进存档头，没有任何规范化。
        assert_eq!(存档头条目[0].version, "0.1.0");

        // Act & Assert 其二：清单版本没动 → 放行。
        assert!(crate::load_error::check_mod_set(&存档头条目, &生成期清单).is_ok());

        // Act & Assert 其三：只改末尾一个字符 → 硬门禁拒绝。
        // 不做语义化版本解析，"0.1.1 是 0.1.0 的兼容升级"这种判断在这
        // 里不存在，也不该存在。
        let 改过版本的清单 = vec![manifest("lostland", "0.1.1")];
        let err = crate::load_error::check_mod_set(&存档头条目, &改过版本的清单)
            .expect_err("版本号改了就该打不开");
        assert!(
            matches!(err, crate::load_error::LoadError::ModSetMismatch(_)),
            "实际是 {err:?}"
        );
    }

    #[test]
    fn generation_mods_to_header_entries产出的条目字段与源数据逐一对应() {
        // 断链三修复的核心验证：GenerationModSet -> Vec<ModHeaderEntry>
        // 这次转换本身只是原样搬运,不丢字段、不改数值。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].namespace, "lostland");
        assert_eq!(entries[0].version, "0.1.0");
        assert_eq!(
            entries[0].content_hash,
            registry.content_hash_of("lostland")
        );
    }

    #[test]
    fn generation_mods_to_header_entries对未贡献内容的mod保留空哈希() {
        // 裁定 P5-8 配套：「在场但从未贡献内容」的 content_hash 是
        // None,转换过程不能把它折叠成任何裸整数（例如 0）。
        // Arrange
        let registry = Registry::new();
        let manifests = vec![manifest("emptymod", "1.0.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert_eq!(entries[0].content_hash, None);
    }

    #[test]
    fn generation_mods_to_header_entries对空集合产出空列表() {
        // Arrange
        let generation = GenerationModSet(Vec::new());

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert!(entries.is_empty());
    }

    #[test]
    fn 每个推荐预设满足zonelayout现有构造约束() {
        // Arrange & Act & Assert
        for preset in RECOMMENDED_PRESETS {
            let result = validate_size_choice(preset.zone_span, preset.zone_count);
            assert!(
                result.is_ok(),
                "预设 {} 未能构造出合法的 ZoneLayout: {:?}",
                preset.label,
                result
            );
        }
    }

    #[test]
    fn validate_size_choice对不满足cell_size整除约束的尺寸返回错误() {
        // Arrange：50 不是 CELL_SIZE(16) 的整数倍。
        // Act
        let result = validate_size_choice(50, (4, 4));

        // Assert
        assert!(matches!(
            result,
            Err(WorldError::ZoneSpanNotAligned { zone_span: 50 })
        ));
    }

    #[test]
    fn validate_size_choice对零区块数返回错误而不panic() {
        // Arrange & Act
        let result = validate_size_choice(128, (0, 32));

        // Assert
        assert!(matches!(result, Err(WorldError::WorldTooSmall { .. })));
    }

    #[test]
    fn 标准预设与zonelayout默认配置产出相同的区块布局() {
        // 「中」档预设不是另起一套数值,是纳入了设计文档十一节的默认值
        // ——这里锁住两者确实一致。
        // Arrange
        let standard = RECOMMENDED_PRESETS[1];

        // Act
        let from_preset =
            validate_size_choice(standard.zone_span, standard.zone_count).expect("标准预设恒合法");

        // Assert
        assert_eq!(from_preset, ZoneLayout::default_config());
    }
}
