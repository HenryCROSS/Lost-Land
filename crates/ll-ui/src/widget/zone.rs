//! 屏幕分三个区，任何一块面板都得说清自己属于哪一个（规格 L1，
//! `knowledge/design/ui-and-navigation.md` §6.2）。
//!
//! | 区 | 归谁 | 定位方式 |
//! |---|---|---|
//! | [`ScreenZone::Resident`] 常驻区 | 状态栏、资源条、昼夜条、角色/背包/装备面板、底栏那两行里的按键提示 | 贴边锚定 + 纵向堆叠 |
//! | [`ScreenZone::Floating`] 浮层区 | 世界地图、交互/背包/制作弹窗、反馈行 | 相对常驻区之外的空白居中或贴边 |
//! | [`ScreenZone::Modal`] 模态区 | 九块 `ScreenState` 屏 | 整屏压暗 + 居中面板 |
//!
//! # 「声明属于哪个区」不需要任何新字段
//!
//! 规格说「任何新面板必须声明自己属于哪一个」，没说怎么声明。这里的
//! 做法是：**区是 [`UiLayer`] 的派生值**（[`ScreenZone::of`]），而选层
//! 本来就是强制的——`LayeredFrame::layer_mut` 的参数没有默认值，加一块
//! 内容就必须挑一层。于是「声明」这件事零成本地已经发生了，也不会出现
//! 「层写了 `Popup`、区却标成常驻」这种两份真相源分叉的可能。
//!
//! 反过来说，本模块**刻意不提供** `ScreenZone → UiLayer` 的反向映射：
//! 那会是一对多，写出来就等于请人去猜。
//!
//! # 模态区今天没有成员
//!
//! 九块模态屏走的是 `crate::screen::render::ScreenFrame`，那条通道压根
//! 不进 `LayeredFrame`（见其类型文档：模态屏恒盖在 HUD 全部层级之上，
//! 套一层层级只会多出三个永远为空的层）。规格 N9 要把它收进一个新的
//! `UiLayer::Modal`，**那是另一批的事**；本枚举先把这个区留出来，
//! [`ScreenZone::of`] 里写清楚今天没有哪个 `UiLayer` 映射到它。
//!
//! # 留白规则：中段不放常驻元素
//!
//! 常驻区左列（角色/背包，x 起点 `SCREEN_MARGIN`）与右列（装备面板，
//! 右对齐）之间的中段**永远不放常驻元素**——那是玩家看世界的地方，
//! 也是浮层区居中时唯一不会挡住角色的落点。这条今天是巧合成立的，
//! 规格 L1 把它变成约束，判据落在
//! `crates/ll-ui/src/hud/render_layout_tests.rs` 那条遍历整个 `Hud` 层的
//! 断言上（**跑在 `build_hud_frame` 的产出上**，因此新加的面板自动进入
//! 判据，不会像手写清单那样漏）。
//!
//! 判据比规格原文多一个例外：**底栏**。规格写的是「要么 `right() <= 屏宽/2`
//! 要么 `x >= 屏宽/2`」两选一，而批次 23 的按键提示行（常驻区、水平居中、
//! 贴屏幕最下沿）照那条会红。它是对的——留白规则的理由是「那是玩家看
//! 世界的地方」，屏幕最下沿那一条窄边不是。底栏有多高由
//! `crate::hud::bottom_rows::BOTTOM_STRIP_HEIGHT` 说了算。

use super::layer::UiLayer;

/// 屏幕的三个区，见模块文档那张表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenZone {
    /// 常驻区：不需要玩家做任何事就一直在那儿的东西。贴边锚定。
    Resident,
    /// 浮层区：玩家主动召唤的、或者系统临时说一句话的东西。相对常驻区
    /// 之外的空白居中或贴边。
    Floating,
    /// 模态区：整屏压暗 + 居中面板，底下的世界一个字节都不动。
    Modal,
}

impl ScreenZone {
    /// 一层属于哪个区。
    ///
    /// **这是「新面板必须声明自己属于哪个区」的全部实现**——选层是强制
    /// 的，选了层就选了区，见模块文档。
    ///
    /// 今天没有任何 `UiLayer` 映射到 [`ScreenZone::Modal`]：模态屏走
    /// `crate::screen::render::ScreenFrame` 那条独立通道，规格 N9 才会
    /// 把它收进一个新的 `UiLayer::Modal`。那一天这里加一条分支，
    /// `Self::of` 的调用方一行都不用改。
    pub const fn of(layer: UiLayer) -> ScreenZone {
        match layer {
            UiLayer::Hud => ScreenZone::Resident,
            // 三层浮层的区别是「谁盖住谁」，不是「摆在哪」——世界地图、
            // 动作菜单、反馈行都相对常驻区之外的空白落位。
            UiLayer::Overlay | UiLayer::Popup | UiLayer::Notice => ScreenZone::Floating,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 每一层都能说出自己属于哪个区() {
        // 这条盯的是「声明是强制的」这件事本身：`UiLayer::ALL` 是层集合
        // 的唯一真相源，遍历它就不可能有哪一层漏了声明——`of` 是
        // `match` 且没有 `_ =>` 兜底分支，新加一层时**编译期**就会红，
        // 比任何运行期断言都早。
        //
        // 因此本条真正的价值在于第二条断言：`ALL` 里确实每一层都被走到，
        // 而不是这个数组本身空了。
        //
        // 反例验证（已实跑）：给 `of` 加一条 `_ => ScreenZone::Modal`
        // 兜底并把 `Hud` 那一支删掉，本条红在「常驻层不该落在模态区」。
        // Arrange & Act & Assert
        assert!(!UiLayer::ALL.is_empty(), "层集合不该是空的");
        for layer in UiLayer::ALL {
            let zone = ScreenZone::of(layer);
            assert_ne!(
                zone,
                ScreenZone::Modal,
                "{layer:?} 落在了模态区，而模态屏今天走的是 ScreenFrame 那条独立通道（规格 N9 才收进 UiLayer）"
            );
        }
    }

    #[test]
    fn 常驻层落在常驻区其余三层落在浮层区() {
        // Arrange & Act & Assert
        assert_eq!(ScreenZone::of(UiLayer::Hud), ScreenZone::Resident);
        for layer in [UiLayer::Overlay, UiLayer::Popup, UiLayer::Notice] {
            assert_eq!(
                ScreenZone::of(layer),
                ScreenZone::Floating,
                "{layer:?} 不该是常驻区——常驻区的东西不需要玩家做任何事就一直在"
            );
        }
    }
}
