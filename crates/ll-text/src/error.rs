//! 本 crate 的错误类型。

use core::fmt;

/// 文本渲染地基的错误。
#[derive(Debug)]
pub enum TextError {
    /// 内置字体数据未能被 `fontdb` 解析出任何字体家族——说明字体文件
    /// 本身损坏，或 `include_bytes!` 打包进来的字节不是合法的 OTF/TTF。
    /// 这不该在正常运行中发生，出现即视为资产损坏。
    FontLoadFailed {
        /// 出问题的字体文件描述（供人读的名字，不是路径）。
        asset: &'static str,
    },
    /// `glyphon` 在把排版结果上传/摆放进图集这一步失败（例如图集空间
    /// 不足）。内层原始错误信息已格式化进字符串，因为 `glyphon` 的
    /// 错误类型未实现 `Clone`，本 crate 的错误类型又需要保持简单。
    Prepare(String),
    /// `glyphon` 在实际提交绘制命令这一步失败。
    Render(String),
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::FontLoadFailed { asset } => {
                write!(f, "内置字体资产解析失败，未得到任何字体家族: {asset}")
            }
            TextError::Prepare(why) => write!(f, "文本排版结果上传图集失败: {why}"),
            TextError::Render(why) => write!(f, "文本渲染提交失败: {why}"),
        }
    }
}

impl core::error::Error for TextError {}
