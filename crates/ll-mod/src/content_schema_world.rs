//! 内容数据文件 schema 的**世界侧**五类：地形、资源、空间层属性、
//! 天气、动画剪辑。
//!
//! 与 [`crate::content_schema`] 同一套形状（`Raw*` + `apply_*` +
//! `deny_unknown_fields` + 两阶段解析），拆成独立模块只是因为
//! `content_schema.rs` 一个文件已经装不下二十类内容——分界线取的是
//! 「写进哪个 crate 的表」：这四类写进 `ll_world`（地形/空间层/天气）
//! 或不进世界状态的展示表（动画剪辑），其余写进 `ll_mod` 自己的内容表。
//!
//! # 布尔字段没有默认值
//!
//! `blocks_sight`／`blocks_move`／`looping` 一族全部**必填**，不带
//! `#[serde(default)]`。理由与 `#[serde(deny_unknown_fields)]` 是同一
//! 条：布尔的默认值（`false`）恰好也是一个完全合法的取值，缺字段与
//! 「作者确实想要 false」在结果上无法区分——一旦漏写，症状是「这块地
//! 形突然不挡视线了」，而没有任何报错。数值字段同理，除非缺省值本身
//! 有明确语义（见各字段文档）。

use ll_world::resource::{ResourceAttrs, ResourceCategory, ResourceError, ResourceTable};
use ll_world::space_profile::{SpaceProfileAttrs, SpaceProfileError, SpaceProfileTable};
use ll_world::terrain::{TerrainAttrs, TerrainError, TerrainKind, TerrainTable};
use ll_world::weather::{WeatherAttrs, WeatherError, WeatherTable};
use serde::Deserialize;

use ll_render::anim::Clip;

use crate::clip::{ClipError, ClipTable};
use crate::content_schema::{Applied, intern_id, parse_id};
use crate::registry::Registry;

// ───────────────────────────── 地形 ─────────────────────────────

/// `terrain.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainFile {
    /// 地形名册，按书写顺序注册。
    pub terrains: Vec<RawTerrain>,
}

/// 一条地形声明——对应此前的
/// `(register-terrain id blocks-sight blocks-move move-cost opens-into)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTerrain {
    /// 完整命名空间标识符。
    pub id: String,
    /// 是否阻挡视线。
    pub blocks_sight: bool,
    /// 是否阻挡移动。
    pub blocks_move: bool,
    /// 移动代价（千分比基准 100 = 一格常规代价）。负值按 0 处理——
    /// 这是数据层面的取舍而非矛盾（0 是一个有意义的答案），与
    /// `register-terrain` 当时逐字相同的处理。
    pub move_cost: i64,
    /// 「打开之后变成哪种地形」（门 → 敞开的门）。整条不写表示这块
    /// 地形打不开。此前脚本里用空串表达同一件事。
    #[serde(default)]
    pub opens_into: Option<String>,
}

/// 把一批地形写进注册表与地形表。
pub fn apply_terrains(
    registry: &mut Registry,
    table: &mut TerrainTable,
    terrains: &[RawTerrain],
) -> Applied {
    for terrain in terrains {
        let index = intern_id(registry, &terrain.id, "地形标识符")?;
        let opens_into = match terrain.opens_into.as_deref() {
            None => None,
            Some(raw) => Some(TerrainKind::from_index(intern_id(
                registry,
                raw,
                "opens_into 标识符",
            )?)),
        };
        table
            .define(
                index,
                TerrainAttrs {
                    blocks_sight: terrain.blocks_sight,
                    blocks_move: terrain.blocks_move,
                    move_cost: terrain.move_cost.max(0) as u32,
                    opens_into,
                },
            )
            .map_err(|err: TerrainError| err.to_string())?;
    }
    Ok(())
}

// ───────────────────────────── 资源 ─────────────────────────────

/// `resources.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceFile {
    /// 资源名册，按书写顺序注册。
    pub resources: Vec<RawResource>,
}

