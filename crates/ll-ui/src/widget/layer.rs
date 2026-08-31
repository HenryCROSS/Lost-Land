//! UI 层级：**显式**声明每一块界面画在哪一层，由层级——而不是调用
//! 顺序或渲染 pass 的先后——决定谁盖住谁。
//!
//! # 这一层为什么存在（一条实机缺陷的根因）
//!
//! 项目所有者实机反馈：「UI 显示层的先后顺序有点问题，像是血条之类的
//! UI 会覆盖地图」。世界地图是一块近全屏的覆盖层，它应该盖住常驻 HUD，
//! 实际却反过来。
//!
//! 根因**不是**有人把调用顺序写反了。`crate::hud::render::build_hud_frame`
//! 的推入顺序一直是对的（状态栏 → 资源条 → 昼夜条 → 角色 → 背包 →
//! 装备 → 世界地图 → 动作菜单 → 反馈行）。真正的问题是**推入顺序根本
//! 决定不了绘制顺序**：这一帧的内容被分装进两个互不相干的容器——纯色
//! 矩形一个、贴图矩形一个——而 `render_hud` 按**固定顺序**先提交完整
//! 一批纯色、再提交完整一批贴图。于是：
//!
//! - 常驻 HUD 在真实贴图皮肤（`crate::widget::skin::NineSliceSkin`）下
//!   落进**贴图**批次；
//! - 世界地图恒只产出**纯色**矩形（见
//!   `crate::hud::world_map::world_map_frame` 文档）；
//! - 纯色批次先提交、贴图批次后提交 → **血条与四块面板画在地图之上**。
//!
//! 同一个机制还制造了第二处所有者可见的缺陷：昼夜滑条的指针恒是纯色
//! 矩形，而整条底图在贴图皮肤下是贴图矩形——指针被自己的底图整个盖住，
//! 屏幕上只剩「一条背景」，看起来像「滑块没画」。
//!
//! # 为什么引入层级，而不是把调用顺序挪一挪
//!
//! 挪顺序治不了这个缺陷：**跨批次的先后不由推入顺序决定**，把世界地图
//! 挪到更后面推入，它仍然在纯色批次里，仍然被贴图批次压住。即使换个
//! 写法勉强绕过这一次，下一个往 HUD 里插内容的人也无从知道自己该插在
//! 哪——「顺序」这条约定不在任何类型上，读代码看不出来，评审也看不出来。
//!
//! 层级把「谁盖谁」从一条口头约定变成一个**写在类型上的声明**：每块
//! 内容显式说自己属于哪一层，提交顺序按层升序，层内才轮到纯色/贴图/
//! 文本三道 pass。跨层遮挡因此与皮肤给不给贴图**完全无关**。
//!
//! # 新加一块 UI 时该怎么选层
//!
//! 问一个问题就够：**它出现的时候，玩家的注意力应该在它身上，还是在
//! 它下面那层身上？**
//!
//! | 层 | 收什么 | 判据 |
//! |---|---|---|
//! | [`UiLayer::Hud`] | 常驻观测层：状态栏、生命/法力条、昼夜滑条、角色/背包/装备面板 | 一直在屏幕上，玩家不主动召唤它，它也不该抢注意力 |
//! | [`UiLayer::Overlay`] | 玩家主动召唤的大块视图：世界地图 | 打开时它就是玩家正在看的东西，常驻 HUD 退居其次 |
//! | [`UiLayer::Popup`] | 需要玩家当场做选择的浮窗：动作菜单 | 玩家正在它里面操作，它下面的一切都只是背景 |
//! | [`UiLayer::Notice`] | 一次性通告：反馈行 | 它要说的正是「你刚才那一下没起作用」，被任何东西挡住就等于没说 |
//!
//! 拿不准就往**低**了放：放低了最坏结果是被别的东西挡住一次（看得见、
//! 报得出来），放高了则会悄悄压住本该在上面的东西，而这类问题正是上面
//! 那条实机缺陷躲过多轮验收的原因。
//!
//! # 模态屏不在这里
//!
//! 菜单/设置/首页（`crate::screen`）由 `ll_game::app::draw_screen` 在
//! `draw_hud` **之后**单独提交，恒盖在本模块全部四层之上——那一层的
//! 压暗背板本来就要把世界层与整个 HUD 一起压暗。它已经是对的，本模块
//! 不接管它，也不需要为它加第五个变体。

