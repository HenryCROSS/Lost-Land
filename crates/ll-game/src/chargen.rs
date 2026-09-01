//! 角色创建：种族 / 性别 / 职业。
//!
//! # 所有者裁定
//!
//! > 「开始游戏的时候需要玩家设置种族，性别，职业。然后设置历史生成的
//! > 配置。接着就是选择地图上在哪重生。」
//!
//! 本模块是这三步里的第一步；第二步在 [`crate::world_setup`]，第三步在
//! [`crate::spawn_pick`]。
//!
//! # 世界比角色活得长——这不是开局流程的一段
//!
//! 所有者还裁定了肉鸽模式的死亡处理：
//!
//! > 「死亡后变成一般模式，可以再创建角色然后选择在某个地方出生。」
//!
//! 也就是说角色创建会被走**两次**：一次在开局（世界还不存在，要先按
//! 玩家选的参数生成），一次在死亡之后（**世界早就存在**，只需要造一个
//! 新角色再选个地方放进去）。
//!
//! 本批**不接死亡重生那条线**（属存档批次），但形状必须留对。接缝有
//! 且只有两个，见 [`crate::draft_world::DraftWorld`] 与
//! `crate::world::build_player_agent` 的文档。
//!
//! # 三项清单全部**从注册表现查**
//!
//! [`ChargenRoster`] 遍历 `Registry::snapshot()`（一个 `Vec`，不经任何
//! 哈希容器——约束 C5）再按各自的内容表 `is_defined` 过滤，与
//! `crates/ll-game/tests/npc_appearance.rs` 里那对同名帮手是同一套做法。
//! **加一个种族，界面那一刻就多一项，本文件一个字都不用改。**
//!
//! 性别那一项同理：清单是 [`Gender::ALL`]，不在这里手抄一张平行表。
//!
//! 展示名走各自内容表声明的 `display_name_key`（**读字段，不按约定拼
//! 键**——旧做法与它的代价见 `ll_mod::damage_category` 模块文档「显示名
//! 字段」一节）。

use ll_i18n::Catalog;
use ll_platform::input::{GameKey, InputState};
use ll_world::entity::Gender;
use ll_world::exploration::ExplorationMemory;
use ll_world::terrain_shape::TerrainShape;

use ll_core::ident::ContentIndex;

use crate::content::LoadedContent;
use crate::menu_screen::{ScreenNotice, ScreenState};
use crate::settings_view::labeled_row;

/// 界面上可选的种族与职业清单——**从注册表现查出来的快照**。
///
/// 做成一份快照而不是每帧现查：一次 `Registry::snapshot()` 要克隆全部
/// 已注册 ID 的 `Vec`，而角色创建屏开着的每一帧都要排版一次。清单在
/// 一局角色创建期间不可能变（内容在装载期就冻结了），快照因此没有
/// 失效风险。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargenRoster {
    races: Vec<ContentIndex>,
    professions: Vec<ContentIndex>,
}

impl ChargenRoster {
    /// 从已装载的内容里现查一份清单。
    pub fn from_content(content: &LoadedContent) -> Self {
        ChargenRoster {
            races: registered(content, |index| content.race_table.is_defined(index)),
            professions: registered(content, |index| content.class_table.is_defined(index)),
        }
    }

    /// 可选的种族，按注册顺序。
    pub fn races(&self) -> &[ContentIndex] {
        &self.races
    }

    /// 可选的职业，按注册顺序。
    pub fn professions(&self) -> &[ContentIndex] {
        &self.professions
    }
}

/// 注册表里全部**已定义属性**的内容，按注册顺序。
///
/// `Registry::snapshot` 返回 `Vec`，不经任何哈希容器（约束 C5）——顺序
/// 因此是注册顺序，同一份 mod 集合下恒定。
fn registered(
    content: &LoadedContent,
    is_defined: impl Fn(ContentIndex) -> bool,
) -> Vec<ContentIndex> {
    content
        .registry
        .snapshot()
        .into_iter()
        .filter_map(|id| content.registry.get(&id))
        .filter(|index| is_defined(*index))
        .collect()
}

/// 玩家在角色创建界面上做的三个选择。
///
/// 存**下标**而不是 [`ContentIndex`]：下标指向 [`ChargenRoster`] 里那
/// 份现查出来的清单，因此「加一个种族，界面自动多一项」这条性质不需要
/// 本类型配合。真正要用时再经 [`Self::race`]/[`Self::profession`] 换回
/// 索引。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CharacterChoice {
    race: usize,
    gender: usize,
    profession: usize,
}

