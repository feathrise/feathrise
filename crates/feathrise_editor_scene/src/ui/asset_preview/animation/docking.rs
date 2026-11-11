//! 中央 GLB 的原控件分流。属性跟随工程选择，时间轴与播放跟随中央文件。

use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_editor_ui::EditorButtonClick;

use super::{
    components::{AnimationPanel, AnimationPoseToolbar},
    resources::AnimationEditorState,
    timeline_dock::Body,
};
use crate::{
    resources::SceneEditorState,
    ui::{
        asset_preview::{AssetPreviewWorkspace, gltf_tree, types::AssetPreviewCanvas},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};

#[derive(Component, Clone, Copy, Default)]
pub(in crate::ui::asset_preview) struct InlineAnimationHost;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationPropertiesHost;

/// 三组原位归还小预览。编辑和运输控件始终留在时间轴面板。
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum PropertySection {
    Clips,
    Binding,
    Pose,
}

#[derive(Component, Clone, Copy)]
pub(super) struct InlinePropertyHost(pub(super) PropertySection);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct BindModelButton;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct BindModelStatus;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct BindModelHome;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct BindModelPropertiesHost;

pub(in crate::ui::asset_preview) fn properties_host() -> impl Scene {
    bsn! {
        AnimationPropertiesHost
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(3) }
    }
}

pub(in crate::ui::asset_preview) fn binding_properties_host() -> impl Scene {
    bsn! {
        BindModelPropertiesHost
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center, column_gap: px(3), row_gap: px(3) }
    }
}

#[derive(SystemParam)]
pub(super) struct Context<'w> {
    media: Option<Res<'w, AssetPreviewWorkspace>>,
    workspace: Option<Res<'w, VegetationPresetEditorState>>,
}

impl Context<'_> {
    pub(super) fn central_glb(&self) -> Option<&str> {
        gltf_tree::central_glb(self.media.as_deref()?, self.workspace.as_deref()?.active_tab)
    }

    pub(super) fn timeline_visible(&self, state: &AnimationEditorState) -> bool {
        self.central_glb().is_some() && (state.visible() || state.in_place_rig_preview())
    }
}