/// 一条资源种类声明，见 [`ll_world::resource`] 模块文档。
///
/// 与 [`RawTerrain`] 同一条纪律：数值与布尔字段全部必填，不带
/// `#[serde(default)]`——漏写一个 `exhaustible` 的症状是「这座矿业城市
/// 永远不会衰败」，而没有任何报错。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawResource {
    /// 完整命名空间标识符，例如 `lostland:iron_vein`。
    pub id: String,
    /// 展示名的 Fluent 本地化键。
    pub display_name_key: String,
    /// 这种资源属于哪个大类，五选一：`food` / `timber` / `metal` /
    /// `stone` / `water`（[`ll_world::resource::ResourceCategory`]）。
    ///
    /// **必填**，与本结构体其余字段同一条纪律：给一个默认值（比如
    /// 「不写就算食物」）的症状是「守着铜矿的据点长出了农夫」，而没有
    /// 任何报错。
    pub category: String,
    /// 这种资源长在哪种地形上（完整命名空间标识符，必须是一条已经
    /// 注册过的地形——`resources.json5` 排在 `terrain.json5` 之后装载
    /// 正是为了这一条，见 `crate::content_data` 的 `CONTENT_FILES`）。
    pub source_terrain: String,
    /// 源地形上每格出现一处资源点的概率，千分比（`1..=1000`）。
    pub abundance: i64,
    /// 每处资源点额外养活多少居民。
    pub residents_supported: i64,
    /// 每处资源点给拓荒概率加多少分。
    pub settlement_draw: i64,
    /// 这种资源会不会被采光。
    pub exhaustible: bool,
}

/// 把一批资源写进注册表与资源表。
pub fn apply_resources(
    registry: &mut Registry,
    table: &mut ResourceTable,
    resources: &[RawResource],
) -> Applied {
    for resource in resources {
        let index = intern_id(registry, &resource.id, "资源标识符")?;
        let display_name_key = parse_id(&resource.display_name_key, "资源展示名键")?;
        let category = ResourceCategory::parse(&resource.category).ok_or_else(|| {
            format!(
                "资源 {:?} 的大类 {:?} 不是 food / timber / metal / stone / water 之一",
                resource.id, resource.category
            )
        })?;
        // 源地形走 `intern` 而不是「只 get」：与 `RawTerrain::opens_into`
        // 完全同一种处理——注册表本身不区分「谁先提到这个 id」，真正的
        // 校验（这条地形有没有被 `terrain.json5` 声明过）由地形表的
        // `is_defined` 在消费侧回答，见 `ll_world::resource::resource_node_at`
        // 的地形比较。
        let source_terrain = TerrainKind::from_index(intern_id(
            registry,
            &resource.source_terrain,
            "资源源地形标识符",
        )?);
        table
            .define(
                index,
                ResourceAttrs {
                    display_name_key,
                    category,
                    source_terrain,
                    abundance: resource.abundance.clamp(0, i64::from(u32::MAX)) as u32,
                    residents_supported: resource.residents_supported.max(0) as u32,
                    settlement_draw: resource.settlement_draw.max(0) as u32,
                    exhaustible: resource.exhaustible,
                },
            )
            .map_err(|err: ResourceError| err.to_string())?;
    }
    Ok(())
}

// ─────────────────────────── 空间层属性 ───────────────────────────

/// `space_profiles.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceProfileFile {
    /// 空间层属性名册，按书写顺序注册。
    pub space_profiles: Vec<RawSpaceProfile>,
}

/// 一条空间层属性声明——对应此前 `register-space-profile` 的七个位置
/// 参数。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSpaceProfile {
    /// 完整命名空间标识符。
    pub id: String,
    /// 环境光下限（千分比，合法区间 0..=1000）。
    pub ambient_light_floor: i64,
    /// 是否直接暴露在天空之下（决定天气/昼夜是否作用于此处）。
    pub exposed_to_sky: bool,
    /// 基准温度（十分之一摄氏度）。
    pub base_temperature: i64,
    /// 能否挖掘。
    pub diggable: bool,
    /// 能否建造。
    pub buildable: bool,
    /// 混响标签（音频用）。整条不写表示无标签，此前脚本里用空串。
    #[serde(default)]
    pub reverb_tag: Option<String>,
}

