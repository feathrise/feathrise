//! 网格页和场景列表的常驻表现同步；只访问固定控件、当前选择、十个虚拟网格行与有界LOD可见行。
use feathrise_editor_ui::UiInteraction;
use super::{components::*, numeric, resources::*, systems};
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState, ui::theme as colors};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputSetValue,
    NumericInputValue, NumericValue,
    localization::EditorLocale,
};
use feathrise_scene_content::{SceneTerrainKind, SceneTerrainObject, MeshLodMode};

#[derive(SystemParam)]
pub(super) struct PanelPresentation<'w, 's> {
    texts: Query<
        'w,
        's,
        (
            Option<&'static Summary>,
            Option<&'static ListSummary>,
            Option<&'static CreationSummary>,
            Option<&'static SourceSummary>,
            Option<&'static LodSummary>,
            Option<&'static NativeLodSummary>,
            Option<&'static RepresentationSummary>,
            Option<&'static LodRowText>,
            Option<&'static RowText>,
            Option<&'static RowKind>,
            Option<&'static ActionLabel>,
            &'static mut Text,
        ),
        Or<(
            With<Summary>,
            With<ListSummary>,
            With<CreationSummary>,
            With<SourceSummary>,
            With<LodSummary>,
            With<NativeLodSummary>,
            With<RepresentationSummary>,
            With<LodRowText>,
            With<RowText>,
            With<RowKind>,
            With<ActionLabel>,
        )>,
    >,
    nodes: Query<
        'w,
        's,
        (
            Option<&'static PanelSection>,
            Option<&'static Slot>,
            Option<&'static Row>,
            Option<&'static LodRow>,
            Option<&'static LodValueFallback>,
            Option<&'static NativeLodEditor>,
            &'static mut Node,
        ),
        (Or<(With<PanelSection>, With<Slot>, With<Row>, With<LodRow>, With<LodValueFallback>)>, Without<ListViewport>, Without<ListContent>, Without<LodList>),
    >,
    list_viewport: Query<'w, 's, (&'static mut Node, &'static mut ScrollPosition), (With<ListViewport>, Without<ListContent>, Without<LodList>, Without<crate::ui::scene_inspector::InspectorBody>)>,
    list_content: Query<'w, 's, &'static mut Node, (With<ListContent>, Without<ListViewport>, Without<LodList>)>,
    lod_list: Query<'w, 's, (&'static LodList, &'static mut Node, &'static ComputedNode, &'static UiGlobalTransform), (Without<ListViewport>, Without<ListContent>)>,
    inspector: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform, &'static mut ScrollPosition), (With<crate::ui::scene_inspector::InspectorBody>, Without<ListViewport>)>,
    lod_bindings: Query<'w, 's, (&'static LodRow, &'static mut LodRowBinding, &'static mut UiInteraction, Option<&'static mut feathrise_editor_ui::EditorButtonClick>)>,
    lod_actions: Query<'w, 's, (&'static ActionButton, &'static mut LodActionBinding, &'static mut UiInteraction, Option<&'static mut feathrise_editor_ui::EditorButtonClick>), Without<LodRow>>,
    fields: Query<
        'w,
        's,
        (
            Entity,
            &'static Field,
            &'static NumericInputValue,
            Has<NumericInputDisabled>,
            Option<&'static LodFieldRow>,
            Option<&'static mut NumericFieldBinding>,
        ),
    >,
    buttons: Query<
        'w,
        's,
        (
            Entity,
            &'static ActionButton,
            Has<bevy::ui::InteractionDisabled>,
            Option<&'static mut BackgroundColor>,
            Option<&'static mut BorderColor>,
        ),
        Without<RowAction>,
    >,
}

pub(super) fn native_lods(object: &SceneTerrainObject) -> bool {
    matches!(
        object.terrain.kind,
        SceneTerrainKind::SparseClipmap | SceneTerrainKind::Voxel
    )
}
fn generic_lods(object: &SceneTerrainObject) -> bool {
    matches!(
        object.terrain.kind,
        SceneTerrainKind::Gltf
    )
}
pub(super) fn lod_count(object: Option<&SceneTerrainObject>, display: bool) -> usize {
    object.map_or(0, |object| {
        if let Some(voxel) = &object.voxel {
            if display {
                voxel.display_lods.len()
            } else {
                voxel.collision_lods.len()
            }
        } else if display {
            object.terrain.display_lods.as_ref().map_or(0, Vec::len)
        } else {
            object.terrain.collision_lods.len()
        }
    })
}

