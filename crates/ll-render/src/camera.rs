//! 相机：世界坐标到离屏渲染目标像素坐标的换算。

use crate::sprite::TILE_SIZE;
use crate::target::{LOGICAL_HEIGHT, LOGICAL_WIDTH};
use ll_core::bounded::{BoundedPos, BoundedSize};
use ll_core::torus::{TorusPos, TorusSize};

/// 大陆世界地图相机：固定跟随一个环面世界坐标，把其余坐标换算成
/// 离屏渲染目标（640×360，见 [`crate::target`]）上的像素位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    /// 相机注视的环面世界坐标，恒显示在视口正中。
    pub center: TorusPos,
    /// 相机所在的环面世界尺寸，坐标换算离不开它才能定义。
    pub world: TorusSize,
}

impl Camera {
    /// 把环面世界坐标换算成屏幕像素坐标。
    ///
    /// # 为什么不需要画多份拷贝
    ///
    /// 环面世界里，相机附近的目标可能「绕过接缝」才最近。常见做法是把整个
    /// 场景画 2~4 份偏移拷贝以覆盖接缝，那既慢又容易出边界错误。
    ///
    /// 这里直接用 [`TorusSize::delta`] 求出目标相对相机的**最短带符号
    /// 位移**——绕接缝的情形已经被它处理掉了，每个目标仍然只画一次。
    /// 这是 P0 把距离与位移做成 `TorusSize` 的方法而非坐标的方法所带来的
    /// 直接回报。
    pub fn world_to_screen(&self, pos: TorusPos) -> (i32, i32) {
        let (dx, dy) = self.world.delta(self.center, pos);
        (
            offset_screen(LOGICAL_WIDTH as i32 / 2, dx),
            offset_screen(LOGICAL_HEIGHT as i32 / 2, dy),
        )
    }

    /// 列出当前视口范围内应绘制的全部瓦片坐标。
    ///
    /// 以相机为中心，向四周各取「半个视口 + 1 格」的范围，用
    /// [`TorusSize::wrap`] 归一化后返回。多取的一格是为了让边缘的瓦片
    /// 不会在相机移动时突然出现——视口边界上被半格裁切的瓦片仍需完整
    /// 绘制，否则相机每移动一像素，边缘就会有瓦片凭空冒出或消失。
    ///
    /// **世界跨度小于 43×25 格时会产出重复坐标、地形填不满**：横向
    /// 偏移覆盖 `[-21, 21]`（43 个值，由 `LOGICAL_WIDTH / TILE_SIZE / 2 + 1`
    /// 算出）、纵向覆盖 `[-12, 12]`（25 个值）。世界某一维小于对应跨度
    /// 时，不同偏移会 `wrap` 到同一个 [`TorusPos`]——但调用方后续多半会
    /// 用 [`Self::world_to_screen`] 把它换算回屏幕坐标，而那一步走的是
    /// `TorusSize::delta` 算出的最短路径，与生成这个坐标时用的原始偏移
    /// 无关。结果是同一张世界瓦片只会被画在（这个最短路径决定的）唯一
    /// 一处屏幕位置，而不是视觉上小世界环绕重复应该出现的每一处——
    /// 那些位置就会留空。调用方需要保证世界尺寸不小于这个跨度（本 crate
    /// 的验收 demo 用 48×32 的世界规避了这一点）。
    pub fn visible_tiles(&self) -> Vec<TorusPos> {
        let half_tiles_x = (LOGICAL_WIDTH / TILE_SIZE / 2) as i32 + 1;
        let half_tiles_y = (LOGICAL_HEIGHT / TILE_SIZE / 2) as i32 + 1;

        let mut tiles =
            Vec::with_capacity((2 * half_tiles_x as usize + 1) * (2 * half_tiles_y as usize + 1));
        for offset_y in -half_tiles_y..=half_tiles_y {
            for offset_x in -half_tiles_x..=half_tiles_x {
                tiles.push(
                    self.world
                        .wrap(self.center.x() + offset_x, self.center.y() + offset_y),
                );
            }
        }
        tiles
    }