/// 把一批空间层属性写进注册表与空间层属性表。
///
/// 两个数值字段的 `i64 → i32` 窄化**不钳位、直接拒绝**——与
/// `register-space-profile` 当时逐字相同的纪律：越界的
/// `ambient_light_floor` 本来就会被 `SpaceProfileTable::define` 的
/// `0..=1000` 校验拒绝，先钳成 `i32::MAX` 只会把错误消息变得更难懂。
pub fn apply_space_profiles(
    registry: &mut Registry,
    table: &mut SpaceProfileTable,
    profiles: &[RawSpaceProfile],
) -> Applied {
    for profile in profiles {
        let reverb_tag = match profile.reverb_tag.as_deref() {
            None => None,
            Some(raw) => Some(parse_id(raw, "reverb_tag 标识符")?),
        };
        let ambient_light_floor = i32::try_from(profile.ambient_light_floor).map_err(|_| {
            format!(
                "ambient_light_floor {} 超出 32 位整数范围，合法区间是 0..=1000",
                profile.ambient_light_floor
            )
        })?;
        let base_temperature = i32::try_from(profile.base_temperature).map_err(|_| {
            format!(
                "base_temperature {} 超出 32 位整数范围",
                profile.base_temperature
            )
        })?;
        let index = intern_id(registry, &profile.id, "空间层属性标识符")?;
        table
            .define(
                index,
                SpaceProfileAttrs {
                    ambient_light_floor,
                    exposed_to_sky: profile.exposed_to_sky,
                    base_temperature,
                    diggable: profile.diggable,
                    buildable: profile.buildable,
                    reverb_tag,
                },
            )
            .map_err(|err: SpaceProfileError| err.to_string())?;
    }
    Ok(())
}

// ───────────────────────────── 天气 ─────────────────────────────

/// `weather.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherFile {
    /// 天气名册，按书写顺序注册。
    pub weathers: Vec<RawWeather>,
}

/// 四季出现权重——具名字段而不是四元数组：`(list 2 8 6 0)` 那种写法
/// 整体错位一格之后每个数字仍然合法、不报任何错，症状是「这个 mod 的
/// 灰烬天气改在冬天下」。这正是本批次搬迁想消灭的那一类失败模式。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSeasonWeights {
    /// 春季权重。
    pub spring: i64,
    /// 夏季权重。
    pub summer: i64,
    /// 秋季权重。
    pub autumn: i64,
    /// 冬季权重。
    pub winter: i64,
}

/// 一条天气声明——对应此前 `register-weather` 的九个位置参数。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWeather {
    /// 完整命名空间标识符。
    pub id: String,
    /// 显示名的本地化键。
    pub display_name_key: String,
    /// 光照乘数（千分比，合法区间 0..=1000）。
    pub light_scale: i64,
    /// 视距乘数（千分比，合法区间 0..=1000）。
    pub sight_scale: i64,
    /// 温度偏移（十分之一摄氏度，合法区间 ±500）。
    pub temperature_offset: i64,
    /// 四季出现权重。
    pub season_weights: RawSeasonWeights,
}

