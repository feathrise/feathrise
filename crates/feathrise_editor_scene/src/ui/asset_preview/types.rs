//! 资产预览沿用同一套组件、会话和载入身份；动画插件读取与更新同一个资源。

pub use feathrise_editor_animation_model::media::*;

#[cfg(test)]
#[path = "types_tests.rs"]
mod placement_target_tests; // 摆放协议映射夹具独立维护，保持状态定义文件低于模块拆分阈值。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_extensions_are_case_insensitive_and_typed() {
        assert_eq!(
            preview_file_kind("textures/leaf.PNG"),
            AssetPreviewFileKind::Image
        );
        assert_eq!(
            preview_file_kind("gltf/tree.GLB"),
            AssetPreviewFileKind::Model
        );
        for path in [
            "scenes/a.json", "scenes/a.ron", "text/readme.MD", "shaders/test.wgsl",
            "scripts/init.py", "data/table.csv", "folder.with.dot/README",
        ] {
            assert_eq!(
                preview_file_kind(path), AssetPreviewFileKind::Document, "{path}"
            );
        }
        assert_eq!(
            preview_file_kind("models/tree.GLTF"), AssetPreviewFileKind::Model
        );
        assert_eq!(preview_file_kind("scenes/height.r16"), AssetPreviewFileKind::Data);
        for path in ["audio/wind.OGG", "audio/剑声.WaV", "audio/music.MP3", "audio/wind.FlAc"] {
            assert_eq!(preview_file_kind(path), AssetPreviewFileKind::Audio, "{path}");
        }
        assert_eq!(preview_file_kind("audio/wind.aac"), AssetPreviewFileKind::Unsupported);
    }
}
