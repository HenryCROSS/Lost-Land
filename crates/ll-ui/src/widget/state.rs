//! 持久 UI 状态：一张按控件 id 索引的旁表。
//!
//! # 为什么是旁表，不是塞进 `WorldState`
//!
//! 项目所有者的硬要求：焦点、滚动位置、动画进度这类「跨帧持续存在，
//! 但只影响画面、从不影响玩法判定」的状态，必须放在 UI 层自己的一张
//! 表里，绝不能碰 `WorldState`。这是 [ADR
//! 0020](../../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
//! 「甲区（渲染/表现层浮点）与乙区（世界状态整数）边界靠 crate 位置
//! 而不是约定守住」在「UI 交互状态该放哪」这个问题上的直接应用：
//! [`WidgetStateTable`] 活在 `ll-ui`（未来会是 `ll-game::app::Demo` 持有
//! 的一个字段，与它已有的 `zoom`/`camera`/`anim` 等运行期渲染状态同一
//! 层），结构上不可能被序列化进存档或参与任何世界状态哈希。
//!
//! # 表要能同时容纳三类状态，不能只为动画量身定制
//!
//! 项目所有者点名：这张表将来要同时装得下**焦点**（`Option<Id>` 之
//! 类）、**滚动位置**（每个可滚动区域一个偏移）、**动画进度**（当前值
//! /目标值/起始时刻）——若现在只按动画的形状设计（例如只存一个裸
//! `AnimatedValue`），将来加焦点/滚动就要推翻重来。[`WidgetState`]
//! 因此从一开始就是三个字段的结构体，本批次只有 `anim`
//! （见 [`super::anim::AnimatedValue`]）与 `last_discrete`（经验条升级
//! 检测用,见 [`animate_experience_bar`]）有真实消费者，`focused`/
//! `scroll_offset` 现在恒为默认值,但字段已经在,将来的焦点导航/可滚动
//! 列表批次只需要开始读写它们,不需要改这个类型的形状。
//!
//! # UI 交互层批次核实结论：形状容得下，加了两个字段
//!
//! 上一段的预留结论核实成立——`focused` 字段不需要改形状,直接就是
//! 本批次 [`crate::widget::focus`] 要读写的那个「这个控件是否持有
//! 焦点」标志。**但悬停与按下这两类状态之前没有预留位置**，如实记录
//! 这处偏差：本批次新增 `hovered`/`pressed` 两个字段（见下方文档），
//! 不是推翻既有形状重新设计，是在同一张「按 widget id 索引的旁表」
//! 里追加两个同量级的布尔字段——这正是这张表当初的设计目标（「将来
//! 只需要开始读写,不需要改类型形状」）在还没预料到的一个维度
//! （鼠标悬停/按下）上打的一个小补丁,不是设计失败。

use std::collections::HashMap;

use super::anim::{AnimatedValue, FrameTick};

/// 控件 id——本批次用静态字符串（例如 `"hud.health_bar"`），足够本 HUD
/// 固定数量的几个控件使用；若未来出现动态数量的控件（例如可变长度的
/// 背包列表每一行都要独立状态），换成 `String`/`(静态前缀, 索引)` 元组
/// 是局部改动，[`WidgetStateTable`] 的存储与查询接口不需要跟着改。
pub type WidgetId = &'static str;

/// 一个控件的持久状态——见模块文档「表要能同时容纳三类状态」一节。
#[derive(Debug, Clone, Default)]
pub struct WidgetState {
    /// 是否持有键盘/手柄焦点——[`crate::widget::focus::move_focus`] 在
    /// 一组控件间移动焦点时,同一时刻只有其中一个的 `focused` 为真。
    pub focused: bool,
    /// 滚动偏移（像素）——本批次没有可滚动区域，恒为 `0.0`，字段先
    /// 占位。
    pub scroll_offset: f32,
    /// 光标当前是否悬停在这个控件上——[`crate::widget::button::update_button`]
    /// 每帧现算并写回,纯粹的表现层状态（决定悬停高亮走哪个皮肤样式），
    /// 不影响任何判定逻辑本身。
    pub hovered: bool,
    /// 鼠标左键是否在这个控件上按下、且尚未松开——用于区分「按下时在
    /// 这个控件上」与「按住后拖出控件范围」，见
    /// [`crate::widget::button::update_button`] 文档「按下与触发」
    /// 一节：只有按下与松开都发生在同一个控件上才算一次点击触发，本
    /// 字段就是跨帧记住「这次按下是不是从我这里开始的」的地方。
    pub pressed: bool,
    /// 数值动画状态——血条/经验条这类「显示值应平滑追上真实值」的
    /// 控件用它，`None` 表示这个控件从未开始过任何动画。
    pub anim: Option<AnimatedValue>,
    /// 上一次观察到的离散整数值——目前只有经验条的升级检测在用（存
    /// 角色等级），命名刻意不叫 `last_level`：这是「检测两帧之间某个
    /// 离散事件是否发生」这一类需求的通用挂点，不是经验条专属字段。
    pub last_discrete: Option<i64>,
    /// 是否处于「先冲满旧值、清零、再继续填」的多帧过渡序列中——经验
    /// 条升级动画专用，见 [`animate_experience_bar`]。
    pub wrap_pending: bool,
}