/// 把一批天气写进注册表与天气表。
///
/// 三个数值字段的窄化**不钳位、直接拒绝**，理由同
/// [`apply_space_profiles`]；负权重更没有「最接近的合理值」可言——钳成
/// 0 会把「这里写错了」悄悄变成「这一季不出现」，是最糟的一种静默。
pub fn apply_weathers(
    registry: &mut Registry,
    table: &mut WeatherTable,
    weathers: &[RawWeather],
) -> Applied {
    for weather in weathers {
        let display_name_key = parse_id(&weather.display_name_key, "本地化键标识符")?;
        let light_scale = i32::try_from(weather.light_scale).map_err(|_| {
            format!(
                "light_scale {} 超出 32 位整数范围，合法区间是 0..=1000",
                weather.light_scale
            )
        })?;
        let sight_scale = i32::try_from(weather.sight_scale).map_err(|_| {
            format!(
                "sight_scale {} 超出 32 位整数范围，合法区间是 0..=1000",
                weather.sight_scale
            )
        })?;
        let temperature_offset = i32::try_from(weather.temperature_offset).map_err(|_| {
            format!(
                "temperature_offset {} 超出 32 位整数范围，合法区间是 ±500",
                weather.temperature_offset
            )
        })?;

        // 顺序固定为春/夏/秋/冬，与 `ll_world::weather::WeatherAttrs`
        // 的 `season_weights` 数组下标一一对应。
        let raw_weights = [
            ("spring", weather.season_weights.spring),
            ("summer", weather.season_weights.summer),
            ("autumn", weather.season_weights.autumn),
            ("winter", weather.season_weights.winter),
        ];
        let mut season_weights = [0u32; 4];
        for (slot, (name, raw)) in raw_weights.iter().enumerate() {
            season_weights[slot] = u32::try_from(*raw)
                .map_err(|_| format!("{name} 权重 {raw} 不是合法的非负 32 位权重"))?;
        }

        let index = intern_id(registry, &weather.id, "天气标识符")?;
        table
            .define(
                index,
                WeatherAttrs {
                    display_name_key,
                    light_scale,
                    sight_scale,
                    temperature_offset,
                    season_weights,
                },
            )
            .map_err(|err: WeatherError| err.to_string())?;
    }
    Ok(())
}

// ─────────────────────────── 动画剪辑 ───────────────────────────

/// `animations.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationFile {
    /// 动画剪辑名册，按书写顺序注册。
    pub clips: Vec<RawClip>,
}

/// 一条动画剪辑声明——对应此前的
/// `(register-animation-clip id frames frames-per-step looping exit-grace-frames)`。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClip {
    /// 完整命名空间标识符。
    pub id: String,
    /// 逐帧的精灵名（**不是**命名空间标识符，是资产 VFS 里的裸名）。
    pub frames: Vec<String>,
    /// 每一步停留多少帧。
    pub frames_per_step: u32,
    /// 是否循环播放。
    pub looping: bool,
    /// 退出前的宽限帧数。
    pub exit_grace_frames: u32,
}

/// 把一批动画剪辑写进注册表与剪辑表。
pub fn apply_clips(registry: &mut Registry, table: &mut ClipTable, clips: &[RawClip]) -> Applied {
    for clip in clips {
        let index = intern_id(registry, &clip.id, "动画剪辑标识符")?;
        table
            .define(
                index,
                Clip {
                    frames: clip.frames.clone(),
                    frames_per_step: clip.frames_per_step,
                    looping: clip.looping,
                    exit_grace_frames: clip.exit_grace_frames,
                },
            )
            .map_err(|err: ClipError| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 四季权重整体错位一格在具名字段下不可能发生() {
        // 这是本模块把 `(list 2 8 6 0)` 换成具名字段的理由：位置参数
        // 错位之后每个数字仍然合法，具名字段则会因为多/少一个键而
        // 当场报错。
        // Arrange：少写了 winter。
        let source = r#"{ weathers: [ { id: "m:ash", display_name_key: "m:ash.name",
            light_scale: 500, sight_scale: 400, temperature_offset: 0,
            season_weights: { spring: 2, summer: 8, autumn: 6 } } ] }"#;

        // Act
        let error = json5::from_str::<WeatherFile>(source).expect_err("缺字段必须报错");

        // Assert
        assert!(
            error.to_string().contains("winter"),
            "错误应当点名缺的那一季：{error}"
        );
    }

    #[test]
    fn 地形的opens_into缺席表示打不开() {
        // 此前脚本里用空串表达「打不开」，数据文件换成字段缺席——
        // 本测试钉住换法没有把语义弄丢。
        // Arrange
        let mut registry = Registry::new();
        let mut table = TerrainTable::new();
        let terrains = [RawTerrain {
            id: "m:lava".to_string(),
            blocks_sight: false,
            blocks_move: false,
            move_cost: 350,
            opens_into: None,
        }];

        // Act
        apply_terrains(&mut registry, &mut table, &terrains).expect("合法声明应当注册成功");

        // Assert
        let index = registry
            .get(&NamespacedId::parse("m:lava").expect("合法标识符"))
            .expect("刚注册的内容应能查到索引");
        assert_eq!(table.opens_into(TerrainKind::from_index(index)), None);
    }
}