pub(super) fn action_available(
    action: Action,
    object: Option<&SceneTerrainObject>,
    state: &TerrainObjectsEditorState,
) -> bool {
    let writable = object.is_some_and(|object| !state.locked.contains(&object.id));
    let native = |display| object.is_some_and(native_lods) && state.section_open(TerrainObjectsEditorState::lod_section(display));
    match action {
        Action::ToggleCreation => true,
        Action::Cuboid | Action::Clipmap | Action::Voxel | Action::ImportGlb
            | Action::PlacementViewport | Action::PlacementCoordinates => state.creation_open,
        Action::CuboidCollision => state.creation_open && state.creation_source == super::types::CreationSource::Cuboid,
        Action::StartCreation => state.creation_open && state.point_query.is_none()
            && super::creation::validate_selected(state).is_ok(),
        Action::CancelPlacement => state.creation_open,
        Action::BasicProperties | Action::TypeProperties | Action::SourceProperties | Action::StitchProperties => {
            object.is_some()
        }
        Action::Remove => writable && state.section_open(MeshPropertySection::Source),
        Action::StitchNearest => writable && state.section_open(MeshPropertySection::Stitch) && state.stitch_preview.is_none() && object.is_some_and(|o|o.terrain.kind==SceneTerrainKind::SparseClipmap && !state.hidden.contains(&o.id)),
        Action::Unstitch => writable && state.section_open(MeshPropertySection::Stitch) && state.stitch_preview.is_none() && object.is_some_and(|o|!o.clipmap_stitches.is_empty()),
        Action::ConfirmStitch | Action::CancelStitch => state.section_open(MeshPropertySection::Stitch) && state.stitch_preview.is_some(),
        Action::Rename => {
            writable
        }
        Action::Enabled => writable,
        Action::Visible | Action::Lock => object.is_some(),
        Action::DisplayPolicy | Action::CollisionPolicy => {
            writable
                && object.is_some_and(generic_lods)
                && state.section_open(if action == Action::DisplayPolicy { MeshPropertySection::DisplayLods } else { MeshPropertySection::CollisionLods })
        }
        Action::DisplayLods | Action::CollisionLods => object.is_some(),
        Action::SelectLod(local, display) => native(display) && local < super::lod_list::ROW_POOL_SIZE && state.lod(display).start.saturating_add(local) < lod_count(object, display),
        // 所有Clipmap网格都保存显式分层；无效数据不能借按钮创建另一套隐式配置。
        Action::AddLod(display) => writable && native(display) && lod_count(object, display) > 0,
        Action::RemoveLod(display) => {
            writable
                && native(display)
                && lod_count(object, display) > 1
                && state.lod(display).selected < lod_count(object, display)
        }
    }
}

