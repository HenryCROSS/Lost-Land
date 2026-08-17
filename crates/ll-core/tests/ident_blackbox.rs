//! 命名空间标识符的黑箱属性测试。

use ll_core::ident::{Interner, NamespacedId};
use proptest::prelude::*;

/// 生成合法段落的策略：小写字母、数字、下划线，长度 1..12。
fn 合法段落() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z0-9_]{1,12}").expect("正则合法")
}

proptest! {
    #[test]
    fn 解析与显示互为逆运算(ns in 合法段落(), path in 合法段落()) {
        // 存档写出时依赖 Display，读入时依赖 parse。两者若不互逆，
        // 存档就会在往返中损坏。
        // Arrange
        let raw = format!("{ns}:{path}");

        // Act
        let parsed = NamespacedId::parse(&raw).expect("由合法段落拼成");

        // Assert
        prop_assert_eq!(parsed.to_string(), raw);
    }

    #[test]
    fn 登记后索引反查恒得原标识符(ns in 合法段落(), path in 合法段落()) {
        // Arrange
        let mut interner = Interner::new();
        let id = NamespacedId::parse(&format!("{ns}:{path}")).expect("合法");

        // Act
        let index = interner.intern(id.clone());

        // Assert
        prop_assert_eq!(interner.resolve(index), Some(&id));
    }

    #[test]
    fn 任意输入都不会崩溃(raw in ".{0,64}") {
        // 标识符会来自第三方 mod 的清单文件，属于外部不可信输入。
        // 无论内容多畸形都只能返回 Err，绝不能 panic。
        // Act
        let _ = NamespacedId::parse(&raw);
    }
}
