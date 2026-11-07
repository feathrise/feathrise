//! 语言专项的漏译回归。原按钮、滚轮和真实窗口截图共同验证显示链路。

use super::*;
use bevy::input::{mouse::{MouseScrollUnit, MouseWheel}, touch::TouchPhase};
use bevy::ui::RelativeCursorPosition;
use feathrise_editor_map::workbench::WorkbenchSettingsBody;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailStage {
    PluginMenu, PluginOpen, PluginDetails, PluginClose,
    OpenSettings, Lod, LodDisplay, LodToggles, LodReset, Snapping, SettingsHelp, SettingsClose,
    ExportMenu, ExportOpen, ExportDetails, ExportClose,
    MapHelp, MapHelpDetails, Sculpt, Puddles, PuddleDetails,
    Vegetation, VegetationDetails, World, WorldStatus, WorldStatusDetails,
    Diagnostics, DiagnosticsDetails, DiagnosticsOutside, CloseDiagnostics, DiagnosticsClosed,
    OpenLog, Log, LogHeader, LogWithCode, CaptureLogWithCode, CloseLog, CloseLogCode, LogCodeClosed,
    Editor3d, Editor3dDetails, CloseEditor3dDetails,
}

/// 插件包来自本轮夹具，真实安装和活动索引仍由原包事务生成。
pub(super) fn prepare_plugin(root: &Path) -> Result<(), String> {
    use crate::native_plugins::management::{packages, types::{ManagementCommand, PluginRoots, PluginScope}, validation_binary};
    let build_dir = root.join("plugin-probe");
    fs::create_dir(&build_dir).map_err(|error| error.to_string())?;
    let source = validation_binary::build_versions(&build_dir)?.into_iter().next().ok_or("缺少协议探针")?;
    let roots = PluginRoots { project: None, global: root.join("plugins") };
    packages::stage_at(&roots, PluginScope::Global, ManagementCommand::Install { source, replace: false })?;
    let warnings = packages::apply_pending_with_roots(&roots);
    if !warnings.is_empty() {
        return Err(warnings.join("\n"));
    }
    Ok(())
}

