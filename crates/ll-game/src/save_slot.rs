//! 存档槽位：从「唯一一份 `save.llsave`」变成一个目录下的多份。
//!
//! # 一份存档 = 一个世界，角色是世界里的一段生命
//!
//! 项目所有者的裁定串起来是这样一个模型：
//!
//! > 「保存存档的界面应该有个输入名字的地方让存档可标识。也或者每多久
//! > 自动保存一次。当然这是保存模式，肉鸽模式是只有自动保存的，并且
//! > 死亡就删除存档。」
//! > **（追问后的修正）**「死亡后变成一般模式，可以再创建角色然后选择
//! > 在某个地方出生。」
//!
//! 所以**死亡不删档**：世界比角色活得长。一个槽位在它的生命期里可以
//! 先后住过好几个角色，而它始终是同一个世界（同一份世界身份、同一个
//! 种子）。这就是存档头里 [`ll_content::header::SaveHeader::save_name`]
//! 与 `character_name` 必须是两个字段的原因。
//!
//! # 槽位标识为什么在创建那一刻定死
//!
//! [`SlotId`] 是**文件名主干**，由玩家输入的名字过滤而来。它在建档那
//! 一刻算一次，此后这个槽位永远写同一个文件——再存一次是**覆盖**，不
//! 是新建。若每次存档都按当前名字重算文件名，玩家改一次名就会凭空多
//! 出一份档，而旧的那份还在原地，列表里出现两个同一个世界的条目。
//!
//! 展示名与文件名因此是两件事：展示名是存档头里那一份（可以有空格、
//! 大小写），文件名是过滤后的 ASCII 主干。
//!
//! # 时间戳为什么自己算
//!
//! [`ll_content::header::SaveHeader::saved_at`] 是 Unix 秒。把它变成
//! 「2026-08-29 14:03」需要一次历法换算，而那个字段的文档已经明确
//! 否决过为此新增 `chrono` 这样的重依赖。[`format_saved_at`] 是二十
//! 来行纯整数算术，无依赖、可测、跨平台逐位相同。
//!
//! **按 UTC 显示，不按本地时区**：本地时区需要读操作系统的时区数据库
//! （又是一个依赖），而且会让同一份存档在两台机器上显示成不同时间。
//! 存档列表要回答的是「哪一份更新」，UTC 完全够用，且它是确定的。

use std::path::{Path, PathBuf};

use ll_content::header::SaveHeader;
use ll_content::mode::SaveMode;
use ll_content::save_file::load_from_header_only;

/// 存档目录名，相对数据目录——与 `mods/`、`assets/` 同一层。
pub const SAVES_DIR_NAME: &str = "saves";

/// 存档文件扩展名，与迁移前那份单文件存档一致。
pub const SAVE_EXTENSION: &str = "llsave";

/// 槽位标识（也就是文件名主干）里允许出现的字符之外，一律换成这个。
const REPLACEMENT_CHAR: char = '_';

/// 名字被过滤成空时用的兜底主干。
const FALLBACK_STEM: &str = "save";

/// 存档名的长度上限（字符数）——够认出「哪一份是哪一份」，又不至于把
/// 那块居中面板撑破。
pub const MAX_SAVE_NAME_CHARS: usize = 24;

/// 一个存档槽位在磁盘上的身份：文件名主干。
///
/// 不是玩家看见的名字（那一份在存档头里），是**文件名**。见模块文档
/// 「槽位标识为什么在创建那一刻定死」。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotId(String);

