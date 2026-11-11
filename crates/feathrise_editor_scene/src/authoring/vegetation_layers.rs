//! 植被作者层到所属组和固定正式运行时层的选择映射。

use feathrise_scene_content::{
    SceneVegetationAuthorGroup, SceneVegetationAuthorLayer, SceneVegetationLayer,
}; // 返回值显式区分作者画布、组规则和正式profile。

use super::resources::EditorSceneDraft; // 当前完整草稿拥有作者描述与像素表。

/// 当前选中作者层的三段稳定引用。
pub(crate) struct SelectedVegetationLayer<'a> {
    /// F12真正绘制和锁定的作者层。
    pub(crate) author: &'a SceneVegetationAuthorLayer,
    /// 作者层所属的组内规则。
    pub(crate) group: &'a SceneVegetationAuthorGroup,
    /// 编译结果进入的现有正式运行时层。
    pub(crate) output: &'a SceneVegetationLayer,
}

/// 按UI索引解析作者层、所属组和正式输出；任一悬空引用都返回None。
pub(crate) fn selected_vegetation_layer(
    draft: &EditorSceneDraft,
    index: usize,
) -> Option<SelectedVegetationLayer<'_>> {
    let vegetation = draft.vegetation.as_ref()?; // 当前场景必须登记植被文件。
    let authoring = vegetation.authoring.as_ref()?; // 加载边界保证旧文件已在内存迁移。
    let author = authoring.layers.get(index)?; // UI索引永远指作者层而非正式层。
    let group = authoring
        .groups
        .iter()
        .find(|group| group.id == author.group_id)?; // 稳定组引用在公共校验后仍防御性查找。
    let output = vegetation
        .layers
        .iter()
        .find(|output| output.id == author.output_layer_id)?; // 正式层数与GPU四级身份保持固定。
    Some(SelectedVegetationLayer {
        author, // 独立R8作者画布。
        group,  // 共存、共享容量或覆盖规则。
        output, // Blade或Mesh现有正式profile。
    })
}
