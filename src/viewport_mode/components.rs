//! 原工作台顶栏的模式按钮和状态标签；不另建工作台或覆盖原作者控件。

use super::ViewportMode;
use bevy::prelude::*;

#[derive(Component)]
pub(super) struct ModeButton(pub ViewportMode);

#[derive(Component)]
pub(super) struct ModeLabel;

#[derive(Component)]
pub(super) struct ModeStatus;

/// 工程3D按钮内部的绝对定位边框层；不占文字空间，连接文字由统一底栏呈现。
#[derive(Component)]
pub(super) struct ConnectionStatus;

/// 只呈现普通启动准备阶段的沿边层，与工程3D连接状态分别投影。
#[derive(Component)]
pub(super) struct LaunchStatus;

#[derive(Component)]
pub(super) struct LaunchBorderMounted;

/// 仅标记停止阶段由宿主施加的禁用，结束后不能撤销其他功能原有的禁用资格。
#[derive(Component)]
pub(super) struct CompilationLaunchDisabled;

/// 固定数量的重叠小段合成沿圆角边框移动的亮段，不生成逐帧实体或资产。
#[derive(Component)]
pub(super) struct ConnectionBorderSegment(pub u8);

#[derive(Component)]
pub(super) struct ProfileOption(pub feathrise_editor_core::ProjectBuildProfile);

#[derive(Component)]
pub(super) struct ProfileToggle;

#[derive(Component)]
pub(super) struct ProfileMenu;

#[derive(Component)]
pub(super) struct ProfileBackdrop;

/// None 显示当前配置，Some 为菜单中带选中标记的配置项。
#[derive(Component)]
pub(super) struct ProfileLabel(pub Option<feathrise_editor_core::ProjectBuildProfile>);

#[derive(Component)]
pub(super) struct BackendOptions;

#[derive(Component)]
pub(super) struct BackendOption(pub crate::native_play::ProjectBuildBackend);

#[derive(Component)]
pub(super) struct BackendLabel(pub crate::native_play::ProjectBuildBackend);

#[derive(Component)]
pub(super) struct BuildEnvironmentButton;

#[derive(Component)]
pub(super) struct BuildEnvironmentLabel;