impl SlotId {
    /// 把玩家输入的名字过滤成一个能出现在任何文件系统上的主干。
    ///
    /// 规则刻意保守：只保留 ASCII 字母、数字、`-`、`_`，其余（含空格、
    /// 中文、路径分隔符、`..`）一律换成 `_`；过滤后为空则退回
    /// [`FALLBACK_STEM`]。
    ///
    /// **这同时是一道路径穿越的闸门**：`/`、`\`、`:`、`.` 全部不在白
    /// 名单里，所以一个叫 `../../etc/passwd` 的存档名过滤出来是
    /// `_________etc_passwd`，落不出目标目录。白名单而不是黑名单——
    /// 仓库在 `ll_mod::asset_vfs` 那次路径校验事故里已经付过一次「黑
    /// 名单漏了一种写法」的代价。
    pub fn from_name(name: &str) -> SlotId {
        let stem: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    REPLACEMENT_CHAR
                }
            })
            .collect();
        let trimmed = stem.trim_matches(REPLACEMENT_CHAR);
        if trimmed.is_empty() {
            SlotId(FALLBACK_STEM.to_string())
        } else {
            SlotId(trimmed.to_string())
        }
    }

    /// 直接用一个已经合法的主干（例如从目录里扫出来的文件名）。
    pub fn from_stem(stem: &str) -> SlotId {
        SlotId::from_name(stem)
    }

    /// 主干本身。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 这个槽位在 `dir` 下的存档文件路径。
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.{}", self.0, SAVE_EXTENSION))
    }
}

/// 一次存档要写到哪里、写成什么名字——**进世界那一刻定下来**，此后
/// 手动存档、自动存档、退出存档全部写同一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTarget {
    /// 槽位标识（文件名主干）。
    pub id: SlotId,
    /// 存档文件的完整路径。
    pub path: PathBuf,
    /// 玩家看见的名字，写进存档头。
    pub name: String,
}

impl SaveTarget {
    /// 在 `dir` 下为 `name` 开一个**不与任何已有槽位冲突**的目标。
    ///
    /// 重名时追加 `-2`、`-3`……：两个都叫「测试」的世界是两份存档，
    /// 后一份不该悄悄覆盖前一份。展示名保持玩家输入的原样（两行都显示
    /// 「测试」），区分它们的是时间戳——**刻意不把 `-2` 塞进展示名**，
    /// 那是文件系统的实现细节，不是玩家给世界起的名字。
    pub fn create_in(dir: &Path, name: &str) -> SaveTarget {
        let base = SlotId::from_name(name);
        let mut candidate = base.clone();
        let mut suffix = 2u32;
        while candidate.path_in(dir).exists() {
            candidate = SlotId(format!("{}-{suffix}", base.as_str()));
            suffix += 1;
        }
        SaveTarget {
            path: candidate.path_in(dir),
            id: candidate,
            name: name.to_string(),
        }
    }

    /// 对准一个**已经存在**的槽位——读档之后继续往同一份里写。
    pub fn existing(slot: &SaveSlot) -> SaveTarget {
        SaveTarget {
            id: slot.id.clone(),
            path: slot.path.clone(),
            name: slot.display_name(),
        }
    }
}

/// 存档列表里的一项，**只读头部**得来。
///
/// 存档的物理布局是「4 字节长度前缀 + 头部 JSON + 压缩主体」，
/// [`load_from_header_only`] 只读前两段、**不解压主体**——列出二十份
/// 存档因此是二十次几百字节的读取，不是二十次全世界解压。这正是
/// [`ll_content::header::SaveHeader::world_seed`] 字段文档里
/// 「存档列表界面只读头部」那句话的兑现点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlot {
    /// 槽位标识（文件名主干）。
    pub id: SlotId,
    /// 存档文件路径。
    pub path: PathBuf,
    /// 玩家起的名字；空串表示这份存档写于 `save_name` 字段存在之前。
    pub save_name: String,
    /// 存档里那个角色的名字。
    pub character_name: String,
    /// 存档时间，Unix 秒。
    pub saved_at: i64,
    /// 这个世界的模式。
    pub mode: SaveMode,
}

impl SaveSlot {
    /// 列表里该显示的名字：玩家起的那个；没有（老存档）就退回文件名
    /// 主干——**绝不显示空白行**。
    pub fn display_name(&self) -> String {
        if self.save_name.trim().is_empty() {
            self.id.as_str().to_string()
        } else {
            self.save_name.clone()
        }
    }

    /// 这个槽位允许手动存档吗——判据走
    /// [`ll_content::mode::SaveMode`]，UI 层不自己 `match`。
    pub fn allows_manual_save(&self) -> bool {
        matches!(self.mode, SaveMode::FreeSave { .. })
    }

