//! 世界（历史）生成配置屏：四档预设 + 五个形态旋钮。
//!
//! # 所有者裁定里的「历史生成的配置」
//!
//! > 「……然后设置历史生成的配置。」
//!
//! 数据侧早就就绪：`ll_world::terrain_shape::TerrainShape` 的五个字段
//! 与 `ll_content::world_identity::TERRAIN_PRESETS` 的四档预设，从世界
//! 生成参数落地批次起就已经接进新游戏流程并进存档
//! （`ll_platform::config::NewGameConfig`）。**缺的只是一块屏**，本模块
//! 就是那块屏背后的状态机与排版。
//!
//! # 非法值：判据只有一份，在 `TerrainShape::validate`
//!
//! 每次调整先算出**候选**形态，交给 `TerrainShape::validate()`；`Err`
//! 就**整体丢弃这次调整**，并把它返回的那句中文原因显示出来。
//!
//! **绝不在 UI 层抄第二份判据**——那是本仓库反复付过代价的形态（设置屏
//! 的键位判重复用 `KeyBindings::try_bind` 而不是重写一份，是同一条纪律
//! 的上一个实例）。抄一份的那一刻，「海平面到底能不能调到 900」就有了
//! 两个答案，而它们迟早会分叉。
//!
//! # 预设清单也是现查的
//!
//! `TERRAIN_PRESETS` 是一个 `const` 切片，本模块遍历它、读它声明的
//! `display_name_key`。**加一档预设，界面自动多一项**，本文件一个字都
//! 不用改。

use ll_content::mode::SaveMode;
use ll_content::world_identity::{DEFAULT_TERRAIN_PRESET_ID, TERRAIN_PRESETS, terrain_preset};
use ll_i18n::Catalog;
use ll_platform::input::{GameKey, InputState};
use ll_world::terrain_shape::TerrainShape;

use crate::chargen::{ChargenUpdate, cycle, horizontal, move_cursor};
use crate::menu_screen::{ScreenNotice, ScreenState};
use crate::nav_row::HorizontalRow;
use crate::settings_view::labeled_row;
use crate::spawn_pick::SpawnOrigin;

/// 一个旋钮每按一次左右键的步长。
///
/// 三个千分比旋钮（海平面/山地阈值/气候带宽）取 25：`TerrainShape` 的
/// 文档实测「海平面每上调 50，水域比例约上升 11 个百分点」，25 是一档
/// 「按一下看得出差别、又不至于一下跨过合法区间」的粒度。层数与缩减
/// 档位本身就是小整数，步长恒为 1。
const PERMILLE_STEP: i32 = 25;

/// 世界配置屏的行，顺序即导航顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldSetupRow {
    /// 四档地形预设，左右键循环；切换即整组覆写五个旋钮。
    Preset,
    /// 海平面（千分比）。
    SeaLevel,
    /// 山地阈值（千分比）。
    MountainLevel,
    /// 噪声倍频层数。
    Octaves,
    /// 大陆尺度缩减档位。
    ContinentShrink,
    /// 气候条带单侧带宽（千分比）。
    ClimateBandWidth,
    /// 存档模式：肉鸽（只有自动保存、死后模式转普通）还是普通（可手动
    /// 命名存档）。左右键在两者之间切换。
    ///
    /// # 为什么在这块屏上，不是单独一块
    ///
    /// **模式是世界的属性，不是角色的**（`crate::save_slot` 模块文档
    /// 「一份存档 = 一个世界」）：同一个世界里死了一个角色再建一个，
    /// 模式跟着世界走。它因此与地形形态旋钮属于同一个决定面，摆在同一
    /// 块屏上。
    ///
    /// # 这块屏上它是**唯一**一个可以往肉鸽方向改的地方
    ///
    /// 世界还没建出来，所以这里改它不是「把一个已有世界改回肉鸽」——
    /// 那件事在类型层面写不出来（`ll_content::mode::SaveMode` 模块
    /// 文档）。
    Mode,
    /// 按这一行生成世界，随后进选出生地屏。
    Generate,
    /// 回到角色创建屏。
    Back,
}