pub(super) fn advance(world: &mut World, run: &mut Run, window: Entity, detail: DetailStage) -> Result<(), String> {
    use DetailStage::*;
    let next = match detail {
        PluginMenu if click(world, run, window, "Project ▼") => PluginOpen,
        PluginOpen if click(world, run, window, "Manage Plugins…") => PluginDetails,
        PluginDetails if visible_text(world, "Plugin Manager") && visible_text(world, "Loaded this user's global plugins.") => {
            if !visible_containing(world, "协议探针")
                || !visible_text(world, "1 item")
                || !visible_containing(world, "Source  Global.")
                || !visible_text(world, "Current Status  Active")
                || !visible_containing(world, "Runtime Dependencies  example_runtime")
            {
                return Ok(());
            }
            if !capture(world, run, "plugin-manager-en")? { return Ok(()); }
            escape(world, window);
            PluginClose
        }
        PluginClose if !visible_text(world, "Plugin Manager") => {
            if run.panels {
                run.stage = Stage::HomeNewEnglish;
            } else {
                world.write_message(WorkspaceCommand::Open(run.root.join("project")));
                run.stage = Stage::WaitForAuthor;
            }
            return Ok(());
        }
        OpenSettings if click(world, run, window, "Settings") => Lod,
        Lod if settings_in_view(world, window, &[
            "Global Mesh LOD", "Render LOD: On", "Screen-Space Error", "Switch Hysteresis",
            "Ratio", "Collision LOD: Original Mesh", "Collision Safety Radius", "Collision Error",
            "Reset Mesh LOD Defaults",
        ]) => {
            if !capture(world, run, "mesh-lod-en")? { return Ok(()); }
            LodDisplay
        }
        LodDisplay if click(world, run, window, "Render LOD: On") => LodToggles,
        LodToggles if visible_text(world, "Render LOD: Original Mesh")
            && click(world, run, window, "Collision LOD: Original Mesh") => LodReset,
        LodReset if visible_text(world, "Collision LOD: On")
            && click(world, run, window, "Reset Mesh LOD Defaults") => Snapping,
        Snapping if visible_text(world, "Render LOD: On") && visible_text(world, "Collision LOD: Original Mesh")
            && settings_in_view(world, window, &["Transform Snapping", "Move Increment", "m", "Rotation Increment", "Scale Increment", "×", "Reset Snapping"]) => {
            if !capture(world, run, "transform-snapping-en")? { return Ok(()); }
            SettingsHelp
        }
        SettingsHelp if click_help(world, run, window, "Settings") => SettingsClose,
        SettingsClose if visible_containing(world, "Changes apply immediately and save automatically.") => {
            if !capture(world, run, "settings-help-en")? { return Ok(()); }
            escape(world, window);
            ExportMenu
        }
        ExportMenu => {
            if visible_text(world, "Done") {
                click(world, run, window, "Done");
                return Ok(());
            }
            if !click(world, run, window, "保存 ▼") { return Ok(()); }
            ExportOpen
        }
        ExportOpen if click(world, run, window, "Export Project…") => ExportDetails,
        ExportDetails if visible_text(world, "Export the current project's saved content.") => {
            if !capture(world, run, "export-en")? { return Ok(()); }
            escape(world, window);
            ExportClose
        }
        ExportClose if !visible_text(world, "Export Project") => MapHelp,
        MapHelp if click_help(world, run, window, "Root") => MapHelpDetails,
        MapHelpDetails if visible_text(world, "Map Catalog · Details and Controls")
            && visible_text(world, "No Map Loaded")
            && visible_containing(world, "New maps and folders go in the root folder.") => {
            if !capture(world, run, "map-catalog-help-en")? { return Ok(()); }
            escape(world, window);
            Sculpt
        }
        Sculpt if !visible_text(world, "Map Catalog · Details and Controls")
            && click(world, run, window, "Sculpt") => Puddles,
        Puddles if click(world, run, window, "Puddles") => PuddleDetails,
        PuddleDetails if visible_text(world, "Mode: Select · Puddle Potential (standby)")
            && visible_text(world, "Current Mesh: Waiting for map data")
            && visible_text(world, "Waiting for current map data") => {
            if !capture(world, run, "sculpt-puddles-en")? { return Ok(()); }
            Vegetation
        }
        Vegetation if click(world, run, window, "Vegetation") => VegetationDetails,
        VegetationDetails if visible_containing(world, "Selected Preset: None")
            && visible_text(world, "Preset Library · No presets. Create one above.")
            && visible_text(world, "Select a preset from the library above to create an empty layer.")
            && visible_containing(world, "Map Layers · None") => {
            if !capture(world, run, "vegetation-en")? { return Ok(()); }
            World
        }
        World if click(world, run, window, "World") => WorldStatus,
        WorldStatus if visible_text(world, "World Debug · Current Run")
            && visible_text(world, "Time Paused: Off") && visible_text(world, "Grass Tip Sway")
            && visible_text(world, "Randomize Weather") && visible_text(world, "Heavy Rain")
            && click(world, run, window, "Details") => WorldStatusDetails,
        WorldStatusDetails if visible_text(world, "Status Details")
            && show_status_middle(world, window, "Global source is up to date.") => {
            if !capture(world, run, "status-en")? { return Ok(()); }
            let detail = world.query::<&Text>().iter(world)
                .find(|text| text.contains("Current Task:") && text.contains("Global source is up to date."))
                .ok_or("状态详情正文未生成")?;
            report(run, "status-details", json!({"text":detail.as_str()}))?;
            if detail.chars().any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)) {
                return Err("空地图英文状态详情仍含内置中文文案".into());
            }
            escape(world, window);
            Diagnostics
        }
        Diagnostics => {
            if visible_text(world, "Status Details") {
                click(world, run, window, "Details");
                return Ok(());
            }
            if !click(world, run, window, "Code") { return Ok(()); }
            DiagnosticsDetails
        }
        DiagnosticsDetails if visible_text(world, "Code Diagnostics")
            && visible_text(world, "Saving code starts a build")
            && visible_containing(world, "No build diagnostics.")
            && visible_containing(world, "Diagnostics: 0") => {
            if !capture(world, run, "code-diagnostics-en")? { return Ok(()); }
            DiagnosticsOutside
        }
        DiagnosticsDetails if !visible_text(world, "Code Diagnostics") => {
            click(world, run, window, "Code");
            return Ok(());
        }
        DiagnosticsOutside if visible_text(world, "Code Diagnostics")
            && click(world, run, window, "Sculpt") => CloseDiagnostics,
        CloseDiagnostics if visible_text(world, "Code Diagnostics") => {
            // 以工具页切换和浮窗实际显隐为准。冷启动中被输入门丢弃的点击只能重新从原按钮发起。
            if !visible_text(world, "Puddles") {
                click(world, run, window, "Sculpt");
                return Ok(());
            }
            if !click(world, run, window, "Code") { return Ok(()); }
            DiagnosticsClosed
        }
        DiagnosticsClosed => {
            if visible_text(world, "Code Diagnostics") {
                click(world, run, window, "Code");
            } else {
                run.stage = if run.panels { Stage::BuildMenu } else { Stage::OpenDocument };
            }
            return Ok(());
        }
        OpenLog if !visible_text(world, "Status Details")
            && click(world, run, window, "Log") => Log,
        Log if visible_text(world, "Project Log") => {
            let evidence = world.resource::<crate::project_log::ProjectLogState>().validation_tail();
            let text = evidence.to_string();
            let command_visible = text.contains("Working directory: ") && text.contains("Build command");
            let code_task = text.contains("Code Build #");
            let viewport_task = text.contains("Project 3D #");
            // 构建槽可能先执行代码编译。浮窗专项只要求实际命令已进入日志，完整双语验收仍核对两类任务。
            let log_ready = if run.floating_panels {
                command_visible && (code_task || viewport_task)
            } else {
                command_visible && code_task && viewport_task && text.contains("Preparing the project")
                    && text.contains("Build tool: Native Cargo")
            };
            if !log_ready { return Ok(()); }
            if !click(world, run, window, "Follow: On") { return Ok(()); }
            report(run, "project-log-lines", evidence)?;
            LogHeader
        }
        Log => {
            click(world, run, window, "Log");
            return Ok(());
        }
        LogHeader if visible_text(world, "Follow: Off") => {
            if !capture(world, run, "project-log-en")? { return Ok(()); }
            LogWithCode
        }
        LogHeader => {
            click(world, run, window, "Follow: On");
            return Ok(());
        }
        LogWithCode if click(world, run, window, "Code") => CaptureLogWithCode,
        CaptureLogWithCode if visible_text(world, "Code Diagnostics") && visible_text(world, "Project Log") => {
            if !capture(world, run, "project-log-with-code-en")? { return Ok(()); }
            CloseLog
        }
        CaptureLogWithCode if visible_text(world, "Project Log") => {
            click(world, run, window, "Code");
            return Ok(());
        }
        CloseLog if click(world, run, window, "Log") => CloseLogCode,
        CloseLogCode if visible_text(world, "Code Diagnostics") => {
            if visible_text(world, "Project Log") {
                click(world, run, window, "Log");
                return Ok(());
            }
            if !click(world, run, window, "Code") { return Ok(()); }
            LogCodeClosed
        }
        LogCodeClosed if !visible_text(world, "Project Log") => {
            if visible_text(world, "Code Diagnostics") {
                click(world, run, window, "Code");
            } else {
                run.stage = Stage::StopBuild;
            }
            return Ok(());
        }
        Editor3d if visible_containing(world, "Switched to Editor 3D. Click Run to launch the saved project.")
            && click(world, run, window, "Details") => Editor3dDetails,
        Editor3dDetails if visible_text(world, "Status Details")
            && visible_containing(world, "Switched to Editor 3D. Click Run to launch the saved project.") => {
            if !capture(world, run, "editor-3d-ready-en")? { return Ok(()); }
            CloseEditor3dDetails
        }
        CloseEditor3dDetails if click(world, run, window, "Details") => {
            run.stage = Stage::MeshTab;
            return Ok(());
        }
        _ => return Ok(()),
    };
    run.stage = Stage::Details(next);
    Ok(())
}