    /// [`Self::visible_tiles`] 的缩放感知版本：`zoom` 决定枚举范围
    /// （见 [`visible_half_extent`]），`zoom = 1.0` 时两者产出逐位相同
    /// 的结果——这是保持向后兼容的显式测试点，见本方法的测试。
    ///
    /// 调用方还需要对每个坐标算出的屏幕位置调用 [`apply_zoom`]，两者
    /// 是同一套几何变换缺一不可的两半，见 [`Zoom`] 文档。
    pub fn visible_tiles_zoomed(&self, zoom: Zoom) -> Vec<TorusPos> {
        let half_tiles_x = visible_half_extent(LOGICAL_WIDTH, zoom);
        let half_tiles_y = visible_half_extent(LOGICAL_HEIGHT, zoom);

        let mut tiles =
            Vec::with_capacity((2 * half_tiles_x as usize + 1) * (2 * half_tiles_y as usize + 1));
        for offset_y in -half_tiles_y..=half_tiles_y {
            for offset_x in -half_tiles_x..=half_tiles_x {
                tiles.push(
                    self.world
                        .wrap(self.center.x() + offset_x, self.center.y() + offset_y),
                );
            }
        }
        tiles
    }
}

/// `Interior`（地下城/建筑内部楼层）用的相机变体。
///
/// 与 [`Camera`] 平行但不环绕。`Surface` 场景（本身就是环面世界的一
/// 部分）不需要改一行——`world_to_screen`/`visible_tiles` 的屏幕换算
/// 算法本身与拓扑无关，这里只是把坐标运算换成
/// [`BoundedSize`]/[`BoundedPos`] 的非环绕版本，供 `Interior` 使用。见
/// `knowledge/design/coordinate-system-and-layers.md` 六节「`ll-render`
/// 是否需要改动」一节的核实结论：「不是要重写屏幕换算算法本身，而是
/// 需要给 `Camera` 提供一个基于有界坐标的等价『世界』上下文」。
///
/// # 为什么不与 [`Camera`] 共享一个 trait（对照 `ll_world::fov::SightGrid`）
///
/// `Camera`/`BoundedCamera` 与 `SightGrid` 表面上是同一类问题——都要
/// 同时服务环面与有界两种坐标系——但 `SightGrid` 抽了 trait，这里
/// 没有，且这不是风格取舍。`compute_fov` 抽 trait 是因为阴影投射的
/// 扫描逻辑本身要被两种网格**共用**（不抽会复制一整套热路径算法）；
/// `Camera`/`BoundedCamera` 的方法集本就不同（环面要处理绕接缝，有界
/// 要处理边界钳制/留白），也从未被同一份泛型代码统一调用过——没有
/// 算法要共用，抽 trait 只会插入一层没有实际内容的接口。判据见
/// `knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md`：
/// 抽象的理由是「有算法要共用」，不是「看起来该对称」。若未来两种
/// 相机确实长出共享逻辑，那时候抽出 trait 或共享函数才是对的时机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedCamera {
    /// 相机注视的有界局部坐标，恒显示在视口正中。
    pub center: BoundedPos,
    /// 相机所在的有界局部地图尺寸，坐标换算离不开它才能定义。
    pub world: BoundedSize,
}

impl BoundedCamera {
    /// 把有界局部坐标换算成屏幕像素坐标。
    ///
    /// 换算规则与 [`Camera::world_to_screen`] 完全一致（相机为中心，
    /// 位移乘瓦片边长）。[`Camera`] 用 [`TorusSize::delta`] 求「最短
    /// 带符号位移」是为了处理绕接缝；有界地图没有接缝，
    /// [`BoundedSize::delta`] 本来就是两点的原始差值，不需要额外的
    /// 「哪条路更近」判断。
    pub fn world_to_screen(&self, pos: BoundedPos) -> (i32, i32) {
        let (dx, dy) = self.world.delta(self.center, pos);
        (
            offset_screen(LOGICAL_WIDTH as i32 / 2, dx),
            offset_screen(LOGICAL_HEIGHT as i32 / 2, dy),
        )
    }

