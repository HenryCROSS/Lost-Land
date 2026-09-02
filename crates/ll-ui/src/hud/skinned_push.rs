//! 四条「推一块内容进渲染批次」的助手：面板背景、单层条形、双层条形、
//! 昼夜滑条。
//!
//! # 它们凑在一起的理由只有一条：**同一个皮肤分支**
//!
//! 四条的正文都是同一个 `match`——[`Skin`] 给得出真实贴图外观就走贴图
//! 路径，给不出就回退到纯色路径，两条路径互斥（同一块内容只会落进
//! [`LayerBatch::quads`] 或 [`LayerBatch::textured_quads`] 其中一个）。
//! 这条分支是「换皮肤只改『构造哪个 `Skin` 实现』这一处，控件与调用点
//! 一行不动」那条硬要求的落点，见 `crate::widget::skin` 模块文档。
//!
//! # 为什么从 [`super::render`] 里搬出来
//!
//! 与 [`super::placement`]/[`super::bottom_rows`] 当初搬出去是同一条
//! 理由：`render.rs` 在行数棘轮的快照里
//! （`scripts/ci/file_size_budget.json`），而它承担的主线是**那张固定
//! 分区的平铺表**（六块常驻面板各在屏幕的哪一角）。皮肤分支是另一件
//! 自成一体的事，四条函数从头到尾不认识任何一块具体面板。
//!
//! 所有者要的方向是「重构让代码为之后的开发做准备」，不是为了压行数
//! 而砍：真正的九宫格边框美术到位那一天，要改的正好就是这一个文件。

use crate::widget::bar::{bar_quads, textured_bar_quads, textured_two_layer_bar_quads};
use crate::widget::day_night_bar::{
    day_night_bar_quads, day_night_pointer_quad, textured_day_night_bar_quads,
    textured_day_night_pointer_quad,
};
use crate::widget::geometry::Rect;
use crate::widget::label::Label;
use crate::widget::layer::LayerBatch;
use crate::widget::panel::{panel_quads, textured_panel_quads};
use crate::widget::skin::{BarStyleId, DayNightBarStyleId, PanelStyleId, Skin};

/// 推入一块面板的背景——皮肤给出真实贴图外观
/// （[`Skin::textured_panel`]）就走贴图路径,否则回退到
/// [`Skin::panel`] 的纯色路径,两条路径互斥（同一块面板只会落进
/// [`LayerBatch::quads`] 或 [`LayerBatch::textured_quads`] 其中一个）。
pub(crate) fn push_panel(
    batch: &mut LayerBatch,
    rect: &Rect,
    panel_labels: Vec<Label>,
    skin: &dyn Skin,
) {
    match skin.textured_panel(PanelStyleId::Window) {
        Some(appearance) => batch
            .textured_quads
            .extend(textured_panel_quads(*rect, &appearance)),
        None => batch
            .quads
            .extend(panel_quads(*rect, &skin.panel(PanelStyleId::Window))),
    }
    batch.labels.extend(panel_labels);
}

/// 推入一条单层条形（经验条），分支逻辑同 [`push_panel`]。
pub(crate) fn push_bar(batch: &mut LayerBatch, rect: Rect, fraction: f32, skin: &dyn Skin) {
    match skin.textured_bar(BarStyleId::Progress) {
        Some(appearance) => {
            batch
                .textured_quads
                .extend(textured_bar_quads(rect, fraction, &appearance))
        }
        None => batch
            .quads
            .extend(bar_quads(rect, fraction, &skin.bar(BarStyleId::Progress))),
    }
}

/// 推入一条双层条形（生命/法力），分支逻辑同 [`push_panel`]。`style`
/// 由调用方指定是 [`BarStyleId::Health`] 还是 [`BarStyleId::Mana`]——
/// 两者外观（含贴图 tint）在 [`crate::widget::skin`] 里各自独立解析,
/// 是「两条资源条能分清哪条是哪条」这条修复的直接落点,见
/// `crate::widget::skin::BarStyleId::Health` 文档。
pub(crate) fn push_two_layer_bar(
    batch: &mut LayerBatch,
    rect: Rect,
    immediate_fraction: f32,
    lagging_fraction: f32,
    style: BarStyleId,
    skin: &dyn Skin,
) {
    match skin.textured_two_layer_bar(style) {
        Some(appearance) => batch.textured_quads.extend(textured_two_layer_bar_quads(
            rect,
            immediate_fraction,
            lagging_fraction,
            &appearance,
        )),
        None => {
            // 纯色回退没有专门的"双层"外观数据（`FlatBarAppearance`
            // 只有背景/前景两色）,复用同一份外观：背景当轨道,前景色
            // 同时充当立即层与余晖层——纯色场景下没有真实贴图可供
            // 区分两层的明暗,这是可接受的简化,不影响真实贴图路径
            // （`NineSliceSkin` 已经用 `afterglow_tint` 正确区分）。
            let appearance = skin.bar(style);
            batch.quads.extend(crate::widget::bar::two_layer_bar_quads(
                rect,
                immediate_fraction,
                lagging_fraction,
                &crate::widget::bar::FlatTwoLayerBarAppearance {
                    background_color: appearance.background_color,
                    afterglow_color: appearance.fill_color,
                    fill_color: appearance.fill_color,
                },
            ));
        }
    }
}

/// 推入昼夜滑条：整条底图 + 滑块，分支逻辑同 [`push_panel`]。
///
/// # 底图与滑块必须落进**同一个**容器
///
/// 两者恒是「底图先、滑块后」推入同一个 `Vec`：贴图路径两块都进
/// `textured_quads`，纯色回退两块都进 `quads`。**不允许一边贴图一边
/// 纯色**——那正是所有者实机反馈「少了滑条，只显示了背景条」的根因：
/// 纯色滑块与贴图底图分处层内两道 pass，底图恒后提交、把滑块整个盖住，
/// 见 `crate::widget::day_night_bar` 模块文档「曾经的缺陷」一节。
///
/// 这个不变式由皮肤层保证而不是靠这里的调用纪律：
/// `crate::widget::skin::NineSliceSkin::textured_day_night_bar` 里底图与
/// 滑块的 UV 各带一个 `?`，任一张查不到就整条返回 `None`，本函数因此
/// 只能整条走贴图或整条走纯色。
pub(crate) fn push_day_night_bar(
    batch: &mut LayerBatch,
    rect: Rect,
    pointer_fraction: f32,
    skin: &dyn Skin,
) {
    match skin.textured_day_night_bar(DayNightBarStyleId::Clock) {
        Some(appearance) => {
            batch
                .textured_quads
                .extend(textured_day_night_bar_quads(rect, &appearance));
            batch.textured_quads.push(textured_day_night_pointer_quad(
                rect,
                &appearance,
                pointer_fraction,
            ));
        }
        None => {
            let appearance = skin.day_night_bar(DayNightBarStyleId::Clock);
            batch.quads.extend(day_night_bar_quads(rect, &appearance));
            batch.quads.push(day_night_pointer_quad(
                rect,
                appearance.pointer_color,
                pointer_fraction,
            ));
        }
    }
}
