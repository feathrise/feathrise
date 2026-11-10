//! 验收只读真实作者节点、布局、相机和选择；模式标志不能代替已经收起/还原的界面证据。

use crate::{
    components::{MapEditorPaneSplitter, MapEditorRoot, MapEditorViewport},
    resources::{MapEditorLayoutState, MapEditorState, MapEditorViewportState},
    types::EditorPane,
    ui::gltf_browser::EditorGltfBrowserState,
    workbench::WorkbenchHeader,
};
use bevy::prelude::*;
use feathrise_editor_ui::UiInteraction;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UiPresentationProbe {
    pub entity: String,
    pub display: String,
    pub visibility: String,
    pub inherited_visible: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CameraPresentationProbe {
    pub entity: String,
    pub active: bool,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NavigationProbe {
    pub center: [f32; 3],
    pub extent: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub render_size: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SelectionProbe {
    pub map: Option<String>,
    pub directory_map: Option<String>,
    pub placement: Option<String>,
    pub portal: Option<String>,
    pub view: String,
    pub orthographic: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PanePresentationProbe {
    pub content_width: f32,
    pub inspector_width: f32,
    pub content_visible: bool,
    pub inspector_visible: bool,
    pub focus_restore: Option<(bool, bool)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PresentationProbe {
    pub roots: Vec<UiPresentationProbe>,
    pub headers: Vec<UiPresentationProbe>,
    /// 已布局且实际继承可见的中央节点：[宽、高、中心x、中心y]，均为物理像素。
    pub layout: Option<[f32; 4]>,
    pub camera: Option<CameraPresentationProbe>,
    pub navigation: Option<NavigationProbe>,
    pub selection: SelectionProbe,
    pub panes: Option<PanePresentationProbe>,
}

/// 原内容栏分隔线的物理中心、首选宽度、悬停及拖动状态；仅供自有工程实窗取证。
pub fn content_splitter_probe(world: &mut World) -> Option<(Vec2, f32, bool, bool)> {
    if !world.contains_resource::<super::Installed>() {
        return None;
    }
    let (center, hovered) = world
        .query::<(&MapEditorPaneSplitter, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &UiInteraction)>()
        .iter(world)
        .find_map(|(splitter, node, transform, visible, interaction)| {
            (splitter.0 == EditorPane::Content && visible.get() && !node.is_empty())
                .then_some((transform.translation, *interaction == UiInteraction::Hovered))
        })?;
    let layout = world.get_resource::<MapEditorLayoutState>()?;
    Some((center, layout.content_width, hovered, layout.drag.is_some_and(|drag| drag.pane == EditorPane::Content)))
}

/// 可变 World 仅用于建立定向 ECS 查询缓存；不写节点、模式、文档或任何验收成功状态。
pub fn presentation_probe(world: &mut World) -> Result<PresentationProbe, String> {
    if !world.contains_resource::<super::Installed>() {
        return Err("常驻验收作者端口未安装".into());
    }
    let mut roots = Vec::new();
    let mut headers = Vec::new();
    let mut query = world.query_filtered::<(
        Entity,
        &Node,
        &Visibility,
        &InheritedVisibility,
        Has<MapEditorRoot>,
    ), Or<(With<MapEditorRoot>, With<WorkbenchHeader>)>>();
    // 正式壳只有固定根和顶栏；异常数量明确失败，不以遍历作者实体总量掩盖异常布局。
    for (entity, node, visibility, inherited, root) in query.iter(world).take(9) {
        if roots.len() + headers.len() == 8 {
            return Err("作者根与顶栏数量超过固定验收预算".into());
        }
        let item = UiPresentationProbe {
            entity: format!("{entity:?}"),
            display: format!("{:?}", node.display),
            visibility: format!("{visibility:?}"),
            inherited_visible: inherited.get(),
        };
        if root {
            roots.push(item);
        } else {
            headers.push(item);
        }
    }
    roots.sort_by(|left, right| left.entity.cmp(&right.entity));
    headers.sort_by(|left, right| left.entity.cmp(&right.entity));
    let layout = world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<MapEditorViewport>>()
        .iter(world).find_map(|(node, transform, visible)| {
            (visible.get() && !node.is_empty()).then_some([
                node.size().x, node.size().y, transform.translation.x, transform.translation.y,
            ])
        });
    let viewport = world.get_resource::<MapEditorViewportState>();
    let camera = viewport.and_then(|viewport| {
        let camera = world.get::<Camera>(viewport.camera)?;
        let transform = world.get::<Transform>(viewport.camera)?;
        Some(CameraPresentationProbe {
            entity: format!("{:?}", viewport.camera),
            active: camera.is_active,
            position: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
        })
    });
    let navigation = viewport.map(|viewport| NavigationProbe {
        center: viewport.center.to_array(),
        extent: viewport.extent,
        yaw: viewport.yaw,
        pitch: viewport.pitch,
        render_size: viewport.render_size.to_array(),
    });
    let editor = world
        .get_resource::<MapEditorState>()
        .ok_or("作者界面状态未就绪")?;
    let selection = SelectionProbe {
        map: editor.selected_map_id.as_ref().map(|id| id.0.clone()),
        directory_map: editor.map_list_selection_id.as_ref().map(|id| id.0.clone()),
        placement: world
            .get_resource::<EditorGltfBrowserState>()
            .and_then(|browser| browser.selected_placement_id.clone()),
        portal: editor.selected_current_portal_id.clone(),
        view: format!("{:?}", editor.view),
        orthographic: editor.orthographic,
    };
    let panes = world
        .get_resource::<MapEditorLayoutState>()
        .map(|layout| PanePresentationProbe {
            content_width: layout.content_width,
            inspector_width: layout.inspector_width,
            content_visible: layout.content_visible,
            inspector_visible: layout.inspector_visible,
            focus_restore: layout.focus_restore,
        });
    Ok(PresentationProbe {
        roots,
        headers,
        layout,
        camera,
        navigation,
        selection,
        panes,
    })
}