    /// 列出当前视口范围内应绘制的全部有界局部坐标。
    ///
    /// 与 [`Camera::visible_tiles`] 同样以相机为中心向四周各取「半个
    /// 视口 + 1 格」的范围，但越界时用 [`BoundedSize::try_pos`] 拒绝
    /// 而非 [`TorusSize::wrap`] 绕回——**这正是「有界相机在世界边缘
    /// 不产出环绕坐标」的落点**：走到 `Interior` 楼层边缘时，视口里
    /// 贴着边缘那一侧自然留白（没有对应的瓦片可画），而不是像环面
    /// 相机那样在边缘冒出地图另一侧的内容。返回的坐标数量因此在贴近
    /// 边缘时会少于视口能容纳的全部格数，调用方（渲染循环）需要按
    /// 「有则画，无则空」处理，不能假设恒定长度——这与
    /// [`Camera::visible_tiles`] 文档警告的「世界小于视口跨度时地形
    /// 填不满」是同一类现象在有界地图上的正常表现，不是缺陷。
    pub fn visible_tiles(&self) -> Vec<BoundedPos> {
        let half_tiles_x = (LOGICAL_WIDTH / TILE_SIZE / 2) as i32 + 1;
        let half_tiles_y = (LOGICAL_HEIGHT / TILE_SIZE / 2) as i32 + 1;

        let mut tiles = Vec::new();
        for offset_y in -half_tiles_y..=half_tiles_y {
            for offset_x in -half_tiles_x..=half_tiles_x {
                if let Some(pos) = self
                    .world
                    .try_pos(self.center.x() + offset_x, self.center.y() + offset_y)
                {
                    tiles.push(pos);
                }
            }
        }
        tiles
    }

    /// [`Self::visible_tiles`] 的缩放感知版本，与
    /// [`Camera::visible_tiles_zoomed`] 同一套公式，唯一差异是越界时
    /// 用 [`BoundedSize::try_pos`] 跳过而非环绕，理由见
    /// [`Self::visible_tiles`] 文档。
    pub fn visible_tiles_zoomed(&self, zoom: Zoom) -> Vec<BoundedPos> {
        let half_tiles_x = visible_half_extent(LOGICAL_WIDTH, zoom);
        let half_tiles_y = visible_half_extent(LOGICAL_HEIGHT, zoom);

        let mut tiles = Vec::new();
        for offset_y in -half_tiles_y..=half_tiles_y {
            for offset_x in -half_tiles_x..=half_tiles_x {
                if let Some(pos) = self
                    .world
                    .try_pos(self.center.x() + offset_x, self.center.y() + offset_y)
                {
                    tiles.push(pos);
                }
            }
        }
        tiles
    }
}

/// 相机缩放倍率：围绕视口中心，把 [`Camera::world_to_screen`]/
/// [`BoundedCamera::world_to_screen`] 算出的离屏目标像素坐标再做一次
/// 连续（非整数档）缩放。
///
/// # 为什么是围绕视口自由函数、不是 `Camera` 的字段
///
/// 缩放不改变相机看向哪个世界坐标（`center`不变），只改变「同一批
/// 屏幕坐标该显示得多大」——这是一个纯粹的后处理步骤，与相机本身
/// 「世界坐标换算成屏幕坐标」这条职责正交。把它做成独立的
/// [`apply_zoom`] 与 [`visible_half_extent`] 自由函数，而不是给
/// `Camera`/`BoundedCamera` 加字段，有两个理由：
///
/// - `Camera`/`BoundedCamera` 目前是 `PartialEq, Eq`（整数字段），一旦
///   混入浮点字段就必须降级成只有 `PartialEq`——这个改动本身没有必要，
///   现有 24 个测试断言的相机相等性/换算逻辑完全不需要知道缩放的存在。
/// - 调用方（[`crate::target`] 的离屏画布之外，真正持有「当前缩放是
///   多少」这份状态的其实是上层游戏循环——见
///   `crates/ll-game/src/app.rs` 的 `Demo::zoom`）可以把缩放当成一个
///   独立于相机中心的输入，不需要每次调整缩放都重新构造一个 `Camera`。
///
/// # ADR 0020 甲区：绝不流回世界状态
///
/// 缩放的结果只变成像素——它只影响这一帧画在离屏渲染目标上的位置与
/// 尺寸，从不参与任何游戏逻辑判断（命中、寻路、FOV 半径都不读取它），
/// 因此可以放心用 `f32`，不需要走 `Milli` 或任何整数近似（[ADR
/// 0020](../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
/// 甲区表格第一行原文就是「渲染、相机、动画插值、粒子」）。
///
/// # 常驻区块集合完全解耦
///
/// 缩放只改变 [`Camera::visible_tiles_zoomed`]/
/// [`BoundedCamera::visible_tiles_zoomed`] 这两个**渲染剔除范围**的
/// 计算——拉远（`zoom` 变小）时这两个方法会枚举更多瓦片坐标，但这只是
/// 「本帧打算尝试画哪些坐标」的列表，不触碰、也不知道
/// `SurfaceStore::stream_neighborhood` 维护的**常驻区块集合**（那由
/// 玩家所在区块 + 固定半径决定，定义在 `crates/ll-game/src/world.rs`
/// 的 `STREAM_RADIUS_ZONES`，与本模块完全无关，`ll-render` 从未、也不
/// 应该认识 `SurfaceStore`/`ZoneLayout` 这些类型）。真正的安全前提是
/// 「拉远后枚举的坐标范围不能超出常驻区块集合的覆盖半径，否则
/// `SurfaceWindow::terrain_at` 会因未常驻而 panic」——这条前提由调用方
/// （`ll-game`）负责证明并强制执行一个更窄的安全缩放区间（见
/// `crates/ll-game/src/world.rs` 的 `MIN_SAFE_ZOOM` 文档），本模块只
/// 提供通用、与任何具体世界的常驻区块策略无关的宽泛上下限
/// （[`Zoom::MIN`]/[`Zoom::MAX`]），不代为判断某个具体世界「安全」。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom(f32);

