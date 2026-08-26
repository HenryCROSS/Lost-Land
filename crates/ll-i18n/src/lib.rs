//! 本地化查表层：把 `_key` 字段（`display_name_key`/`title_key`/
//! `description_key`/`message_key`）解析成 Fluent `.ftl` 里的真实字串。
//!
//! # 为什么单独成 crate，不放进 `ll-text`
//!
//! `ll-text`（`crates/ll-text`）是排版/栅格化地基：cosmic-text 断行整形、
//! glyphon 上屏，依赖 `ll-render` 拿 GPU 设备。本 crate 做的是完全
//! 不同的另一件事——「给一个键、一个语言标签，查出应该显示哪段文字」
//! ——不摸任何 GPU 资源，也不关心文字最终怎么画到屏幕上。把两者混进
//! 同一个 crate，会让 `ll-text` 平白多出一条与渲染无关的 Fluent 解析
//! 依赖；反过来，任何只需要查表（比如存档校验错误信息、mod 清单校验）
//! 而不需要渲染的调用方，也会被迫拖进 cosmic-text/glyphon/wgpu 这整条
//! 渲染管线。拆开后 `ll-text` 管「怎么画」，本 crate 管「画什么」，
//! 各自可以独立被消费。
//!
//! # 依赖方向
//!
//! 本 crate 只依赖 `fluent`/`tracing`，不依赖任何本项目上游 crate——
//! 这是刻意的：本地化是纯表现层关注点（哪种语言、哪个键对应哪段文字），
//! `ll-world`/`ll-sim` 的确定性世界模拟不应该、也不需要知道任何字符串
//! 长什么样。谁消费本 crate 完全由上层决定（目前是 `ll-game`），本
//! crate 自身不知道也不关心调用方是谁。
//!
//! # 缺键与缺语言：返回键名本身，不 panic、不空白
//!
//! [`Catalog::resolve`]/[`Catalog::resolve_with_args`] 在键不存在、
//! 语言未装载、或 Fluent 格式化本身失败时，一律回退到把**原始键**当
//! 显示文本返回，并记一条 `tracing::warn!`。理由：
//!
//! - **不能静默**——缺键是内容制作过程中的真实缺陷（忘了写翻译、
//!   `.ftl` 文件手改坏了、mod 声明了键却没配套翻译），必须在日志里
//!   看得见，否则永远没人会去补。
//! - **不能 panic**——本地化资源是运行时可替换的用户/mod 数据（见
//!   `knowledge/design/mod-package-structure.md`「本地化文件」一节），
//!   与 `ll-platform` 配置文件系统（`ll_platform::config` 模块文档
//!   「损坏时的退化策略」一节）同一条纪律：外部数据损坏不该让游戏
//!   直接崩掉。
//! - **回退到键名而非空字符串**——键名虽然对玩家不友好（会看到形如
//!   `lostland:keybind.action.up` 的原始文本），但一眼就能看出「这里
//!   缺了翻译」，比空白或占位符更容易在测试/游玩中被发现并追溯到具体
//!   缺失的是哪一条。

use std::collections::HashMap;
use std::path::Path;

