//! 地表作者层显式创建与精确选择校验；笔刷不再隐式创建作者层。

use feathrise_map_content::{MapSurfaceAsset, MapSurfaceLayer, SurfacePresetRegistry};

use super::resources::{EditorAuthoringSession, EditorCommand};
use crate::resources::MapEditorState;

/// 笔刷只接受已选中且匹配预设的作者层，不创建或自动切换同材质层。
pub(crate) fn ensure_selected_surface_layer(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<usize, String> {
    let Some(selected_id) = editor.surface_selected_preset_id.as_deref() else {
        let surface = session
            .draft
            .as_ref()
            .and_then(|draft| draft.surface.as_ref())
            .ok_or_else(|| "当前地图没有索引式地表作者文件".to_owned())?;
        return (editor.selected_surface_layer < surface.layers.len())
            .then_some(editor.selected_surface_layer)
            .ok_or_else(|| "当前地图地表层选择已失效，请重新选择预设".to_owned());
    };
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    {
        let surface = session
            .draft
            .as_mut()
            .and_then(|draft| draft.surface.as_mut())
            .ok_or_else(|| "当前地图没有索引式地表作者文件".to_owned())?;
        if let Some(layer) = surface.layers.get(editor.selected_surface_layer) // 明确点选作者层时先保留该精确局部位置。
            && let Some(selected) = registry.resolve(selected_id) // 历史别名按当前全局目录解析规范身份。
            && registry.resolve(&layer.preset_id).is_some_and(|preset| preset.id == selected.id)
        // 稳定作者层和预设引用保持真实对应关系。
        // 同预设的合法独立层不能被自动切回第一项。
        {
            // 满足上述完整条件后才进入当前分支。
            // 当前索引与所选预设身份一致时无需扫描或追加层。
            return Ok(editor.selected_surface_layer); // 地表作者列表选择直接决定下一笔绘制对象。
        } // 非匹配选择必须由用户显式新建或选择作者层。
        return Err("请先新建地表层，或选择对应的已有作者层".to_owned()); // 笔刷不再隐式新建或切换作者层。
    }
}

/// 显式新建独立空白层，只追加定义，不改动像素。
pub(crate) fn create_surface_layer(
    editor: &mut MapEditorState,              // 当前地图与材质选择。
    session: &mut EditorAuthoringSession,     // 草稿和统一历史。
    registry: Option<&SurfacePresetRegistry>, // 正式预设目录。
) -> Result<usize, String> {
    // 预检失败无副作用。
    if !editor.open || editor.close_requested || editor.save_requested || session.saving {
        // 不与保存关闭交错。
        return Err("当前正在保存或关闭，暂时不能新建地表层".to_owned()); // 保留原状态。
    } // 生命周期检查结束。
    let selected = editor
        .surface_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先选择地表预设".to_owned())?; // 新层必须明确指定材质。
    let registry = registry.ok_or_else(|| "地表预设目录尚未加载".to_owned())?; // 使用正式目录。
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "地图草稿尚未加载".to_owned())?; // 不写未加载草稿。
    if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
        // 防止切图写入旧草稿。
        return Err("地图草稿已切换，请稍后重试".to_owned()); // 保留完整原状态。
    } // 地图身份检查结束。
    let surface = draft
        .surface
        .as_mut()
        .ok_or_else(|| "当前地图没有地表作者文件".to_owned())?; // 要求正式作者定义。
    if surface.map_id != draft.map_id {
        // 地表定义必须属于当前地图。
        return Err("地表作者文件与地图不匹配".to_owned()); // 拒绝混合身份。
    } // 定义身份检查结束。
    let (index, layer) = resolve_or_append_surface_layer(surface, registry, selected, true)?; // 每次新建独立追加。
    let layer = layer.expect("显式新建总是返回新增层"); // 此入口不再复用旧层。
    editor.selected_surface_layer = index; // 新层直接成为绘制目标。
    editor.surface_selected_preset_id = Some(layer.preset_id.clone()); // 规范化材质身份。
    session
        .undo
        .push(EditorCommand::SurfaceLayerAdded { index, layer }); // 一次新建一条撤销命令。
    session.redo.clear(); // 新编辑废弃旧重做分支。
    session.mark_surface_changed(editor); // 更新保存责任和作者修订。
    Ok(index) // UI可将新层滚动展示。
} // 显式新建完成。