impl crate::nav_row::HorizontalRow for WorldSetupRow {
    /// 六个旋钮加存档模式有取值，「生成」「返回」没有——后两行的左右键
    /// 因此等同上下键（规格 N12）。
    fn horizontal_role(self) -> crate::nav_row::HorizontalRole {
        match self {
            WorldSetupRow::Preset
            | WorldSetupRow::SeaLevel
            | WorldSetupRow::MountainLevel
            | WorldSetupRow::Octaves
            | WorldSetupRow::ContinentShrink
            | WorldSetupRow::ClimateBandWidth
            | WorldSetupRow::Mode => crate::nav_row::HorizontalRole::AdjustsValue,
            WorldSetupRow::Generate | WorldSetupRow::Back => {
                crate::nav_row::HorizontalRole::MovesFocus
            }
        }
    }
}

impl crate::nav_row::NavRow for WorldSetupRow {
    /// 「返回」是**返回**——退回角色创建屏那一层。「生成世界」不是导航
    /// 角色：它往流程的下一层走。见 `crate::nav_row` 模块文档。
    fn nav_role(self) -> Option<crate::nav_row::NavRole> {
        match self {
            WorldSetupRow::Back => Some(crate::nav_row::NavRole::Back),
            WorldSetupRow::Preset
            | WorldSetupRow::SeaLevel
            | WorldSetupRow::MountainLevel
            | WorldSetupRow::Octaves
            | WorldSetupRow::ContinentShrink
            | WorldSetupRow::ClimateBandWidth
            | WorldSetupRow::Mode
            | WorldSetupRow::Generate => None,
        }
    }
}

/// 世界配置屏这一帧的全部行，顺序固定。
pub fn world_setup_rows() -> [WorldSetupRow; 9] {
    [
        WorldSetupRow::Mode,
        WorldSetupRow::Preset,
        WorldSetupRow::SeaLevel,
        WorldSetupRow::MountainLevel,
        WorldSetupRow::Octaves,
        WorldSetupRow::ContinentShrink,
        WorldSetupRow::ClimateBandWidth,
        WorldSetupRow::Generate,
        WorldSetupRow::Back,
    ]
}

/// 一个稳定标识在 [`TERRAIN_PRESETS`] 里是第几档；不认识的标识退回默认
/// 那一档的下标。
///
/// 退回而不是报错，与 `ll_game::worldgen::resolve_gen_params` 对配置
/// 文件里那个字段的处理同一条纪律：玩家手写的配置是不可信输入，写错
/// 一个字不该让游戏起不来。
pub fn preset_index_of(id: &str) -> usize {
    TERRAIN_PRESETS
        .iter()
        .position(|preset| preset.id == id)
        .unwrap_or_else(|| {
            TERRAIN_PRESETS
                .iter()
                .position(|preset| preset.id == DEFAULT_TERRAIN_PRESET_ID)
                .unwrap_or(0)
        })
}

/// 把一次调整应用到形态参数上——**合法才生效**。
///
/// 返回 `Err` 时 `shape` 一个字节都没动，`Err` 里是
/// [`TerrainShape::validate`] 给出的那句中文原因。
///
/// 这个函数存在的全部意义是把「先造候选、再校验、再决定要不要落盘」
/// 这三步收在一处：散在六个 `match` 臂里，迟早有一臂忘了校验。
pub fn apply_adjust(
    shape: &mut TerrainShape,
    adjust: impl FnOnce(&mut TerrainShape),
) -> Result<(), String> {
    let mut candidate = *shape;
    adjust(&mut candidate);
    candidate.validate()?;
    *shape = candidate;
    Ok(())
}