impl CharacterChoice {
    /// 选中的种族；清单为空（内容里一个种族都没有）时返回 `None`。
    pub fn race(&self, roster: &ChargenRoster) -> Option<ContentIndex> {
        roster.races().get(self.race).copied()
    }

    /// 选中的职业；清单为空时返回 `None`。
    pub fn profession(&self, roster: &ChargenRoster) -> Option<ContentIndex> {
        roster.professions().get(self.profession).copied()
    }

    /// 选中的性别。[`Gender::ALL`] 恒非空，因此这一项不会是 `None`。
    pub fn gender(&self) -> Gender {
        Gender::ALL
            .get(self.gender)
            .copied()
            .unwrap_or_else(Gender::default)
    }
}

/// 角色创建屏的行，顺序即导航顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterRow {
    /// 种族，左右键在注册表清单里循环。
    Race,
    /// 性别，左右键在 [`Gender::ALL`] 里循环。
    Gender,
    /// 职业，左右键在注册表清单里循环。
    Profession,
    /// 进入下一步（世界配置）。
    Next,
    /// 回到首页。
    Back,
}

impl crate::nav_row::NavRow for CharacterRow {
    /// 「返回」是**返回**——退回首页那一层。「下一步」不是导航角色：
    /// 它往流程的**下一**层走，不是往回。见 `crate::nav_row` 模块文档。
    fn nav_role(self) -> Option<crate::nav_row::NavRole> {
        match self {
            CharacterRow::Back => Some(crate::nav_row::NavRole::Back),
            CharacterRow::Race
            | CharacterRow::Gender
            | CharacterRow::Profession
            | CharacterRow::Next => None,
        }
    }
}

/// 角色创建屏这一帧的全部行，顺序固定。
///
/// 与 `crate::menu_screen::settings_rows` 同一条纪律：**行列表每帧现算，
/// 光标是一个 `usize`**，不手抄一张与枚举平行的静态 `WidgetId` 表。
pub fn character_rows() -> [CharacterRow; 5] {
    [
        CharacterRow::Race,
        CharacterRow::Gender,
        CharacterRow::Profession,
        CharacterRow::Next,
        CharacterRow::Back,
    ]
}

/// 处理完角色创建屏这一帧输入之后，调用方该做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargenUpdate {
    /// 要切到哪一块屏，`None` 表示留在原地。
    pub next: Option<ScreenState>,
    /// 这一帧要说的一句话。
    pub notice: Option<ScreenNotice>,
}

impl ChargenUpdate {
    /// 什么都没发生。
    pub fn idle() -> ChargenUpdate {
        ChargenUpdate {
            next: None,
            notice: None,
        }
    }

    /// 切到另一块屏。
    pub fn going(next: ScreenState) -> ChargenUpdate {
        ChargenUpdate {
            next: Some(next),
            notice: None,
        }
    }
}