use super::label::Label;
use super::quad::QuadInstance;
use super::textured_quad::TexturedQuadInstance;

/// UI 层级，**声明顺序即由下到上的绘制顺序**（`Ord` 由此派生，
/// [`UiLayer::ALL`] 也按同一顺序排列）。选层判据见模块文档「新加一块
/// UI 时该怎么选层」一节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UiLayer {
    /// 常驻观测层——状态栏、生命/法力条、昼夜滑条、角色/背包/装备面板。
    Hud,
    /// 玩家主动召唤的大块覆盖视图——世界地图。
    Overlay,
    /// 需要玩家当场做选择的浮窗——动作菜单。
    Popup,
    /// 一次性通告——反馈行。
    Notice,
}

impl UiLayer {
    /// 全部层级，**按由下到上的绘制顺序**排列。
    ///
    /// 做成常量数组而不是遍历某个容器：遍历顺序必须是确定的（约束 C5），
    /// 而数组字面量的顺序写在源码里、评审时一眼可见。
    pub const ALL: [UiLayer; 4] = [
        UiLayer::Hud,
        UiLayer::Overlay,
        UiLayer::Popup,
        UiLayer::Notice,
    ];

    /// 本层在 [`Self::ALL`] 里的下标——[`LayeredFrame`] 用它做数组
    /// 索引，同时也是「谁在上面」的唯一数值判据。
    pub const fn index(self) -> usize {
        match self {
            UiLayer::Hud => 0,
            UiLayer::Overlay => 1,
            UiLayer::Popup => 2,
            UiLayer::Notice => 3,
        }
    }
}

/// 一层里这一帧要提交的全部内容。
///
/// 三个容器的先后就是**层内**的提交顺序：纯色 → 贴图 → 文本。层内仍
/// 然存在「贴图恒盖住纯色」这条既有性质（两道 pass 无法交错），因此
/// **同一层里互相重叠的两块内容必须落在同一个容器里**才谈得上用推入
/// 顺序决定遮挡——世界地图的边框/格子/据点标记全在 `quads` 里、昼夜
/// 滑条的底图与指针在贴图皮肤下同在 `textured_quads` 里，都是这条要求
/// 的具体落点。真正需要跨这条界线的遮挡关系，用**层**表达，不要指望
/// 容器内的推入顺序。
#[derive(Debug, Default)]
pub struct LayerBatch {
    /// 纯色矩形（皮肤给不出贴图时的回退，以及本就没有贴图可采样的
    /// 内容，例如世界地图的地形格）。
    pub quads: Vec<QuadInstance>,
    /// 贴图矩形。
    pub textured_quads: Vec<TexturedQuadInstance>,
    /// 文本行。
    pub labels: Vec<Label>,
}

impl LayerBatch {
    /// 这一层这一帧有没有任何内容——空层不必开渲染 pass。
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.textured_quads.is_empty() && self.labels.is_empty()
    }
}

/// 按层分装的一帧 UI 内容。
///
/// 内部是**定长数组**而不是 `BTreeMap`/`HashMap`：层集合是编译期已知
/// 的封闭枚举，数组下标即 [`UiLayer::index`]，遍历顺序恒等于
/// [`UiLayer::ALL`] 的声明顺序，既不需要哈希（约束 C5 从形状上被排除），
/// 也不需要每帧分配。
#[derive(Debug, Default)]
pub struct LayeredFrame {
    layers: [LayerBatch; UiLayer::ALL.len()],
}

