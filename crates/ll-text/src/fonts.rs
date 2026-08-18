//! 内置字体资产：思源黑体（正文/标题）与 Tabler Icons（功能性 UI 图标）。
//!
//! 字体选型与许可证核实见 `knowledge/pipelines/text-and-font-rendering.md`
//! 第 4、9 节与 `knowledge/licenses/2026-08-18-ll-text-asset-import.md`。
//! 只带 Regular/Bold 两档字重（约 16.2MB），CN 地区子集不做二次子集化
//! ——理由是任何进一步子集化都可能切掉玩家自定义名字/mod 文本用到的字。

use std::sync::Arc;

use cosmic_text::fontdb::{Database, Source};

use crate::TextError;

/// 思源黑体 CN 地区子集，常规字重（拉丁 + 简体中文字形同在一个文件里）。
const SOURCE_HAN_SANS_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/SourceHanSansCN-Regular.otf");

/// 思源黑体 CN 地区子集，粗体字重。
const SOURCE_HAN_SANS_BOLD: &[u8] =
    include_bytes!("../../../assets/fonts/SourceHanSansCN-Bold.otf");

/// Tabler Icons 官方 webfont（默认描边粗细）。功能性 UI 图标专用，
/// 游戏内容图标（物品/技能/状态效果）不得使用这个字体，必须手绘像素图
/// ——这条边界写在 `knowledge/pipelines/text-and-font-rendering.md` 第 9 节。
const TABLER_ICONS: &[u8] = include_bytes!("../../../assets/icons/tabler-icons.ttf");

/// 已加载字体的家族名，**从字体文件的 name 表实际解析得到，不是硬编码
/// 猜测**——不同字体打包工具给出的家族名字符串可能与直觉不同（比如
/// Tabler Icons 的 CSS 里写的是 `"tabler-icons"`，但那是 CSS 作者起的
/// 别名，不保证等于字体文件 name 表里的真实值），核实纪律见
/// `knowledge/README.md`：没有一手核实过的字符串不能当结论硬编码。
#[derive(Debug, Clone)]
pub struct FontCatalog {
    /// 思源黑体的家族名。正文与标题共用同一个家族，字重靠
    /// [`cosmic_text::Weight`] 而不是切换家族名来区分（本项目只用一套
    /// 字体家族的方针见管线文档第 2.5 节）。
    pub text_family: String,
    /// Tabler Icons 的家族名，图标绘制时以此指定
    /// `cosmic_text::Family::Name`。
    pub icon_family: String,
    /// Tabler Icons 那个 face 在 `fontdb` 里的 ID。
    ///
    /// [`crate::layout`] 用它判定某个排版出的字形究竟是被回退路由到了
    /// 图标字体、还是正常落在正文字体——**按 ID 比对，不是按家族名字符
    /// 串比对**：字符串比对要求两边格式化规则完全一致（大小写、语言
    /// 标签变体），ID 是 `fontdb` 内部唯一标识，比对不会有这类隐患。
    pub icon_font_id: cosmic_text::fontdb::ID,
}

impl FontCatalog {
    /// 把三个内置字体文件注册进 `db`，返回它们各自的家族名。
    ///
    /// **`db` 必须是一个新建的空库**（`fontdb::Database::new()`），
    /// 调用方不应该在此之前调用过 `load_system_fonts` 之类的方法——
    /// 本项目的硬约束是「绝不可依赖运行环境已安装的系统字体」（缺字体
    /// 的机器上会显示缺字方块，见管线文档第 4.3 节），只有从一个不含
    /// 系统字体的空库出发，才能保证最终渲染结果只可能来自这三个内置
    /// 文件，不会在开发者本机「碰巧装了思源黑体」的情况下把依赖系统
    /// 字体的问题掩盖过去。
    pub fn load(db: &mut Database) -> Result<FontCatalog, TextError> {
        let text_ids = load_source(db, SOURCE_HAN_SANS_REGULAR);
        load_source(db, SOURCE_HAN_SANS_BOLD);
        let icon_ids = load_source(db, TABLER_ICONS);

        let text_family = first_family_name(db, &text_ids).ok_or(TextError::FontLoadFailed {
            asset: "SourceHanSansCN-Regular.otf",
        })?;
        let icon_family = first_family_name(db, &icon_ids).ok_or(TextError::FontLoadFailed {
            asset: "tabler-icons.ttf",
        })?;
        let icon_font_id = *icon_ids.first().ok_or(TextError::FontLoadFailed {
            asset: "tabler-icons.ttf",
        })?;

        Ok(FontCatalog {
            text_family,
            icon_family,
            icon_font_id,
        })
    }
}

/// 把内嵌的字节数组作为 `fontdb::Source::Binary` 载入，返回新分配到的
/// 字体 ID。用 `load_font_source`（而非 `load_font_data`）是因为前者
/// 直接把新增 ID 返回，不需要靠「载入前后 diff `db.faces()`」这种脆弱
/// 手法去猜哪些 ID 是新增的。
fn load_source(db: &mut Database, bytes: &'static [u8]) -> Vec<cosmic_text::fontdb::ID> {
    db.load_font_source(Source::Binary(Arc::new(bytes)))
        .into_iter()
        .collect()
}

/// 取一批字体 ID 里第一个 face 的第一个家族名。
///
/// 一个字体文件通常只含一个 face、一个主家族名，取第一个已经够用；
/// 后续若要支持字体合集（一个文件多个 face）需要在此扩展，不在本任务
/// 范围内。
fn first_family_name(db: &Database, ids: &[cosmic_text::fontdb::ID]) -> Option<String> {
    let id = *ids.first()?;
    let face = db.face(id)?;
    face.families.first().map(|(name, _lang)| name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 加载三个内置字体后目录含非空家族名() {
        // Arrange
        let mut db = Database::new();

        // Act
        let catalog = FontCatalog::load(&mut db).expect("内置字体资产应能正常解析");

        // Assert
        assert!(!catalog.text_family.is_empty());
    }

    #[test]
    fn 图标字体家族名与正文字体家族名不同() {
        // 两者必须是可区分的家族，排版时才能通过 Family::Name 精确指定
        // 「这段文字用正文字体画」还是「这个字符用图标字体画」。
        // Arrange
        let mut db = Database::new();

        // Act
        let catalog = FontCatalog::load(&mut db).expect("内置字体资产应能正常解析");

        // Assert
        assert_ne!(catalog.text_family, catalog.icon_family);
    }

    #[test]
    fn 加载后库中不含任何系统字体() {
        // 空库 + 仅 load_font_source 三次，不调用 load_system_fonts，
        // 结果应恰好是三个 face（Regular、Bold、Tabler Icons 各一个）
        // ——这条断言直接验证「不依赖系统字体」这条硬约束在代码层面
        // 成立，而不只是靠「我们没写调用系统扫描的代码」这种口头保证。
        // Arrange
        let mut db = Database::new();

        // Act
        FontCatalog::load(&mut db).expect("内置字体资产应能正常解析");

        // Assert
        assert_eq!(db.faces().count(), 3);
    }
}
