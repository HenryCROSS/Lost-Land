//! 双模式存档：模式2（纯永久死亡）与模式3（自由读档），以及两者之间
//! 唯一允许的单向、不可逆降级。
//!
//! 规格 §11.2 决策 10：模式2 只保留断点续玩存档（角色死亡即终局，玩家
//! 不能靠读旧档撤销后果）；模式3 允许多存档位、随时读档。项目所有者
//! 已定：**模式2 → 模式3 单向降级，不可逆，降级动作写入头部并永久
//! 标记**——允许玩家在模式2 感到太严苛时退让为模式3，但不允许反过来
//! 把降级动作本身撤销（否则「永久死亡」这条承诺对玩家就不再可信：
//! 死过一次之后随时能把存档标记改回模式2，跟从一开始就是模式3没有
//! 区别）。
//!
//! # 「不可逆」如何在类型/数据层面保证，而不是只靠约定
//!
//! 若 `downgraded_from_permadeath: bool` 是 [`SaveMode::FreeSave`] 的
//! **公开**字段，任何持有 `&mut SaveMode` 的调用方都能直接
//! `if let SaveMode::FreeSave { downgraded_from_permadeath, .. } =
//! &mut mode { *downgraded_from_permadeath = false; }`——那样「不可逆」
//! 就只是一句写在文档里的约定，没有任何东西真正拦住它。本模块把这个
//! 字段设为**私有**（模块边界之外不可见，不是 `pub`）：外部 crate
//! （甚至本 crate 内 `mode.rs` 之外的模块，例如 `save_file.rs`）既不能
//! 直接构造一个 `downgraded_from_permadeath: false` 的 `FreeSave`
//! 假装它「从来没降级过」，也不能拿到手上已有的一个 `SaveMode` 之后
//! 把这个标记改回 `false`——唯一能创建/修改这个字段的代码，是本模块
//! 自己的 [`SaveMode::downgrade`]（只会把它设成 `true`）与
//! [`SaveMode::fresh_free_save`]（从一开始就诚实地设成 `false`，不是
//! 「降级」）。**没有任何公开 API 能把已经为 `true` 的这个标记改回
//! `false`**——这是编译器实际强制的边界，不是只靠代码审查维持的约定。
//! `serde` 的 `derive` 宏在本模块内展开，可以照常访问私有字段完成
//! 序列化/反序列化，不需要为此把字段公开。

use serde::{Deserialize, Serialize};

/// 存档模式：模式2（纯永久死亡）或模式3（自由读档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveMode {
    /// 模式2：纯永久死亡，仅保留断点续玩存档（见
    /// [`Self::save_slot_limit`]）。
    Permadeath,
    /// 模式3：自由读档，多存档位。
    FreeSave {
        /// 这局游戏是否曾经从 [`SaveMode::Permadeath`] 降级而来——
        /// 私有字段，见模块文档「不可逆如何保证」。即便当前就是
        /// `FreeSave`，这个标记也不会因为任何操作被移除，永久记录
        /// 「这局游戏曾经降级过」。
        downgraded_from_permadeath: bool,
    },
}

impl SaveMode {
    /// 一局从一开始就选择模式3的存档——不是从模式2降级而来，标记
    /// 诚实地设为 `false`。
    pub fn fresh_free_save() -> Self {
        SaveMode::FreeSave {
            downgraded_from_permadeath: false,
        }
    }

    /// 唯一允许的模式变化路径：`Permadeath → FreeSave { downgraded_from_permadeath: true }`。
    ///
    /// 任何其他方向都返回 `None`：`FreeSave` 上调用本方法（无论
    /// `downgraded_from_permadeath` 是 `true` 还是 `false`）——「已经是
    /// `FreeSave`」不存在「再降一次」的意义，也绝不允许借着「降级」的
    /// 名义把标记从 `true` 悄悄改成别的什么值。这正是「`FreeSave`
    /// 无法升级回 `Permadeath`」这条要求在类型上的落点：本方法根本不
    /// 存在任何返回 `Permadeath` 的分支。
    pub fn downgrade(self) -> Option<SaveMode> {
        match self {
            SaveMode::Permadeath => Some(SaveMode::FreeSave {
                downgraded_from_permadeath: true,
            }),
            SaveMode::FreeSave { .. } => None,
        }
    }

