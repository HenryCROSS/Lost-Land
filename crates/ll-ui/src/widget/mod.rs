//! HUD 需要的最小控件集合：面板（九宫格边框）、文本行、列表、条形、
//! 布局，以及 UI 交互层批次新增的命中测试、按钮、焦点导航。
//!
//! # 不是通用控件库,是把这几样做对
//!
//! 项目所有者裁定（见任务书「方向要改」一节）：不造 P7 完整的像素 UI
//! 控件库，但状态栏/角色面板/背包/装备栏这四块屏幕真正需要的控件必须
//! 做扎实，不能是「把字符串画在固定位置」的临时手法——那类代码会变成
//! 后续批次难以迁移的承重墙（`assets/atlas/` 双份美术、`TurnEngine`
//! 长期滞留在 demo 里不并入生产，都是这类「先凑合」代码固化的先例）。
//!
//! 本模块因此提供以下正式控件，各自独立、可单测、可组合：
//!
//! - [`geometry::Rect`]——布局：矩形 + 堆叠/收缩，不是约束求解器；
//!   [`geometry::Rect::contains`] 是命中测试与按钮悬停判定的唯一几何
//!   判据。
//! - [`panel::panel_quads`]——面板：九宫格边框分解，见其模块文档
//!   「没有边框美术，先用纯色」一节。**外观数据从哪来**：控件本身
//!   不决定颜色，见下方「皮肤层」一节。
//! - [`label::Label`]——文本行：内容 + 位置，可转成 [`ll_text::TextRun`]。
//! - [`list::RowCursor`]——列表：纵向堆叠一组 [`label::Label`]。
//! - [`bar::bar_quads`]——条形：背景 + 按比例填充的前景，外观数据同样
//!   经皮肤层解析。
//! - [`hit_test::hit_test`]——命中测试：这一帧光标下方是哪个控件，
//!   见其模块文档「即时模式下怎么做」一节。
//! - [`button::update_button`]——按钮：普通/悬停/按下/禁用四态，鼠标
//!   点击与键盘/手柄确认键都能触发，见其模块文档。
//! - [`focus::move_focus`]/[`focus::navigate_focus`]——焦点导航：不用
//!   鼠标也能在一组控件间移动焦点、触发当前聚焦项，见其模块文档
//!   「为什么不能只支持鼠标」一节。
//! - [`marker::textured_marker_quad`]——世界地图标记：外观数据同样经
//!   皮肤层解析，见其模块文档「为什么单独一个模块」一节。
//! - [`layer::UiLayer`]——UI 层级：显式声明每块界面画在哪一层，由
//!   层级而不是调用顺序或渲染 pass 的先后决定谁盖住谁，见其模块文档
//!   「新加一块 UI 时该怎么选层」一节。
//! - [`ui_mode::UiModeStack`]——模态 UI 栈：驱动
//!   `ll_platform::keybind::InputContext` 在 `Gameplay`/`Menu` 之间
//!   切换，并保证每次切换都清空 `InputState`，见其模块文档。
//!
//! [`quad::QuadRenderer`] 是 [`panel`]/[`bar`] 共用的 GPU 图元（面板与
//! 条形产出的都是 [`quad::QuadInstance`]），不属于「控件」本身，是
//! 控件与屏幕之间的最后一层。
//!
//! # 皮肤层：控件问皮肤要外观，不是自己决定长什么样
//!
//! 见 [`skin`] 模块文档的完整论证与两条验收问题的回答（换皮肤要改
//! 几处、加九宫格要改几处）。一句话概括：[`panel::panel_quads`]/
//! [`bar::bar_quads`] 只认一份「已经算好的外观数据」
//! （[`panel::FlatPanelAppearance`]/[`bar::FlatBarAppearance`]），四块
//! HUD 面板与 [`crate::hud::render`] 只认语义化的样式名
//! （[`skin::PanelStyleId`]/[`skin::BarStyleId`]），两者之间由实现了
//! [`skin::Skin`] trait 的对象（目前唯一实现是 [`skin::FlatColorSkin`]）
//! 接起来——换一套视觉风格只需要换 `Skin` 的具体实现，控件与调用点
//! 代码都不用动。
//!
//! # 即时模式，不是保留模式——核实过什么，为什么选它
//!
//! 见 [`quad`] 模块文档「即时模式：每帧全量重建」一节的完整论证；核实
//! 结论是：`ll_render::batch::SpriteBatch::flush` 每帧末尾
//! `pending.clear()`，`ll_text::TextRenderer::render` 每次调用都从头
//! `CosmicBuffer::new` 重新排版——两条既有渲染通道都是「调用方每帧
//! 重新声明全部要画的内容，渲染器自己不持有跨帧场景状态」，这个核实
//! 结果支持而不是推翻项目所有者的倾向：本模块的五个控件全部是纯函数
//! （`Rect`/`panel_quads`/`bar_quads`/`RowCursor::push` 都不跨帧保留
//! 任何状态，每帧由 [`crate::hud::render::render_hud`] 重新调用一遍），
//! 与底层管线的模型一致，不需要另建一套控件树 + 失效/差分逻辑。
//!
//! # 布局的真实代价：子元素多大，画它之前不知道——本批次怎么解的
//!
//! 即时模式下「让面板刚好包住内容」天然是个先有鸡还是先有蛋的问题：
//! 背景矩形要多大取决于里面有多少行文字，但文字要画在哪要先知道背景
//! 矩形的位置。三种通用解法（两遍走先测量后摆放、调用方显式给尺寸、
//! 记住上一帧量出的尺寸）里，本批次选的是**宽度显式给定 + 单遍流式
//! 高度**，理由是本批次四块面板的布局形状（固定宽度的纵向列表）恰好
//! 不需要真正的两遍：
//!
//! - **宽度**——[`crate::hud::render`] 里的 `CHARACTER_WIDTH`/
//!   `INVENTORY_WIDTH`/`EQUIPMENT_WIDTH`/`STATUS_WIDTH` 是调用方显式
//!   给定的常量，从不由内容反推。
//! - **高度**——[`crate::hud::build_panel`] 内部用 [`list::RowCursor`]
//!   从一个**固定**的左上角原点（`origin + padding`）开始逐行下推，
//!   每一行的最终坐标只依赖「原点 + 已经画了几行 × 行高」，与「这块
//!   面板总共有多高」无关；因此可以先把全部行「画」出来（`RowCursor`
//!   现场推进 `cursor_y`），最后用推进到的终点反推出面板背景矩形的
//!   高度——这不是真正的「先量后摆」两遍扫描（每一行在产出的同时就
//!   已经带着最终坐标了，不需要事后平移），是本批次布局形状（从固定
//!   原点向下流式排列）本身不需要提前知道总高度的直接结果。
//!
//! 这条解法不能推广到「子元素尺寸会影响父元素/兄弟元素怎么摆放」的
//! 场景（例如水平方向按内容自适应宽度、或多个子面板互相挤占对方位置）
//! ——那类布局真的需要两遍扫描或记住上一帧尺寸（项目所有者点名的
//! egui 模型）,不在本批次范围内,届时需要给 [`list::RowCursor`]/
//! [`crate::hud::build_panel`] 新增能力,而不是推翻现有形状（两者的
//! 现有调用点不依赖「布局引擎只能是单遍流式」这个假设,只是恰好这批
//! 面板不需要更复杂的能力）。
//!
//! # 将来加焦点/选中，持久状态放哪——不进 `WorldState`
//!
//! 本批次没有任何交互（不能点、不能选），但即时模式恰好让「以后加」
//! 这件事成本很低，不需要推翻重来。项目所有者点名的模型（egui 一类
//! 即时模式框架的既有实践）是：**持久状态（焦点、滚动位置、动画进度、
//! 选中项）放一张按 widget id 索引的旁表**，这张表本身活在 UI 层
//! （未来会是 `ll-game::app::Demo` 的一个字段，与它已有的
//! `zoom`/`camera`/`anim` 等运行期渲染状态同一层），结构上不可能
//! 污染 `WorldState`——这是 [ADR
//! 0020](../../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
//! 「甲区（渲染/脚本层浮点）与乙区（世界状态整数）的边界」同一条纪律
//! 在「UI 状态该放哪」这个新问题上的直接应用，不是本模块另外发明的
//! 规则：
//!
//! - 焦点/选中状态是调用方（未来的输入处理层，与
//!   `ll_platform::input::InputState` 同一层，本 crate 不依赖
//!   `ll-platform`，此处只是文字引用，不是可解析的文档内链）持有的
//!   一张旁表（例如 `HashMap<WidgetId, FocusState>` 或更简单的「当前
//!   聚焦哪个面板/第几行」的一两个字段），每帧读取后作为参数传给
//!   [`crate::hud::inventory_panel::inventory_panel_lines`] 一类函数——
//!   这些函数已经是「读输入数据，产出这一帧的显示内容」的纯函数形状，
//!   加一个 `focused_index: Option<usize>` 参数、在对应行选一个不同的
//!   `panel_quads`/`Label` 外观（经皮肤层的新样式名），是局部改动。
//! - 不需要给每个控件发一个持久 ID 再维护一棵「上一帧的控件树」去和
//!   「这一帧的控件树」做差分——即时模式下「这一帧点的是哪一行」直接
//!   由「这一帧算出的第几个 `Rect` 包含点击坐标」现算得出，不依赖任何
//!   跨帧记忆；旁表存的只是「哪一项当前被选中/聚焦」这个语义状态本身，
//!   不是控件树的镜像。
//! - 唯一需要新增的持久状态就是这张旁表（类似 `Demo` 已有的
//!   `zoom`/`camera` 字段，量级也相当），不需要新增控件生命周期管理
//!   代码，也不需要 `WorldState` 知道任何 UI 交互状态的存在。

pub mod anim;
pub mod bar;
pub mod button;
pub mod day_night_bar;
pub mod focus;
pub mod geometry;
pub mod highlight;
pub mod hit_test;
pub mod label;
pub mod layer;
pub mod list;
pub mod marker;
pub mod metrics;
pub mod panel;
pub mod quad;
pub mod skin;
pub mod state;
pub mod textured_quad;
pub mod ui_mode;
pub mod zone;