/// 持久 UI 状态表：按 [`WidgetId`] 索引的旁表，见模块文档。
#[derive(Debug, Clone, Default)]
pub struct WidgetStateTable {
    entries: HashMap<WidgetId, WidgetState>,
}

impl WidgetStateTable {
    /// 建一张空表。
    pub fn new() -> WidgetStateTable {
        WidgetStateTable::default()
    }

    /// 取 `id` 对应的状态，不存在则插入一份默认值——本表是 UI 层的纯
    /// 表现状态，`HashMap` 迭代顺序不确定这条顾虑（约束 C5）只约束
    /// `WorldState` 的序列化/哈希路径，本表从不参与那两者，用
    /// `HashMap` 不违反任何既有纪律。
    pub fn entry(&mut self, id: WidgetId) -> &mut WidgetState {
        self.entries.entry(id).or_default()
    }

    /// 只读查询 `id` 对应的状态，不存在时返回 `None`（不插入）。
    pub fn get(&self, id: WidgetId) -> Option<&WidgetState> {
        self.entries.get(id)
    }

    /// 推进（或初始化）`id` 对应的动画值朝 `target` 前进,返回 `now`
    /// 这一帧应显示的值——单层条形（经验条这类没有余晖效果的场景）
    /// 的标准入口。
    pub fn animate(&mut self, id: WidgetId, target: f32, now: FrameTick) -> f32 {
        self.animate_with_duration(id, target, now, super::anim::DEFAULT_ANIM_DURATION_FRAMES)
    }

    /// 同 [`Self::animate`]，但用调用方指定的过渡时长——双层血条的余晖
    /// 层用它取一个比立即层更长的时长,制造「追赶」的滞后感,见
    /// `crate::widget::bar::FlatTwoLayerBarAppearance` 模块文档。
    ///
    /// 时长只在**第一次**为这个 id 建动画时生效——已存在的动画沿用它
    /// 建立时的时长（改时长会让正在进行的过渡半途变速,这里选择「一旦
    /// 建立就固定」而不是每帧都可能悄悄改变节奏）。
    pub fn animate_with_duration(
        &mut self,
        id: WidgetId,
        target: f32,
        now: FrameTick,
        duration_frames: u32,
    ) -> f32 {
        let state = self.entry(id);
        match &mut state.anim {
            Some(anim) => {
                anim.retarget(target, now);
                anim.value_at(now)
            }
            None => {
                state.anim = Some(AnimatedValue::with_duration(target, duration_frames));
                target
            }
        }
    }
}

