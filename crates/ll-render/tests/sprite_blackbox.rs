//! 绘制顺序的黑箱属性测试。
//!
//! 排序的正确性靠单个用例很难说清——真正要保证的是它构成一个**全序**：
//! 任意两个键必定可比较且顺序唯一。这正是属性测试的用武之地。

use ll_render::sprite::{DrawOrder, Layer};
use proptest::prelude::*;

fn any_order() -> impl Strategy<Value = DrawOrder> {
    (0u8..5, -1000i32..1000, 0u64..100)
        .prop_map(|(layer, foot_y, entity)| DrawOrder::new(Layer(layer), foot_y, entity))
}

proptest! {
    #[test]
    fn 排序键构成全序(a in any_order(), b in any_order()) {
        // 全序意味着：要么相等，要么严格一大一小，不存在「无法比较」。
        // Act & Assert
        prop_assert!(a < b || b < a || a == b);
    }

    #[test]
    fn 比较是反对称的(a in any_order(), b in any_order()) {
        // Act & Assert
        prop_assert_eq!(a < b, b > a);
    }

    #[test]
    fn 排序具有传递性(a in any_order(), b in any_order(), c in any_order()) {
        // 传递性若不成立，排序结果会依赖比较顺序，遮挡关系就会逐帧抖动。
        // Act & Assert
        if a < b && b < c {
            prop_assert!(a < c);
        }
    }
}