fn kind_label(object: &SceneTerrainObject) -> &'static str {
    match object.terrain.kind {
        SceneTerrainKind::Gltf => "原始 GLB",
        SceneTerrainKind::SparseClipmap => "高度地形",
        SceneTerrainKind::Voxel => "体素地形",
        SceneTerrainKind::Empty => "空源",
    }
}
fn row_source_label(object: &SceneTerrainObject) -> String {
    let kind = kind_label(object);
    if matches!(object.terrain.kind, SceneTerrainKind::Gltf) && !object.terrain.glb_path.is_empty() {
        let file = object.terrain.glb_path.rsplit('/').next().unwrap_or(&object.terrain.glb_path);
        format!("{kind} · {file}") // 迁为独立网格后仍直接可辨真实GLB；完整路径保留在表示参数中。
    } else { kind.to_owned() }
}
fn source_description(object: &SceneTerrainObject) -> String {
    let header = match object.terrain.kind {
        SceneTerrainKind::Gltf => "原始 GLB · 保留资产几何与材质；当前不提供源塑形。",
        SceneTerrainKind::SparseClipmap => "Clipmap · 米制高度不受创建高度域限制，支持高度与三维雕刻。",
        SceneTerrainKind::Voxel => "体素 · 体素高是世界米制尺寸，修改会整体纵向缩放；作者盒、雕刻与草表源保持原样。",
        SceneTerrainKind::Empty => "网格源尚未就绪。",
    };
    let path = object
        .voxel
        .as_ref()
        .map(|voxel| voxel.source_path.as_str())
        .or(object.terrain.height_path.as_deref())
        .unwrap_or(&object.terrain.glb_path);
    let height_label = if object.terrain.kind == SceneTerrainKind::Gltf {
        "源纵向标定"
    } else {
        "作者高度域"
    };
    let range = object.voxel.as_ref().map_or_else(
        || {
            if object.terrain.kind == SceneTerrainKind::SparseClipmap {
                return format!("源范围：X {:.3}～{:.3} / Z {:.3}～{:.3}\n米制高度：不受创建高度域限制",
                    object.world_min[0], object.world_max[0], object.world_min[1], object.world_max[1]);
            }
            format!(
                "源范围：X {:.3}～{:.3} / Z {:.3}～{:.3}\n{height_label}：{:.3}～{:.3} m",
                object.world_min[0],
                object.world_max[0],
                object.world_min[1],
                object.world_max[1],
                object.terrain.base_height,
                object.terrain.base_height + object.terrain.height_scale
            )
        },
        |voxel| {
            format!(
                "源范围：X {:.3}～{:.3} / Y {:.3}～{:.3} / Z {:.3}～{:.3} m",
                voxel.min[0], voxel.max[0], voxel.min[1], voxel.max[1], voxel.min[2], voxel.max[2]
            )
        },
    );
    // 高度作者文件和生成的GLB可能不同；来源说明保留两者完整路径，便于定位当前场景资产。
    let glb = if !object.terrain.glb_path.is_empty() && object.terrain.glb_path != path {
        format!("\nGLB：{}", object.terrain.glb_path)
    } else { String::new() };
    format!(
        "{header}\n来源：{path}{glb}\n{range}\n稳定 ID：{}",
        object.id.as_str()
    )
}
fn policy_mode<T>(mode: MeshLodMode<T>) -> &'static str {
    match mode {
        MeshLodMode::Inherit => "继承全局",
        MeshLodMode::Disabled => "保持原始",
        MeshLodMode::Override(_) => "自定义",
    }
}
fn lod_description(object: &SceneTerrainObject) -> &'static str {
    if generic_lods(object) {
        "显示简化与碰撞简化分别设置。保持原始不会隐藏网格或移除碰撞；全局总开关关闭时独立参数也暂停简化。"
    } else {
        "显示和碰撞分层分别保存。直接在表格内点击输入或拖动修改范围与采样倍率。"
    }
}

fn lod_usage(display: bool) -> &'static str {
    if display { "显示" } else { "碰撞" }
}

fn lod_row_description(
    object: &SceneTerrainObject,
    display: bool,
    index: usize,
    _selected: usize,
    column: LodColumn,
) -> String {
    let field = 8 + index * 2;
    let Some(multiplier) = super::jobs::field_value(object, field + 1, display) else {
        return String::new();
    };
    match column {
        LodColumn::Layer => index.to_string(), // 列标题已说明LOD，编号与运行时真实下标一致，从LOD0开始。
        LodColumn::Multiplier => multiplier.to_string(),
        LodColumn::Range => super::jobs::field_value(object, field, display)
            .map_or_else(|| "网格边界".to_owned(), |value| format!("{value}")), // 单位只在表头显示。
    }
}

