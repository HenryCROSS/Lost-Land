//! 列表：把「逐行往下堆叠文字」这件事从四个面板模块各自的临时游标
//! 变量收口成一个正式控件。
//!
//! # 即时模式：每帧重新构造,不持有跨帧状态
//!
//! `RowCursor` 不是一棵常驻的行控件树——每次调用 HUD 面板的行生成
//! 函数都会现场 `RowCursor::new` 一个新的,用完即弃（下一帧再重新
//! 构造一个）。这与 `ll-render::batch::SpriteBatch`/
//! `ll_text::TextRenderer::render` 的既有即时模式完全一致：本项目的
//! 整条渲染管线每帧全量重新声明要画的内容,`RowCursor` 只是这套模式
//! 在「按行布局」这个子问题上的具体应用,不是引入了一种新的状态管理
//! 范式。
//!
//! # 背包/装备栏用它——物品列表、装备槽位列表
//!
//! 任务书点名「列表：背包物品、装备槽位」——[`crate::hud::inventory_panel`]/
//! [`crate::hud::equipment_panel`] 都用 `RowCursor` 逐行推进,不再各自
//! 手写 `cursor_y += line_height`。
//!
//! # 纵向推进按**渲染出来的行数**，不按标签条数（规格 W2）
//!
//! 此前 [`RowCursor::push`] 无条件 `cursor_y += row_height`——一条标签
//! 恒占一行。而一条标签在断行宽度内**可能占两行**，于是：
//!
//! 1. 第二行画在下一条标签的位置上，两行叠在一起；
//! 2. 面板高度按 `cursor_y()` 现算，于是背景比内容矮一行。
//!
//! `knowledge/design/ui-and-navigation.md` §8.5 W2 记着这条：
//! 「O-1/O-2/O-3 的『压住下一行』是同一个根因」。现在 `push` 先用
//! [`ll_text::MeasureText`] 量一次这一行在断行宽度内断成几行，再按
//! **实际行数**推进——面板高度因此自动跟着对，调用方一个字都不用改。
//!
//! # 断行宽度是构造参数，不是提交时才补的常数
//!
//! `RowCursor` 建的时候就要知道自己往多宽的一列里写字，它写出的每一个
//! [`Label`] 都带着这个宽度（[`Label::max_width`]）。这条链路的源头是
//! 面板宽度（`crate::hud::build_panel` / `crate::screen::build_screen_panel`
//! 各自从 `面板宽 - 2 × 内边距` 算出来），见 [`Label::max_width`] 文档。

use ll_text::MeasureText;

use super::label::Label;

/// 一个纵向排布的行游标：从 `origin` 开始,每次 [`RowCursor::push`]
/// 产出一个新 [`Label`] 并把内部纵坐标下移**这一行实际渲染出的行数**
/// 乘 `row_height`。
///
/// 生命期参数来自那个测量器：整形要写 `cosmic-text` 的字形缓存，因此
/// 是 `&mut`（不是本类型的设计取舍，见 [`ll_text::MeasureText`]）。
pub struct RowCursor<'m> {
    x: f32,
    cursor_y: f32,
    row_height: f32,
    font_size: f32,
    wrap_width: f32,
    measure: &'m mut dyn MeasureText,
}

