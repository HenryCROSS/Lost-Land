//! 溢出门禁：**每一个 UI 键 × 两种语言**，按它所属面板的内容宽真实
//! 排版一次，行数超过该面板给这一类内容的预算就红。
//!
//! # 为什么非要加这一道
//!
//! `scripts/ci/check_i18n_strings.py` 的判据是
//! `HAN_CHAR = re.compile(r"[一-鿿]")`——**只抓含汉字的字面量**。而
//! `knowledge/design/ui-and-navigation.md` §8.1 实测出的结论是反的：
//! 内嵌思源黑体下 **en:zh ≈ 1.44:1，英文才是每一条散文型字符串的最坏
//! 情况**（254 个 UI 键实测比值 0.694）。
//!
//! 「工具只看中文、开发只跑中文」两件事叠起来，结果是十处溢出里**八处
//! 在中文构建下一个像素都看不见**。这不是谁忘了检查，是**判据只覆盖了
//! 一半，另一半静默失效**——本仓库反复付过代价的那个形状。
//!
//! # 判据为什么是「行数」，不是「宽度」
//!
//! W1/W2 落地之后断行宽度恒等于面板内容宽（见
//! `ll_ui::widget::label::Label::max_width`），于是「渲染宽 ≤ 内容宽」
//! 变成 `cosmic-text` 自己保证的**恒真命题**——一道永远绿的门禁没有牙。
//!
//! 真正会退化的是**行数**：一条本该一行的按钮标签变成两行、一句提示
//! 变成三行。这是玩家看得见的糙，也是「翻译写长了」唯一还能造成的后果。
//! 行数因此是这一层唯一有牙且稳定的判据。
//!
//! 规格 §8.5 W5 指定的反例是「把 `screen-worldsetup-hint` 的 en 文案加
//! 三个词，门禁必须红」——本文件的 [`每一个键在它所属面板里都不超行数预算`]
//! 满足它：那条今天 486.9px、在 500 的内容宽里恰好一行，加三个词就是
//! 两行，而 `screen-*-hint` 这一类的预算是一行。
//!
//! # 「键 ↔ 面板」这层映射怎么来的
//!
//! **能从代码现取的一律现取**：每块面板的内容宽全部来自 `ll_ui` 的公开
//! 常量（`hud::render::STATUS_WIDTH` 等经 `hud::content_width` 折算，
//! 模态屏经 `screen::SCREEN_WIDTH`/`SCREEN_PADDING`）——本文件里**没有
//! 任何一个像素字面量**。改了面板宽度，本门禁的判据自动跟着变。
//!
//! **取不到的是「哪个键画在哪块面板里」**：`item-*-display_name` /
//! `equip_slot-*-display_name` / `race-*-display_name` 这些是经数据表的
//! `display_name_key` 字段在**运行期**解析出来的，没有任何静态调用图能
//! 给出这层映射。因此按前缀分类，并把分类表做成**两个方向都会红**：
//!
//! 1. **缺一条就红**：`en.ftl` 里出现的每一个键都必须命中恰好一条规则
//!    （最长前缀优先），命中不到就 fail——新加一个前缀的人会被门禁挡下、
//!    被迫声明它画在哪儿。
//! 2. **多写一条也红**：一条规则若一个键都没匹配到，同样 fail——死规则
//!    会烂在表里，而烂着的规则正是「多写一条没人管」的那个洞。
//!
//! # 带 Fluent 变量的键单独一类，不硬测
//!
//! `screen-savelist-row = { $name } · { $time } · { $mode }` 这类文案的
//! 宽度取决于**运行期数据**（存档名可以是 24 个汉字），拿模式串本身去
//! 量出来的数没有意义。它们归 [`宽度判据::参数化`]，仍然必须在表里显式
//! 声明——不声明就是「缺一条」，照样红。它们的实际宽度由
//! `ll_game` 那一侧对**拼好的整行**的断言覆盖。

use std::collections::BTreeSet;
use std::path::Path;

use ll_i18n::Catalog;
use ll_text::{MeasureText, TextMeasurer};
use ll_ui::hud::content_width;
use ll_ui::hud::render::{
    ACTION_MENU_WIDTH, CHARACTER_WIDTH, EQUIPMENT_WIDTH, FEEDBACK_WIDTH, INVENTORY_WIDTH,
    STATUS_WIDTH,
};
use ll_ui::screen::{SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, SCREEN_PADDING, SCREEN_WIDTH};

