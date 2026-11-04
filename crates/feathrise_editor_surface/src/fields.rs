//! 地表三个顶部分区的固定字段编排；不计算材质、不读取纹理或改写作者协议。

use bevy::prelude::*;
use feathrise_map_content::SurfaceProjection;

use super::components::SurfaceButtonKind;
use super::types::{
    SurfacePresetAction, SurfacePresetEditorState, SurfacePresetField, SurfaceSection, SurfaceTab,
    SurfaceTextureField,
};
use super::widgets::{action_button, field, group, readonly, row, texture_row};

pub(super) fn spawn_fields(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    state: &SurfacePresetEditorState,
    tab: &SurfaceTab,
) {
    let descriptor = &tab.draft.descriptor;
    match tab.section {
        SurfaceSection::Textures => {
            group(
                parent,
                face,
                "地表资料",
                "稳定 ID 不可在普通编辑中修改；地图继续按原引用识别此预设。",
                |body| {
                    readonly(body, face, &format!("稳定 ID：{}", descriptor.id));
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "显示名称",
                            SurfacePresetField::Label,
                        )
                    });
                },
            );
            group(
                parent,
                face,
                "作者纹理",
                "在左侧资产页选择 PNG/JPG/JPEG，再明确采用到对应通道。",
                |body| {
                    texture_row(
                        body,
                        face,
                        "基础色 · sRGB",
                        descriptor.base_color_texture_path.as_deref(),
                        SurfaceTextureField::BaseColor,
                    );
                    texture_row(
                        body,
                        face,
                        "切线法线 · 线性",
                        descriptor.normal_texture_path.as_deref(),
                        SurfaceTextureField::Normal,
                    );
                    texture_row(
                        body,
                        face,
                        "ARM + 高度 · 线性",
                        descriptor.arm_height_texture_path.as_deref(),
                        SurfaceTextureField::ArmHeight,
                    );
                    readonly(
                        body,
                        face,
                        "ARM 通道：R＝遮蔽，G＝粗糙度，B＝金属度，A＝高度。空路径保留内部中性回退。",
                    );
                },
            );
        }
        SurfaceSection::Material => {
            group(
                parent,
                face,
                "基础色乘色",
                "四个分量均为 0—1 的 sRGBA；保留原纹理乘色规则。",
                |body| {
                    row(body, |grid| {
                        for (channel, label) in ["R · 红", "G · 绿", "B · 蓝", "A · 透明度"]
                            .into_iter()
                            .enumerate()
                        {
                            field(
                                grid,
                                face,
                                state,
                                descriptor,
                                label,
                                SurfacePresetField::BaseColor(channel),
                            );
                        }
                    });
                },
            );
            group(
                parent,
                face,
                "表面反射",
                "粗糙度与金属度继续使用原乘数和 0—1 范围。",
                |body| {
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "粗糙度乘数",
                            SurfacePresetField::Roughness,
                        );
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "金属度乘数",
                            SurfacePresetField::Metallic,
                        );
                    });
                },
            );
            group(
                parent,
                face,
                "法线",
                "强度非负；仅按作者源的 DirectX/OpenGL 约定翻转 Y。",
                |body| {
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "法线强度",
                            SurfacePresetField::NormalStrength,
                        )
                    });
                    action_button(
                        body,
                        face,
                        if descriptor.normal_y_flip {
                            "法线 Y：反转（DX）"
                        } else {
                            "法线 Y：原样（GL）"
                        },
                        SurfacePresetAction::ToggleNormalY,
                        SurfaceButtonKind::Ordinary,
                        descriptor.normal_y_flip,
                    );
                },
            );
        }
        SurfaceSection::Mapping => {
            group(
                parent,
                face,
                "世界映射",
                "平铺米数必须大于 0；陡坡可切换三平面投影，地表绘制协议不变。",
                |body| {
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "平铺尺寸 · 米",
                            SurfacePresetField::TileMeters,
                        )
                    });
                    action_button(
                        body,
                        face,
                        match descriptor.projection {
                            SurfaceProjection::WorldXz => "投影：世界 XZ",
                            SurfaceProjection::Triplanar => "投影：三平面",
                        },
                        SurfacePresetAction::CycleProjection,
                        SurfaceButtonKind::Ordinary,
                        descriptor.projection == SurfaceProjection::Triplanar,
                    );
                },
            );
            group(
                parent,
                face,
                "高度混合",
                "使用 ARM 的 A 通道，强度非负；不新增地形位移或材质预览视图。",
                |body| {
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "高度混合强度",
                            SurfacePresetField::HeightBlendStrength,
                        )
                    });
                },
            );
            group(
                parent,
                face,
                "技能形变响应",
                "这不是 PBR 反射参数。0 不下沉，1 使用完整技能浅坑深度；硬表面可设为 0.1。",
                |body| {
                    row(body, |grid| {
                        field(
                            grid,
                            face,
                            state,
                            descriptor,
                            "浅坑响应 · 0—1",
                            SurfacePresetField::CraterResponse,
                        )
                    });
                },
            );
        }
    }
}