/// 详情正文是一个长 Text，已排版仍可能在裁剪区之外。用原滚轮打开中段环境回执。
fn show_status_middle(world: &mut World, window: Entity, fragment: &str) -> bool {
    let Some(mut entity) = world.query::<(Entity, &Text, &InheritedVisibility, &ComputedNode)>().iter(world)
        .find_map(|(entity, text, visible, node)| (visible.get() && !node.is_empty() && text.contains(fragment)).then_some(entity))
        else { return false; };
    loop {
        // Bevy 的普通 Node 也自带 ScrollPosition，只有真实纵向滚动正文可作为裁剪边界。
        if world.get::<Node>(entity).is_some_and(|node| node.overflow.y == OverflowAxis::Scroll)
            && let Some((scroll, node)) = world.get::<ScrollPosition>(entity).zip(world.get::<ComputedNode>(entity))
        {
            let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            let delta = scroll.y - maximum / 4.0;
            if delta.abs() < 0.5 { return true; }
            if move_to(world, window, entity)
                && world.get::<RelativeCursorPosition>(entity).is_some_and(RelativeCursorPosition::cursor_over)
            {
                world.write_message(MouseWheel {
                    unit: MouseScrollUnit::Pixel, x: 0., y: delta.clamp(-120., 120.), window, phase: TouchPhase::Moved,
                });
            }
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else { return false; };
        entity = parent.parent();
    }
}

/// 核对正文裁剪区域内的真实字形。滚动只发官方输入，由原正文消费。
fn settings_in_view(world: &mut World, window: Entity, labels: &[&str]) -> bool {
    let Some(body) = world.query_filtered::<Entity, With<WorkbenchSettingsBody>>().iter(world)
        .find(|entity| bounds(world, *entity).is_some()) else { return false; };
    let Some(clip) = bounds(world, body) else { return false; };
    let mut group = None::<Rect>;
    for label in labels {
        let nodes: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
            .filter_map(|(entity, text)| (text.as_str() == *label).then_some(entity)).collect();
        let found = nodes.into_iter().filter(|entity| below(world, *entity, body)).filter_map(|entity| {
            let rect = bounds(world, entity)?;
            let center = rect.center();
            if *label == "m" || *label == "×" {
                // 单位可能在多个字段重复，只接受同一组中已经找到的字段。
                if group.is_none_or(|group| center.y < group.min.y || center.y > group.max.y) {
                    return None;
                }
            }
            Some(rect)
        }).next();
        let Some(rect) = found else { return false; };
        group = Some(group.map_or(rect, |group| Rect { min: group.min.min(rect.min), max: group.max.max(rect.max) }));
    }
    let Some(group) = group else { return false; };
    let margin = 20.;
    let delta = if group.max.y > clip.max.y - margin {
        clip.max.y - margin - group.max.y
    } else if group.min.y < clip.min.y + margin {
        clip.min.y + margin - group.min.y
    } else {
        return labels.iter().all(|label| visible_text(world, label));
    };
    let pointer = Vec2::new(clip.max.x - margin, clip.center().y);
    world.get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(pointer.as_dvec2()));
    if world.get::<RelativeCursorPosition>(body).is_some_and(RelativeCursorPosition::cursor_over) {
        world.write_message(MouseWheel {
            unit: MouseScrollUnit::Pixel, x: 0., y: delta.clamp(-120., 120.), window, phase: TouchPhase::Moved,
        });
    }
    false
}

