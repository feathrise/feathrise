//! 空气墙对象面板：固定行池与对象级事务接入编辑器阶段。
mod components; // 界面标记与动作。
mod scene; // 一次创建固定控件。
mod systems; // 选择、修改与回显。
use bevy::prelude::*; // 插件安装。
pub(crate) use components::WallPanelSlot; // 左栏提供挂载位置。
#[derive(Resource, Default)]
pub(super) struct PanelState {
    // 页码与名称编辑的稳定身份。
    page: usize, // 每页八条，实体数量不随地图增长。
    name_target: Option<(feathrise_map_content::MapId, String)>, // 防止名称写入另一墙。
    selection_valid: bool, // 只在会话变更时复验稳定ID，空列表不显示所选对象操作。
}
pub(crate) fn install(app: &mut App) {
    // 复用统一输入与展示阶段。
    app.init_resource::<PanelState>()
        .add_systems(
            Update,
            scene::setup.in_set(crate::MapEditorSystemSet::Presentation),
        )
        .add_systems(
            Update,
            systems::actions.in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            systems::sync
                .after(scene::setup)
                .in_set(crate::MapEditorSystemSet::Presentation),
        );
}

/// 离屏夹具只安装原固定行池和只读回显，不接入墙体作者动作或后台任务。
#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<PanelState>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_systems(Update, (scene::setup, systems::sync).chain()
            .after(crate::ui::scene_inspector::sync_selected_tool_properties)
            .in_set(crate::MapEditorSystemSet::Presentation));
}