use fluent::{FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

/// Fluent 的实参表，[`Catalog::resolve_with_args`] 的入参类型。
///
/// **必须重新导出**：`resolve_with_args` 从一开始就是 `pub`，但它的
/// 参数类型此前只在本 crate 内部 `use`，crate 外没有任何合法途径构造
/// 一个 `FluentArgs`——那个方法因此是「公开但调不动」的。第一个真实
/// 调用方（`ll_ui::hud::character_panel` 的规则修正行）出现时补上这条
/// 导出，而不是让下游各自再声明一份 `fluent` 依赖：多一份独立的版本
/// 声明就多一个「同名不同类型」的漂移风险点，理由与 `ll-ui` 走
/// `ll-render` 重新导出的 `wgpu` 一致。
pub use fluent::FluentArgs;

/// 一个语言的完整消息集合：`语言标签 → FluentBundle`。
///
/// 语言标签是 `.ftl` 文件名去掉扩展名，如 `zh-CN.ftl` 对应标签
/// `"zh-CN"`——与
/// `knowledge/design/mod-package-structure.md`「本地化文件」一节的
/// `locales/<语言标签>.ftl` 约定完全一致，不需要在文件名之外再维护
/// 一份语言标签清单。
pub struct Catalog {
    bundles: HashMap<String, FluentBundle<FluentResource>>,
}

impl Catalog {
    /// 从一个目录装载全部 `*.ftl` 文件，每个文件是一种语言。
    ///
    /// **不返回 `Result`，永不失败**：目录不存在、某个文件语法有误、
    /// 文件名不是合法语言标签——这些情况各自跳过对应的语言/文件并记一条
    /// `tracing::warn!`，装载流程整体继续。理由与模块文档「缺键与缺
    /// 语言」一节一致：本地化资源是外部数据，任何单点损坏都不该阻塞
    /// 启动。极端情况下（目录整个不存在）会得到一个空 `Catalog`——
    /// 此后每次 `resolve` 都会走「回退到键名」分支，游戏仍然能跑，只是
    /// 玩家会看到键名而不是译文，这比直接起不来更好。
    pub fn load_dir(dir: &Path) -> Catalog {
        let mut bundles = HashMap::new();

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    dir = %dir.display(),
                    %error,
                    "本地化目录不存在或无法读取，本次运行没有任何已装载的语言"
                );
                return Catalog { bundles };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("ftl") {
                continue;
            }
            let Some(language) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            match load_bundle(&path, language) {
                Ok(bundle) => {
                    bundles.insert(language.to_string(), bundle);
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "本地化文件装载失败，跳过这一种语言"
                    );
                }
            }
        }

        Catalog { bundles }
    }

    /// 已成功装载的语言标签数量——只用于启动日志与测试断言，不参与
    /// 任何查表逻辑。
    pub fn loaded_language_count(&self) -> usize {
        self.bundles.len()
    }

    /// 查 `key` 在 `language` 下的文本，不带参数插值。
    ///
    /// `key` 既可以是裸 Fluent 路径（如 `"window.title"`，
    /// `ll_platform::window::WindowConfig::title_key` 的既有形状——本
    /// crate 不依赖 `ll-platform`，此处只是文字引用，不做可解析的文档
    /// 内链），也可以是带命名空间前缀的完整键（如
    /// `"lostland:race.human.display_name"`，`ll-mod` 内容表的
    /// `display_name_key: NamespacedId` 既有形状）——命名空间前缀会被
    /// 剥离后再查表。**当前只装载了本体（`lostland`）一份 `.ftl`**，
    /// 剥离命名空间前缀这一步只是让两种既有键形状都能被同一个
    /// `resolve` 处理，尚不代表按命名空间分流去查对应 mod 的
    /// `locales/`——那是「五、mod 的 `.ftl`」一节留的接口，见模块文档。
    pub fn resolve(&self, language: &str, key: &str) -> String {
        self.resolve_with_args(language, key, None)
    }

    /// 查 `key` 在 `language` 下的文本，`args` 提供 Fluent 消息里
    /// `{ $变量 }` 占位符的实参（例如
    /// `ll_content::load_error::ModSetMismatch` 的 `namespace`/
    /// `required_version`）。
    pub fn resolve_with_args(
        &self,
        language: &str,
        key: &str,
        args: Option<&FluentArgs>,
    ) -> String {
        let fluent_id = to_fluent_id(key);

        let Some(bundle) = self.bundles.get(language) else {
            tracing::warn!(language, key, "请求的语言未装载，回退到键名本身");
            return key.to_string();
        };

        let Some(message) = bundle.get_message(&fluent_id) else {
            tracing::warn!(language, key, fluent_id, "本地化键不存在，回退到键名本身");
            return key.to_string();
        };

        let Some(pattern) = message.value() else {
            tracing::warn!(
                language,
                key,
                fluent_id,
                "本地化键存在但没有可显示的值，回退到键名本身"
            );
            return key.to_string();
        };

        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned();

        if !errors.is_empty() {
            tracing::warn!(
                language,
                key,
                fluent_id,
                ?errors,
                "本地化文本格式化出现错误，结果可能不完整"
            );
        }

        text
    }
}

/// 把一个 `_key` 字段的取值转成 Fluent 消息 id。
///
/// # 为什么需要转换，不能直接拿字符串当 id
///
/// 两条既有约束叠在一起，逼出这次转换：
///
/// 1. `NamespacedId`（`ll-mod` 内容表用它存 `display_name_key`）的
///    `路径` 部分允许包含点号（如 `race.human.display_name`，见
///    `ll_core::ident::NamespacedId::parse` 的合法字符集），这是内容
///    ID 命名空间自身的分层约定。
/// 2. Fluent 的消息标识符语法（`[a-zA-Z][a-zA-Z0-9_-]*`）**不允许
///    点号**——`.` 在 `.ftl` 语法里是消息「属性」（`message.attr`）的
///    分隔符，只能出现在专门声明的属性行里，不能作为主消息 id 的一
///    部分。若把 `race.human.display_name` 原样当 Fluent id 写进
///    `.ftl`，解析会在第一个点号处失败。
///
/// 解法：查表前把点号换成连字符（`race.human.display_name` →
/// `race-human-display_name`），下划线保留不动——连字符和下划线都是
/// Fluent id 合法字符，只有点号不合法。`.ftl` 文件里的条目因此用
/// 连字符分隔而不是点号，见 `assets/locales/zh-CN.ftl`。
///
/// 命名空间前缀（`lostland:`）在这一步之前就被剥离——冒号同样不是
/// Fluent id 合法字符，且剥离的理由已在 [`Catalog::resolve`] 文档
/// 说明。
fn to_fluent_id(key: &str) -> String {
    let path = key.split_once(':').map_or(key, |(_, path)| path);
    path.replace('.', "-")
}