/// 本体两份 `.ftl` 所在的目录。
const LOCALES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/locales");
/// 本体的本地化命名空间。
const NAMESPACE: &str = "lostland";
/// 门禁覆盖的语言——**两种都测**，见模块文档开头那条「英文才是更宽的
/// 那一侧」。
const LANGUAGES: [&str; 2] = ["zh-CN", "en"];

/// 模态屏的**基准**内容宽：面板宽度今天按内容伸缩（`ll_ui::screen`），
/// 但伸缩的下限就是这个数，也是「这一行要不要换行」的判据。
fn 模态屏内容宽() -> f32 {
    SCREEN_WIDTH - SCREEN_PADDING * 2.0
}

/// 一条键的宽度该怎么判。
#[derive(Debug, Clone, Copy, PartialEq)]
enum 宽度判据 {
    /// 在 `内容宽` 里排版，行数不得超过 `行数预算`。
    行数上限 { 内容宽: f32, 行数预算: usize },
    /// 文案带 Fluent 变量，宽度取决于运行期数据——本门禁不硬测，
    /// 见模块文档最后一节。字符串是「为什么不测」的理由，空着就红。
    参数化(&'static str),
    /// 压根不画在任何面板里（窗口标题、装载期日志）。同样要显式声明。
    不画在面板里(&'static str),
}

/// 一条「这个前缀的键画在哪块面板里」的声明。
struct 分类规则 {
    前缀: &'static str,
    面板: &'static str,
    判据: fn() -> 宽度判据,
}

/// 一行按钮/标签/名字：设计上就该占一行。
fn 一行(内容宽: f32) -> 宽度判据 {
    宽度判据::行数上限 {
        内容宽,
        行数预算: 1,
    }
}

/// 一段散文（提示语、说明、通知）：本来就是多行的，两行之内算正常。
fn 散文(内容宽: f32) -> 宽度判据 {
    宽度判据::行数上限 {
        内容宽,
        行数预算: 2,
    }
}

/// 分类表。**最长前缀优先**，因此更细的规则可以写在更粗的规则旁边，
/// 顺序不影响结果。
fn 分类表() -> Vec<分类规则> {
    vec![
        // ── 模态屏（九块屏共用一块居中面板） ──────────────────
        分类规则 {
            前缀: "screen-savelist-row",
            面板: "存档列表",
            判据: || 宽度判据::参数化("存档名由玩家输入，宽度取决于运行期数据"),
        },
        分类规则 {
            前缀: "screen-settings-row",
            面板: "设置屏",
            判据: || 宽度判据::参数化("动作名与键名都是运行期插值"),
        },
        分类规则 {
            前缀: "screen-settings-conflict",
            面板: "设置屏",
            判据: || 宽度判据::参数化("冲突的另一个动作名是运行期插值"),
        },
        分类规则 {
            前缀: "screen-settings-bound",
            面板: "设置屏",
            判据: || 宽度判据::参数化("键名是运行期插值"),
        },
        分类规则 {
            前缀: "screen-settings-cleared",
            面板: "设置屏",
            判据: || 宽度判据::参数化("动作名是运行期插值"),
        },
        分类规则 {
            前缀: "screen-chargen-player-died",
            面板: "模态屏通知",
            判据: || 散文(模态屏内容宽()),
        },
        分类规则 {
            前缀: "screen-savename-hint",
            面板: "模态屏提示行",
            判据: || 散文(模态屏内容宽()),
        },
        分类规则 {
            前缀: "screen-",
            面板: "模态屏行",
            判据: || 一行(模态屏内容宽()),
        },
        // ── 会话屏 ──────────────────────────────────────────
        // 批次 18 先落了对话内容表与文案，会话 UI 本身要到对话批次 2
        // 才建——**键先于面板落地**，本门禁与那批内容在两条分支上并行，
        // 合并后当场红（「缺一条」方向生效，正是它该有的行为）。
        //
        // 按模态屏内容宽计量：设计文档把会话 UI 定为模态屏，这不是豁免，
        // 是照它将来的宽度真量。实测预算是紧的——台词里有 9 条（zh 2 条
        // / en 7 条）正好排到 2 行，最长的 `dialogue-steward-reward` 是
        // 491.1px / 500px，再长一个词就红。批次 2 若把会话屏做成别的
        // 宽度，改这一条规则即可。
        分类规则 {
            前缀: "dialogue-",
            面板: "会话屏",
            判据: || 散文(模态屏内容宽()),
        },
        // ── 常驻 HUD ────────────────────────────────────────
        分类规则 {
            前缀: "hud-status-",
            面板: "状态栏",
            判据: || 一行(content_width(STATUS_WIDTH)),
        },
        分类规则 {
            前缀: "season-",
            面板: "状态栏",
            判据: || 一行(content_width(STATUS_WIDTH)),
        },
        分类规则 {
            前缀: "weather-",
            面板: "状态栏",
            判据: || 一行(content_width(STATUS_WIDTH)),
        },
        分类规则 {
            前缀: "hud-character-rule-modifier-sources",
            面板: "角色面板",
            判据: || 宽度判据::参数化("条数是运行期插值，且带 Fluent 复数选择"),
        },
        分类规则 {
            前缀: "hud-character-",
            面板: "角色面板",
            判据: || 一行(content_width(CHARACTER_WIDTH)),
        },
        分类规则 {
            前缀: "attribute-",
            面板: "角色面板",
            判据: || 一行(content_width(CHARACTER_WIDTH)),
        },
        分类规则 {
            前缀: "rule-modifier-",
            面板: "角色面板（规则修正行）",
            判据: || 宽度判据::参数化("修正量是运行期插值"),
        },
        分类规则 {
            前缀: "check_context-",
            面板: "角色面板（规则修正行）",
            判据: || 一行(content_width(CHARACTER_WIDTH)),
        },
        分类规则 {
            前缀: "damage_category-",
            面板: "角色面板（规则修正行）",
            判据: || 一行(content_width(CHARACTER_WIDTH)),
        },
        分类规则 {
            前缀: "trait-",
            面板: "角色面板",
            判据: || 散文(content_width(CHARACTER_WIDTH)),
        },
        分类规则 {
            前缀: "hud-inventory-panel-title",
            面板: "背包面板",
            判据: || 一行(content_width(INVENTORY_WIDTH)),
        },
        分类规则 {
            前缀: "hud-inventory-empty",
            面板: "背包面板",
            判据: || 一行(content_width(INVENTORY_WIDTH)),
        },
        分类规则 {
            前缀: "hud-inventory-durability-label",
            面板: "背包面板",
            判据: || 一行(content_width(INVENTORY_WIDTH)),
        },
        分类规则 {
            前缀: "hud-item-",
            面板: "背包面板",
            判据: || 一行(content_width(INVENTORY_WIDTH)),
        },
        分类规则 {
            前缀: "item-corpse-display_name",
            面板: "背包面板",
            判据: || 宽度判据::参数化("物种名是运行期插值"),
        },
        分类规则 {
            前缀: "item-",
            面板: "背包/装备面板",
            判据: || 散文(content_width(INVENTORY_WIDTH)),
        },
        分类规则 {
            前缀: "hud-equipment-",
            面板: "装备栏",
            判据: || 一行(content_width(EQUIPMENT_WIDTH)),
        },
        分类规则 {
            前缀: "equip_slot-",
            面板: "装备栏",
            判据: || 一行(content_width(EQUIPMENT_WIDTH)),
        },
        分类规则 {
            前缀: "hud-feedback-",
            面板: "反馈行",
            判据: || 散文(content_width(FEEDBACK_WIDTH)),
        },
        // ── 动作菜单（背包/制作/交互/方向四块共用一块弹窗） ────
        分类规则 {
            前缀: "hud-inventory-menu-",
            面板: "动作菜单",
            判据: || 散文(content_width(ACTION_MENU_WIDTH)),
        },
        分类规则 {
            前缀: "hud-craft-",
            面板: "动作菜单",
            判据: || 散文(content_width(ACTION_MENU_WIDTH)),
        },
        分类规则 {
            前缀: "hud-interact-",
            面板: "动作菜单",
            判据: || 散文(content_width(ACTION_MENU_WIDTH)),
        },
        分类规则 {
            前缀: "hud-direction-",
            面板: "动作菜单",
            判据: || 一行(content_width(ACTION_MENU_WIDTH)),
        },
        分类规则 {
            前缀: "recipe_category-",
            面板: "动作菜单",
            判据: || 一行(content_width(ACTION_MENU_WIDTH)),
        },
        分类规则 {
            前缀: "recipe-",
            面板: "动作菜单",
            判据: || 一行(content_width(ACTION_MENU_WIDTH)),
        },
        // ── 世界地图浮层 ───────────────────────────────────
        分类规则 {
            前缀: "hud-world-map-scale-label",
            面板: "世界地图比例尺",
            判据: || 宽度判据::参数化("每格多少 tile 是运行期插值"),
        },
        分类规则 {
            前缀: "hud-world-map-",
            面板: "世界地图",
            判据: || 散文(模态屏内容宽()),
        },
        // ── 角色创建 / 世界配置屏上的行 ────────────────────
        分类规则 {
            前缀: "worldgen-preset-",
            面板: "世界配置屏",
            判据: || 散文(模态屏内容宽()),
        },
        分类规则 {
            前缀: "race-",
            面板: "角色创建屏",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "class-",
            面板: "角色创建屏",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "subclass-",
            面板: "角色创建屏",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "culture-",
            面板: "角色创建屏",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "gender-",
            面板: "角色创建屏",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "resource-",
            面板: "世界地图/据点行",
            判据: || 一行(模态屏内容宽()),
        },
        分类规则 {
            前缀: "keybind-",
            面板: "设置屏行",
            判据: || 一行(模态屏内容宽()),
        },
        // ── 不画在任何面板里的 ─────────────────────────────
        分类规则 {
            前缀: "window-title",
            面板: "（窗口标题栏）",
            判据: || 宽度判据::不画在面板里("操作系统画的窗口标题，不经本项目的排版"),
        },
        分类规则 {
            前缀: "language-name",
            面板: "（设置屏的值）",
            判据: || 宽度判据::参数化("作为设置行的值被插进 screen-settings-row"),
        },
        分类规则 {
            前缀: "mod-",
            面板: "（装载报告）",
            判据: || 宽度判据::参数化("mod 名与版本号是运行期插值"),
        },
        分类规则 {
            前缀: "save-",
            面板: "（装载报告）",
            判据: || 宽度判据::参数化("mod 名与版本号是运行期插值"),
        },
    ]
}

/// 读一份 `.ftl` 里所有的**顶层消息键**（不含属性行、注释、续行）。
fn 全部键(language: &str) -> Vec<String> {
    let path = Path::new(LOCALES).join(format!("{language}.ftl"));
    let text = std::fs::read_to_string(&path).expect("本体的 .ftl 必然读得到");
    text.lines()
        .filter_map(|line| {
            // 顶层消息行的形状是 `key = value`，且 `key` 顶格。
            if line.starts_with([' ', '\t', '#']) || line.trim().is_empty() {
                return None;
            }
            let (key, _) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_lowercase()) {
                return None;
            }
            Some(key.to_string())
        })
        .collect()
}

/// 给一个键找它的分类规则——**最长前缀优先**。
fn 命中规则<'a>(表: &'a [分类规则], key: &str) -> Option<&'a 分类规则> {
    表.iter()
        .filter(|rule| key.starts_with(rule.前缀))
        .max_by_key(|rule| rule.前缀.len())
}

