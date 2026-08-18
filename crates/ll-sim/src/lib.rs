//! 迷途大陆的回合与模拟层。
//!
//! 本 crate 目前建了两样东西：[`entity`] 的实体存储（薄层人口 + 厚层
//! 实体池），以及 [`naming`] 的纯函数名字生成。时间轴调度器与
//! `Intent → resolve → Effect → apply` 单向管线是后续批次的内容，见
//! `docs/superpowers/specs/2026-08-17-p3-turn-combat.md`。

pub mod entity;
pub mod naming;
