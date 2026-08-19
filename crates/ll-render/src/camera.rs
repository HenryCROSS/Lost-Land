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
}