/// 一次渲染提交——[`LayeredFrame::draw_batches`] 产出的元素。
///
/// 这个类型存在的唯一理由是让「提交顺序」成为**可断言的数据**：
/// `crate::hud::render::render_hud` 直接遍历
/// [`LayeredFrame::draw_batches`] 逐条提交，因此测试断言这个序列的先后
/// 就等于断言了屏幕上的遮挡关系，不需要开窗口截图（ADR 0025 禁止用
/// 合成按键做验收，视觉遮挡这类问题因此必须有数据层的抓手）。
#[derive(Debug, Clone, Copy)]
pub enum DrawBatch<'a> {
    /// 一批纯色矩形。
    Quads(&'a [QuadInstance]),
    /// 一批贴图矩形。
    Textured(&'a [TexturedQuadInstance]),
    /// 一批文本行。
    Labels(&'a [Label]),
}

impl LayeredFrame {
    /// 取某一层的可变引用，供调用方往里推内容。
    pub fn layer_mut(&mut self, layer: UiLayer) -> &mut LayerBatch {
        &mut self.layers[layer.index()]
    }

    /// 取某一层的只读引用。
    pub fn layer(&self, layer: UiLayer) -> &LayerBatch {
        &self.layers[layer.index()]
    }

    /// 本帧真实的提交顺序：**按层升序**，层内纯色 → 贴图 → 文本，空
    /// 批次不出现。
    ///
    /// 这是遮挡关系的**唯一真相源**——`render_hud` 遍历它逐条提交，
    /// 测试也断言它，两者不可能分叉。
    pub fn draw_batches(&self) -> Vec<DrawBatch<'_>> {
        let mut batches = Vec::new();
        for layer in UiLayer::ALL {
            let batch = self.layer(layer);
            if !batch.quads.is_empty() {
                batches.push(DrawBatch::Quads(&batch.quads));
            }
            if !batch.textured_quads.is_empty() {
                batches.push(DrawBatch::Textured(&batch.textured_quads));
            }
            if !batch.labels.is_empty() {
                batches.push(DrawBatch::Labels(&batch.labels));
            }
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_quad(color: [f32; 4]) -> QuadInstance {
        QuadInstance {
            position: [0.0, 0.0],
            size: [1.0, 1.0],
            color,
        }
    }

    fn sample_textured() -> TexturedQuadInstance {
        TexturedQuadInstance {
            position: [0.0, 0.0],
            size: [1.0, 1.0],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        }
    }

    #[test]
    fn 层级枚举的声明顺序即由下到上() {
        // Arrange & Act & Assert：`Ord` 派生自声明顺序，`index` 与
        // `ALL` 必须与它一致——三者分叉就是遮挡关系被悄悄改掉。
        assert!(UiLayer::Hud < UiLayer::Overlay);
        assert!(UiLayer::Overlay < UiLayer::Popup);
        assert!(UiLayer::Popup < UiLayer::Notice);
        for (index, layer) in UiLayer::ALL.iter().enumerate() {
            assert_eq!(layer.index(), index);
        }
    }

    #[test]
    fn 高层的贴图批次排在低层的纯色批次之后() {
        // 这正是实机缺陷的形状：低层（HUD）出贴图、高层（世界地图）
        // 出纯色。分批提交时若不按层走，低层贴图会盖住高层纯色。
        // Arrange
        let mut frame = LayeredFrame::default();
        frame
            .layer_mut(UiLayer::Hud)
            .textured_quads
            .push(sample_textured());
        frame
            .layer_mut(UiLayer::Overlay)
            .quads
            .push(sample_quad([1.0, 0.0, 0.0, 1.0]));

        // Act
        let batches = frame.draw_batches();

        // Assert：HUD 的贴图批次在前，覆盖层的纯色批次在后。
        assert_eq!(batches.len(), 2);
        assert!(matches!(batches[0], DrawBatch::Textured(_)));
        assert!(matches!(batches[1], DrawBatch::Quads(_)));
    }

    #[test]
    fn 空层不产出批次() {
        // Arrange
        let mut frame = LayeredFrame::default();
        frame
            .layer_mut(UiLayer::Notice)
            .quads
            .push(sample_quad([0.0, 1.0, 0.0, 1.0]));

        // Act
        let batches = frame.draw_batches();

        // Assert：只有通告层有内容，其余三层不该开 pass。
        assert_eq!(batches.len(), 1);
    }

    #[test]
    fn 层内顺序是纯色贴图文本() {
        // Arrange
        let mut frame = LayeredFrame::default();
        let hud = frame.layer_mut(UiLayer::Hud);
        hud.labels.push(Label {
            text: "x".to_string(),
            x: 0.0,
            y: 0.0,
            max_width: 400.0,
        });
        hud.textured_quads.push(sample_textured());
        hud.quads.push(sample_quad([1.0, 1.0, 1.0, 1.0]));

        // Act：推入顺序与提交顺序无关——层内顺序由本方法固定。
        let batches = frame.draw_batches();

        // Assert
        assert!(matches!(batches[0], DrawBatch::Quads(_)));
        assert!(matches!(batches[1], DrawBatch::Textured(_)));
        assert!(matches!(batches[2], DrawBatch::Labels(_)));
    }
}