fn click_help(world: &mut World, run: &mut Run, window: Entity, heading: &str) -> bool {
    let mut query = world.query::<(Entity, &Text)>();
    let headings: Vec<_> = query.iter(world)
        .filter_map(|(entity, text)| (text.as_str() == heading).then(|| bounds(world, entity)).flatten()).collect();
    let questions: Vec<_> = query.iter(world)
        .filter_map(|(entity, text)| (text.as_str() == "?").then_some(entity)).collect();
    let candidate = questions.into_iter().filter_map(|mut entity| {
        for _ in 0..5 {
            if world.get::<Button>(entity).is_some() {
                let rect = bounds(world, entity)?;
                let score = headings.iter().filter(|heading| {
                    (heading.center().y - rect.center().y).abs() <= rect.height() * 0.6
                        && heading.min.x < rect.center().x
                }).map(|heading| rect.center().distance_squared(heading.center()))
                    .min_by(f32::total_cmp)?;
                return Some((entity, score));
            }
            entity = world.get::<ChildOf>(entity)?.parent();
        }
        None
    }).min_by(|(_, first), (_, second)| first.total_cmp(second)).map(|(entity, _)| entity);
    click_entity(world, run, window, candidate, MouseButton::Left)
}

fn bounds(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let at = world.get::<UiGlobalTransform>(entity)?;
    (world.get::<InheritedVisibility>(entity)?.get() && !node.is_empty())
        .then(|| Rect::from_center_size(at.translation, node.size()))
}

fn below(world: &World, mut entity: Entity, ancestor: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == ancestor { return true; }
    }
    false
}