fn 目录() -> Catalog {
    Catalog::load_one(NAMESPACE, Path::new(LOCALES))
}

#[test]
fn 每一个键都声明了自己画在哪块面板里() {
    // 「缺一条就红」那一半：新加一个前缀的人会被这条挡下，被迫声明它
    // 画在哪儿。**不是「多写一条没人管」的那种表**。
    //
    // 反例验证（已实跑）：把分类表里 `"equip_slot-"` 那条删掉，本条
    // 立刻列出 23 个未分类的键并变红。
    // Arrange
    let 表 = 分类表();

    // Act
    let mut 未分类: Vec<String> = Vec::new();
    for language in LANGUAGES {
        for key in 全部键(language) {
            if 命中规则(&表, &key).is_none() {
                未分类.push(format!("{language}: {key}"));
            }
        }
    }

    // Assert
    assert!(
        未分类.is_empty(),
        "这些键没有声明自己画在哪块面板里，请在 分类表() 里补一条：\n{}",
        未分类.join("\n")
    );
}

#[test]
fn 分类表里没有一条死规则() {
    // 「多写一条也红」那一半：一条规则若一个键都匹配不到，它就是烂在
    // 表里的死记录——而死记录正是「真相源之外的副本」开始分叉的地方。
    //
    // 反例验证（已实跑）：往分类表里加一条 `前缀: "nonexistent-"`，
    // 本条立刻变红。
    // Arrange
    let 表 = 分类表();
    let mut 全部: BTreeSet<String> = BTreeSet::new();
    for language in LANGUAGES {
        全部.extend(全部键(language));
    }

    // Act
    let 死规则: Vec<&str> = 表
        .iter()
        .filter(|rule| {
            !全部
                .iter()
                .any(|key| 命中规则(&表, key).map(|hit| hit.前缀) == Some(rule.前缀))
        })
        .map(|rule| rule.前缀)
        .collect();

    // Assert
    assert!(
        死规则.is_empty(),
        "这些分类规则一个键都没匹配到，删掉它们：{死规则:?}"
    );
}

