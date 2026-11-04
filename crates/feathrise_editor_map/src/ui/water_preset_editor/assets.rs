//! 水体预设编辑标签与右侧 Asset 浏览器之间的显式微法线绑定。

use crate::resources::MapEditorState;
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;

use super::types::WaterPresetEditorState;

/// 把作者明确选择的图片绑定到一层微法线，或显式清除该层。
pub(super) fn set_texture(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    layer: usize,
    clear: bool,
) -> Result<(), String> {
    let value = if clear {
        None
    } else {
        let path = editor
            .selected_asset
            .as_deref()
            .ok_or_else(|| "请先在右侧 Asset 浏览器选择 PNG/JPG/JPEG/KTX2 法线图".to_owned())?;
        let extension = path.rsplit_once('.').map(|(_, extension)| extension);
        if !extension.is_some_and(|extension| {
            ["png", "jpg", "jpeg", "ktx2"]
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        }) {
            return Err("水体微法线只接受 PNG/JPG/JPEG/KTX2".to_owned());
        }
        Some(path.to_owned())
    };
    state
        .active_draft_mut(workspace.active_tab)
        .and_then(|draft| draft.descriptor.micro_normals.get_mut(layer))
        .ok_or_else(|| "微法线层索引无效".to_owned())?
        .texture_path = value;
    state.touch_ui();
    workspace.touch_ui();
    Ok(())
}