impl Zoom {
    /// 通用下限：只防止零、负数、极端病态值，不是任何具体世界常驻区块
    /// 覆盖范围推导出的「安全」下限——那个更窄的界限属于调用方，见本
    /// 类型文档「常驻区块集合完全解耦」一节。
    pub const MIN: f32 = 0.1;
    /// 通用上限：拉近到原始像素的 8 倍，超过这个倍率单个瓦片会占据
    /// 离屏画布的很大一部分，实用价值有限。
    pub const MAX: f32 = 8.0;
    /// 恒等缩放——与「完全不缩放」的既有行为逐位一致，默认值。
    pub const IDENTITY: Zoom = Zoom(1.0);

    /// 构造一个缩放倍率，钳制到 [`Self::MIN`]..=[`Self::MAX`]。
    ///
    /// 钳制而非拒绝：缩放是连续调节的用户输入（滚轮/按键，见
    /// `ll-platform` 的 `GameKey::ZoomIn`/`ZoomOut`），每次一小步调整，
    /// 越界时钉在边界上是符合直觉的行为（「已经拉到最远了」），返回
    /// `Result` 反而会强迫每个调用点处理一个几乎不可能有意义处理方式
    /// 的错误分支。
    pub fn new(value: f32) -> Zoom {
        Zoom(value.clamp(Self::MIN, Self::MAX))
    }

    /// 取出内部倍率。
    pub fn get(self) -> f32 {
        self.0
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom::IDENTITY
    }
}

/// 把一个未缩放的离屏目标像素坐标（例如 [`Camera::world_to_screen`]
/// 或 [`crate::sprite::sprite_draw_position`] 的返回值），按 `zoom`
/// 围绕视口中心缩放。
///
/// 数学上等价于「把整张离屏画布贴在一块可伸缩的布上，捏住画布正中央
/// 向外拉伸」：`zoom > 1` 时画面中心不动、四周的内容离中心更远（视觉
/// 上像拉近）；`zoom < 1` 时四周内容向中心收拢（视觉上像拉远）。这与
/// [`Camera::visible_tiles_zoomed`] 用同一个 `zoom` 扩大/收窄枚举范围
/// 是同一套几何变换的两个必须配套的部分——只放大枚举范围而不做这一步
/// 缩放，新枚举出的瓦片会画在离屏画布之外；只做这一步缩放而不扩大
/// 枚举范围，画面边缘会露出没有内容的黑边而不是更多地形。
pub fn apply_zoom(screen_pos: [f32; 2], zoom: Zoom) -> [f32; 2] {
    let center_x = LOGICAL_WIDTH as f32 / 2.0;
    let center_y = LOGICAL_HEIGHT as f32 / 2.0;
    [
        center_x + (screen_pos[0] - center_x) * zoom.get(),
        center_y + (screen_pos[1] - center_y) * zoom.get(),
    ]
}