#[test]
fn 每一个键在它所属面板里都不超行数预算() {
    // 本门禁的主判据，见模块文档「判据为什么是行数」。
    //
    // 规格 §8.5 W5 指定的反例（已实跑）：把 `screen-worldsetup-hint`
    // 的 en 文案加三个词，它从一行变成两行，本条立刻变红。
    //
    // **文案全部来自 `assets/locales` 真实的两份 `.ftl`**，不是占位符
    // ——拿空串或占位符去测「放不放得下」，断言永远绿。
    // Arrange
    let 表 = 分类表();
    let catalog = 目录();
    let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");

    // Act
    let mut 超标: Vec<String> = Vec::new();
    let mut 真的量过的条数 = 0_usize;
    for language in LANGUAGES {
        for key in 全部键(language) {
            let rule = 命中规则(&表, &key).expect("上一条测试已经保证每个键都有规则");
            let 宽度判据::行数上限 {
                内容宽, 行数预算
            } = (rule.判据)()
            else {
                continue;
            };
            let text = catalog.resolve(language, &key);
            // 解析不出来会回落成键名本身——那说明这条键在这门语言里缺
            // 文案，是另一道门禁的事，但拿键名去量宽度是没有意义的，
            // 这里跳过并单独计数（下面断言「真的量过的条数」不为零）。
            if text == key {
                continue;
            }
            真的量过的条数 += 1;
            let metrics =
                measurer.measure_text(&text, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, 内容宽);
            if metrics.line_count > 行数预算 {
                超标.push(format!(
                    "{language} / {key}（{}）：在 {内容宽}px 的内容宽里排了 {} 行，预算 {行数预算} 行；实测最长行 {:.1}px",
                    rule.面板, metrics.line_count, metrics.max_line_width
                ));
            }
        }
    }

    // Assert：先确认这条测试真的量了东西——一个「什么都没量到」的实现
    // 会让下面那条断言永远绿（ADR 0018 的假绿形状，上一批在「语言」行
    // 上抓到过一次）。
    assert!(
        真的量过的条数 > 200,
        "只量到 {真的量过的条数} 条，说明键或文案没读进来"
    );
    assert!(
        超标.is_empty(),
        "以下文案超出面板行数预算：\n{}",
        超标.join("\n")
    );
}

