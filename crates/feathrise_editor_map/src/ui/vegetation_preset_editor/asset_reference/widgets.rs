//! 路径小动作与非模态子资产选择；复用工作台弹性布局和独立编辑器主题。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::super::types::{
    AssetReferenceAction, PresetEditorAction, PresetEditorButton, PresetField, ReferencePhase,
    SubAssetChoice, VegetationPresetEditorState,
};
use super::helpers::reference_kind; // 所有路径都使用同一个类型分类。
use bevy::prelude::*; // 使用现有Bevy布局和输入组件。
use bevy::ui::FocusPolicy; // 文字不抢按钮命中，选择区不遮挡右侧资产。
use crate::ui::theme as colors; // 与中央编辑器既有风格一致。 // 动作携带具体字段或票据。

pub(super) const CHOICES_PER_PAGE: usize = 8; // 可见行预算，不是模型或材质数量限制。
const CONTROL_HEIGHT: f32 = 32.0; // 与工作台路径输入槽同高；窄窗动作整枚换行。

/// 路径行只追加两个紧凑同高按钮，保留原输入框和手动粘贴能力。
pub(in super::super) fn spawn_field_actions(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    field: PresetField,
) {
    if reference_kind(field).is_none() {
        return;
    } // 普通名字/数字不增加无关按钮。
    button(
        parent,
        font,
        "定位",
        AssetReferenceAction::Locate(field),
        false,
    ); // 从当前引用反向定位浏览器。
    button(
        parent,
        font,
        "使用选中",
        AssetReferenceAction::UseSelected(field),
        false,
    ); // 把右侧文件赋给这一个字段。
}

/// 在中央表单内显示请求和选择列表，不创建覆盖资产栏的模态框。
pub(in super::super) fn spawn_picker(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &VegetationPresetEditorState,
    gltfs: &Assets<bevy::gltf::Gltf>, // 仅查本次根，不扫描Assets集合。
    gltf_meshes: &Assets<bevy::gltf::GltfMesh>, // 可见网格显示真实名称与编号。
) {
    let Some(request) = state.asset_reference.as_ref() else {
        return;
    }; // 稳定界面没有额外占位。
    if state.active_preset_tab_id() != Some(request.target.tab_id) {
        return;
    } // 别的标签不显示旧请求。
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),                     // 与当前大表单共用宽度。
                flex_direction: FlexDirection::Column, // 提示、候选与取消按钮按自然高度排列。
                flex_shrink: 0.0,                      // 过长时由正文外层滚动，不压扁字段。
                row_gap: Val::Px(4.0),                 // 保持紧凑纵向节奏。
                padding: UiRect::all(Val::Px(6.0)),    // 文字与铜边之间留白。
                border: UiRect::all(Val::Px(1.0)),     // 明确区分临时选择与正式参数。
                border_radius: BorderRadius::all(Val::Px(3.0)), // 沿用现有小圆角。
                ..default()                            // 不使用绝对位置或全屏遮罩。
            },
            BackgroundColor(colors::PANEL_BG_STRONG),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
            FocusPolicy::Pass,
        )) // 不抢走右侧预览操作。
        .with_children(|panel| {
            text(
                panel,
                font,
                &format!("资产引用 → {}", request.target.description),
                colors::TEXT_ACCENT,
            ); // 显式回显将修改的目标。
            text(
                panel,
                font,
                &format!("来源：{}", request.source_path),
                colors::TEXT_VALUE,
            ); // 源文件在请求时已冻结。
            text(
                panel,
                font,
                "仅替换这一条引用；保留其他LOD、半径、距离、部件变换和其他材质。",
                colors::TEXT_MUTED,
            ); // 避免误解为整层重新导入。
            if matches!(request.phase, ReferencePhase::Choosing) {
                let pages = request.choices.len().div_ceil(CHOICES_PER_PAGE).max(1); // 任意候选总量按真实数分页。
                text(
                    panel,
                    font,
                    &format!(
                        "请选择子资产 · {}项 · {}/{}",
                        request.choices.len(),
                        request.page + 1,
                        pages
                    ),
                    colors::TEXT_PRIMARY,
                ); // 不隐藏多候选事实。
                let start = request.page * CHOICES_PER_PAGE; // 只读取当前固定八项。
                for (offset, choice) in request
                    .choices
                    .iter()
                    .skip(start)
                    .take(CHOICES_PER_PAGE)
                    .enumerate()
                {
                    let label = match choice {
                        SubAssetChoice::Mesh { mesh, primitive } => {
                            let name = request
                                .gltf
                                .as_ref()
                                .and_then(|handle| gltfs.get(handle))
                                .and_then(|gltf| gltf.meshes.get(*mesh))
                                .and_then(|handle| gltf_meshes.get(handle))
                                .map(|mesh| mesh.name.chars().take(64).collect::<String>())
                                .unwrap_or_default(); // 每页最多八个短名称，超长作者名不能撑破UI。
                            if name.is_empty() {
                                format!("Mesh {mesh} / Primitive {primitive}")
                            } else {
                                format!("{name} · Mesh {mesh} / Primitive {primitive}")
                            } // 名称帮助识别，实际写入仍使用精确数字路径。
                        }
                        SubAssetChoice::Material(index) => format!("Material {index} / std"), // 明确StandardMaterial类型。
                        SubAssetChoice::DefaultMaterial => {
                            "默认材质 / DefaultMaterial/std".to_owned()
                        } // 默认材质也明确标识。
                    };
                    button(
                        panel,
                        font,
                        &label,
                        AssetReferenceAction::Choose {
                            ticket: request.ticket,
                            index: start + offset,
                        },
                        true,
                    ); // 一次点击只选择这个请求内的真实项。
                }
                if pages > 1 {
                    panel
                        .spawn(Node {
                            column_gap: Val::Px(4.0),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|row| {
                            // 固定两个翻页按钮，不建全部候选UI。
                            button(
                                row,
                                font,
                                "上一组",
                                AssetReferenceAction::Previous {
                                    ticket: request.ticket,
                                },
                                false,
                            ); // 首组保持不变。
                            button(
                                row,
                                font,
                                "下一组",
                                AssetReferenceAction::Next {
                                    ticket: request.ticket,
                                },
                                false,
                            ); // 可访问所有真实候选。
                        });
                }
            } else {
                text(
                    panel,
                    font,
                    "正在加载并验证资产…成功前保留原值；可以随时取消。",
                    colors::TEXT_MUTED,
                ); // 不把任务开始说成已完成。
            }
            button(
                panel,
                font,
                "取消资产选择",
                AssetReferenceAction::Cancel {
                    ticket: request.ticket,
                },
                false,
            ); // 取消不改草稿、不写文件。
        });
}