/// 在 `0..len` 里循环移动一格；`len` 为零时原地不动（不做除零运算）。
///
/// 循环而不是到头就停：三项选择都是**环形**的一组同类选项（种族之间
/// 没有先后），到头就停会让玩家以为按坏了。这与
/// `ll_world::world_map::WorldMapView::zoom_in`「到头就停、不回绕」不
/// 矛盾——那里的档位是有序的尺度，这里的选项是无序的集合。
pub fn cycle(current: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let current = current.min(len - 1);
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

/// 上下移动光标，到头就停（与 `ll_ui::widget::focus::move_focus` 的
/// 语义一致：列表是有序的，到头回绕会让玩家找不到自己在第几行）。
pub fn move_cursor(cursor: usize, len: usize, input: &InputState) -> usize {
    if len == 0 {
        return 0;
    }
    let mut next = cursor.min(len - 1);
    if input.was_just_pressed(GameKey::Down) {
        next = (next + 1).min(len - 1);
    }
    if input.was_just_pressed(GameKey::Up) {
        next = next.saturating_sub(1);
    }
    next
}

/// 这一帧玩家有没有按左右键，按的是哪一边。
pub fn horizontal(input: &InputState) -> Option<bool> {
    if input.was_just_pressed(GameKey::Right) {
        Some(true)
    } else if input.was_just_pressed(GameKey::Left) {
        Some(false)
    } else {
        None
    }
}

/// 处理角色创建屏这一帧的输入。
pub fn update_character_creation(
    cursor: &mut usize,
    choice: &mut CharacterChoice,
    roster: &ChargenRoster,
    input: &InputState,
    pointer: crate::pointer::RowPointer,
    next_screen: ScreenState,
) -> ChargenUpdate {
    let rows = character_rows();
    *cursor = move_cursor(*cursor, rows.len(), input);
    if let Some(row) = pointer.focus_row() {
        *cursor = row.min(rows.len() - 1);
    }
    let row = rows[(*cursor).min(rows.len() - 1)];

    if let Some(forward) = horizontal(input) {
        match row {
            CharacterRow::Race => {
                choice.race = cycle(choice.race, roster.races().len(), forward);
            }
            CharacterRow::Gender => {
                choice.gender = cycle(choice.gender, Gender::ALL.len(), forward);
            }
            CharacterRow::Profession => {
                choice.profession = cycle(choice.profession, roster.professions().len(), forward);
            }
            // 「下一步」「返回」两行没有取值可调，左右键什么都不做。
            CharacterRow::Next | CharacterRow::Back => {}
        }
    }

    // 取消键 = 「返回」那一行：两条路通向同一处，不让 Esc 在这块屏上
    // 什么都不做（那会让玩家以为卡住了），也不让它直接退出游戏
    // （上一批刚从游戏内改掉的行为）。
    if input.was_just_pressed(GameKey::Cancel) {
        return ChargenUpdate::going(ScreenState::Title);
    }
    if !input.was_just_pressed(GameKey::Confirm) && !pointer.activated() {
        return ChargenUpdate::idle();
    }
    match row {
        // **「下一步」去哪儿，取决于世界存不存在**——这正是
        // [`crate::draft_world::DraftWorld`] 那条接缝的落点，也是批次 8
        // 计划文档第七节写的「状态机因此要按 `world.is_some()` 决定下一步
        // 去哪块屏，而不是写死一条固定的三屏顺序」。
        //
        // 死亡重生那条路上世界早就存在，再进一次世界配置屏等于让玩家
        // 重新生成一个世界——这局玩过的一切当场被抹掉，而他只是想换个
        // 角色。
        //
        // **判据不在本函数里**：它是草稿手里那个世界的属性，由
        // [`crate::draft_world::DraftWorld::screen_after_character_creation`]
        // 算好再传进来。此前这里读的是一个裸布尔
        // （`NewGameDraft::world_already_exists`），而那个布尔与「存哪个
        // 槽位」是两个能各自漂移的字段——D1 就是它们漂移出来的。
        CharacterRow::Next => ChargenUpdate::going(next_screen),
        CharacterRow::Back => ChargenUpdate::going(ScreenState::Title),
        // 在取值行上按确认什么都不做——改取值用左右键，与设置屏的
        // 语言/垂直同步两行同一套手感。
        _ => ChargenUpdate::idle(),
    }
}

/// 把角色创建屏这一帧的每一行排好版。
pub fn character_row_texts(
    choice: &CharacterChoice,
    roster: &ChargenRoster,
    content: &LoadedContent,
    catalog: &Catalog,
    language: &str,
) -> Vec<String> {
    character_rows()
        .into_iter()
        .map(|row| match row {
            CharacterRow::Race => labeled_row(
                catalog,
                language,
                "screen-chargen-race",
                &race_display_name(choice, roster, content, catalog, language),
            ),
            CharacterRow::Gender => labeled_row(
                catalog,
                language,
                "screen-chargen-gender",
                &catalog.resolve(language, choice.gender().display_name_key()),
            ),
            CharacterRow::Profession => labeled_row(
                catalog,
                language,
                "screen-chargen-profession",
                &profession_display_name(choice, roster, content, catalog, language),
            ),
            CharacterRow::Next => catalog.resolve(language, "screen-chargen-next"),
            CharacterRow::Back => catalog.resolve(language, "screen-chargen-back"),
        })
        .collect()
}

/// 当前选中种族的展示名；清单为空时显示「（没有条目）」。
///
/// **读内容表声明的 `display_name_key`，不按约定拼键**。
fn race_display_name(
    choice: &CharacterChoice,
    roster: &ChargenRoster,
    content: &LoadedContent,
    catalog: &Catalog,
    language: &str,
) -> String {
    match choice
        .race(roster)
        .and_then(|index| content.race_table.get(index))
    {
        Some(view) => catalog.resolve(language, &view.display_name_key.to_string()),
        None => catalog.resolve(language, "screen-chargen-empty"),
    }
}

/// 当前选中职业的展示名，理由同 [`race_display_name`]。
fn profession_display_name(
    choice: &CharacterChoice,
    roster: &ChargenRoster,
    content: &LoadedContent,
    catalog: &Catalog,
    language: &str,
) -> String {
    match choice
        .profession(roster)
        .and_then(|index| content.class_table.get(index))
    {
        Some(view) => catalog.resolve(language, &view.display_name_key.to_string()),
        None => catalog.resolve(language, "screen-chargen-empty"),
    }
}

/// 一局**尚未开始**的游戏：一个还没进世界的角色，加上他要进入的那个
/// 世界的配置。
///
/// # 接缝：这个类型就是「死亡之后重新入世」的入口
///
/// 见 [`Self::world`] 与 [`crate::draft_world::DraftWorld`]。
pub struct NewGameDraft {
    /// 可选的种族与职业清单，进入角色创建屏那一刻现查一次。
    pub roster: ChargenRoster,
    /// 玩家的三项选择。
    pub choice: CharacterChoice,
    /// 世界形态旋钮，见 [`crate::world_setup`]。
    pub shape: TerrainShape,
    /// 当前选中的地形预设，是 `ll_content::world_identity::TERRAIN_PRESETS`
    /// 的下标。
    pub preset: usize,
    /// 世界种子。
    pub seed: u64,
    /// 选出生地屏用的「全图已探索」记忆——**只活在这里，绝不写进
    /// `WorldState`**，见 `ll_world::exploration::ExplorationMemory::fully_explored`
    /// 文档。
    ///
    /// 进选点屏那一刻才建（默认世界六千多个区块，白建一次是白付的
    /// 开销），因此是 `Option`。
    pub exploration: Option<ExplorationMemory>,
    /// 选点光标落在地图的哪一格（列, 行）。
    pub cursor_cell: (u32, u32),
    /// 这一局的世界**从哪来**、将来**存进哪个槽位**——两件事绑在一个
    /// 类型里，见 [`crate::draft_world::DraftWorld`]。
    ///
    /// 它就是最终会交给 `crate::session::Session::begin` 的那一局；
    /// 选出生地屏期间它已经完全建好，只是玩家还没决定在哪落脚。
    ///
    /// # 它此前是三个字段
    ///
    /// `world: Option<GameWorld>` + `world_already_exists: bool` +
    /// `existing_target: Option<SaveTarget>`。三者必然同真同假却互不相干，
    /// 而「新生成的世界 + 老槽位」这个非法组合就是 D1 造成数据丢失的那
    /// 一步。合成一个类型之后它**表示不出来**。
    pub world: crate::draft_world::DraftWorld,
    /// 世界地图用的粗粒度地形场，与 [`Self::world`] 同生同死。
    pub continent_field: Option<ll_world::overview::ContinentField>,
    /// 选点屏的地图视野，与 [`Self::world`] 同生同死。
    pub map_view: Option<ll_world::world_map::WorldMapView>,
    /// 玩家正在给这份存档打的名字，见 [`crate::save_name`]。
    ///
    /// 它住在草稿上而不是 `ScreenState::SaveNaming` 里：`ScreenState`
    /// 是 `Copy` 的（装不下 `String`），而且玩家从命名屏退回选点屏再
    /// 回来时，那串字不该丢。
    pub save_name: crate::save_name::NameField,
    /// 玩家在选出生地屏上确认的那一格——命名屏在它之后，真正把玩家挪
    /// 过去要等命名结束，因此得先记下来。
    pub spawn: Option<ll_core::torus::TorusPos>,
    /// 这一局的存档模式（肉鸽 / 普通），在世界配置屏上选。
    ///
    /// 死亡重生那条路它取自那个世界身份里已经有的那一份——模式跟着世界
    /// 走，不跟着角色走。
    pub mode: ll_content::mode::SaveMode,
}

impl NewGameDraft {
    /// 按当前配置建一份草稿：三项选择取各清单的第一项，世界形态取配置
    /// 文件里那一档。
    pub fn new(content: &LoadedContent, config: &ll_platform::config::NewGameConfig) -> Self {
        let params = crate::worldgen::resolve_gen_params(config);
        NewGameDraft {
            roster: ChargenRoster::from_content(content),
            choice: CharacterChoice::default(),
            shape: params.shape,
            preset: crate::world_setup::preset_index_of(&config.terrain_preset),
            seed: params.seed,
            exploration: None,
            cursor_cell: (0, 0),
            world: crate::draft_world::DraftWorld::fresh(),
            continent_field: None,
            map_view: None,
            save_name: crate::save_name::NameField::new(),
            spawn: None,
            // 默认普通档：肉鸽是玩家必须主动选择的约束，见
            // `crate::world::build_new_world` 文档。
            mode: ll_content::mode::SaveMode::fresh_free_save(),
        }
    }

    /// 为「死亡之后重新入世」建一份草稿：世界、槽位、模式全部沿用现有
    /// 的那一局，只有角色是新的。
    ///
    /// # 这是批次 8 第七节留的那条接缝真正被用上的地方
    ///
    /// 草稿手里那个世界是 [`crate::draft_world::DraftWorld::Reborn`] ⇒
    /// 状态机跳过世界配置屏（重新生成等于把这局玩过的一切抹掉），角色
    /// 创建之后直接去选出生地；而「重新生成」这件事在那个变体上**根本
    /// 写不出来**，见该模块文档。
    pub fn for_reincarnation(
        content: &LoadedContent,
        world: crate::world::GameWorld,
        target: crate::save_slot::SaveTarget,
    ) -> Self {
        let shape = world.identity.terrain_shape();
        let seed = world.identity.seed();
        let mode = world.identity.mode();
        NewGameDraft {
            roster: ChargenRoster::from_content(content),
            choice: CharacterChoice::default(),
            shape,
            preset: 0,
            seed,
            exploration: None,
            cursor_cell: (0, 0),
            world: crate::draft_world::DraftWorld::reborn(world, target),
            continent_field: None,
            map_view: None,
            save_name: crate::save_name::NameField::new(),
            spawn: None,
            mode,
        }
    }

    /// 这份草稿对应的世界生成参数。
    pub fn gen_params(&self) -> ll_world::generate::GenParams {
        ll_world::generate::GenParams {
            seed: self.seed,
            shape: self.shape,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 左右键在清单里循环而不是到头就停() {
        // 三项选择都是无序的一组同类选项，到头就停会让玩家以为按坏了。
        assert_eq!(cycle(0, 3, false), 2, "在第一项按左键应当绕到最后一项");
        assert_eq!(cycle(2, 3, true), 0, "在最后一项按右键应当绕回第一项");
        assert_eq!(cycle(0, 3, true), 1);
    }

    #[test]
    fn 空清单时循环不做除零运算也不越界() {
        // 内容里一个种族都没有是一种可能的（虽然不健康的）状态，
        // 界面不该因此 panic——ADR 0015「查不到就是查不到」。
        assert_eq!(cycle(0, 0, true), 0);
        assert_eq!(cycle(7, 0, false), 0);
    }

    #[test]
    fn 越界的光标被钳回最后一行而不是越界读() {
        // 行数会随内容变（将来加一行「随机」按钮），而光标是跨帧带过来
        // 的一个裸 `usize`——不钳制就会在行数变少的那一帧索引越界。
        // 这里用一个**没有按下任何键**的输入状态，验的只有钳制本身。
        let idle = InputState::new();
        assert_eq!(move_cursor(99, 5, &idle), 4);
        assert_eq!(move_cursor(2, 5, &idle), 2, "没按键就不该移动");
        assert_eq!(move_cursor(3, 0, &idle), 0, "零行时不做减一运算");
    }

    #[test]
    fn 角色创建屏恰好五行且顺序固定() {
        assert_eq!(
            character_rows(),
            [
                CharacterRow::Race,
                CharacterRow::Gender,
                CharacterRow::Profession,
                CharacterRow::Next,
                CharacterRow::Back,
            ]
        );
    }

    #[test]
    fn 性别选择在gender的全部取值里循环() {
        let mut choice = CharacterChoice::default();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..Gender::ALL.len() {
            seen.insert(choice.gender());
            choice.gender = cycle(choice.gender, Gender::ALL.len(), true);
        }
        assert_eq!(
            seen.len(),
            Gender::ALL.len(),
            "循环一圈应当把 Gender::ALL 全部走过" // i18n-exempt：测试断言的失败消息，只在测试失败时打给开发者看
        );
    }
}