/// 按 `zoom` 算出一个方向上的可见半径（瓦片数），供
/// [`Camera::visible_tiles_zoomed`]/[`BoundedCamera::visible_tiles_zoomed`]
/// 与调用方的安全区间校验共用——两处都需要同一个公式，任何一处各自
/// 重新推导都会有算出不一致范围的风险。
///
/// `zoom` 越小，同一块固定 `logical_extent` 像素的离屏画布能容纳的瓦片
/// 越多，半径必须相应增大；这正是「拉远看到更多区块」的算术落点——
/// 但它只决定「渲染时打算枚举多大范围」，与常驻区块集合的决定完全
/// 无关，见 [`Zoom`] 文档「常驻区块集合完全解耦」一节。
///
/// 返回值即 [`Camera::visible_tiles`] 文档里的 `half_tiles_x`/
/// `half_tiles_y`：`zoom = 1.0` 时与该函数写死的公式
/// （`LOGICAL_WIDTH / TILE_SIZE / 2 + 1`，两次**向下取整**的整数除法）
/// 逐位相同，这是保持向后兼容的显式测试点。
///
/// # 为什么用 `floor` 而不是 `ceil`
///
/// 数论恒等式 `floor(floor(a/b)/c) = floor(a/(b*c))`（对正整数
/// a、b、c 恒成立）保证了「先做两次整数除法再向下取整」与「一次性
/// 除以两者之积再向下取整」是同一个结果——`LOGICAL_HEIGHT`（360）
/// 不能被 `2 * TILE_SIZE`（32）整除，若这里用 `ceil` 会在 `zoom =
/// 1.0` 时算出比原公式多一圈的半径（`ceil(360.0/32.0) = 12` 而不是
/// `floor(360.0/32.0) + 1 = 11 + 1 = 12`——凑巧同一个数字，但推导
/// 路径不同；真正会分叉的是先除 `TILE_SIZE` 再除 `2` 与一次性除以
/// 两者之积在取整点上的差异，`ceil` 在这一步会引入一整格的偏差），
/// 破坏「向后兼容的显式测试点」这条承诺。用 `floor` 才能在
/// `zoom = 1.0` 时与既有整数公式逐位相同，同时对
/// `crate::world::MIN_SAFE_ZOOM` 一类的安全区间校验更保守（`floor`
/// 恒不大于 `ceil`，枚举范围只会更小，不会让常驻区块集合的覆盖余量
/// 计算冒进）。
pub fn visible_half_extent(logical_extent: u32, zoom: Zoom) -> i32 {
    let effective_tile_px = TILE_SIZE as f32 * zoom.get();
    (logical_extent as f32 / (2.0 * effective_tile_px)).floor() as i32 + 1
}

