//! HSL 颜色空间的最小实现：色彩理论里的邻近色、互补色本质都是「色相
//! 环上转个角度」，但图集像素数据只存 RGB，因此点缀配色的全部计算都
//! 要先转到 HSL、算完再转回来。
//!
//! 不引入外部颜色库：需要的只是「旋转色相」「调明度」「调饱和度」
//! 三个操作，手写这几十行比拉一个新依赖更简单，也不需要额外审计一个
//! 依赖的许可证与维护状态（见仓库根 `deny.toml` 的门禁理由）。

/// 色相-饱和度-明度颜色。`h` 取值 `[0, 360)` 度，`s`/`l` 取值 `[0, 1]`。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hsl {
    h: f32,
    s: f32,
    l: f32,
}

impl Hsl {
    /// 从 8 位 RGB 换算到 HSL，公式是色彩空间转换的标准定义。
    pub(crate) fn from_rgb(r: u8, g: u8, b: u8) -> Hsl {
        let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let delta = max - min;

        if delta.abs() < f32::EPSILON {
            // 灰阶：色相无意义，饱和度恒为 0。
            return Hsl { h: 0.0, s: 0.0, l };
        }

        let s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        let h_raw = if (max - r).abs() < f32::EPSILON {
            ((g - b) / delta) % 6.0
        } else if (max - g).abs() < f32::EPSILON {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        // h_raw 可能因浮点减法为负，先按 360° 换算再取模拉回 [0, 360)。
        let h = (h_raw * 60.0 + 360.0) % 360.0;

        Hsl { h, s, l }
    }

    /// 换算回 8 位 RGB，公式同样是标准 HSL→RGB 定义。
    pub(crate) fn to_rgb(self) -> (u8, u8, u8) {
        if self.s.abs() < f32::EPSILON {
            let v = (self.l * 255.0).round().clamp(0.0, 255.0) as u8;
            return (v, v, v);
        }

        let c = (1.0 - (2.0 * self.l - 1.0).abs()) * self.s;
        let h_prime = self.h / 60.0;
        let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
        let (r1, g1, b1) = match h_prime as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.l - c / 2.0;
        let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;

        (to_u8(r1), to_u8(g1), to_u8(b1))
    }

    /// 色相环上旋转 `degrees` 度（可正可负），环绕取模到 `[0, 360)`。
    ///
    /// 色彩理论里的「邻近色」「互补色」本质都是这一个操作：邻近色是转
    /// 一个小角度（约 15°~30°），互补色是转 180°——本工具把这两种关系
    /// 都实现成对同一个函数传不同角度，而不是分别写两套换算逻辑。
    pub(crate) fn rotated(self, degrees: f32) -> Hsl {
        Hsl {
            h: (self.h + degrees).rem_euclid(360.0),
            ..self
        }
    }

    /// 当前明度，取值 `[0, 1]`。
    ///
    /// 存在的唯一理由是「让笔画色的推法自己决定推哪个方向」：
    /// [`crate::npc`] 的徽记笔画色由底板色沿明度轴推一个固定量得出，
    /// 亮底板往暗推、暗底板往亮推，需要先问一句底板到底是亮是暗。
    /// 不给 `h`/`s` 开同样的读取口——那两个目前没有调用方，YAGNI。
    pub(crate) fn lightness(self) -> f32 {
        self.l
    }

    /// 明度整体偏移 `delta`（可正可负），钳制在 `[0, 1]`。
    pub(crate) fn lighten(self, delta: f32) -> Hsl {
        Hsl {
            l: (self.l + delta).clamp(0.0, 1.0),
            ..self
        }
    }

    /// 饱和度整体偏移 `delta`（可正可负），钳制在 `[0, 1]`。
    ///
    /// 用于给低饱和度的中性色（如 `terrain_mountain` 的灰、`terrain_snow`
    /// 的近白）点缀：这两种颜色本身饱和度接近 0，单纯旋转色相在换算回
    /// RGB 后几乎看不出变化（饱和度为零时色相本就不影响最终颜色），
    /// 必须先把点缀像素的饱和度顶上去，旋转后的色相才有视觉效果。
    pub(crate) fn saturate(self, delta: f32) -> Hsl {
        Hsl {
            s: (self.s + delta).clamp(0.0, 1.0),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb转hsl再转回得到接近原值的颜色() {
        // Arrange：terrain_grass 的草绿色。
        let original = (86u8, 125u8, 70u8);

        // Act
        let hsl = Hsl::from_rgb(original.0, original.1, original.2);
        let roundtrip = hsl.to_rgb();

        // Assert：浮点换算允许 ±2 的舍入误差。
        assert!((roundtrip.0 as i16 - original.0 as i16).abs() <= 2);
        assert!((roundtrip.1 as i16 - original.1 as i16).abs() <= 2);
        assert!((roundtrip.2 as i16 - original.2 as i16).abs() <= 2);
    }

    #[test]
    fn 灰阶颜色的饱和度为零() {
        // Arrange & Act：纯灰色 r=g=b。
        let hsl = Hsl::from_rgb(128, 128, 128);

        // Assert
        assert_eq!(hsl.to_rgb(), (128, 128, 128));
    }

    #[test]
    fn 旋转180度两次会换回原色相() {
        // Arrange：terrain_sand 的沙黄色。
        let hsl = Hsl::from_rgb(214, 196, 140);

        // Act
        let back = hsl.rotated(180.0).rotated(180.0);

        // Assert
        assert!((back.h - hsl.h).abs() < 0.01);
    }

    #[test]
    fn 色相旋转会环绕到0到360度区间内() {
        // Arrange
        let hsl = Hsl {
            h: 350.0,
            s: 0.5,
            l: 0.5,
        };

        // Act
        let rotated = hsl.rotated(20.0);

        // Assert
        assert!((rotated.h - 10.0).abs() < 0.01);
    }

    #[test]
    fn 明度偏移超出上界会被钳制在1() {
        // Arrange
        let hsl = Hsl {
            h: 0.0,
            s: 0.5,
            l: 0.9,
        };

        // Act
        let lightened = hsl.lighten(0.5);

        // Assert
        assert_eq!(lightened.l, 1.0);
    }

    #[test]
    fn 饱和度偏移超出下界会被钳制在0() {
        // Arrange
        let hsl = Hsl {
            h: 0.0,
            s: 0.1,
            l: 0.5,
        };

        // Act
        let desaturated = hsl.saturate(-0.5);

        // Assert
        assert_eq!(desaturated.s, 0.0);
    }
}