/// 显式追加独立局部层，允许同预设重复创建，保持稳定身份唯一。
fn resolve_or_append_surface_layer(
    surface: &mut MapSurfaceAsset,
    registry: &SurfacePresetRegistry,
    selected_id: &str,
    allow_create: bool,
) -> Result<(usize, Option<MapSurfaceLayer>), String> {
    let preset = registry
        .resolve(selected_id)
        .ok_or_else(|| format!("所选地表预设{selected_id}已不存在"))?;
    if !allow_create {
        return Err(format!("“{}”尚未用于当前地图，无需擦除", preset.label));
    }
    let new_count = surface
        .layers
        .len()
        .checked_add(1)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| "地图局部地表层数量已超出 u32 可表达范围".to_owned())?;
    let index = usize::try_from(new_count - 1).expect("usize 已经表达同一个 Vec 长度");
    let mut layer = preset.create_map_layer();
    if surface
        .layers
        .iter()
        .any(|candidate| candidate.id == layer.id)
    {
        let mut suffix = 2_u32;
        loop {
            let candidate = format!("{}-{suffix}", preset.id);
            if !surface.layers.iter().any(|layer| layer.id == candidate) {
                layer.id = candidate;
                layer.label = format!("{} {suffix}", preset.label); // 同预设的独立作者层显示可辨序号。
                break;
            }
            suffix = suffix
                .checked_add(1)
                .ok_or_else(|| "无法为地图地表层生成唯一稳定 ID".to_owned())?;
        }
    }
    surface.layers.push(layer.clone());
    Ok((index, Some(layer)))
}

#[cfg(test)]
mod tests {
    use bevy::prelude::Handle;
    use feathrise_map_content::{
        MAP_SURFACE_SCHEMA_VERSION, MapId, ResolvedSurfacePresetCatalog,
        SURFACE_PRESET_SCHEMA_VERSION, SurfacePresetCatalogAsset, SurfacePresetDescriptor,
        SurfaceProjection,
    };

    use super::*;

    fn descriptor(id: &str) -> SurfacePresetDescriptor {
        SurfacePresetDescriptor {
            id: id.to_owned(),
            aliases: Vec::new(),
            label: id.to_owned(),
            base_color_srgba: [1.0; 4],
            base_color_texture_path: None,
            normal_texture_path: None,
            arm_height_texture_path: None,
            tile_meters: 4.0,
            normal_strength: 1.0,
            normal_y_flip: false,
            height_blend_strength: 0.0,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            crater_response: 1.0, // 测试地表沿用旧技能深度。
            projection: SurfaceProjection::WorldXz,
        }
    }

    fn registry() -> SurfacePresetRegistry {
        let catalog = SurfacePresetCatalogAsset {
            schema_version: SURFACE_PRESET_SCHEMA_VERSION,
            texture_resolution: [4, 4],
            base_color_array_path: "config/surface/base.ktx2".to_owned(),
            normal_array_path: "config/surface/normal.ktx2".to_owned(),
            arm_height_array_path: "config/surface/arm.ktx2".to_owned(),
            presets: vec![descriptor("soil"), descriptor("flower")],
        };
        SurfacePresetRegistry {
            catalog: Handle::default(),
            revision: 1,
            resolved: ResolvedSurfacePresetCatalog::build(&catalog).expect("测试目录有效"),
            last_error: None,
        }
    }

    fn surface() -> MapSurfaceAsset {
        MapSurfaceAsset {
            schema_version: MAP_SURFACE_SCHEMA_VERSION,
            map_id: MapId::parse("MAP-surface-layer-test").expect("测试地图 ID 有效"),
            weight_source_path: "maps/test/surface/weights.rgba8".to_owned(),
            weight_texture_path: "maps/test/surface/weights.ktx2".to_owned(),
            index_source_path: "maps/test/surface/indices.rgba32".to_owned(),
            index_texture_path: "maps/test/surface/indices.ktx2".to_owned(),
            resolution: [4, 4],
            layers: vec![descriptor("soil").create_map_layer()],
        }
    }

    #[test]
    fn explicit_surface_create_repeats_with_independent_identity() {
        let registry = registry();
        let mut surface = surface();
        let before = surface.layers.clone();
        assert!(resolve_or_append_surface_layer(&mut surface, &registry, "flower", false).is_err());
        assert_eq!(surface.layers, before); // 管理选择或未使用层擦除不得制造地图引用。

        let (index, added) =
            resolve_or_append_surface_layer(&mut surface, &registry, "flower", true)
                .expect("第一次真实落笔应创建局部层");
        assert_eq!(index, 1);
        assert_eq!(
            added.as_ref().map(|layer| layer.preset_id.as_str()),
            Some("flower")
        );

        let (same_index, repeated) =
            resolve_or_append_surface_layer(&mut surface, &registry, "flower", true)
                .expect("后续落笔复用已有层");
        assert_eq!(same_index, index + 1); // 每次显式创建都追加独立作者层。
        let repeated = repeated.expect("重复预设同样新建独立层"); // 不复用旧层。
        assert_ne!(repeated.id, added.as_ref().unwrap().id); // 稳定身份独立。
        assert_ne!(repeated.label, added.as_ref().unwrap().label); // 列表标签可辨。
        assert_eq!(surface.layers.len(), 3); // 初始层加两次明确新建。
    }

