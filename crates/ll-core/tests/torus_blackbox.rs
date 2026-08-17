//! 环面坐标的黑箱属性测试。
//!
//! 本文件位于 `tests/`，只能访问 `ll-core` 的公开 API，看不到任何内部
//! 实现。这个限制是刻意的：它能发现「改了内部实现就崩」的脆弱设计。
//!
//! 具体用例只能证明「这一个输入是对的」；属性测试证明「所有输入都满足
//! 某不变量」。环面几何正是属性测试的理想对象——手写用例几乎不可能
//! 覆盖到所有绕法组合。

use ll_core::torus::TorusSize;
use proptest::prelude::*;

proptest! {
    #[test]
    fn 绕回后的坐标恒落在世界范围内(
        w in 1u32..500,
        h in 1u32..500,
        x in -1_000_000i32..1_000_000,
        y in -1_000_000i32..1_000_000,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");

        // Act
        let pos = world.wrap(x, y);

        // Assert
        prop_assert!(pos.x() >= 0 && pos.x() < w as i32);
        prop_assert!(pos.y() >= 0 && pos.y() < h as i32);
    }

    #[test]
    fn 切比雪夫距离对称(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act & Assert
        prop_assert_eq!(world.chebyshev(a, b), world.chebyshev(b, a));
    }

    #[test]
    fn 任意两点的单轴距离不超过半个世界(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // 这是环面拓扑的定义性质：绕远路永远不是最短路。
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act
        let (dx, dy) = world.delta(a, b);

        // Assert
        prop_assert!(dx.unsigned_abs() * 2 <= w);
        prop_assert!(dy.unsigned_abs() * 2 <= h);
    }

    #[test]
    fn 东西接缝处连续(w in 1u32..500, h in 1u32..500, y in 0i32..500) {
        // 地形生成依赖这条性质：若接缝不连续，玩家跨越边界时会看到
        // 地形突变。
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");

        // Act
        let west_edge = world.wrap(0, y);
        let east_wrapped = world.wrap(w as i32, y);

        // Assert
        prop_assert_eq!(west_edge, east_wrapped);
    }

    #[test]
    fn 曼哈顿距离不小于切比雪夫距离(
        w in 1u32..500, h in 1u32..500,
        ax in 0i32..500, ay in 0i32..500,
        bx in 0i32..500, by in 0i32..500,
    ) {
        // Arrange
        let world = TorusSize::new(w, h).expect("宽高均不为零");
        let a = world.wrap(ax, ay);
        let b = world.wrap(bx, by);

        // Act & Assert
        prop_assert!(world.manhattan(a, b) >= world.chebyshev(a, b));
    }
}
