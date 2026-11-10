//! Blender四种视图着色及其仅影响作者相机的显示选项。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShadingMode {
    Wireframe,
    Solid,
    MaterialPreview,
    #[default]
    Rendered,
}

impl ShadingMode {
    pub(super) const ALL: [Self; 4] = [
        Self::Wireframe,
        Self::Solid,
        Self::MaterialPreview,
        Self::Rendered,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Wireframe => "线框",
            Self::Solid => "实体",
            Self::MaterialPreview => "材质预览",
            Self::Rendered => "渲染",
        }
    }
    pub(super) fn index(self) -> f32 {
        match self {
            Self::Wireframe => 0.0,
            Self::Solid => 1.0,
            Self::MaterialPreview => 2.0,
            Self::Rendered => 3.0,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ShadingAction {
    Mode(ShadingMode),
    Menu,
    Xray,
    Cavity,
    Normals,
    Studio,
    Background,
}