/// 把「视口中心 + 位移 × 瓦片边长」的结果收窄回 `i32`，不让它静默溢出。
///
/// `TorusSize` 允许的世界尺寸上限约 10.7 亿格（[`TorusSize::MAX_EXTENT`]），
/// `delta` 可能返回接近半个世界宽的位移；乘上 [`TILE_SIZE`] 后会远超
/// `i32::MAX`——debug 下 panic，release 下静默环绕成一个指向错误方向的
/// 屏幕坐标。实际大陆地图远小于这个上限，但 `torus` 模块自己的文档就
/// 点名要防这类静默错误，这里不能双标。
///
/// 中间计算全部走 `i64`（其值域足以容纳 `i32::MAX` 乘 `TILE_SIZE` 的
/// 结果，不会自己溢出），最终用 `clamp` 饱和收窄回 `i32`，而不是让它
/// 环绕：一个被钳制到屏幕范围之外的坐标仍然「指向正确的方向」（只是
/// 画面外），后续裁剪/剔除能正确处理；环绕后的坐标可能指向完全相反的
/// 方向，是更隐蔽也更危险的静默错误。
fn offset_screen(center: i32, offset_tiles: i32) -> i32 {
    let screen = center as i64 + offset_tiles as i64 * TILE_SIZE as i64;
    screen.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::torus::TorusSize;

    fn camera_at(x: i32, y: i32) -> Camera {
        let world = TorusSize::new(32, 32).expect("常量非零");
        Camera {
            center: world.wrap(x, y),
            world,
        }
    }

    #[test]
    fn 相机中心落在视口正中() {
        // Arrange
        let camera = camera_at(10, 10);

        // Act
        let (sx, sy) = camera.world_to_screen(camera.center);

        // Assert
        assert_eq!(
            (sx, sy),
            (LOGICAL_WIDTH as i32 / 2, LOGICAL_HEIGHT as i32 / 2)
        );
    }

    #[test]
    fn 相邻一格相差一个瓦片边长() {
        // Arrange
        let camera = camera_at(10, 10);
        let neighbour = camera.world.wrap(11, 10);

        // Act
        let (cx, _) = camera.world_to_screen(camera.center);
        let (nx, _) = camera.world_to_screen(neighbour);

        // Assert
        assert_eq!(nx - cx, TILE_SIZE as i32);
    }

    #[test]
    fn 跨接缝的目标按最短方向绘制而非绕远() {
        // 相机在 x=1、目标在 x=31，向西绕 2 格即到，不该被画到屏幕右侧
        // 30 格开外——那正是「小地图上明明相邻、画面上却在天边」的成因。
        // Arrange
        let camera = camera_at(1, 10);
        let target = camera.world.wrap(31, 10);

        // Act
        let (cx, _) = camera.world_to_screen(camera.center);
        let (tx, _) = camera.world_to_screen(target);

        // Assert
        assert_eq!(tx - cx, -2 * TILE_SIZE as i32);
    }

    #[test]
    fn 可见瓦片数量覆盖整个视口() {
        // 视口 640×360、瓦片 16×16，即 40×23 格（含边缘半格各多一列/行）。
        // Arrange
        let camera = camera_at(10, 10);

        // Act
        let tiles = camera.visible_tiles();

        // Assert
        assert!(
            tiles.len()
                >= (LOGICAL_WIDTH / TILE_SIZE) as usize * (LOGICAL_HEIGHT / TILE_SIZE) as usize
        );
    }

    #[test]
    fn 超大世界下的位移不会溢出而是被钳制在屏幕坐标范围内() {
        // 世界尺寸取允许的上限，相机与目标相距半个世界宽，delta 返回的
        // 位移乘以 TILE_SIZE 后远超 i32::MAX——这里断言的是「不 panic
        // 且被钳制」，而不是环绕成一个看似合法但方向错误的坐标。
        // Arrange
        let world = TorusSize::new(TorusSize::MAX_EXTENT, TorusSize::MAX_EXTENT)
            .expect("上限本身是合法尺寸");
        let camera = Camera {
            center: world.wrap(0, 0),
            world,
        };
        let target = world.wrap((TorusSize::MAX_EXTENT / 2) as i32, 0);

        // Act
        let (sx, _sy) = camera.world_to_screen(target);

        // Assert
        assert_eq!(sx, i32::MAX);
    }

    #[test]
    fn 有界相机的相机中心落在视口正中() {
        // 对照 Camera 版本的同名测试：换算规则完全一致，只是坐标系
        // 换成不环绕的 BoundedSize/BoundedPos。
        // Arrange
        let world = BoundedSize::new(32, 32).expect("32x32 是合法尺寸");
        let center = world.try_pos(10, 10).expect("10,10 在范围内");
        let camera = BoundedCamera { center, world };

        // Act
        let (sx, sy) = camera.world_to_screen(camera.center);

        // Assert
        assert_eq!(
            (sx, sy),
            (LOGICAL_WIDTH as i32 / 2, LOGICAL_HEIGHT as i32 / 2)
        );
    }

    #[test]
    fn 有界相机在世界边缘不产出环绕坐标() {
        // 视口半径（横 21、纵 12 格）远大于这张 8x8 的小地图：若像
        // 环面相机那样绕回，会有多个不同的偏移量 wrap 到同一个坐标，
        // 产出重复项；有界相机没有接缝可绕，每个偏移量要么落在地图
        // 内产出一个坐标，要么越界被跳过，返回的坐标不可能重复。
        // Arrange
        let world = BoundedSize::new(8, 8).expect("8x8 是合法尺寸");
        let center = world.try_pos(0, 0).expect("0,0 在范围内");
        let camera = BoundedCamera { center, world };

        // Act
        let tiles = camera.visible_tiles();
        let distinct: std::collections::HashSet<_> = tiles.iter().collect();

        // Assert
        assert_eq!(tiles.len(), distinct.len());
    }

    #[test]
    fn 恒等缩放的可见半径与未缩放公式逐位相同() {
        // 向后兼容的显式测试点：新增的缩放路径在 zoom=1.0 时不能算出
        // 与既有 visible_tiles 不同的范围。
        // Arrange & Act & Assert
        assert_eq!(
            visible_half_extent(LOGICAL_WIDTH, Zoom::IDENTITY),
            (LOGICAL_WIDTH / TILE_SIZE / 2) as i32 + 1
        );
    }

    #[test]
    fn 恒等缩放下缩放感知的可见瓦片与未缩放版本数量相同() {
        // Arrange
        let camera = camera_at(10, 10);

        // Act
        let unzoomed = camera.visible_tiles();
        let zoomed = camera.visible_tiles_zoomed(Zoom::IDENTITY);

        // Assert
        assert_eq!(unzoomed.len(), zoomed.len());
    }

    #[test]
    fn 拉远后可见瓦片数量增多() {
        // 「拉远看到更多区块」的直接验证：zoom 变小时,
        // visible_tiles_zoomed 枚举出的坐标数量必须比恒等缩放时更多。
        // Arrange
        let camera = camera_at(10, 10);
        let zoomed_out = Zoom::new(0.5);

        // Act
        let baseline = camera.visible_tiles_zoomed(Zoom::IDENTITY).len();
        let wider = camera.visible_tiles_zoomed(zoomed_out).len();

        // Assert
        assert!(wider > baseline);
    }

    #[test]
    fn 拉近后可见瓦片数量减少() {
        // Arrange
        let camera = camera_at(10, 10);
        let zoomed_in = Zoom::new(2.0);

        // Act
        let baseline = camera.visible_tiles_zoomed(Zoom::IDENTITY).len();
        let narrower = camera.visible_tiles_zoomed(zoomed_in).len();

        // Assert
        assert!(narrower < baseline);
    }

    #[test]
    fn 缩放倍率低于下限时被钳制() {
        // Arrange & Act
        let zoom = Zoom::new(Zoom::MIN - 1.0);

        // Assert
        assert_eq!(zoom.get(), Zoom::MIN);
    }

    #[test]
    fn 缩放倍率高于上限时被钳制() {
        // Arrange & Act
        let zoom = Zoom::new(Zoom::MAX + 1.0);

        // Assert
        assert_eq!(zoom.get(), Zoom::MAX);
    }

    #[test]
    fn 恒等缩放不改变屏幕坐标() {
        // Arrange
        let pos = [123.0, 45.0];

        // Act
        let zoomed = apply_zoom(pos, Zoom::IDENTITY);

        // Assert
        assert_eq!(zoomed, pos);
    }

    #[test]
    fn 缩放围绕视口中心不移动中心点本身() {
        // 视口正中央的坐标无论怎么缩放都应该保持不动——这是「围绕
        // 中心缩放」而非「围绕原点缩放」的直接验证。
        // Arrange
        let center = [LOGICAL_WIDTH as f32 / 2.0, LOGICAL_HEIGHT as f32 / 2.0];

        // Act
        let zoomed = apply_zoom(center, Zoom::new(2.5));

        // Assert
        assert_eq!(zoomed, center);
    }

    #[test]
    fn 放大后偏离中心的坐标离中心更远() {
        // Arrange
        let center_x = LOGICAL_WIDTH as f32 / 2.0;
        let pos = [center_x + 10.0, LOGICAL_HEIGHT as f32 / 2.0];

        // Act
        let zoomed = apply_zoom(pos, Zoom::new(2.0));

        // Assert
        assert_eq!(zoomed[0], center_x + 20.0);
    }
}
