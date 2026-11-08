//! 中央视口右上角的Blender式XYZ方向轴。
//!
//! 方向轴复用现有轨道相机与正式释放点击：六个端点精确对轴，中心切换投影。

use feathrise_editor_ui::InteractionButton;
use bevy::picking::Pickable; // 装饰线与文字穿透，按钮和控件空隙拥有指针。
use bevy::prelude::*; // UI节点、文本、颜色、相机姿态与ECS查询。
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_ui::EditorButtonClick;

use crate::components::{
    MapEditorCamera, MapEditorOrientationAxisEndpoint, MapEditorOrientationAxisLine,
    MapEditorOrientationAxisRoot, MapEditorViewport,
}; // 使用结构化marker同步固定节点，不依赖中文Name查询。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState};
use crate::types::EditorView;

#[derive(Component)]
pub(crate) struct OrientationProjectionToggle;

const ORIENTATION_FONT_PATH: &str = "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf"; // 与F12其余文字共享同一字体资产。
const WIDGET_SIZE: f32 = 82.0; // 紧凑尺寸不遮挡地图，同时容纳正负六个端点。
const WIDGET_INSET: f32 = 8.0; // 与中央画面铜边保持小幅呼吸空间。
const WIDGET_CENTER: f32 = WIDGET_SIZE * 0.5; // 全部轴线和端点共用固定逻辑像素中心。
const AXIS_RADIUS: f32 = 25.0; // 正方向端点距中心的最大屏幕半径。
const LINE_THICKNESS: f32 = 2.0; // 接近Blender细轴线，不做粗重游戏HUD描边。
const POSITIVE_ENDPOINT_SIZE: f32 = 19.0; // 实心端点同时容纳单个拉丁字母。
const NEGATIVE_ENDPOINT_SIZE: f32 = 14.0; // 负方向只显示较小空心圆。
const NEGATIVE_ENDPOINT_HIDE_RADIUS: f32 = 4.0; // 轴正对相机时隐藏与正端点重叠的负向圆。

/// 一个世界轴经编辑相机旋转后得到的二维位置和相机空间深度。
#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectedAxis {
    offset: Vec2, // 从控件中心指向正方向端点的UI像素偏移，Y轴已经翻转为向下为正。
    depth: f32,   // 正方向端点的相机局部Z；越大表示越靠近观察者。
}

/// 把单位世界轴投影到固定大小的屏幕空间方向控件。
fn project_world_axis(camera_rotation: Quat, world_axis: Vec3) -> ProjectedAxis {
    let camera_axis = camera_rotation.inverse() * world_axis; // 世界方向转入相机局部坐标，不受相机平移和投影缩放影响。
    ProjectedAxis {
        offset: Vec2::new(camera_axis.x, -camera_axis.y) * AXIS_RADIUS, // Bevy UI向下为正，因此翻转相机局部Y。
        depth: camera_axis.z,
    }
}

/// 返回编辑器方向控件采用的标准轴色。
fn axis_color(world_axis: Vec3) -> Color {
    if world_axis == Vec3::X {
        Color::srgb(0.91, 0.22, 0.31) // X：红色。
    } else if world_axis == Vec3::Y {
        Color::srgb(0.46, 0.72, 0.16) // Y：绿色；项目世界仍保持Y向上。
    } else {
        Color::srgb(0.20, 0.48, 0.86) // Z：蓝色。
    }
}