pub(super) fn sync(
    mut commands: Commands,
    mut editor: ResMut<SceneEditorState>,
    locale: Option<Res<EditorLocale>>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut ui: PanelPresentation,
    mut visibility: ResMut<feathrise_scene::TerrainObjectEditorVisibility>,
    gate: super::names::NamePanelGate,
    stitches:Option<feathrise_scene::TerrainObjectStitchStatus>,
    mut list_identity: Local<Option<(Option<feathrise_scene_content::SceneId>, Option<feathrise_scene_content::TerrainId>, usize)>>,
    mut lod_identity: Local<[Option<(Option<feathrise_scene_content::SceneId>, Option<feathrise_scene_content::TerrainId>, usize, usize, bool)>; 2]>,
    mut lod_focus: Local<Option<bool>>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let map = session.draft.as_ref().map(|draft| draft.scene_id.clone());
    if state.scene_id != map {
        state.scene_id = map.clone();
        state.selected = None;
        state.selected_index = None;
        state.list_start = 0;
        state.reset_lod_selection();
        state.observed_revision = None;
        state.brush_history = None;
        state.locked.clear();
        state.hidden.clear();
        editor.terrain_locked.clear();
        editor.terrain_hidden = Default::default();
        visibility.scene_id = map;
        visibility.hidden.clear();
    }
    let visibility_map = if editor.open { state.scene_id.clone() } else { None };
    if visibility.scene_id != visibility_map {
        visibility.scene_id = visibility_map;
    }
    if state.observed_revision != Some(session.revision) {
        state.observed_revision = Some(session.revision);
        state.legacy = session
            .draft
            .as_ref()
            .filter(|draft| draft.manifest.terrains.is_none())
            .and_then(|draft| draft.manifest.terrain_objects_for_edit().first().cloned())
            .map(std::sync::Arc::new);
    }
    let legacy = state.legacy.clone();
    let objects = session
        .draft
        .as_ref()
        .and_then(|draft| draft.manifest.terrains.as_deref())
        .unwrap_or_else(|| legacy.as_deref().map_or(&[], std::slice::from_ref));
    if state.selected_index.is_some() && systems::selected(&session, &state).is_none() {
        state.selected = None;
        state.selected_index = None;
    }
    let identity = (state.scene_id.clone(), state.selected.clone(), objects.len());
    let reset = list_identity.as_ref().is_none_or(|old| old.0 != identity.0);
    let reveal = (list_identity.as_ref() != Some(&identity)).then_some(state.selected_index).flatten();
    if let (Ok((mut viewport, mut scroll)), Ok(mut content)) = (ui.list_viewport.single_mut(), ui.list_content.single_mut()) {
        if list_identity.as_ref() != Some(&identity) || scroll.is_changed() {
            // 稳定帧不重写布局，滚动与结构变化只更新固定可见窗口。
            let start = crate::ui::list_layout::fit_scene_object_list(
                objects.len(), reset, reveal, &mut viewport, &mut content, &mut scroll);
            if state.list_start != start { state.list_start = start; }
            *list_identity = Some(identity);
        }
    }
    let counts = [lod_count(systems::selected(&session, &state), false), lod_count(systems::selected(&session, &state), true)];
    for display in [false, true] {
        let list = state.lod_mut(display);
        list.selected = list.selected.min(counts[usize::from(display)].saturating_sub(1));
    }
    let is_native = systems::selected(&session, &state).is_some_and(native_lods);
    let mut lod_pitch = super::lod_list::ROW_HEIGHT + super::lod_list::ROW_GAP;
    if let Ok((body, body_transform, mut scroll)) = ui.inspector.single_mut()
        && body.size().y > 0.0 && is_native
    {
        let inverse = body.inverse_scale_factor();
        let viewport_height = body.size().y * inverse;
        lod_pitch = super::lod_list::row_pitch(viewport_height);
        let body_top = body_transform.affine().translation.y - body.size().y * 0.5;
        let mut origins = [None; 2];
        let mut height_deltas = [0.0; 2];
        for (list, mut node, layout, transform) in &mut ui.lod_list {
            let usage = usize::from(list.0);
            let height = super::lod_list::content_height(counts[usage], lod_pitch);
            if state.section_open(TerrainObjectsEditorState::lod_section(list.0)) && layout.size().y > 0.0 {
                // 加回上帧已解析滚动，分别求出两组在同一属性正文中的稳定起点。
                origins[usage] = Some((transform.affine().translation.y - layout.size().y * 0.5 - body_top + body.scroll_position.y) * inverse);
                height_deltas[usage] = height - layout.size().y * inverse;
            }
            if node.height != px(height) { node.height = px(height); }
        }
        // 显示表在碰撞表之前；本帧层数改变时，第二组的起点与总高度一起补偿。
        if let Some(origin) = origins[0].as_mut() { *origin += height_deltas[1]; }
        let maximum = (body.content_size().y * inverse + height_deltas.iter().sum::<f32>() - viewport_height).max(0.0);
        let focus = state.display_lods;
        let mut next_scroll = scroll.y;
        for display in [true, false] {
            let usage = usize::from(display);
            let open = state.section_open(TerrainObjectsEditorState::lod_section(display));
            let identity = (state.scene_id.clone(), state.selected.clone(), state.lod(display).selected, counts[usage], open);
            if let Some(origin) = origins[usage] {
                // 初始两组同时展开时只揭示聚焦组；另一组的初始化不能把整栏跳到下面。
                if focus == display && (lod_identity[usage].as_ref() != Some(&identity) || *lod_focus != Some(focus)) {
                    next_scroll = super::lod_list::reveal_scroll(next_scroll, origin, state.lod(display).selected, lod_pitch, viewport_height);
                }
                lod_identity[usage] = Some(identity);
            } else if !open {
                lod_identity[usage] = Some(identity);
            }
        }
        next_scroll = next_scroll.clamp(0.0, maximum);
        if scroll.y != next_scroll { scroll.y = next_scroll; }
        for display in [false, true] {
            if let Some(origin) = origins[usize::from(display)] {
                let start = super::lod_list::first_row(counts[usize::from(display)], next_scroll - origin, lod_pitch);
                if state.lod(display).start != start { state.lod_mut(display).start = start; }
            }
        }
        *lod_focus = Some(focus);
    } else {
        for (list, mut node, _, _) in &mut ui.lod_list {
            let height = px(super::lod_list::content_height(counts[usize::from(list.0)], lod_pitch));
            if node.height != height { node.height = height; }
        }
    }
    let object = systems::selected(&session, &state);
    for (row, mut binding, mut interaction, click) in &mut ui.lod_bindings {
        let index = state.lod(row.1).start.saturating_add(row.0);
        let source = object.filter(|object| native_lods(object) && index < counts[usize::from(row.1)] && state.section_open(TerrainObjectsEditorState::lod_section(row.1)))
            .zip(session.draft.as_ref());
        let unchanged = match (&binding.0, source) {
            (Some(target), Some((object, draft))) => target.scene_id == draft.scene_id && target.revision == session.revision
                && target.terrain == object.id && target.index == index && target.display == row.1,
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            // 滚动/换代后的槽位先撤销旧Interaction；仍按住的鼠标不能给新目标补领一次按下。
            binding.0 = source.map(|(object, draft)| super::types::LodRowTarget {
                scene_id: draft.scene_id.clone(), revision: session.revision, terrain: object.id.clone(), index, display: row.1,
            });
            if *interaction != UiInteraction::None { *interaction = UiInteraction::None; }
            if let Some(mut click) = click { click.invalidate(); }
        }
    }
    for (section, slot, row, lod_row, fallback, native_editor, mut node) in &mut ui.nodes {
        let show = if let Some(row) = row {
            let top = px((state.list_start + row.0) as f32 * crate::ui::list_layout::SCENE_OBJECT_ROW_PITCH);
            if node.top != top { node.top = top; }
            objects.get(state.list_start + row.0).is_some()
        } else if let Some(row) = lod_row {
            let index = state.lod(row.1).start.saturating_add(row.0);
            let top = px(index as f32 * lod_pitch);
            if node.top != top { node.top = top; }
            object.is_some_and(native_lods) && index < counts[usize::from(row.1)]
        } else if let Some(slot) = slot {
            numeric::field_visible(slot.0, &state)
                && numeric::field_value(slot.0, &session, &state).is_some()
        } else if let Some(fallback) = fallback {
            numeric::field_visible(fallback.0, &state)
                && numeric::field_value(fallback.0, &session, &state).is_none()
        } else {
            match section.expect("分区、数值槽和列表行构成固定表现池").0 {
                Section::Creation => state.creation_open,
                Section::CreationCuboid => state.creation_source == super::types::CreationSource::Cuboid,
                Section::Selection => object.is_some(),
                Section::Basic => state.section_open(MeshPropertySection::Basic),
                Section::Type => state.section_open(MeshPropertySection::Type),
                Section::DisplayLods => state.section_open(MeshPropertySection::DisplayLods),
                Section::CollisionLods => state.section_open(MeshPropertySection::CollisionLods),
                Section::Source => state.section_open(MeshPropertySection::Source),
                Section::Stitch => state.section_open(MeshPropertySection::Stitch),
                Section::NativeLods => object.is_some_and(native_lods)
                    && native_editor.is_none_or(|editor| state.section_open(TerrainObjectsEditorState::lod_section(editor.0))),
                Section::PolicyLods => object.is_some_and(generic_lods),
                Section::DisplayOverride => object
                    .and_then(|object| object.mesh_lod)
                    .is_some_and(|policy| matches!(policy.display, MeshLodMode::Override(_))),
                Section::CollisionOverride => object
                    .and_then(|object| object.mesh_lod)
                    .is_some_and(|policy| matches!(policy.collision, MeshLodMode::Override(_))),
            }
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (
        summary,
        list,
        creation,
        source,
        lod,
        native,
        representation,
        lod_row,
        row,
        row_kind,
        label,
        mut text,
    ) in &mut ui.texts
    {
        let value = if summary.is_some() {
            object.map_or_else(
                || locale.text("mesh.selection.hint").to_owned(),
                |object| {
                    format!(
                        "{} · {}{}{}{}{}",
                        object.display_name,
                        row_source_label(object),
                        if state.pending.is_some() {
                            " · 后台准备中"
                        } else {
                            ""
                        },
                        if state.locked.contains(&object.id) {
                            " · 已锁定"
                        } else {
                            ""
                        },
                        if state.hidden.contains(&object.id) {
                            " · 编辑器隐藏"
                        } else {
                            ""
                        },
                        if object.clipmap_stitches.is_empty(){String::new()} else {
                            let status=session.draft.as_ref().and_then(|d|stitches.as_ref()?.for_object(&d.scene_id,&object.id));
                            match status.filter(|(n,_)|*n as usize==object.clipmap_stitches.len()) {
                                Some((n,active)) if n==active=>format!("\n拼接：{active}/{n} 条生效"),
                                Some((n,active))=>format!("\n拼接：{active}/{n} 条生效；失配关系已暂停"),
                                None=>"\n拼接：等待地形完整采用".into(),
                            }
                        }
                    )
                },
            )
        } else if list.is_some() {
            session.draft.as_ref().map_or_else(String::new, |draft| draft.manifest.display_name.clone())
        } else if creation.is_some() {
            super::creation::summary(&state.creation, editor.selected_asset.as_deref())
        } else if source.is_some() {
            object.map_or_else(String::new, source_description)
        } else if representation.is_some() {
            locale.editor_label(object.map_or("", kind_label)).to_owned()
        } else if lod.is_some() {
            locale.editor_label(object.map_or("", lod_description)).to_owned()
        } else if let Some(native) = native {
            let count = counts[usize::from(native.0)];
            if count == 0 {
                format!("{} · 没有有效分层，当前只读。", lod_usage(native.0))
            } else {
                format!("{} · {} 层", lod_usage(native.0), count)
            }
        } else if let Some(row) = lod_row {
            let list = state.lod(row.2);
            let index = list.start.saturating_add(row.0);
            object.map_or_else(String::new, |object| {
                lod_row_description(object, row.2, index, list.selected, row.1)
            })
        } else if let Some(row) = row_kind {
            locale.editor_label(objects.get(state.list_start + row.0).map_or("", kind_label)).to_owned()
        } else if let Some(row) = row {
            objects
                .get(state.list_start + row.0)
                .map_or_else(String::new, |object| {
                    format!(
                        "{}{}{}{}",
                        object.display_name,
                        if !object.enabled {
                            "（不参与场景）"
                        } else {
                            ""
                        },
                        if state.hidden.contains(&object.id) {
                            "（隐藏）"
                        } else {
                            ""
                        },
                        if state.locked.contains(&object.id) {
                            "（锁定）"
                        } else {
                            ""
                        }
                    )
                })
        } else if let Some(label) = label {
            // 按钮由作者状态逐帧回显，必须在这里取译文，否则会覆盖场景初始译文。
            let section_label = |section, label| {
                let marker = if state.section_open(section) { "▼" } else { "▶" };
                format!("{marker} {}", locale.editor_label(label))
            };
            let value = match label.0 {
                Action::ToggleCreation => if state.creation_open {
                    "添加网格 ▼"
                } else {
                    "添加网格 ▶"
                }
                .to_owned(),
                Action::CuboidCollision => format!("盒碰撞：{}", if state.cuboid_collision { "开" } else { "关" }),
                Action::StartCreation => if state.placement_mode == super::types::CreationPlacementMode::Coordinates { "按坐标创建" } else { "开始点放" }.to_owned(),
                Action::Enabled => if object.is_some_and(|object| object.enabled) {
                    "参与场景：是"
                } else {
                    "参与场景：否"
                }
                .to_owned(),
                Action::Visible => {
                    if object.is_some_and(|object| state.hidden.contains(&object.id)) {
                        "编辑器：隐藏"
                    } else {
                        "编辑器：显示"
                    }
                    .to_owned()
                }
                Action::Lock => if object.is_some_and(|object| state.locked.contains(&object.id)) {
                    "已锁定"
                } else {
                    "未锁定"
                }
                .to_owned(),
                Action::DisplayPolicy => locale.format(
                    "mesh.lod.display_policy",
                    &[("mode", locale.editor_label(policy_mode(
                        object
                            .and_then(|object| object.mesh_lod)
                            .unwrap_or_default()
                            .display
                    )).into())],
                ),
                Action::CollisionPolicy => locale.format(
                    "mesh.lod.collision_policy",
                    &[("mode", locale.editor_label(policy_mode(
                        object
                            .and_then(|object| object.mesh_lod)
                            .unwrap_or_default()
                            .collision
                    )).into())],
                ),
                Action::BasicProperties => section_label(MeshPropertySection::Basic, "变换"),
                Action::TypeProperties => section_label(MeshPropertySection::Type, "几何"),
                Action::SourceProperties => section_label(MeshPropertySection::Source, "来源与高级"),
                Action::StitchProperties => section_label(MeshPropertySection::Stitch, "拼接"),
                Action::DisplayLods => section_label(MeshPropertySection::DisplayLods, "显示 LOD"),
                Action::CollisionLods => section_label(MeshPropertySection::CollisionLods, "碰撞 LOD"),
                Action::RemoveLod(display) => {
                    let count = counts[usize::from(display)];
                    let selected = state.lod(display).selected;
                    if count == 1 {
                        "至少保留一层".to_owned()
                    } else if selected < count {
                        locale.format("mesh.lod.remove", &[("level", selected.to_string())])
                    } else {
                        "删除所选层".to_owned()
                    }
                }
                _ => continue,
            };
            let localized = locale.editor_label(&value);
            if localized == value { value } else { localized.to_owned() }
        } else {
            continue;
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    let usable = systems::enabled(&editor, &session, &state);
    for (button, mut binding, mut interaction, click) in &mut ui.lod_actions {
        let display = match button.0 { Action::AddLod(display) | Action::RemoveLod(display) => display, _ => continue };
        let target = if usable && gate.action_visible(button.0, &state) && action_available(button.0, object, &state) {
            object.zip(session.draft.as_ref()).map(|(object, draft)| super::types::LodRowTarget {
                scene_id: draft.scene_id.clone(), revision: session.revision, terrain: object.id.clone(),
                index: state.lod(display).selected, display,
            })
        } else { None };
        if binding.0 != target {
            binding.0 = target;
            if *interaction != UiInteraction::None { *interaction = UiInteraction::None; }
            if let Some(mut click) = click { click.invalidate(); }
        }
    }
    for (entity, field, current, disabled, lod_row, binding) in &mut ui.fields {
        let value = numeric::field_value(*field, &session, &state);
        let row = lod_row.and_then(|row| ui.lod_bindings.get(row.0).ok()).and_then(|(_, binding, _, _)| binding.0.as_ref());
        let target = if usable && gate.field_visible(*field, &state) {
            numeric::target(*field, &session, &state, row)
        } else {
            None
        };
        let editable = target.is_some();
        if editable && disabled {
            commands.entity(entity).remove::<NumericInputDisabled>();
        }
        if !editable && !disabled {
            commands.entity(entity).insert(NumericInputDisabled);
        }
        let identity_changed = binding
            .as_ref()
            .is_none_or(|binding| binding.current != target);
        let editing = active.entity == Some(entity) || scrub.entity == Some(entity);
        let layer_changed = editing && lod_row.is_some()
            && target.as_ref().is_some_and(|target| (target.index - 8) / 2 != state.lod(target.display_lods).selected);
        if let Some(mut binding) = binding {
            if identity_changed {
                // 保留失效编辑的旧身份直至公共控件结束；迟到Accepted不能借新绑定重新获权。
                if binding.editing.is_none() {
                    binding.editing = binding.current.clone();
                }
                if binding.current.is_some() {
                    binding.invalidated = true;
                }
                binding.current = target;
            }
            if layer_changed { binding.invalidated = true; }
        } else {
            commands.entity(entity).insert(NumericFieldBinding {
                current: target,
                ..Default::default()
            });
        }
        if identity_changed || layer_changed || (!editable && editing) {
            values.write(NumericInputSetValue {
                entity,
                value: value.map_or(current.0, NumericValue::F32),
                cancel_edit: true,
            });
        } else if !editing && value.is_some_and(|value| current.0.as_f32() != Some(value)) {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(value.unwrap()),
                cancel_edit: true,
            });
        }
    }
    for (entity, button, disabled, background, border) in &mut ui.buttons {
        let visible = gate.action_visible(button.0, &state);
        let confirming=matches!(button.0,Action::ConfirmStitch|Action::CancelStitch)&&state.stitch_preview.is_some();
        let placement_active = state.point_placement.is_some()
            || crate::authoring::basic_shapes::is_cuboid_selection(&editor);
        let available = (usable||confirming) && visible && action_available(button.0, object, &state)
            && (button.0 != Action::CancelPlacement || placement_active)
            && (button.0 != Action::StartCreation || state.creation_source != super::types::CreationSource::ImportedGlb
                || editor.selected_asset.as_ref().is_some_and(|path| path.to_ascii_lowercase().ends_with(".glb")));
        if available && disabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        }
        if !available && !disabled {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
        let selected = match button.0 {
            Action::Cuboid => state.creation_source == super::types::CreationSource::Cuboid,
            Action::ImportGlb => state.creation_source == super::types::CreationSource::ImportedGlb,
            Action::Clipmap => state.creation_source == super::types::CreationSource::Clipmap,
            Action::Voxel => state.creation_source == super::types::CreationSource::Voxel,
            Action::PlacementViewport => state.placement_mode == super::types::CreationPlacementMode::Viewport,
            Action::PlacementCoordinates => state.placement_mode == super::types::CreationPlacementMode::Coordinates,
            // 分组标题靠箭头表达展开状态，不能整块染成“选中按钮”。
            Action::BasicProperties | Action::TypeProperties | Action::SourceProperties | Action::StitchProperties
                | Action::DisplayLods | Action::CollisionLods => false,
            Action::SelectLod(local, display) => {
                let list = state.lod(display);
                local < super::lod_list::ROW_POOL_SIZE
                    && list.start.saturating_add(local) == list.selected
                    && list.selected < counts[usize::from(display)]
            }
            _ => false,
        };
        if let Some(mut background) = background {
            let accordion = matches!(button.0, Action::BasicProperties | Action::TypeProperties | Action::SourceProperties
                | Action::StitchProperties | Action::DisplayLods | Action::CollisionLods);
            let fill = if selected {
                colors::SELECT_BG
            } else if accordion {
                colors::PANEL_BG
            } else {
                colors::BUTTON_DEV_SMALL_NORMAL
            };
            if background.0 != fill {
                background.0 = fill;
            }
        }
        if let Some(mut border) = border {
            let outline = BorderColor::all(colors::COPPER_BORDER_WEAK); // 选层仅用行底色，数值焦点由公共控件独立表达。
            if *border != outline {
                *border = outline;
            }
        }
    }
}