/// 标题、提示和候选使用当前中文字体；长路径可在内嵌面板自然换行。
fn text(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, label: &str, color: Color) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font: font.clone().into(), // 复用现有字体句柄。
            font_size: ui_metrics::FONT_CONTROL.into(),     // 与路径字段正文同字号。
            ..default()                // 正常中文排版。
        },
        TextColor(color),
        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            ..default()
        },
        FocusPolicy::Pass,
    )); // 说明文本不截获按钮点击。
}

/// 同行按钮按文字测宽，候选标签可增高换行，精确子资产编号不被裁切。
fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: AssetReferenceAction,
    full_width: bool,
) {
    parent
        .spawn((
            InteractionButton,                                                         // 由Bevy产生真实Interaction。
            PresetEditorButton(PresetEditorAction::AssetReference(action)), // 原有按钮样式系统继续生效。
            Node {
                width: if full_width {
                    Val::Percent(100.0)
                } else {
                    Val::Auto
                }, // 路径按钮不占大块固定宽度。
                min_height: Val::Px(CONTROL_HEIGHT), // 长候选可自然增高，不裁掉精确子资产编号。
                min_width: Val::Px(0.0),         // 超长模型名称不撑大候选面板。
                max_width: Val::Percent(100.0), // 路径动作和候选都限制在所属正文内。
                flex_shrink: 0.0,             // 整枚按钮换行；长候选内部可按字符折行。
                align_items: AlignItems::Center, // 纵向中心严格对齐。
                justify_content: JustifyContent::Center, // 小按钮文字居中。
                padding: UiRect::horizontal(Val::Px(6.0)), // 只留必要点击留白。
                border: UiRect::all(Val::Px(1.0)), // 沿用铜边按钮样式。
                border_radius: BorderRadius::all(Val::Px(3.0)), // 同一编辑器圆角。
                ..default()                   // 不新增UI缩放、绝对坐标或输入绑定。
            },
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL), // 原有样式系统负责悬停/按下反馈。
            BorderColor::all(colors::COPPER_BORDER_WEAK),     // 与相邻字段同一层级。
            FocusPolicy::Block,                               // 操作只归属这个按钮。
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font: font.clone().into(), // 复用中央字体。
                font_size: ui_metrics::FONT_CONTROL.into(),     // 保持统一字号。
                ..default()                // 其余字体参数不变。
            },
            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
            TextColor(colors::TEXT_PRIMARY),
            FocusPolicy::Pass,
        )); // 点击字形也命中所属按钮。
}