/// 为中央ViewportNode创建固定UI覆盖层；端点按松开执行，装饰层不截获点击。
pub(crate) fn setup_editor_orientation_axis(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    viewports: Query<Entity, Added<MapEditorViewport>>,
) {
    let Ok(viewport_entity) = viewports.single() else {
        return; // BSN中央节点尚未出现或结构异常时不生成游离控件。
    };
    let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, ORIENTATION_FONT_PATH); // 三个字母共享一个强类型句柄。
    commands.entity(viewport_entity).with_children(|viewport| {
        viewport
            .spawn((
                Name::new("地图编辑器XYZ方向轴"),
                MapEditorOrientationAxisRoot,
                Node {
                    position_type: PositionType::Absolute, // 固定在ViewportNode内部，不参与中央画面Flex布局。
                    right: px(WIDGET_INSET),
                    top: px(42.0), // 四球着色位于同一右上角首行，方向球从其下方开始。
                    width: px(WIDGET_SIZE),
                    height: px(WIDGET_SIZE),
                    ..default()
                },
                ZIndex(30), // 独立观察区覆盖元素栏空白背景；按钮本身另由窄屏布局避让，不用层级掩盖按钮。
                Interaction::default(),
                RelativeCursorPosition::default(),
                FocusPolicy::Block,
            ))
            .with_children(|widget| {
                widget.spawn((
                    Name::new("方向球中心：切换正交与透视"),
                    InteractionButton,
                    OrientationProjectionToggle,
                    crate::ui::tooltips::EditorTooltipTarget(
                        crate::ui::tooltips::TooltipAction::OrientationProjection,
                    ),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(WIDGET_CENTER - 5.0),
                        top: px(WIDGET_CENTER - 5.0),
                        width: px(10.0),
                        height: px(10.0),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.78, 0.80, 0.78, 0.9)),
                    ZIndex(40), // 中心点击切投影；正对镜头的轴仍保留周围可点击圆环。
                    FocusPolicy::Block,
                ));
                for (world_axis, label) in [(Vec3::X, "X"), (Vec3::Y, "Y"), (Vec3::Z, "Z")] {
                    let color = axis_color(world_axis);
                    widget.spawn((
                        Name::new(format!("{label}方向轴线")),
                        MapEditorOrientationAxisLine { world_axis },
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(WIDGET_CENTER),
                            top: px(WIDGET_CENTER - LINE_THICKNESS * 0.5),
                            width: px(1.0),
                            height: px(LINE_THICKNESS),
                            border_radius: BorderRadius::all(px(1.0)),
                            ..default()
                        },
                        UiTransform::default(),
                        BackgroundColor(color),
                        Visibility::Hidden, // 相机姿态同步系统在节点完整出现后首次定位并显示。
                        ZIndex(2),
                        FocusPolicy::Pass,
                        Pickable::IGNORE,
                    ));
                    widget
                        .spawn((
                            Name::new(format!("{label}正方向端点")),
                            InteractionButton,
                            crate::ui::tooltips::EditorTooltipTarget(
                                crate::ui::tooltips::TooltipAction::OrientationAxis,
                            ),
                            MapEditorOrientationAxisEndpoint {
                                world_axis,
                                positive: true,
                            },
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(WIDGET_CENTER - POSITIVE_ENDPOINT_SIZE * 0.5),
                                top: px(WIDGET_CENTER - POSITIVE_ENDPOINT_SIZE * 0.5),
                                width: px(POSITIVE_ENDPOINT_SIZE),
                                height: px(POSITIVE_ENDPOINT_SIZE),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(percent(50)),
                                ..default()
                            },
                            BackgroundColor(color),
                            ZIndex(20),
                            FocusPolicy::Block,
                        ))
                        .with_child((
                            Text::new(label),
                            TextFont {
                                font: font.clone().into(),
                                font_size: 10.5.into(),
                                ..default()
                            },
                            TextColor(Color::srgb(0.04, 0.05, 0.05)),
                            FocusPolicy::Pass,
                            Pickable::IGNORE,
                        ));
                    widget.spawn((
                        Name::new(format!("{label}负方向端点")),
                        InteractionButton,
                        crate::ui::tooltips::EditorTooltipTarget(
                            crate::ui::tooltips::TooltipAction::OrientationAxis,
                        ),
                        MapEditorOrientationAxisEndpoint {
                            world_axis,
                            positive: false,
                        },
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(WIDGET_CENTER - NEGATIVE_ENDPOINT_SIZE * 0.5),
                            top: px(WIDGET_CENTER - NEGATIVE_ENDPOINT_SIZE * 0.5),
                            width: px(NEGATIVE_ENDPOINT_SIZE),
                            height: px(NEGATIVE_ENDPOINT_SIZE),
                            border: UiRect::all(px(2.0)),
                            border_radius: BorderRadius::all(percent(50)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        BorderColor::all(color),
                        ZIndex(10),
                        FocusPolicy::Block,
                    ));
                }
            });
    });
}