/// 装载单个 `.ftl` 文件为一个语言的 `FluentBundle`。
fn load_bundle(path: &Path, language: &str) -> Result<FluentBundle<FluentResource>, LoadError> {
    let text = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    let resource = FluentResource::try_new(text).map_err(|(_, errors)| LoadError::Syntax {
        error_count: errors.len(),
    })?;

    let lang_id: LanguageIdentifier = language
        .parse()
        .map_err(|_| LoadError::InvalidLanguageTag)?;
    let mut bundle = FluentBundle::new(vec![lang_id]);
    // use_isolating = false：本地化文本会与游戏内其余 UI 文字混排，
    // Fluent 默认插入的双向文本隔离符（U+2068/U+2069）在纯左到右的
    // 中英混排场景下没有实际作用，反而可能被下游文本渲染当成不可见
    // 的异常字形处理——见 `ll-text` 的 CJK 排版取舍（
    // `knowledge/pipelines/text-and-font-rendering.md`），本项目暂不
    // 支持阿拉伯语/希伯来语等真正需要双向隔离的语言。
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .map_err(|errors| LoadError::Syntax {
            error_count: errors.len(),
        })?;

    Ok(bundle)
}

/// [`load_bundle`] 的失败原因，只用于日志诊断（见
/// [`Catalog::load_dir`] 文档「不返回 `Result`」一节）——不向调用方
/// 暴露，调用方只关心装载后 [`Catalog::loaded_language_count`] 少了
/// 一种语言，具体哪里错、错在哪一行留给日志。
#[derive(Debug)]
enum LoadError {
    Io(std::io::Error),
    Syntax { error_count: usize },
    InvalidLanguageTag,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(error) => write!(f, "读取文件失败：{error}"),
            LoadError::Syntax { error_count } => {
                write!(f, "Fluent 语法解析失败，共 {error_count} 处错误")
            }
            LoadError::InvalidLanguageTag => write!(f, "文件名不是合法的语言标签"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在临时目录写两个最小 `.ftl` 文件，模拟 `assets/locales/` 的
    /// 真实形状——用临时目录而非直接读仓库里的 `assets/locales/`，
    /// 是为了让本测试不依赖仓库内容是否被后续改动，只验证
    /// [`Catalog`] 自身的查表逻辑。真实内容的端到端验证见
    /// `ll-game` 对 `assets/locales/` 的装载测试。
    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(dir.join("zh-CN.ftl"), "greeting = 你好\n").expect("测试用写入应当成功");
        std::fs::write(dir.join("en.ftl"), "greeting = Hello\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ll-i18n-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    #[test]
    fn 给定键和语言解析出该语言对应的字串() {
        // Arrange
        let dir = temp_dir("resolve-basic");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);

        // Act
        let text = catalog.resolve("zh-CN", "greeting");

        // Assert
        assert_eq!(text, "你好");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 切换语言后同一个键解析出不同的字串() {
        // 这是本地化系统区别于「查了张表」的关键验证点：不是随便查出
        // 点什么都算数，必须是**同一个键**在**不同语言**下真的产出
        // 两段不同的文本。
        // Arrange
        let dir = temp_dir("resolve-switch-language");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);

        // Act
        let zh_text = catalog.resolve("zh-CN", "greeting");
        let en_text = catalog.resolve("en", "greeting");

        // Assert
        assert_ne!(zh_text, en_text);
        assert_eq!(zh_text, "你好");
        assert_eq!(en_text, "Hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 缺键时回退到键名本身而不是空字符串() {
        // Arrange
        let dir = temp_dir("resolve-missing-key");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);

        // Act
        let text = catalog.resolve("zh-CN", "no.such.key");

        // Assert
        assert_eq!(text, "no.such.key");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 未装载的语言回退到键名本身而不是空字符串() {
        // Arrange
        let dir = temp_dir("resolve-missing-language");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_dir(&dir);

        // Act
        let text = catalog.resolve("fr", "greeting");

        // Assert
        assert_eq!(text, "greeting");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 目录不存在时装载出空目录而不是崩溃() {
        // Arrange
        let dir =
            std::env::temp_dir().join(format!("ll-i18n-test-nonexistent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // Act
        let catalog = Catalog::load_dir(&dir);

        // Assert
        assert_eq!(catalog.loaded_language_count(), 0);
    }

    #[test]
    fn 命名空间前缀被剥离后仍能查到同一个键() {
        // 验证 ll-mod 内容表的 NamespacedId 键形状
        // （"lostland:race.human.display_name"）与 ll-platform 的裸键
        // 形状（"window.title"）走同一条查表路径——见 `to_fluent_id`
        // 文档。
        // Arrange
        let dir = temp_dir("resolve-namespaced");
        std::fs::write(dir.join("zh-CN.ftl"), "race-human-display_name = 人类\n")
            .expect("测试用写入应当成功");
        let catalog = Catalog::load_dir(&dir);

        // Act
        let text = catalog.resolve("zh-CN", "lostland:race.human.display_name");

        // Assert
        assert_eq!(text, "人类");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 参数插值把变量替换进最终文本() {
        // Arrange
        let dir = temp_dir("resolve-with-args");
        std::fs::write(
            dir.join("zh-CN.ftl"),
            "save-mod-missing = 缺少模组 { $namespace }\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load_dir(&dir);
        let mut args = FluentArgs::new();
        args.set("namespace", "examplemod");

        // Act
        let text = catalog.resolve_with_args("zh-CN", "save-mod-missing", Some(&args));

        // Assert
        assert_eq!(text, "缺少模组 examplemod");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 全部约七十处 `_key` 使用点背后实际用到的键——本体三个种族、
    /// 三个职业、两个转职、十二个键位动作、窗口标题、三条 mod/存档
    /// 校验消息 id，逐一核实真实 `assets/locales/` 里都配了对应译文。
    /// 与其余用临时目录 fixture 的测试不同，这条故意读仓库真实资产，
    /// 就是要在 CI 里当场发现「代码里新增了一个 `_key` 常量，但忘了
    /// 补对应 `.ftl` 条目」这类遗漏。
    ///
    /// P7 第一批（只读观测 HUD，`ll-ui::hud`）追加了状态栏/角色面板/
    /// 背包/装备面板一批键——这批键不来自任何 `_key` 字段（它们是 UI
    /// 布局自身的标签与 `AttributeKind`/`EquipSlot` 的展示名，不是内容
    /// 表字段），但同样必须在真实 `.ftl` 里有译文，纳入同一条覆盖率
    /// 检查，理由一致：不要让「新增一个 HUD 常量，但忘了补 `.ftl`」这
    /// 类遗漏在 CI 里潜伏下去。
    const PRODUCTION_KEYS: &[&str] = &[
        "window.title",
        "lostland:keybind.action.up",
        "lostland:keybind.action.down",
        "lostland:keybind.action.left",
        "lostland:keybind.action.right",
        "lostland:keybind.action.confirm",
        "lostland:keybind.action.cancel",
        "lostland:keybind.action.menu",
        "lostland:keybind.action.map",
        "lostland:keybind.action.wait",
        "lostland:keybind.action.screenshot",
        "lostland:keybind.action.zoom_in",
        "lostland:keybind.action.zoom_out",
        "lostland:race.human.display_name",
        "lostland:race.dwarf.display_name",
        "lostland:race.elf.display_name",
        "lostland:class.warrior.display_name",
        "lostland:class.mage.display_name",
        "lostland:class.ranger.display_name",
        "lostland:subclass.duelist.display_name",
        "lostland:subclass.apprentice.display_name",
        // 上面十一条（三个种族、四个职业、剑舞者/学徒两个副职）现在
        // **全部**是真实生产内容，都走 mod 脚本注册：本体职业/技能/
        // 副职/任务迁移批次把 `materialize_base_{classes,skills,
        // subclasses,quests}` 四个函数连同它们的测试夹具一并删除，
        // 那四个函数此前从来不在生产装载路径上（见 `ll_mod::class`
        // 模块文档同名一节），于是这批键此前指向的内容根本没被装载
        // 过。下面八条（四个制作类副职 + 四个配方类别）来自副职获得
        // 机制批次，一直就是真实生产内容。
        //
        // 技能与任务没有出现在本清单里，因为 `SkillAttrs`/`QuestAttrs`
        // 根本没有 `display_name_key` 字段——那不是遗漏，是这两类内容
        // 至今没有任何展示层需要它们的名字。
        "lostland:subclass.artisan.display_name",
        "lostland:subclass.tailor.display_name",
        "lostland:subclass.alchemist.display_name",
        "lostland:subclass.cook.display_name",
        "lostland:recipe_category.forging.display_name",
        "lostland:recipe_category.tailoring.display_name",
        "lostland:recipe_category.alchemy.display_name",
        "lostland:recipe_category.cooking.display_name",
        "save-mod-missing",
        "save-mod-version-mismatch",
        "mod-dependency-version-mismatch",
        "hud-status-time-label",
        "hud-status-health-label",
        "hud-status-mana-label",
        "lostland:season.spring.display_name",
        "lostland:season.summer.display_name",
        "lostland:season.autumn.display_name",
        "lostland:season.winter.display_name",
        "hud-character-panel-title",
        "hud-character-level-label",
        "hud-character-experience-label",
        "hud-character-modifiers-title",
        "hud-character-modifiers-empty",
        "lostland:attribute.strength.display_name",
        "lostland:attribute.dexterity.display_name",
        "lostland:attribute.constitution.display_name",
        "lostland:attribute.intelligence.display_name",
        "lostland:attribute.willpower.display_name",
        "lostland:attribute.charisma.display_name",
        "hud-inventory-panel-title",
        "hud-inventory-empty",
        "hud-inventory-durability-label",
        "hud-equipment-panel-title",
        "hud-equipment-empty-slot",
        "lostland:equip_slot.main_hand.display_name",
        "lostland:equip_slot.off_hand.display_name",
        "lostland:equip_slot.head.display_name",
        "lostland:equip_slot.face.display_name",
        "lostland:equip_slot.eyes.display_name",
        "lostland:equip_slot.neck.display_name",
        "lostland:equip_slot.body.display_name",
        "lostland:equip_slot.outer.display_name",
        "lostland:equip_slot.back.display_name",
        "lostland:equip_slot.shoulder_l.display_name",
        "lostland:equip_slot.shoulder_r.display_name",
        "lostland:equip_slot.arm_l.display_name",
        "lostland:equip_slot.arm_r.display_name",
        "lostland:equip_slot.hand_l.display_name",
        "lostland:equip_slot.hand_r.display_name",
        "lostland:equip_slot.belt.display_name",
        "lostland:equip_slot.tasset.display_name",
        "lostland:equip_slot.legs.display_name",
        "lostland:equip_slot.boot_l.display_name",
        "lostland:equip_slot.boot_r.display_name",
        "lostland:equip_slot.ring_l.display_name",
        "lostland:equip_slot.ring_r.display_name",
        "lostland:equip_slot.unknown.display_name",
    ];

    /// 仓库根目录——`ll-i18n` 位于 `crates/ll-i18n`，向上两级到根。
    fn repo_locales_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("locales")
    }

    #[test]
    fn 真实资产目录覆盖全部本体键的中文翻译() {
        // Arrange
        let catalog = Catalog::load_dir(&repo_locales_dir());

        // Act & Assert：任何一条真的缺译文都会退化成键名本身，
        // 与键相等即视为该键未被覆盖。
        for key in PRODUCTION_KEYS {
            let text = catalog.resolve("zh-CN", key);
            assert_ne!(&text, key, "键 {key} 在 zh-CN.ftl 里没有对应译文");
        }
    }

    #[test]
    fn 真实资产目录覆盖全部本体键的英文翻译() {
        // Arrange
        let catalog = Catalog::load_dir(&repo_locales_dir());

        // Act & Assert
        for key in PRODUCTION_KEYS {
            let text = catalog.resolve("en", key);
            assert_ne!(&text, key, "键 {key} 在 en.ftl 里没有对应译文");
        }
    }

    #[test]
    fn 真实资产目录里同一批键的中英文文本逐一互不相同() {
        // 比前两条更强的断言：不只是「两种语言各自都有译文」，而是
        // 同一个键在两种语言下产出的文本确实不同——否则无法排除
        // 「两份 .ftl 手滑复制成了同一份内容」这种两条测试各自都通过
        // 但本地化其实没有真正切换的情形。
        // Arrange
        let catalog = Catalog::load_dir(&repo_locales_dir());

        // Act & Assert
        for key in PRODUCTION_KEYS {
            let zh_text = catalog.resolve("zh-CN", key);
            let en_text = catalog.resolve("en", key);
            assert_ne!(zh_text, en_text, "键 {key} 的中英文译文相同，怀疑内容重复");
        }
    }
}
