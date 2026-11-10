//! 场景侧栏的目录、当前场景、所选对象及原操作反馈；不拥有目录或场景事务。

mod components;
mod context_menu;
mod feedback;
mod lookup;
mod properties;
mod properties_scene;
mod resources;
mod scene;
mod systems;
mod tree_style;
mod types;

pub(crate) use components::{SceneSidebarActionBinding, ScenePropertiesEditBinding};
pub(crate) use context_menu::SceneDirectoryMenuState;
#[cfg(test)]
pub(crate) use context_menu::{DirectoryMenuSnapshot, install_snapshot, assert_snapshot_bounds};
pub(crate) use resources::{SceneSidebarFeedback, SceneSidebarInputContext, SceneBrowserTreeIcons};
pub(crate) use scene::scene_page;
pub(crate) use properties_scene::scene_properties_page;
pub(crate) use tree_style::tree_row_colors;

/// 新宿主先接相同的目录右键实体与手势；目录写入和元数据仍由宿主独立接线。
pub(crate) fn install_workbench_menu(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    context_menu::install(app);
    app.init_resource::<SceneSidebarFeedback>()
        .add_systems(Update, systems::sync_sidebar.in_set(crate::SceneEditorSystemSet::Presentation));
}

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    context_menu::install(app);
    app.init_resource::<SceneSidebarFeedback>().add_systems(
        Update,
        systems::sync_sidebar
            .after(crate::ui::rows::sync_scene_rows)
            .after(crate::ui::rows::sync_dynamic_list_layouts)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
    app.add_systems(
        Update,
        properties::sync_scene_properties
            .after(crate::ui::sidebars::sync_editor_sidebar_presentation)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

/// 完整工作台离屏夹具复用同一元数据同步，不安装场景目录IO和结构事务。
#[cfg(test)]
pub(crate) fn install_properties_snapshot_presentation(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(Update, properties::sync_scene_properties
        .after(crate::ui::sidebars::sync_editor_sidebar_presentation));
}

#[cfg(test)]
pub(crate) fn assert_properties_snapshot(world: &mut bevy::prelude::World, id: &feathrise_scene_content::SceneId, name: &str) {
    use bevy::prelude::*;
    use types::ScenePropertiesField;
    let actual = world.query::<(&components::ScenePropertiesText, &Text, &ComputedNode)>()
        .iter(world).map(|(field, text, computed)| (field.0, text.0.clone(), computed.size())).collect::<Vec<_>>();
    for (field, expected) in [(ScenePropertiesField::Name, name.to_owned()), (ScenePropertiesField::Id, id.to_string())] {
        let (_, value, size) = actual.iter().find(|(kind, ..)| *kind == field).unwrap();
        assert_eq!(*value, expected);
        assert!(size.x > 20.0 && size.y > 10.0, "真实所选场景字段必须参与属性页布局");
    }
    assert!(actual.iter().any(|(field, value, size)| *field == ScenePropertiesField::Source
        && value.contains("个独立网格") && size.min_element() > 10.0));
    // 检查真实字形高度及相邻行边界，不能只证明Text节点非零而漏掉换行后的覆盖。
    let mut bounds = Vec::new();
    for (field, computed, at, layout, parent) in world.query::<(&components::ScenePropertiesText,
        &ComputedNode, &UiGlobalTransform, &bevy::text::TextLayoutInfo, &ChildOf)>().iter(world) {
        if computed.size().min_element() <= 0.0 { continue; }
        let row = world.get::<ComputedNode>(parent.parent()).unwrap();
        let row_at = world.get::<UiGlobalTransform>(parent.parent()).unwrap().translation;
        assert!(layout.size.y <= computed.size().y + 1.1,
            "场景属性字形高度超出字段：{:?} 字形={:?} 字段={:?}", field.0, layout.size, computed.size());
        assert!(at.translation.y - computed.size().y / 2.0 >= row_at.y - row.size().y / 2.0 - 1.1);
        assert!(at.translation.y + computed.size().y / 2.0 <= row_at.y + row.size().y / 2.0 + 1.1);
        bounds.push((field.0.index(), row_at.y - row.size().y / 2.0, row_at.y + row.size().y / 2.0));
    }
    bounds.sort_by_key(|(index, ..)| *index);
    assert!(bounds.windows(2).all(|pair| pair[1].1 >= pair[0].2 - 1.1), "场景属性换行行不能与下一项重叠：{bounds:?}");
    let (binding, action, node, computed) = world.query::<(&components::ScenePropertiesEditBinding,
        &crate::components::SceneEditorActionButton, &Node, &ComputedNode)>().single(world).unwrap();
    assert!(binding.enabled && binding.scene_id.as_ref() == Some(id), "编辑入口必须绑定已呈现场景及其正式清单");
    assert_eq!(action.0, crate::types::EditorAction::SceneFolderRename);
    assert_eq!(node.display, Display::Flex);
    assert!(computed.size().min_element() >= 20.0, "原事务编辑按钮必须真实可见");
}

#[cfg(test)]
mod tests;