/// 处理世界配置屏这一帧的输入。
///
/// `shape`/`preset` 就地改写；返回值只表达「要不要换一块屏、要不要说
/// 一句话」。
pub fn update_world_setup(
    cursor: &mut usize,
    shape: &mut TerrainShape,
    preset: &mut usize,
    mode: &mut SaveMode,
    input: &InputState,
    pointer: crate::pointer::RowPointer,
) -> ChargenUpdate {
    let rows = world_setup_rows();
    *cursor = move_cursor(*cursor, rows.len(), input);
    if let Some(row) = pointer.focus_row() {
        *cursor = row.min(rows.len() - 1);
    }
    let row = rows[(*cursor).min(rows.len() - 1)];

    // 规格 N12：没有横向维度的行（「生成」「返回」）上，左右键等同上下键。
    // 分派走 `HorizontalRow::horizontal_role`，那条声明因此是载重的——
    // 把 `SeaLevel` 标成 `MovesFocus` 的那一刻，左右键就不再调海平面了。
    if let Some(forward) = horizontal(input)
        && row.horizontal_role() == crate::nav_row::HorizontalRole::MovesFocus
    {
        *cursor = crate::nav_row::stepped_cursor(*cursor, forward, rows.len());
        return ChargenUpdate::idle();
    }
    if row == WorldSetupRow::Mode && horizontal(input).is_some() {
        // 两档之间切换，方向无关（只有两个值，左右都是「换到另一个」）。
        *mode = match *mode {
            SaveMode::Permadeath => SaveMode::fresh_free_save(),
            SaveMode::FreeSave { .. } => SaveMode::Permadeath,
        };
        return ChargenUpdate::idle();
    }
    if let Some(forward) = horizontal(input)
        && let Err(reason) = adjust_row(row, shape, preset, forward)
    {
        // 拒绝时形态参数逐字段回到调整前，只把原因说出来。
        //
        // 原因文本本身**没有**进 `ScreenNotice`——那个枚举是 `Copy` 的、
        // 只携带 i18n 键，而 `validate` 返回的中文串是给日志用的诊断
        // 信息（同 `ll_core::error::CoreError::Display` 那条既有先例：
        // 面向开发者与日志，不面向玩家）。玩家看到的是一句走 i18n 的
        // 「这个取值不合法」。
        tracing::info!(%reason, "世界配置：这次调整被形态参数校验拒绝，参数未改动");
        return ChargenUpdate {
            next: None,
            notice: Some(ScreenNotice::InvalidTerrainShape),
        };
    }

    if input.was_just_pressed(GameKey::Cancel) {
        return ChargenUpdate::going(ScreenState::CharacterCreation { cursor: 0 });
    }
    if !input.was_just_pressed(GameKey::Confirm) && !pointer.activated() {
        return ChargenUpdate::idle();
    }
    match row {
        // 这块屏是选点屏三个入口里的一个，按取消要回得到这里来，
        // 见 `crate::spawn_pick::SpawnOrigin`。
        WorldSetupRow::Generate => ChargenUpdate::going(ScreenState::SpawnPick {
            origin: SpawnOrigin::WorldSetup,
        }),
        WorldSetupRow::Back => ChargenUpdate::going(ScreenState::CharacterCreation { cursor: 0 }),
        _ => ChargenUpdate::idle(),
    }
}