    #[test] // 显式创建只增加定义与历史，不更换像素所有权。
    fn explicit_surface_creation_preserves_pixels_and_records_one_command() {
        let registry = registry(); // 正式测试材质目录。
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 有效地图草稿。
        let mut definition = surface(); // 正式地表定义。
        definition.map_id = draft.map_id.clone(); // 对齐地图身份。
        definition.layers.clear(); // 模拟删除最后一层后重新创建。
        draft.surface = Some(definition); // 保留空作者层表。
        let pixels = std::sync::Arc::new(vec![0; 64]); // 裸地权重画布。
        draft.surface_weights = Some(pixels.clone()); // 共享像素可检测隐式复制。
        let mut editor = MapEditorState {
            open: true,                                          // 编辑器已打开。
            selected_map_id: Some(draft.map_id.clone()),         // 明确当前地图。
            surface_selected_preset_id: Some("soil".to_owned()), // 新层材质。
            ..Default::default()                                 // 无保存关闭意图。
        }; // 可执行显式创建。
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        }; // 空历史。
        assert!(ensure_selected_surface_layer(&mut editor, &mut session, Some(&registry)).is_err()); // 笔刷不隐式创建。
        assert_eq!(
            create_surface_layer(&mut editor, &mut session, Some(&registry)).unwrap(),
            0
        ); // 零层可以显式新建。
        assert_eq!(session.undo.len(), 1); // 一次创建一条历史。
        assert!(matches!(
            session.undo.last(),
            Some(EditorCommand::SurfaceLayerAdded { index: 0, .. })
        )); // 使用统一可撤销命令。
        assert!(std::sync::Arc::ptr_eq(
            &pixels,
            session
                .draft
                .as_ref()
                .unwrap()
                .surface_weights
                .as_ref()
                .unwrap()
        )); // 不修改或复制像素。
        editor.save_requested = true; // 保存事务开始后拒绝结构变化。
        assert!(create_surface_layer(&mut editor, &mut session, Some(&registry)).is_err()); // 保护保存快照。
        assert_eq!(session.undo.len(), 1); // 拒绝操作不产生额外历史。
    } // 显式创建回归结束。

    #[test] // 作者列表点选同预设的独立层后，笔刷必须保持精确地图局部索引。
    fn explicit_duplicate_preset_author_layer_keeps_selected_local_index() {
        // 地表作者操作在受检输入下执行当前步骤。
        // 旧实现会按预设搜索并错误切回首个同预设层。
        let registry = registry(); // 当前目录包含soil和flower两个稳定预设。
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 复用有效地图上下文，无需真实文件IO。
        let mut definition = surface(); // 初始局部零层引用soil。
        definition.map_id = draft.map_id.clone(); // 地表文件与草稿保持真实同图身份。
        let mut second = descriptor("soil").create_map_layer(); // 允许旧地图含同预设不同作者层。
        second.id = "soil-independent".to_owned(); // 局部层稳定ID与预设ID分别承担不同身份。
        definition.layers.push(second); // 明确建立位于局部索引1的独立层。
        draft.surface = Some(definition); // 选择测试只需地表定义，不读取像素。
        let mut session = EditorAuthoringSession {
            // 保持当前地表操作的数据和原子采用边界。
            draft: Some(draft),   // 完成当前地表作者事务的受检参数或值转换。
            ..Default::default()  // 完成当前地表作者事务的受检参数或值转换。
        }; // 初始历史保持空白。
        let mut editor = MapEditorState {
            // 保持当前地表操作的数据和原子采用边界。
            selected_surface_layer: 1, // 完成当前地表作者事务的受检参数或值转换。
            surface_selected_preset_id: Some("soil".to_owned()), // 稳定作者层和预设引用保持真实对应关系。
            ..Default::default() // 完成当前地表作者事务的受检参数或值转换。
        }; // 模拟用户点选第二个作者层。
        assert_eq!(
            // 回归检查当前作者定义、像素或生命周期不变量。
            ensure_selected_surface_layer(&mut editor, &mut session, Some(&registry)) // 完成当前地表作者事务的受检参数或值转换。
                .expect("所选作者层预设仍有效"), // 前置身份和长度检查已保证该测试或作者值存在。
            1 // 完成当前地表作者事务的受检参数或值转换。
        ); // 笔刷不能跳回局部索引0。
        assert!(session.undo.is_empty()); // 纯选择不能添加预设或制造作者命令。
        assert!(!editor.dirty); // 原层复用不产生保存责任。
    } // 同预设独立层选择回归结束。
}