#[test]
fn 英文确实是更宽的那一侧() {
    // §8.1 的核心结论（en:zh ≈ 1.44:1）——它正是「门禁只看中文所以看不
    // 见八处溢出」的原因。把它钉成一条会红的断言：结论一旦不再成立
    // （例如有人把英文文案整体改短、或换了字体），上面那道门禁的取舍
    // 就要重新论证，不该静悄悄地过去。
    // Arrange
    let catalog = 目录();
    let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");
    let 表 = 分类表();

    // Act：对每一个两种语言都有文案、且判据是行数上限的键各量一次总宽。
    let mut zh = 0.0_f32;
    let mut en = 0.0_f32;
    for key in 全部键("zh-CN") {
        let Some(rule) = 命中规则(&表, &key) else {
            continue;
        };
        if !matches!((rule.判据)(), 宽度判据::行数上限 { .. }) {
            continue;
        }
        let a = catalog.resolve("zh-CN", &key);
        let b = catalog.resolve("en", &key);
        if a == key || b == key {
            continue;
        }
        zh += measurer
            .measure_text(&a, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, 1.0e6)
            .max_line_width;
        en += measurer
            .measure_text(&b, SCREEN_FONT_SIZE, SCREEN_LINE_HEIGHT, 1.0e6)
            .max_line_width;
    }

    // Assert
    assert!(zh > 0.0 && en > 0.0, "两种语言都必须真的量到了东西");
    assert!(
        en > zh * 1.2,
        "英文总宽 {en:.1} 应显著宽于中文 {zh:.1}（实测比值 {:.3}）",
        zh / en
    );
}