/// 六轴观察保持焦点和缩放；固定视图采用正交，随后右键仍可自由环绕。
fn axis_angles(axis: Vec3, positive: bool) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let sign = if positive { 1.0 } else { -1.0 };
    if axis == Vec3::X {
        (sign * FRAC_PI_2, 0.0)
    } else if axis == Vec3::Y {
        (0.0, sign * FRAC_PI_2)
    } else {
        (if positive { 0.0 } else { PI }, 0.0)
    }
}

/// UI只更新观察状态，不改作者记录、对象选择或几何事务。
pub(crate) fn orientation_axis_input(
    mut editor: ResMut<MapEditorState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    input: super::view_controls_input::ViewControlsInput,
    viewport: Option<ResMut<MapEditorViewportState>>,
    mesh: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    endpoints: Query<
        (
            Entity,
            &MapEditorOrientationAxisEndpoint,
            &EditorButtonClick,
            &Visibility,
        ),
        Changed<EditorButtonClick>,
    >,
    toggles: Query<
        (Entity, &EditorButtonClick),
        (
            With<OrientationProjectionToggle>,
            Changed<EditorButtonClick>,
        ),
    >,
    nodes: Query<(&Node, Option<&ChildOf>)>,
) {
    if !input.available(&editor)
        || guard.blocked()
        || editor.new_map_dialog_open
        || editor.map_switch_confirmation.is_some()
        || editor.map_transition_in_progress
        || editor.pending_map_transition.is_some()
        || mesh.as_ref().is_some_and(|mesh| mesh.moving())
    {
        return;
    }
    let Some(mut viewport) = viewport else {
        return;
    };
    for (entity, endpoint, click, visibility) in &endpoints {
        if !click.is_clicked()
            || *visibility == Visibility::Hidden
            || !super::modes::controls_visible(entity, &nodes)
        {
            continue;
        }
        let rotation = Quat::from_rotation_y(viewport.yaw) * Quat::from_rotation_x(-viewport.pitch);
        let desired = endpoint.world_axis * if endpoint.positive { 1.0 } else { -1.0 };
        let aligned = (rotation * Vec3::Z).dot(desired) > 0.9999;
        // 对轴后负端点会与正端点重叠；再次点当前正轴端点翻到背面，不要求寻找被遮住的小圆。
        (viewport.yaw, viewport.pitch) = axis_angles(
            endpoint.world_axis,
            if aligned {
                !endpoint.positive
            } else {
                endpoint.positive
            },
        );
        editor.view = EditorView::Perspective; // 自由姿态字段是六个方向的权威，避免旧四视图枚举覆盖负轴。
        editor.orthographic = true;
        editor.suppress_world_pick_this_frame = true; // 完成点击的释放帧也不得落入地图选择。
        return;
    }
    if toggles
        .iter()
        .any(|(entity, click)| click.is_clicked() && super::modes::controls_visible(entity, &nodes))
    {
        editor.orthographic = !editor.orthographic;
        editor.suppress_world_pick_this_frame = true;
    }
}

