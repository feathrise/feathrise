//! 水体文档的四个参数分区；所有字段仍使用唯一描述符读写映射。

use bevy::prelude::*;

use super::types::{WaterPresetEditorState, WaterPresetField, WaterSection, WaterTab};
use super::widgets::{field, group, readonly, row, texture_row};

pub(super) fn spawn_fields(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    state: &WaterPresetEditorState,
    tab: &WaterTab,
) {
    let descriptor = &tab.draft.descriptor;
    match tab.section {
        WaterSection::Appearance => {
            group(
                parent,
                face,
                "预设资料",
                "名称用于浏览；场景始终引用只读稳定 ID。",
                |body| {
                    readonly(body, face, &format!("稳定 ID：{}", descriptor.id));
                    row(body, |fields| {
                        field(
                            fields,
                            face,
                            state,
                            descriptor,
                            "显示名称",
                            WaterPresetField::Label,
                        );
                    });
                },
            );
            group(
                parent,
                face,
                "基础水色",
                "RGB 为 sRGB 水色，A 为透明度；四个分量均为 0～1。",
                |body| {
                    row(body, |fields| {
                        for (label, component) in
                            [("水色 R", 0), ("水色 G", 1), ("水色 B", 2), ("透明度 A", 3)]
                        {
                            field(
                                fields,
                                face,
                                state,
                                descriptor,
                                label,
                                WaterPresetField::BaseColor(component),
                            );
                        }
                    });
                },
            );
        }
        WaterSection::Waves => {
            readonly(
                parent,
                face,
                "固定四组宏观波；静止水位不变，风浪速度独立于场景水流速度。方向 X/Z 使用世界坐标。",
            );
            for wave in 0..descriptor.macro_waves.len() {
                group(
                    parent,
                    face,
                    &format!("宏观波 {}", wave + 1),
                    "振幅 0～10 m · 波长 0.1～100000 m · 波速 0～1000 m/s · 陡峭度 0～1；保存时联合校验方向与波形。",
                    |body| {
                        row(body, |fields| {
                            for (label, field_id) in [
                                (
                                    "方向 X",
                                    WaterPresetField::WaveDirection { wave, component: 0 },
                                ),
                                (
                                    "方向 Z",
                                    WaterPresetField::WaveDirection { wave, component: 1 },
                                ),
                                ("振幅（m）", WaterPresetField::WaveAmplitude(wave)),
                                ("波长（m）", WaterPresetField::WaveWavelength(wave)),
                                ("波速（m/s）", WaterPresetField::WaveSpeed(wave)),
                                ("陡峭度", WaterPresetField::WaveSteepness(wave)),
                            ] {
                                field(fields, face, state, descriptor, label, field_id);
                            }
                        });
                    },
                );
            }
        }
        WaterSection::Normals => {
            readonly(
                parent,
                face,
                "在右侧资产页选择 PNG/JPG/JPEG/KTX2 后明确采用；线性数据、Repeat 寻址与三线性过滤保持不变。仅浏览文件不修改草稿。",
            );
            for layer in 0..descriptor.micro_normals.len() {
                group(
                    parent,
                    face,
                    &format!("微法线 {}", layer + 1),
                    "平铺 0.01～100000 m · 强度 0～4；滚动速度独立于水流。清空只关闭当前层。",
                    |body| {
                        texture_row(
                            body,
                            face,
                            "纹理来源",
                            descriptor.micro_normals[layer].texture_path.as_deref(),
                            layer,
                        );
                        row(body, |fields| {
                            for (label, field_id) in [
                                ("平铺米数（m）", WaterPresetField::NormalTile(layer)),
                                (
                                    "滚动 X（m/s）",
                                    WaterPresetField::NormalVelocity {
                                        layer,
                                        component: 0,
                                    },
                                ),
                                (
                                    "滚动 Z（m/s）",
                                    WaterPresetField::NormalVelocity {
                                        layer,
                                        component: 1,
                                    },
                                ),
                                ("法线强度", WaterPresetField::NormalStrength(layer)),
                            ] {
                                field(fields, face, state, descriptor, label, field_id);
                            }
                        });
                    },
                );
            }
        }
        WaterSection::Material => {
            group(
                parent,
                face,
                "PBR 材质",
                "粗糙度与反射率均为 0～1，继续使用正式透明 PBR 水材质。",
                |body| {
                    row(body, |fields| {
                        for (label, field_id) in [
                            ("粗糙度", WaterPresetField::Roughness),
                            ("反射率", WaterPresetField::Reflectance),
                        ] {
                            field(fields, face, state, descriptor, label, field_id);
                        }
                    });
                },
            );
            group(
                parent,
                face,
                "波谷与岸边",
                "波谷压暗 0～1 · 岸边衰减指数 0.1～8；岸边安全深度仍由正式波形与水域联合校验。",
                |body| {
                    row(body, |fields| {
                        for (label, field_id) in [
                            ("波谷压暗", WaterPresetField::TroughDarkening),
                            ("岸边衰减指数", WaterPresetField::ShoreWaveAttenuation),
                        ] {
                            field(fields, face, state, descriptor, label, field_id);
                        }
                    });
                },
            );
        }
    }
}