/// 经验条专用的动画推进：检测到等级上升时，先把当前动画冲满到
/// `1.0`，冲满后再清零并继续朝 `real_fraction` 填——「填满 → 清零 →
/// 继续填」的多帧序列，而不是直接从旧比例跳到新比例。
///
/// `level`/`real_fraction` 都是每帧传入的**真实值**（[`WidgetStateTable`]
/// 只负责animation本身,不负责判断"现在该显示多少",这条职责边界与
/// [`WidgetStateTable::animate`] 一致)。
pub fn animate_experience_bar(
    table: &mut WidgetStateTable,
    id: WidgetId,
    level: i32,
    real_fraction: f32,
    now: FrameTick,
) -> f32 {
    let level_marker = level as i64;
    {
        let state = table.entry(id);
        if state
            .last_discrete
            .is_some_and(|previous| level_marker > previous)
        {
            state.wrap_pending = true;
        }
        state.last_discrete = Some(level_marker);
    }

    let state = table.entry(id);
    let anim = state
        .anim
        .get_or_insert_with(|| AnimatedValue::new(real_fraction));

    if state.wrap_pending {
        anim.retarget(1.0, now);
        let value = anim.value_at(now);
        if value >= 1.0 {
            // 已经冲满：清零并朝真实比例继续填,结束这段过渡序列。
            anim.snap_to(0.0);
            anim.retarget(real_fraction, now);
            state.wrap_pending = false;
        }
        anim.value_at(now)
    } else {
        anim.retarget(real_fraction, now);
        anim.value_at(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animate对全新id从目标值本身开始不产生跳变() {
        // Arrange
        let mut table = WidgetStateTable::new();

        // Act
        let value = table.animate("hud.test_bar", 42.0, 0);

        // Assert
        assert_eq!(value, 42.0);
    }

    #[test]
    fn animate_with_duration用更长的自定义时长比默认时长收敛得更慢() {
        // Arrange：同一个起点/目标,一个用默认时长,一个用两倍时长。
        let mut default_table = WidgetStateTable::new();
        let mut slow_table = WidgetStateTable::new();
        default_table.animate("hud.fast", 0.0, 0);
        slow_table.animate_with_duration(
            "hud.slow",
            0.0,
            0,
            super::super::anim::DEFAULT_ANIM_DURATION_FRAMES * 2,
        );

        // Act：两者都朝 100 前进,取默认时长那一刻的两条显示值。
        default_table.animate("hud.fast", 100.0, 0);
        slow_table.animate_with_duration(
            "hud.slow",
            100.0,
            0,
            super::super::anim::DEFAULT_ANIM_DURATION_FRAMES * 2,
        );
        let fast_value = default_table.animate(
            "hud.fast",
            100.0,
            super::super::anim::DEFAULT_ANIM_DURATION_FRAMES as u64,
        );
        let slow_value = slow_table.animate_with_duration(
            "hud.slow",
            100.0,
            super::super::anim::DEFAULT_ANIM_DURATION_FRAMES as u64,
            super::super::anim::DEFAULT_ANIM_DURATION_FRAMES * 2,
        );

        // Assert：默认时长这一刻,快的那条已经收敛到 100,慢的那条还没。
        assert_eq!(fast_value, 100.0);
        assert!(slow_value < 100.0);
    }

    #[test]
    fn animate推进足够多帧后精确收敛到目标值() {
        // Arrange
        let mut table = WidgetStateTable::new();
        table.animate("hud.test_bar", 0.0, 0);

        // Act
        table.animate("hud.test_bar", 1.0, 0);
        let converged = table.animate("hud.test_bar", 1.0, DEFAULT_ANIM_DURATION_FRAMES_FOR_TEST);

        // Assert
        assert_eq!(converged, 1.0);
    }

    /// 与 `super::anim::DEFAULT_ANIM_DURATION_FRAMES` 保持一致，避免
    /// 这条测试因为默认时长改变而需要跟着改字面量。
    const DEFAULT_ANIM_DURATION_FRAMES_FOR_TEST: u64 =
        super::super::anim::DEFAULT_ANIM_DURATION_FRAMES as u64 + 1;

    #[test]
    fn 经验条升级时先冲满旧比例不直接跳到新比例() {
        // Arrange：1 级,进度条一半,尚未升级。
        let mut table = WidgetStateTable::new();
        animate_experience_bar(&mut table, "hud.xp_bar", 1, 0.5, 0);

        // Act：升到 2 级,新比例只有 0.1，触发升级的那一帧本身还没来
        // 得及推进（`AnimatedValue::retarget` 从"现在显示的值"起跑,
        // 起跑瞬间不产生跳变，见 `crate::widget::anim` 模块文档「收敛
        // 保证」一节），再往后推进几帧——若走"冲满→清零→继续填",这
        // 几帧应该正在朝 1.0 冲；若被误实现成直接跳到新比例,这里会是
        // 一个远低于 0.5、朝 0.1 靠拢的值。
        animate_experience_bar(&mut table, "hud.xp_bar", 2, 0.1, 1);
        let later = animate_experience_bar(&mut table, "hud.xp_bar", 2, 0.1, 10);

        // Assert
        assert!(later > 0.5, "升级应先朝满进度冲刺,而不是直接跌到新比例");
    }

    #[test]
    fn 经验条升级动画冲满清零后最终收敛到新比例() {
        // Arrange：1 级,进度条一半,然后升到 2 级、新比例 0.3——真实
        // 调用方式是每帧调用一次并递增 `now`（`ll-game::app` 每帧调用
        // 一次这个函数），不是一次性跳到很远的 `now`：冲满、清零、
        // 重新起跑三个阶段各自都需要真实流逝的帧数,一次性跳跃会跳过
        // 中间阶段,见本测试与上一条测试的对照。
        let mut table = WidgetStateTable::new();
        animate_experience_bar(&mut table, "hud.xp_bar", 1, 0.5, 0);

        // Act：模拟连续多帧调用,足够跑完"冲满→清零→重新爬到 0.3"
        // 整段序列。
        let mut last_value = 0.0;
        for frame in 1..=(DEFAULT_ANIM_DURATION_FRAMES_FOR_TEST * 3) {
            last_value = animate_experience_bar(&mut table, "hud.xp_bar", 2, 0.3, frame);
        }

        // Assert：足够多帧之后应该精确收敛到新比例本身,不多不少。
        assert_eq!(last_value, 0.3);
    }

    #[test]
    fn 经验条未升级时正常朝真实比例收敛不触发冲满序列() {
        // Arrange
        let mut table = WidgetStateTable::new();
        animate_experience_bar(&mut table, "hud.xp_bar", 1, 0.2, 0);

        // Act：等级不变,比例提升到 0.4,模拟连续多帧调用（真实调用方式,
        // 见上一条测试同样的说明）。
        let mut last_value = 0.0;
        for frame in 1..=DEFAULT_ANIM_DURATION_FRAMES_FOR_TEST {
            last_value = animate_experience_bar(&mut table, "hud.xp_bar", 1, 0.4, frame);
        }

        // Assert：足够多帧后应精确收敛到 0.4,不是先冲到 1.0
        // （未升级,不触发冲满序列）。
        assert_eq!(last_value, 0.4);
    }
}