    fn from_header(id: SlotId, path: PathBuf, header: &SaveHeader) -> SaveSlot {
        SaveSlot {
            id,
            path,
            save_name: header.save_name.clone(),
            character_name: header.character_name.clone(),
            saved_at: header.saved_at,
            mode: header.mode(),
        }
    }
}

/// 列出 `dir` 下的全部存档槽位，**最近存过的排在最前**。
///
/// # 一份坏档不该让整个列表打不开
///
/// 读不出头部的条目（损坏、被别的程序占着、根本不是存档）**跳过并记一
/// 条 `warn`**，其余照常列出。相反的做法（整个列表返回错误）会让玩家
/// 因为一份无关的坏档而读不了另外五份好档，而他在界面上没有任何办法
/// 把那份坏的挑出来删掉。
///
/// # 次序为什么要显式定死
///
/// `read_dir` 的顺序依赖文件系统，跨平台不一致。按 `saved_at` 倒序、
/// 同刻按 [`SlotId`] 升序——后一半是必须的：只按时间排，两份同一秒写出
/// 的存档在两次运行里可能换位置，玩家的光标会莫名其妙落在另一份上。
pub fn list_slots(dir: &Path) -> Vec<SaveSlot> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // 目录不存在 = 一份存档都没有，是全新玩家的正常状态，不是错误。
        Err(_) => return Vec::new(),
    };
    let mut slots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some(SAVE_EXTENSION) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        match load_from_header_only(&path) {
            Ok(header) => slots.push(SaveSlot::from_header(
                SlotId::from_stem(stem),
                path.clone(),
                &header,
            )),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "这份存档的头部读不出来，跳过它；其余存档照常列出"
                );
            }
        }
    }
    slots.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then_with(|| a.id.cmp(&b.id)));
    slots
}

/// 把迁移前那份单文件存档收编进 `dir`。
///
/// 返回收编出来的路径；没有可收编的东西（老文件不存在，或者已经收编过）
/// 时返回 `None`。
///
/// # 为什么是复制而不是移动
///
/// 移动会让老文件消失。万一收编本身有缺陷（或者玩家想装回旧版本），
/// 原始那份就再也找不回来了。复制**永远不删除任何东西**，是这两个选择
/// 里唯一不可能造成数据丢失的那个。
///
/// 代价如实记录：玩家若把收编出来的槽位删掉，下次启动会再收编一次。
/// 这个方向是安全的——多一份档远好于少一份档。
///
/// # 为什么顺带把模式降级
///
/// 老存档头里记的一律是 [`SaveMode::Permadeath`]，因为迁移前
/// `save_game` 的 7 处调用点**全部硬编码**这个字面量——它是一个玩家
/// 从未做过的选择，不是一句承诺。把一个占位值当成「你选了永久死亡」
/// 去限制玩家（不能手动存档、死了要换角色），比反过来糟得多。
///
/// 而这次改判**恰好就是允许的那个方向**（肉鸽 → 普通），走的是
/// [`SaveMode::downgrade`] 本身，没有绕过任何东西、也没有新开一条
/// 机制。
pub fn adopt_legacy_save(legacy: &Path, dir: &Path) -> Option<PathBuf> {
    if !legacy.exists() {
        return None;
    }
    let target = SlotId::from_name(legacy_slot_stem()).path_in(dir);
    if target.exists() {
        // 已经收编过了。不再复制第二次，否则每次启动都会把老档重新
        // 盖到收编出来的那一份上，玩家在收编后的进度会被反复抹掉。
        return None;
    }
    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!(dir = %dir.display(), %error, "建不出存档目录，跳过老存档收编");
        return None;
    }
    if let Err(error) = std::fs::copy(legacy, &target) {
        tracing::warn!(
            from = %legacy.display(),
            to = %target.display(),
            %error,
            "老存档收编失败，原文件原样留在原地"
        );
        return None;
    }
    tracing::info!(
        from = %legacy.display(),
        to = %target.display(),
        "已把迁移前那份单文件存档收编成一个槽位（复制，原文件不动）"
    );
    Some(target)
}