/// 九块模态屏各自的标题键与提示键——**提示行是这个游戏唯一的按键教学**
/// （规格 §9.3 F5），它被布局吃掉不是「不好看」，是玩家不知道能按什么。
const 九块屏: [(&str, &str); 9] = [
    ("screen-title-title", "screen-title-hint"),
    ("screen-menu-title", "screen-menu-hint"),
    ("screen-settings-title", "screen-settings-hint"),
    ("screen-savelist-title", "screen-savelist-hint"),
    ("screen-savename-title", "screen-savename-hint"),
    ("screen-chargen-title", "screen-chargen-hint"),
    ("screen-worldsetup-title", "screen-worldsetup-hint"),
    ("screen-spawnpick-title", "screen-spawnpick-hint"),
    ("screen-modlist-title", "screen-modlist-hint"),
];

#[test]
fn 每一块模态屏的每一行都画在面板里面() {
    // **结构不变式**，比行数预算更强、且零映射表：走的是生产代码自己的
    // `build_screen_panel`，喂真实的 `assets/locales`，断言每一行的包围盒
    // 都落在面板的内边距之内。
    //
    // 它盯的正是那十处溢出的形状：断行宽度与面板宽度对不上时，文字会
    // 画到面板外面（O-6/O-7）或掉出面板底边（O-1/O-2/O-3）。
    //
    // 反例验证（已实跑）：把 `ll_ui::screen` 的 `wrap_width` 改回
    // `SCREEN_WIDTH`（面板宽本身，不减内边距），本条立刻变红。
    // Arrange
    let catalog = 目录();
    let mut measurer = TextMeasurer::new().expect("内置字体资产应能正常解析");
    let rows: Vec<String> = vec!["甲".to_string(), "乙".to_string()];
    let mut 量过的行数 = 0_usize;

    for language in LANGUAGES {
        for (title_key, hint_key) in 九块屏 {
            let data = ll_ui::screen::ScreenData {
                title_key,
                rows: &rows,
                cursor: 0,
                empty_key: "screen-menu-empty",
                hint_key,
                notice: None,
                hovered: None,
            };

            // Act
            let content = ll_ui::screen::build_screen_panel(
                &data,
                &catalog,
                language,
                &mut measurer,
                1280.0,
                720.0,
            );

            // Assert
            let 内容宽 = content.panel.width - SCREEN_PADDING * 2.0;
            for label in &content.labels {
                assert!(
                    !label.text.is_empty(),
                    "{language} / {title_key}：测出来的是空串，这种断言永远绿"
                );
                量过的行数 += 1;
                assert_eq!(
                    label.max_width, 内容宽,
                    "{language} / 「{}」的断行宽不等于面板内容宽",
                    label.text
                );
                let metrics = measurer.measure_text(
                    &label.text,
                    SCREEN_FONT_SIZE,
                    SCREEN_LINE_HEIGHT,
                    label.max_width,
                );
                assert!(
                    label.x + metrics.max_line_width
                        <= content.panel.right() - SCREEN_PADDING + 0.5,
                    "{language} / 「{}」右边界 {} 越过了面板内侧 {}",
                    label.text,
                    label.x + metrics.max_line_width,
                    content.panel.right() - SCREEN_PADDING
                );
                assert!(
                    label.y + metrics.line_count as f32 * SCREEN_LINE_HEIGHT
                        <= content.panel.bottom() - SCREEN_PADDING + 0.5,
                    "{language} / 「{}」底边掉出了面板",
                    label.text
                );
            }
        }
    }

    // 自证这条测试真的量了东西（ADR 0018 的假绿形状）。
    assert!(量过的行数 >= 九块屏.len() * LANGUAGES.len() * 4);
}
