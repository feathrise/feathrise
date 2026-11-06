//! 工程与资产右键菜单的固定UI节点标记，与其他编辑器按钮保持查询互斥。

use bevy::prelude::Component; // 菜单节点只携带轻量身份。

/// 透明全屏命中遮罩：外部点击只关闭菜单，不能点透底下数字框或地图。
#[derive(Component)] // 只在菜单显示时参与布局与命中。
pub(super) struct AssetMenuBackdrop;

/// 贴近右键位置、在编辑器范围内避让边缘的小面板。
#[derive(Component)] // 与遮罩分开，背景不影响菜单自身定位。
pub(super) struct AssetMenuPopup;

/// 复制路径菜单项始终使用右键冻结的条目，不依赖当前左键选择。
#[derive(Component)] // 不复用EditorAction，避免落入普通按钮分派。
pub(super) struct AssetMenuCopyButton(pub super::types::AssetPathCopyKind);

/// 外部入口只操作右键冻结的工程或资产条目，不切换左键选择。
#[derive(Component)]
pub(super) struct AssetMenuExternalButton(pub crate::ui::external_files::ExternalFileAction);

/// 只读引用查询冻结右键文件路径，不读取当前左键选中资产。
#[derive(Component)]
pub(crate) struct AssetMenuReferencesButton;

/// 收藏动作同样冻结右键文件或目录，不借用左键选择。
#[derive(Component)]
pub(crate) struct AssetMenuFavoriteButton;