pub(super) fn sync(
    mut commands: Commands,
    context: Context,
    editor: Res<SceneEditorState>,
    state: Res<AnimationEditorState>,
    mut properties: Query<(Entity, &mut Node), With<AnimationPropertiesHost>>,
    mut inline: Query<(Entity, &mut Node), (With<InlineAnimationHost>, Without<AnimationPropertiesHost>)>,
    canvas: Query<Entity, With<AssetPreviewCanvas>>,
    body: Query<Entity, With<Body>>,
    panel: Query<(Entity, &ChildOf), With<AnimationPanel>>,
    mut pose_toolbar: Query<(Entity, &ChildOf, &mut Node), (With<AnimationPoseToolbar>, Without<AnimationPropertiesHost>, Without<InlineAnimationHost>, Without<InlinePropertyHost>)>,
    mut homes: Query<
        (Entity, &InlinePropertyHost, &mut Node),
        (Without<AnimationPropertiesHost>, Without<InlineAnimationHost>, Without<BindModelPropertiesHost>),
    >,
    sections: Query<(Entity, &PropertySection, &ChildOf)>,
    mut binding_properties: Query<
        (Entity, &mut Node),
        (With<BindModelPropertiesHost>, Without<AnimationPropertiesHost>, Without<InlineAnimationHost>, Without<AnimationPoseToolbar>),
    >,
    binding_home: Query<Entity, With<BindModelHome>>,
    mut binding_button: Query<(Entity, &ChildOf, Option<&mut EditorButtonClick>), With<BindModelButton>>,
    binding_status: Query<(Entity, &ChildOf), With<BindModelStatus>>,
) {
    let (Ok((properties, mut host)), Ok((inline, mut home)), Ok(canvas)) =
        (properties.single_mut(), inline.single_mut(), canvas.single())
    else {
        return;
    };
    let glb = context.central_glb();
    let display = if glb == editor.selected_asset.as_deref() && context.timeline_visible(&state) {
        Display::Flex
    } else {
        Display::None
    };
    if host.display != display {
        host.display = display;
    }
    let docked = glb.is_some();
    let inline_display = if !docked && state.visible() { Display::Flex } else { Display::None };
    if home.display != inline_display {
        home.display = inline_display;
    }
    let mut inline_targets = [None; 3];
    for (entity, marker, mut node) in &mut homes {
        inline_targets[marker.0 as usize] = Some(entity);
        let display = if docked { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }
    // 明确右侧小组顺序，换宿主只改父子关系，不重建按钮或关键帧槽。
    for section in [PropertySection::Clips, PropertySection::Binding, PropertySection::Pose] {
        let target = if docked { Some(properties) } else { inline_targets[section as usize] };
        if let Some(target) = target
            && let Some((entity, _, parent)) = sections.iter().find(|(_, group, _)| **group == section)
            && parent.parent() != target
        {
            commands.entity(entity).insert(ChildOf(target));
        }
    }
    if let Ok((entity, parent, mut node)) = pose_toolbar.single_mut() {
        let target = if docked { properties } else { canvas };
        if parent.parent() != target {
            // 同一组按钮换父节点时才改布局，画布内的真实姿势手柄继续使用原系统。
            node.position_type = if docked { PositionType::Relative } else { PositionType::Absolute };
            node.left = if docked { Val::Auto } else { px(8) };
            node.top = if docked { Val::Auto } else { px(8) };
            node.width = if docked { percent(100) } else { px(76) };
            node.flex_direction = if docked { FlexDirection::Row } else { FlexDirection::Column };
            node.flex_wrap = if docked { FlexWrap::Wrap } else { FlexWrap::NoWrap };
            node.column_gap = px(3);
            commands.entity(entity).insert(ChildOf(target));
        }
    }
    if let Ok((entity, parent)) = panel.single() {
        let target = if docked { body.single().ok() } else { Some(inline) };
        if let Some(target) = target && parent.parent() != target {
            commands.entity(entity).insert(ChildOf(target));
        }
    }
    let (Ok((binding_properties, mut binding_host)), Ok(binding_home), Ok((button, parent, click))) =
        (binding_properties.single_mut(), binding_home.single(), binding_button.single_mut())
    else {
        return;
    };
    let borrowed = state.visible() && !state.in_place_rig_preview()
        && glb.zip(editor.selected_asset.as_deref()).is_some_and(|(source, selected)| {
        source != selected
            && std::path::Path::new(selected).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    });
    let display = if borrowed { Display::Flex } else { Display::None };
    if binding_host.display != display {
        binding_host.display = display;
    }
    let target = if borrowed { binding_properties } else { binding_home };
    if parent.parent() != target {
        // 属性随选择更新时只借原绑定入口。回到动作资产或收起中央标签时原位归还。
        if let Some(mut click) = click {
            click.invalidate();
        }
        commands.entity(target).insert_child(0, button);
    }
    if let Ok((entity, parent)) = binding_status.single()
        && parent.parent() != target
    {
        // 兼容性失败仍在按钮旁回显，使用同一份结果文字。
        commands.entity(entity).insert(ChildOf(target));
    }
}

#[cfg(feature = "validation")]
pub(in crate::ui::asset_preview) fn properties_panel_visible(world: &mut World) -> bool {
    world
        .query::<(&PropertySection, &ChildOf, &ComputedNode)>()
        .iter(world)
        .any(|(section, parent, node)| {
            *section == PropertySection::Clips && node.size().min_element() > 1.0
                && world.get::<AnimationPropertiesHost>(parent.parent()).is_some()
        })
}

#[cfg(feature = "validation")]
pub(crate) struct ProjectClipProperties {
    pub label: String,
    pub parameters: String,
    pub title_width: f32,
    pub section_width: f32,
    pub title_lines: u32,
}

#[cfg(feature = "validation")]
pub(crate) fn project_clip_properties(world: &mut World) -> Option<ProjectClipProperties> {
    use super::components::AnimationLabel;
    use bevy::text::TextLayoutInfo;
    if !properties_panel_visible(world) {
        return None;
    }
    let mut clip = None;
    let mut parameters = None;
    for (label, text, node, visible, layout, parent) in world
        .query::<(&AnimationLabel, &Text, &ComputedNode, &InheritedVisibility, &TextLayoutInfo, &ChildOf)>()
        .iter(world)
    {
        if !visible.get() || node.size().min_element() <= 1.0 {
            continue;
        }
        match label {
            AnimationLabel::Clip => {
                let section = world.get::<ComputedNode>(parent.parent())?;
                let lines = layout.glyphs.iter().map(|glyph| glyph.line_index).max()? + 1;
                clip = Some((text.as_str().to_owned(), node.size().x, section.size().x, lines));
            }
            AnimationLabel::ClipParameters => parameters = Some(text.as_str().to_owned()),
            _ => (),
        }
    }
    let (label, title_width, section_width, title_lines) = clip?;
    Some(ProjectClipProperties { label, parameters: parameters?, title_width, section_width, title_lines })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{data::tests::test_document, resources::Session};
    use crate::ui::{
        asset_preview::types::{AssetPreviewModelTarget, ExpandedAssetPreview},
        vegetation_preset_editor::PresetWorkspaceTab,
    };

    #[test]
    fn other_model_properties_borrow_only_the_existing_bind_button_and_return_it_in_place() {
        let mut app = App::new();
        app.init_resource::<VegetationPresetEditorState>()
            .insert_resource(SceneEditorState {
                selected_asset: Some("动画.glb".into()),
                ..default()
            })
            .insert_resource(AssetPreviewWorkspace {
                expanded: Some(ExpandedAssetPreview {
                    id: 1,
                    path: "动画.glb".into(),
                    return_tab: PresetWorkspaceTab::Scene,
                    view: default(),
                    target: AssetPreviewModelTarget::Scene0,
                    audio_position: 0.0,
                    restore_cursor: 0,
                }),
                ..default()
            })
            .init_resource::<AnimationEditorState>()
            .add_systems(Update, sync);
        let world = app.world_mut();
        world.resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::AssetPreview;
        let source = std::path::PathBuf::from("动画.glb");
        let mut animation = world.resource_mut::<AnimationEditorState>();
        animation.active = Some(source.clone());
        animation.sessions.insert(source, Session::new(test_document()));
        animation.visible = true;
        world.spawn(AssetPreviewCanvas);
        let inline = world.spawn((InlineAnimationHost, Node::default())).id();
        let timeline = world.spawn(Body).id();
        let properties = world.spawn((AnimationPropertiesHost, Node::default())).id();
        let binding_properties = world.spawn((BindModelPropertiesHost, Node::default())).id();
        let panel = world.spawn((AnimationPanel, ChildOf(inline))).id();
        let clips_home = world.spawn((InlinePropertyHost(PropertySection::Clips), Node::default(), ChildOf(panel))).id();
        let clips = world.spawn((PropertySection::Clips, ChildOf(clips_home))).id();
        let binding_home = world.spawn((InlinePropertyHost(PropertySection::Binding), Node::default(), ChildOf(panel))).id();
        let home = world.spawn((BindModelHome, PropertySection::Binding, ChildOf(binding_home))).id();
        let button = world.spawn((BindModelButton, ChildOf(home), EditorButtonClick::default())).id();
        let sibling = world.spawn(ChildOf(home)).id();
        let status = world.spawn((BindModelStatus, ChildOf(home))).id();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<ChildOf>(panel).unwrap().parent(), timeline);
        assert_eq!(world.get::<ChildOf>(clips).unwrap().parent(), properties);
        assert_eq!(world.get::<ChildOf>(button).unwrap().parent(), home);
        world.resource_mut::<SceneEditorState>().selected_asset = Some("角色.GLB".into());
        world.entity_mut(button).insert(EditorButtonClick::triggered());
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<ChildOf>(panel).unwrap().parent(), timeline);
        assert_eq!(world.get::<Node>(properties).unwrap().display, Display::None);
        assert_eq!(world.get::<Node>(binding_properties).unwrap().display, Display::Flex);
        assert_eq!(world.get::<ChildOf>(button).unwrap().parent(), binding_properties);
        assert_eq!(world.get::<ChildOf>(status).unwrap().parent(), binding_properties);
        assert!(!world.get::<EditorButtonClick>(button).unwrap().is_clicked());
        assert_eq!(world.get::<ChildOf>(sibling).unwrap().parent(), home);
        world.resource_mut::<SceneEditorState>().selected_asset = Some("图片.png".into());
        app.update();
        let world = app.world_mut();
        assert_eq!(world.get::<Node>(binding_properties).unwrap().display, Display::None);
        assert_eq!(world.get::<Children>(home).unwrap().first().copied(), Some(button));
        assert_eq!(world.get::<Children>(home).unwrap().last().copied(), Some(status));
        world.resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Scene;
        app.update();
        let world = app.world();
        assert_eq!(world.get::<ChildOf>(panel).unwrap().parent(), inline);
        assert_eq!(world.get::<Node>(inline).unwrap().display, Display::Flex);
        assert_eq!(world.get::<ChildOf>(button).unwrap().parent(), home);
        assert_eq!(world.get::<ChildOf>(home).unwrap().parent(), binding_home);
        assert_eq!(world.get::<ChildOf>(clips).unwrap().parent(), clips_home);
    }
}