impl<'m> RowCursor<'m> {
    /// 从 `origin`（左上角）开始,行高 `row_height` 像素、字号
    /// `font_size`、断行宽度 `wrap_width`。
    ///
    /// `wrap_width` 应当是**这块面板的内容宽**（面板宽减去两侧内边距），
    /// 不是面板宽本身——传面板宽会让最后几个字压在边框上。
    pub fn new(
        measure: &'m mut dyn MeasureText,
        origin: (f32, f32),
        row_height: f32,
        font_size: f32,
        wrap_width: f32,
    ) -> RowCursor<'m> {
        RowCursor {
            x: origin.0,
            cursor_y: origin.1,
            row_height,
            font_size,
            wrap_width,
            measure,
        }
    }

    /// 追加一行文字到 `labels`,并把内部游标下移**这一行真正占的行数**
    /// ——四个 HUD 面板模块的「产出全部文本行」函数都是反复调用这一个
    /// 方法拼出来的。
    ///
    /// 需要逐行几何的调用方（模态屏的行矩形）从 [`RowCursor::cursor_y`]
    /// 的前后差读这一行占了多高——本方法因此不另外返回行数：多一个
    /// 返回值就多一个可以与游标本身对不上的真相源。
    pub fn push(&mut self, labels: &mut Vec<Label>, text: String) {
        let metrics =
            self.measure
                .measure_text(&text, self.font_size, self.row_height, self.wrap_width);
        // 规格 L0：**提交出去的那一份**取整，内部游标不取整。
        //
        // 顺序要紧：`cursor_y` 保持精确的累计值，只有写进 `Label` 的
        // 那一刻 `round()` 一次。反过来（取整内部游标）会让每一行的
        // 取整误差逐行累加——十行之后整块面板的底边就会与
        // `cursor_y()` 现算出来的面板高度对不上。
        labels.push(Label {
            text,
            x: self.x.round(),
            y: self.cursor_y.round(),
            max_width: self.wrap_width,
        });
        self.cursor_y += metrics.line_count as f32 * self.row_height;
    }

    /// 当前已经推进到的纵坐标——供调用方在写完全部行之后测量面板
    /// 实际占用的高度（例如拿去建 [`crate::widget::panel::panel_quads`]
    /// 的背景矩形高度）。
    pub fn cursor_y(&self) -> f32 {
        self.cursor_y
    }

    /// 把**一行里的若干格**从左往右摆开：每一格量出自己的自然宽度，
    /// 下一格接着往右挪 `gap` 像素，游标只推进**这一行**。
    ///
    /// # 为什么住在这里，不住在状态栏那个模块里
    ///
    /// 规格 W6 要求状态栏「拆成字段，不再拼成一整行」——横排是那一条
    /// 的落点。它要的四样（起点 `x`、当前 `cursor_y`、字号、测量器）
    /// 正是 `RowCursor` 已经持有的；写在外面就要把测量器再传一遍，而
    /// [`RowCursor::push`] 与本方法对「这一行推进多少」必须给出同一个
    /// 答案，两处实现迟早分叉。
    ///
    /// # 每一格的断行宽是「到内容区右边界还剩多少」
    ///
    /// 不是整块面板的内容宽：那样最后一格会以为自己还有整行可用，
    /// 一路画到面板外面去。取剩余量之后，真的排不下的那一格会在**它
    /// 自己身上**换行——难看，但看得见，而不是静悄悄溢出。
    /// 纵向推进因此按**全部格子里最高的那一个**算（与
    /// [`RowCursor::push`] 按实际渲染行数推进是同一条纪律，规格 W2）。
    ///
    /// `fields` 为空时什么都不做、游标也不动——没有内容就不占高度，
    /// 与「没有就不画，不留占位」那条一贯纪律一致。
    pub fn push_fields(&mut self, labels: &mut Vec<Label>, fields: &[String], gap: f32) {
        if fields.is_empty() {
            return;
        }
        let 内容右 = self.x + self.wrap_width;
        let mut x = self.x;
        let mut 最多行数 = 1_usize;
        for text in fields {
            let 剩余 = (内容右 - x).max(0.0);
            let metrics = self
                .measure
                .measure_text(text, self.font_size, self.row_height, 剩余);
            最多行数 = 最多行数.max(metrics.line_count);
            // 规格 L0：提交出去的那一份取整，内部推进保持精确。
            labels.push(Label {
                text: text.clone(),
                x: x.round(),
                y: self.cursor_y.round(),
                max_width: 剩余,
            });
            x += metrics.max_line_width + gap;
        }
        self.cursor_y += 最多行数 as f32 * self.row_height;
    }

    /// 这个游标每一行的左边界——即所在面板的**内容区左边界**。
    ///
    /// 需要逐行**矩形**（而不只是文字）的调用方要它：一行的矩形横向
    /// 占满内容区，纵向是这一行推进前后的那一段（见
    /// [`crate::hud::action_menu::action_menu_content`] 与
    /// `crate::screen` 的 `layout_screen`）。
    pub fn x(&self) -> f32 {
        self.x
    }

    /// 这个游标写出的每一行的断行宽度——即所在面板的内容宽。
    pub fn wrap_width(&self) -> f32 {
        self.wrap_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_text::TextMeasurer;

    /// 一个「所有文字都恰好占一行」的测量器——本模块几条只关心坐标
    /// 推进的测试用它，避免把断言绑死在具体字体度量上。
    struct 单行测量器;

    impl MeasureText for 单行测量器 {
        fn measure_text(
            &mut self,
            _text: &str,
            _font_size: f32,
            _line_height: f32,
            _max_width: f32,
        ) -> ll_text::TextMetrics {
            ll_text::TextMetrics {
                line_count: 1,
                max_line_width: 0.0,
            }
        }
    }

    #[test]
    fn push产出的每一行y坐标按行高递增() {
        // Arrange
        let mut measure = 单行测量器;
        let mut cursor = RowCursor::new(&mut measure, (10.0, 20.0), 16.0, 14.0, 400.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());

        // Assert
        assert_eq!(labels[1].y - labels[0].y, 16.0);
    }

    #[test]
    fn push产出的每一行x坐标恒等于起点x() {
        // Arrange
        let mut measure = 单行测量器;
        let mut cursor = RowCursor::new(&mut measure, (10.0, 20.0), 16.0, 14.0, 400.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());

        // Assert
        assert_eq!(labels[0].x, 10.0);
        assert_eq!(labels[1].x, 10.0);
    }

    #[test]
    fn cursor_y反映已经推进的行数() {
        // Arrange
        let mut measure = 单行测量器;
        let mut cursor = RowCursor::new(&mut measure, (0.0, 0.0), 16.0, 14.0, 400.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());
        cursor.push(&mut labels, "row-b".to_string());
        cursor.push(&mut labels, "row-c".to_string());

        // Assert
        assert_eq!(cursor.cursor_y(), 48.0);
    }

    #[test]
    fn 内部游标按精确值累计只有写进标签的那一刻才取整() {
        // 规格 L0 在本模块的那一半：`push` 写 `Label` 时 `round()` 一次，
        // **内部 `cursor_y` 不取整**，否则每行不到一像素的取整误差会
        // 逐行累加，十行之后面板背景的底边就与 `cursor_y()` 现算出来的
        // 高度对不上。
        //
        // # 这条断言是补上来的，补的原因如实记在这里
        //
        // 落地时先写的反例是「把 `push` 改成取整内部 `cursor_y`」，
        // 实跑**它不红**——因为全项目今天的行高（`hud::DEFAULT_LINE_HEIGHT`
        // 与 `screen::SCREEN_LINE_HEIGHT` 都是 18.0）都是整数，取整一个
        // 本来就是整数的累加器是空操作，既有那几条测试用的也全是整数
        // 行高。也就是说**那条不变式当时根本没有任何测试咬着**。
        // 按 ADR 0022，不粉饰、也不把断言改宽，改成用**分数行高**
        // 把它真正逼出来。
        //
        // 反例验证（已实跑）：把 `push` 末尾改成
        // `self.cursor_y = (self.cursor_y + …).round();`，本条红在
        // 「四行之后游标应当是 70，实际 72」——两像素正是四次取整
        // 累加出来的漂移。
        // Arrange：17.5 的行高，四行的精确终点是 70.0。
        let mut measure = 单行测量器;
        let mut cursor = RowCursor::new(&mut measure, (0.0, 0.0), 17.5, 14.0, 400.0);
        let mut labels = Vec::new();

        // Act
        for n in 0..4 {
            cursor.push(&mut labels, format!("第{n}行"));
        }

        // Assert 一：游标是精确累计值，一点都没漂。
        assert_eq!(
            cursor.cursor_y(),
            70.0,
            "四行 × 17.5 的精确终点是 70，游标漂了"
        );

        // Assert 二：写出去的每一行原点都是整数（这才是提交那一刻要的）。
        for label in &labels {
            assert_eq!(label.y.fract(), 0.0, "标签原点 {} 没取整", label.y);
        }

        // Assert 三：取整是**就地**取整，不是逐行累加取整——第三行的
        // 精确值 35.0 取整还是 35，而逐行累加的版本会给出 36。
        assert_eq!(labels[2].y, 35.0);
    }

    #[test]
    fn 每一行都带着这个游标的断行宽度() {
        // Arrange
        let mut measure = 单行测量器;
        let mut cursor = RowCursor::new(&mut measure, (0.0, 0.0), 16.0, 14.0, 208.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, "row-a".to_string());

        // Assert
        assert_eq!(labels[0].max_width, 208.0);
    }

    #[test]
    fn 换行的一条按渲染出的两行推进而不是按一条标签推进() {
        // 规格 W2 的核心断言。**用真实字体度量与真实长度的文案**——
        // 空串或占位符会让 `line_count` 恒为 1，断言就永远绿（本会话
        // 上一批在「语言」行上抓到过这种假绿）。
        //
        // 反例验证（已实跑）：把 `push` 里的
        // `metrics.line_count as f32 * self.row_height` 改回
        // `self.row_height`，本条立刻变红。
        // Arrange
        let mut measure = TextMeasurer::new().expect("内置字体资产应能正常解析");
        let 长句 = "An inland world that barely sees the sea: roughly 16% water, farming and herding in place of fishing.";
        // 先自证这段文案在 200px 内确实断成不止一行——否则下面那条
        // 断言测的就不是「按渲染行数推进」。
        let 行数 = measure.measure_text(长句, 14.0, 18.0, 200.0).line_count;
        assert!(行数 > 1, "测试文案必须真的会换行，实测 {行数} 行");

        let mut cursor = RowCursor::new(&mut measure, (0.0, 0.0), 18.0, 14.0, 200.0);
        let mut labels = Vec::new();

        // Act
        cursor.push(&mut labels, 长句.to_string());
        cursor.push(&mut labels, "第二条".to_string());

        // Assert：第二条必须落在第一条的**全部**渲染行之下。
        assert_eq!(labels[1].y, 行数 as f32 * 18.0);
    }
}