    /// 这局存档是否曾经从 `Permadeath` 降级而来。
    pub fn was_downgraded_from_permadeath(self) -> bool {
        matches!(
            self,
            SaveMode::FreeSave {
                downgraded_from_permadeath: true
            }
        )
    }

    /// 当前模式允许几个存档位——本方法只交付判定逻辑，存档管理 UI
    /// （不在本任务范围）应据此决定「新建存档」这个操作在 `Permadeath`
    /// 模式下是否应该覆盖已有的那一份，而不是允许并存多份。
    pub fn save_slot_limit(self) -> SaveSlotLimit {
        match self {
            SaveMode::Permadeath => SaveSlotLimit::Single,
            SaveMode::FreeSave { .. } => SaveSlotLimit::Unlimited,
        }
    }
}

/// [`SaveMode::save_slot_limit`] 的返回值：当前模式允许几个存档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSlotLimit {
    /// 只保留断点续玩这一份——新存档应当覆盖旧的，而不是并存。
    Single,
    /// 允许任意数量的并存存档位。
    Unlimited,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permadeath可以降级为freesave() {
        // Arrange & Act
        let downgraded = SaveMode::Permadeath.downgrade();

        // Assert
        assert!(matches!(
            downgraded,
            Some(SaveMode::FreeSave {
                downgraded_from_permadeath: true
            })
        ));
    }

    #[test]
    fn freesave无法升级回permadeath() {
        // Arrange
        let free_save = SaveMode::fresh_free_save();

        // Act
        let result = free_save.downgrade();

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 降级后的freesave标记为真() {
        // Arrange & Act
        let downgraded = SaveMode::Permadeath
            .downgrade()
            .expect("Permadeath 必然可以降级");

        // Assert
        assert!(downgraded.was_downgraded_from_permadeath());
    }

    #[test]
    fn 已经降级的freesave再次调用downgrade仍不产生permadeath() {
        // 「不允许借着降级的名义把标记从 true 改成别的什么值」——即便
        // 对一个已经降级过的 FreeSave 再调用一次 downgrade,也不会产出
        // Permadeath 或任何标记被清除的结果。
        // Arrange
        let already_downgraded = SaveMode::Permadeath
            .downgrade()
            .expect("Permadeath 必然可以降级");

        // Act
        let result = already_downgraded.downgrade();

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 从一开始就是freesave时标记为假() {
        // Arrange & Act
        let fresh = SaveMode::fresh_free_save();

        // Assert
        assert!(!fresh.was_downgraded_from_permadeath());
    }

    #[test]
    fn permadeath模式只允许一个存档位() {
        // Arrange & Act & Assert
        assert_eq!(
            SaveMode::Permadeath.save_slot_limit(),
            SaveSlotLimit::Single
        );
    }

    #[test]
    fn freesave模式允许无限存档位() {
        // Arrange & Act & Assert
        assert_eq!(
            SaveMode::fresh_free_save().save_slot_limit(),
            SaveSlotLimit::Unlimited
        );
    }

    #[test]
    fn 降级标记经过json序列化往返后依然存在() {
        // 落地「降级动作写入头部并永久标记」——不能只在内存里的一次
        // 调用成立,存档往返（写出再读回）之后这个标记必须原样还在。
        // Arrange
        let downgraded = SaveMode::Permadeath
            .downgrade()
            .expect("Permadeath 必然可以降级");
        let json = serde_json::to_string(&downgraded).expect("SaveMode 应当总是可序列化");

        // Act
        let restored: SaveMode = serde_json::from_str(&json).expect("刚序列化的数据必然合法");

        // Assert
        assert!(restored.was_downgraded_from_permadeath());
    }
}