/// 把一次左右键调整落到具体某一行上。
fn adjust_row(
    row: WorldSetupRow,
    shape: &mut TerrainShape,
    preset: &mut usize,
    forward: bool,
) -> Result<(), String> {
    match row {
        WorldSetupRow::Preset => {
            *preset = cycle(*preset, TERRAIN_PRESETS.len(), forward);
            // 预设整组覆写五个旋钮。预设本身恒合法（`TERRAIN_PRESETS`
            // 是内容侧钉死的常量，有测试守着），但仍然走同一条
            // `apply_adjust` 校验路径——不给「常量当然合法」这种推理
            // 留一条绕过校验的旁路。
            let shape_of_preset = TERRAIN_PRESETS[*preset].shape;
            apply_adjust(shape, |candidate| *candidate = shape_of_preset)
        }
        WorldSetupRow::SeaLevel => apply_adjust(shape, |candidate| {
            candidate.sea_level += step(PERMILLE_STEP, forward);
        }),
        WorldSetupRow::MountainLevel => apply_adjust(shape, |candidate| {
            candidate.mountain_level += step(PERMILLE_STEP, forward);
        }),
        WorldSetupRow::Octaves => apply_adjust(shape, |candidate| {
            candidate.octaves = candidate.octaves.saturating_add_signed(step(1, forward));
        }),
        WorldSetupRow::ContinentShrink => apply_adjust(shape, |candidate| {
            candidate.continent_shrink = candidate
                .continent_shrink
                .saturating_add_signed(step(1, forward));
        }),
        WorldSetupRow::ClimateBandWidth => apply_adjust(shape, |candidate| {
            candidate.climate_band_width += step(PERMILLE_STEP, forward);
        }),
        // 两行按钮没有取值可调。
        // 模式那一行在 `update_world_setup` 里就地处理（它不是形态参数，
        // 不走 `TerrainShape::validate`），到不了这里。
        WorldSetupRow::Mode | WorldSetupRow::Generate | WorldSetupRow::Back => Ok(()),
    }
}

/// 按方向取步长的正负。
fn step(magnitude: i32, forward: bool) -> i32 {
    if forward { magnitude } else { -magnitude }
}

/// 把世界配置屏这一帧的每一行排好版。
pub fn world_setup_row_texts(
    shape: &TerrainShape,
    preset: usize,
    mode: SaveMode,
    catalog: &Catalog,
    language: &str,
) -> Vec<String> {
    world_setup_rows()
        .into_iter()
        .map(|row| match row {
            WorldSetupRow::Mode => labeled_row(
                catalog,
                language,
                "screen-worldsetup-mode",
                // 模式的展示名走 `crate::save_list::mode_key`——存档列表
                // 与这里说的必须是同一个词，两处各写一份迟早会分叉。
                &catalog.resolve(language, crate::save_list::mode_key(mode)),
            ),
            WorldSetupRow::Preset => labeled_row(
                catalog,
                language,
                "screen-worldsetup-preset",
                &preset_display_name(preset, catalog, language),
            ),
            WorldSetupRow::SeaLevel => number_row(
                catalog,
                language,
                "screen-worldsetup-sea-level",
                shape.sea_level as i64,
            ),
            WorldSetupRow::MountainLevel => number_row(
                catalog,
                language,
                "screen-worldsetup-mountain-level",
                shape.mountain_level as i64,
            ),
            WorldSetupRow::Octaves => number_row(
                catalog,
                language,
                "screen-worldsetup-octaves",
                shape.octaves as i64,
            ),
            WorldSetupRow::ContinentShrink => number_row(
                catalog,
                language,
                "screen-worldsetup-continent-shrink",
                shape.continent_shrink as i64,
            ),
            WorldSetupRow::ClimateBandWidth => number_row(
                catalog,
                language,
                "screen-worldsetup-climate-band-width",
                shape.climate_band_width as i64,
            ),
            WorldSetupRow::Generate => catalog.resolve(language, "screen-worldsetup-generate"),
            WorldSetupRow::Back => catalog.resolve(language, "screen-worldsetup-back"),
        })
        .collect()
}

/// 一行数值：分隔符走同一个 i18n 模板，见
/// `crate::settings_view::labeled_row`。
fn number_row(catalog: &Catalog, language: &str, label_key: &str, value: i64) -> String {
    labeled_row(catalog, language, label_key, &value.to_string())
}