/// 相机旋转变化后原地更新三条线和六个端点；稳定姿态不触发UI布局改写。
pub(crate) fn sync_editor_orientation_axis(
    editor: Res<MapEditorState>,
    cameras: Query<&Transform, With<MapEditorCamera>>,
    mut cached_rotation: Local<Option<Quat>>,
    mut lines: Query<
        (
            &MapEditorOrientationAxisLine,
            &mut Node,
            &mut UiTransform,
            &mut Visibility,
        ),
        Without<MapEditorOrientationAxisEndpoint>,
    >, // 轴线与端点都会写Node/Visibility，显式排除端点后向Bevy证明两个可变查询互斥。
    mut endpoints: Query<
        (
            &MapEditorOrientationAxisEndpoint,
            &mut Node,
            &mut Visibility,
            &mut ZIndex,
        ),
        Without<MapEditorOrientationAxisLine>, // 端点反向排除轴线；错误地同时挂两种marker的实体也不会进入任一写集合。
    >,
) {
    if !editor.open || lines.is_empty() || endpoints.is_empty() {
        return; // 关闭时父节点已经隐藏；延迟生成期间不能提前缓存相机姿态。
    }
    let Ok(camera_transform) = cameras.single() else {
        return; // 离屏相机尚未由命令缓冲采用时等待下一帧。
    };
    let rotation = camera_transform.rotation;
    if cached_rotation.as_ref() == Some(&rotation) {
        return; // 相机未旋转时端点和线段布局完全不变，平移/缩放无需重复更新。
    }

    for (axis, mut node, mut ui_transform, mut visibility) in &mut lines {
        let projected = project_world_axis(rotation, axis.world_axis);
        let length = projected.offset.length();
        if length <= 0.25 {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden; // 正对镜头的轴退化为中心圆点，不绘制零长矩形。
            }
            continue;
        }
        let midpoint = Vec2::splat(WIDGET_CENTER) + projected.offset * 0.5;
        let desired_left = px(midpoint.x - length * 0.5);
        let desired_top = px(midpoint.y - LINE_THICKNESS * 0.5);
        let desired_width = px(length);
        if node.left != desired_left || node.top != desired_top || node.width != desired_width {
            node.left = desired_left;
            node.top = desired_top;
            node.width = desired_width;
        }
        let desired_rotation = Rot2::radians(projected.offset.y.atan2(projected.offset.x));
        if ui_transform.rotation != desired_rotation {
            ui_transform.rotation = desired_rotation; // UiTransform绕线段中心旋转，节点中点已经位于轴线半程。
        }
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }

    for (endpoint, mut node, mut visibility, mut z_index) in &mut endpoints {
        let projected = project_world_axis(rotation, endpoint.world_axis);
        let offset = if endpoint.positive {
            projected.offset
        } else {
            -projected.offset
        };
        let diameter = if endpoint.positive {
            POSITIVE_ENDPOINT_SIZE
        } else {
            NEGATIVE_ENDPOINT_SIZE
        };
        let desired_left = px(WIDGET_CENTER + offset.x - diameter * 0.5);
        let desired_top = px(WIDGET_CENTER + offset.y - diameter * 0.5);
        if node.left != desired_left || node.top != desired_top {
            node.left = desired_left;
            node.top = desired_top;
        }
        let desired_visibility =
            if !endpoint.positive && projected.offset.length() < NEGATIVE_ENDPOINT_HIDE_RADIUS {
                Visibility::Hidden // 与正端点几乎重合时只保留带字母的方向身份。
            } else {
                Visibility::Inherited
            };
        if *visibility != desired_visibility {
            *visibility = desired_visibility;
        }
        let endpoint_depth = if endpoint.positive {
            projected.depth
        } else {
            -projected.depth
        };
        let desired_z = 20 + (endpoint_depth * 8.0).round() as i32;
        if z_index.0 != desired_z {
            z_index.0 = desired_z; // 靠近观察者的圆覆盖远端圆，保持透视旋转时的空间读向。
        }
    }
    *cached_rotation = Some(rotation); // 只在完整节点组成功同步后登记缓存。
}

#[cfg(test)]
#[path = "orientation_axis_tests.rs"]
mod tests;
