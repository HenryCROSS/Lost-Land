//! 图集元数据解析的黑箱测试。
//!
//! 图集元数据会来自第三方 mod，属于外部不可信输入。规格 §14.3 要求这类
//! 入口做模糊测试；在正式接入 cargo-fuzz 之前，先用属性测试守住
//! 「任意输入都不崩溃」这条底线。

use ll_render::atlas::AtlasMetadata;
use proptest::prelude::*;

/// 一段结构完整的合法元数据，供截断测试取前缀。
const FULL: &str = r#"{"image":"a.png","entries":[{"name":"x","rect":{"x":0,"y":0,"width":1,"height":1},"pivot":{"x":0,"y":0},"footprint":{"width":1,"height":1}}]}"#;

proptest! {
    #[test]
    fn 任意输入都不会崩溃(raw in ".{0,256}") {
        // Act：只要求不 panic，返回 Err 完全正常。
        let _ = AtlasMetadata::parse(&raw);
    }

    #[test]
    fn 截断的合法输入也不会崩溃(cut in 0usize..FULL.len()) {
        // 损坏的资产文件最常见的形态就是被截断。
        // Arrange
        let truncated = &FULL[..cut];

        // Act
        let _ = AtlasMetadata::parse(truncated);
    }
}