/// 当前选中预设的展示名——**读 `TerrainPreset` 声明的
/// `display_name_key`**，不按约定拼键。
fn preset_display_name(preset: usize, catalog: &Catalog, language: &str) -> String {
    match TERRAIN_PRESETS.get(preset) {
        Some(preset) => catalog.resolve(language, preset.display_name_key),
        // 下标越界在生产路径上不该发生（`cycle` 保证它落在范围内），
        // 但界面不 panic：退回默认那一档的名字，与 `terrain_preset`
        // 对不认识的标识同一条降级纪律。
        None => match terrain_preset(DEFAULT_TERRAIN_PRESET_ID) {
            Some(preset) => catalog.resolve(language, preset.display_name_key),
            None => catalog.resolve(language, "screen-chargen-empty"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 非法调整被拒绝且形态参数逐字段回到调整前() {
        // 判据只有一份，在 TerrainShape::validate。这里验的是「拒绝时
        // 真的什么都没改」，不是重抄一遍那份判据。
        // Arrange：山地阈值必须比海平面高出至少 MIN_LEVEL_GAP。
        let mut shape = TerrainShape::default();
        shape.sea_level = shape.mountain_level - TerrainShape::MIN_LEVEL_GAP;
        let before = shape;

        // Act：再往上调一档海平面就会跌破那条约束。
        let result = apply_adjust(&mut shape, |candidate| {
            candidate.sea_level += PERMILLE_STEP;
        });

        // Assert
        assert!(result.is_err(), "跌破最小阈值差的调整必须被拒绝");
        assert_eq!(shape, before, "被拒绝的调整不该改动任何字段");
    }

    #[test]
    fn 合法调整真的落到参数上() {
        // 反例：证明上一条不是「无论如何都拒绝」。
        let mut shape = TerrainShape::default();
        let before = shape.sea_level;

        let result = apply_adjust(&mut shape, |candidate| {
            candidate.sea_level -= PERMILLE_STEP;
        });

        assert!(result.is_ok());
        assert_eq!(shape.sea_level, before - PERMILLE_STEP);
    }

    #[test]
    fn 预设清单是现查的加一档界面自动多一项() {
        // 「行数 = TERRAIN_PRESETS.len()」这条关系不写死成字面量 4：
        // 那样加一档预设时这条会红、变成噪音。断言的是**关系**。
        let mut index = 0usize;
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..TERRAIN_PRESETS.len() {
            seen.insert(TERRAIN_PRESETS[index].id);
            index = cycle(index, TERRAIN_PRESETS.len(), true);
        }
        assert_eq!(
            seen.len(),
            TERRAIN_PRESETS.len(),
            "循环一圈应当把全部预设走过一遍" // i18n-exempt：测试断言的失败消息，只在测试失败时打给开发者看
        );
        assert_eq!(index, 0, "走完一圈应当回到起点");
    }

    #[test]
    fn 切换预设会整组覆写五个旋钮() {
        // Arrange：从默认（大陆）切到下一档。
        let mut shape = TerrainShape::default();
        let mut preset = preset_index_of(DEFAULT_TERRAIN_PRESET_ID);

        // Act
        adjust_row(WorldSetupRow::Preset, &mut shape, &mut preset, true).expect("预设本身恒合法");

        // Assert
        assert_eq!(shape, TERRAIN_PRESETS[preset].shape);
        assert_ne!(
            shape,
            TerrainShape::default(),
            "四档预设互不相同，切一档应当真的换掉参数" // i18n-exempt：测试断言的失败消息，只在测试失败时打给开发者看
        );
    }

    #[test]
    fn 不认识的预设标识退回默认那一档() {
        assert_eq!(
            preset_index_of("nonexistent"),
            preset_index_of(DEFAULT_TERRAIN_PRESET_ID)
        );
    }

    #[test]
    fn 倍频层数下调不会在零处回绕成巨大的数() {
        // `octaves` 是 u32，减到 0 以下若用裸减法会回绕成 4294967295，
        // 而 validate 只看区间、拦得住它——但拦住之后玩家会看到一次
        // 莫名其妙的拒绝。用 saturating_add_signed 在源头挡掉。
        let mut shape = TerrainShape {
            octaves: *TerrainShape::OCTAVES_RANGE.start(),
            ..TerrainShape::default()
        };
        let mut preset = 0;
        // 已经在下界，再往下调会被 validate 拒绝（区间外），形态不变。
        let result = adjust_row(WorldSetupRow::Octaves, &mut shape, &mut preset, false);
        assert!(result.is_err());
        assert_eq!(shape.octaves, *TerrainShape::OCTAVES_RANGE.start());
    }
}