/// 收编出来的那个槽位用什么主干。
///
/// 单独一个函数而不是散在两处字面量：收编的目标路径与「有没有收编过」
/// 的判据必须指的是同一个文件。
pub fn legacy_slot_stem() -> &'static str {
    "legacy"
}

/// 把 Unix 秒格式化成 `YYYY-MM-DD HH:MM`（UTC），见模块文档。
pub fn format_saved_at(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let seconds_of_day = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// 从「1970-01-01 起的天数」反算公历年月日。
///
/// Howard Hinnant 的 `civil_from_days`：把纪元挪到 0000-03-01，让闰日
/// 落在四年周期的**末尾**，于是整段换算变成几次整除，没有任何循环、
/// 没有闰年分支、也没有查表。纯整数算术 ⇒ 跨平台逐位相同（规格 §14.4
/// 对确定性的要求同样适用于任何进存档或进日志的展示文本）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // 把纪元从 1970-01-01 挪到 0000-03-01。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // 三月为第 0 月的内部编号。
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名字过滤只保留白名单字符() {
        // Arrange & Act & Assert
        assert_eq!(SlotId::from_name("MyWorld-1").as_str(), "MyWorld-1");
        assert_eq!(SlotId::from_name("my world").as_str(), "my_world");
        assert_eq!(SlotId::from_name("迷途大陆").as_str(), FALLBACK_STEM);
    }

    #[test]
    fn 路径穿越写法被过滤成落不出目录的主干() {
        // 白名单而不是黑名单——`/`、`\`、`:`、`.` 全都不在名单里。
        // Arrange
        let evil = "../../etc/passwd";

        // Act
        let id = SlotId::from_name(evil);

        // Assert
        assert!(
            !id.as_str().contains('/') && !id.as_str().contains('\\') && !id.as_str().contains('.'),
            "过滤后的主干不该含任何路径成分，实际是 {}",
            id.as_str()
        );
        let path = id.path_in(Path::new("saves"));
        assert_eq!(path.parent(), Some(Path::new("saves")));
    }

    #[test]
    fn 空名字与全非法名字都退回兜底主干() {
        // Arrange & Act & Assert
        assert_eq!(SlotId::from_name("").as_str(), FALLBACK_STEM);
        assert_eq!(SlotId::from_name("   ").as_str(), FALLBACK_STEM);
        assert_eq!(SlotId::from_name("___").as_str(), FALLBACK_STEM);
    }

    #[test]
    fn 时间戳按utc格式化() {
        // Arrange：1_755_000_000 秒 = 2025-08-12 12:00:00 UTC
        // （1_755_000_000 / 86_400 = 20312 天余 43_200 秒 = 12 小时整）。
        // Act & Assert
        assert_eq!(format_saved_at(1_755_000_000), "2025-08-12 12:00");
        assert_eq!(format_saved_at(0), "1970-01-01 00:00");
        // 闰日：2024-02-29。
        assert_eq!(format_saved_at(1_709_164_800), "2024-02-29 00:00");
    }

    #[test]
    fn 时间戳格式化对负数也不崩() {
        // 系统时钟异常时 `saved_at` 可能早于 1970（`now_unix_seconds`
        // 会退回 0，但存档是外部数据，不做「一定合法」的假设）。
        // Arrange & Act
        let text = format_saved_at(-1);

        // Assert
        assert_eq!(text, "1969-12-31 23:59");
    }

    #[test]
    fn 展示名为空时退回文件名主干() {
        // Arrange
        let slot = SaveSlot {
            id: SlotId::from_name("legacy"),
            path: PathBuf::from("saves/legacy.llsave"),
            save_name: String::new(),
            character_name: "旅人".to_string(),
            saved_at: 0,
            mode: SaveMode::Permadeath,
        };

        // Act & Assert
        assert_eq!(slot.display_name(), "legacy", "绝不显示空白行");
    }
}
