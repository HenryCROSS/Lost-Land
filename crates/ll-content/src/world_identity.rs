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

/// 一档推荐的地图尺寸预设：区块边长（固定 128，与
/// [`ZoneLayout::default_config`] 一致）+ 世界区块数。
///
/// 四档预设全部选长方形（`zone_count` 的宽高不相等）——不是因为正方形
/// 必然踩雷（`safe_coarse_scale` 已经是通用算法级修复,见
/// `crates/ll-world/src/noise.rs` 模块文档「一个更隐蔽的退化」，任何
/// 尺寸修复后都安全），而是长方形天然远离「两轴周期相等」这个退化
/// 触发条件，不需要依赖减半分支就能确认安全,见
/// `crates/ll-world/tests/noise_presets.rs` 的多样性回归测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizePreset {
    /// 供 UI 展示的标签，非最终文案（P7 UI 落地时按 Fluent 本地化）。
    pub label: &'static str,
    /// 区块边长（格）。
    pub zone_span: u32,
    /// 世界区块数 `(宽, 高)`。
    pub zone_count: (u32, u32),
}

/// 四档推荐预设：小/中/大/巨，区块边长固定 128。
///
/// 「中」正是 [`ZoneLayout::default_config`] 给出的默认配置（48×32
/// 区块）——推荐预设表不是凭空另起一套数值，是把设计文档十一节的默认
/// 值纳入同一张表，与其余三档并列展示。
pub const RECOMMENDED_PRESETS: &[SizePreset] = &[
    SizePreset {
        label: "小陆地",
        zone_span: 128,
        zone_count: (32, 24),
    },
    SizePreset {
        label: "标准",
        zone_span: 128,
        zone_count: (48, 32),
    },
    SizePreset {
        label: "广阔",
        zone_span: 128,
        zone_count: (64, 48),
    },
    SizePreset {
        label: "浩瀚",
        zone_span: 128,
        zone_count: (96, 64),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;
    use ll_mod::manifest::ModManifest;
    use ll_mod::registry::Registry;
    use std::path::PathBuf;

    fn manifest(namespace: &str, version: &str) -> ModManifest {
        ModManifest {
            id: NamespacedId::parse(&format!("{namespace}:self")).expect("测试用命名空间恒合法"),
            version: version.to_string(),
            dependencies: Vec::new(),
            entry_points: Vec::<PathBuf>::new(),
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
