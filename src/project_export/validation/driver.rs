//! 通过真实可见菜单和导出按钮执行一次后台构建/组包；不直接设置导出回执或成功状态。

use feathrise_editor_ui::InteractionButton;
use super::{Plan, Phase, evidence, resources::Scenario};
use super::super::{components::{ExportAction, ExportProfile}, resources::ExportState};
use bevy::{prelude::*, tasks::{IoTaskPool, futures::check_ready}, window::WindowCloseRequested};
use feathrise_editor_map::standalone::{self, embedded_validation as author};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(button) = plan.pulse.take() { author::release_pulse(world, button); }
        plan.frames = plan.frames.saturating_add(1);
        if plan.phase == Phase::Finished { return; }
        if plan.phase == Phase::Report {
            if let Some(result) = plan.writer.as_mut().and_then(check_ready) {
                plan.writer = None;
                if let Err(error) = result {
                    eprintln!("FEATHRISE_EXPORT_VALIDATION_WRITE_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.advance(Phase::Finished);
                    return;
                }
                if let Err(error) = crate::native_project_close::embedded_validation::request(world) {
                    eprintln!("FEATHRISE_EXPORT_VALIDATION_CLOSE_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.advance(Phase::Finished);
                    return;
                }
                plan.advance(Phase::Closing);
            }
            return;
        }
        if plan.phase == Phase::Closing {
            // 干净工程用原“退出编辑器”按钮；存在夹具未保存内容时才使用原放弃选择。
            if let Some(button) = pulse_text(world, "退出编辑器") { plan.pulse = Some(button); }
            else if let Ok(Some(button)) = crate::native_project_close::embedded_validation::discard(world) { plan.pulse = Some(button); }
            if plan.changed.elapsed() > Duration::from_secs(180) {
                eprintln!("FEATHRISE_EXPORT_VALIDATION_CLOSE_TIMEOUT");
                world.write_message(AppExit::error());
                plan.advance(Phase::Finished);
            }
            return;
        }
        let budget = if plan.phase == Phase::Exporting
            || matches!(plan.phase, Phase::Custom(super::custom::Phase::Exporting | super::custom::Phase::Retrying)) { 1500 } else { 180 };
        let result = if plan.changed.elapsed() > Duration::from_secs(budget) {
            Err(format!("工程导出验收{:?}超过{budget}秒；{}", plan.phase, world.resource::<ExportState>().status))
        } else { advance(world, &mut plan) };
        if let Err(error) = result {
            eprintln!("FEATHRISE_EXPORT_VALIDATION_FAILED {error}");
            let failed_phase = format!("{:?}",plan.phase);
            plan.proofs.push(json!({"stage":"actual_ui_failure_snapshot","phase":failed_phase,"ui":diagnostic(world)}));
            plan.error = Some(error);
            // 错误报告同样独占落盘并走真实关闭事务；不得把失败轮覆盖为成功。
            report(&mut plan);
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if let Phase::Custom(phase) = plan.phase { return super::custom::advance(world, plan, phase); }
    match plan.phase {
        Phase::OpenMap if plan.frames > 25 => {
            world.write_message(crate::viewport_mode::RequestViewportMode(crate::viewport_mode::ViewportMode::SavedWorld));
            standalone::request_open_map(world, "M1-SAMPLE")?;
            plan.advance(Phase::WaitMap);
        }
        Phase::WaitMap if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            plan.baseline = Some(serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?);
            author::request_cuboid(world, [7.0, 1.0, -2.0])?;
            plan.advance(Phase::WaitDraft);
        }
        Phase::WaitDraft => {
            let probe = author::probe(world)?;
            if probe.dirty && probe.last_added_id.is_some() && probe.undo_len > 0 {
                let saved = plan.baseline.as_ref().ok_or("创建前作者基线缺失")?;
                if saved["save_sequence"] != probe.save_sequence { return Err("准备未保存夹具意外触发保存".into()); }
                let author = serde_json::to_value(probe).map_err(|error|error.to_string())?;
                plan.proofs.push(json!({"stage":"unsaved_author_draft_prepared","saved_author":saved,"author":author}));
                plan.baseline = Some(author);
                plan.advance(Phase::OpenMenu);
            }
        }
        Phase::OpenMenu => {
            if let Some(button) = pulse_project_toggle(world)? {
                plan.pulse = Some(button);
                plan.proofs.push(json!({"stage":"original_project_menu_pressed","entity":button.to_bits(),"production_marker":"ChromeSlot::Project"}));
                plan.advance(Phase::OpenExport);
            }
        }
        Phase::OpenExport => {
            let opened = world.resource::<crate::native_project_menu::NativeProjectMenuState>().is_open();
            if !opened && plan.frames > 20 { return Err("已点击原工程菜单，但生产菜单状态没有打开".into()); }
            if opened && let Some(button) = pulse_text(world, "导出工程…") {
                plan.pulse = Some(button);
                plan.proofs.push(json!({"stage":"original_export_menu_pressed","entity":button.to_bits()}));
                plan.advance(Phase::CaptureDialog);
            }
        }
        Phase::CaptureDialog if plan.frames > 4 && world.resource::<ExportState>().open => {
            let expected = crate::project::display_path(&plan.project.root.join("output"));
            if world.resource::<ExportState>().destination != expected {
                return Err("导出面板未将当前工程output设置为默认目录".into());
            }
            let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
                .map_or(feathrise_editor_core::ProjectBuildProfile::Debug, |launch| launch.profile);
            let expected_profile = format!("构建配置：{}（沿用顶栏选择）", profile.label());
            let profiles: Vec<_> = world.query_filtered::<(Entity, &Text, &ComputedNode), With<ExportProfile>>().iter(world)
                .filter(|(entity,text,node)|text.as_str() == expected_profile && node.size().min_element() > 2.
                    && visible_node(world,*entity)).map(|(entity,text,node)|json!({"entity":entity.to_bits(),
                        "text":text.as_str(),"computed_size":node.size().to_array()})).collect();
            // 只观察原正文实际布局；文字为空或被压成零宽时不采图冒充配置已经呈现。
            if profiles.len() != 1 { return Ok(()); }
            if let Some(capture) = evidence::screenshot(world, plan, "export-dialog.png")? {
                plan.proofs.push(json!({"stage":"default_output_and_actual_dialog_gpu","destination":expected,
                    "profile":profiles[0],"capture":capture}));
                plan.advance(Phase::Start);
            }
        }
        Phase::Start => {
            if let Some(button) = pulse_export(world, ExportAction::Start) {
                plan.pulse = Some(button);
                plan.proofs.push(json!({"stage":"original_start_export_pressed","entity":button.to_bits()}));
                plan.advance(Phase::Exporting);
            }
        }
        Phase::Exporting => {
            let state = world.resource::<ExportState>();
            if let Some(error) = &state.failed { return Err(format!("真实工程导出失败：{error}")); }
            if state.busy() { return Ok(()); }
            let Some(receipt) = state.receipt.clone() else { return Ok(()); };
            if receipt.directory != plan.project.root.join("output") {
                return Err("导出完成回执未指向本轮工程的默认output目录".into());
            }
            let current = serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
            if Some(&current) != plan.baseline.as_ref() {
                return Err("工程导出修改了作者草稿、撤销历史或保存修订".into());
            }
            plan.proofs.push(json!({"stage":"real_worker_completed_and_author_preserved","receipt":receipt,"author":current}));
            plan.receipt = Some(receipt);
            plan.advance(Phase::CaptureSuccess);
        }
        Phase::CaptureSuccess if plan.frames > 4 => {
            if let Some(capture) = evidence::screenshot(world, plan, "export-completed.png")? {
                plan.proofs.push(json!({"stage":"actual_success_dialog_gpu","capture":capture}));
                plan.advance(if plan.scenario == Scenario::CustomCancelRetry {
                    Phase::Custom(super::custom::Phase::FillDestination)
                } else { Phase::CloseDialog });
            }
        }
        Phase::CloseDialog => {
            if world.resource::<ExportState>().open {
                if let Some(button) = pulse_export(world, ExportAction::Close) { plan.pulse = Some(button); }
            } else { report(plan); }
        }
        _ => (),
    }
    Ok(())
}

fn report(plan: &mut Plan) {
    let complete = plan.scenario == Scenario::Portable || plan.custom.as_ref().is_some_and(|custom|custom.completed);
    let report: Value = json!({"schema":1,"scope":"owned-project-folder-export-editor",
        "passed":plan.error.is_none() && plan.receipt.is_some() && complete,"error":plan.error,
        "project_id":plan.project.id,"project_root":plan.project.root,"receipt":plan.receipt,"stages":plan.proofs,
        "scenario":plan.scenario.label(),
        "os_input_verified":false,"ordinary_build_installs_driver":false});
    let path = plan.report.clone();
    plan.writer = Some(IoTaskPool::get().spawn(async move {
        super::write_new(&path, &serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)
    }));
    plan.advance(Phase::Report);
}

pub(super) fn pulse_export(world: &mut World, action: ExportAction) -> Option<Entity> {
    let button = world.query::<(Entity, &ExportAction)>().iter(world)
        .find_map(|(entity, found)| (*found == action && visible_button(world, entity)).then_some(entity))?;
    Some(pulse(world, button))
}

pub(super) fn pulse_text(world: &mut World, label: &str) -> Option<Entity> {
    let texts: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter_map(|(entity, text)| (text.0 == label).then_some(entity)).collect();
    for entity in texts {
        if let Some(button) = ancestor_button(world, entity).filter(|entity|visible_button(world, *entity)) {
            return Some(pulse(world, button));
        }
    }
    None
}

fn visible_button(world: &World, entity: Entity) -> bool {
    if world.get::<bevy::ui::InteractionDisabled>(entity).is_some()
        || world.get::<ComputedNode>(entity).is_none_or(|node| node.size().min_element() <= 2.) { return false; }
    visible_node(world, entity)
}

pub(super) fn visible_node(world: &World, entity: Entity) -> bool {
    let mut cursor = Some(entity);
    // 与生产按钮输入相同：隐藏父节点也使目标不可点，不能信任旧ComputedNode残留的尺寸。
    for _ in 0..128 {
        let Some(entity) = cursor else { return true; };
        if world.get::<Node>(entity).is_some_and(|node|node.display == Display::None)
            || world.get::<InheritedVisibility>(entity).is_some_and(|visible|!visible.get()) { return false; }
        cursor = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    false
}

/// 只读记录生产关闭请求、AppExit及进度改变；不阻挡、清除或代发任何退出事件。
pub(super) fn observe_lifecycle(
    plan: Res<Plan>, state: Res<ExportState>,
    mut closes: MessageReader<WindowCloseRequested>, mut exits: MessageReader<AppExit>,
    mut status: Local<String>,
) {
    let mut events = Vec::new();
    for close in closes.read().take(8) {
        events.push(json!({"kind":"WindowCloseRequested","window":close.window.to_bits(),"phase":format!("{:?}",plan.phase)}));
    }
    for exit in exits.read().take(8) {
        events.push(json!({"kind":"AppExit","value":format!("{exit:?}"),"phase":format!("{:?}",plan.phase)}));
    }
    let changed = *status != state.status;
    if changed { *status = state.status.clone(); println!("FEATHRISE_EXPORT_VALIDATION_PROGRESS {}", state.status); }
    if !events.is_empty() { eprintln!("FEATHRISE_EXPORT_VALIDATION_LIFECYCLE {}", json!(events)); }
    if (changed || !events.is_empty()) && let Ok(mut proof) = plan.completion.state.try_lock() {
        proof["status"] = json!(state.status);
        if let Some(recorded) = proof["events"].as_array_mut() {
            for event in events { if recorded.len() < 16 { recorded.push(event); } }
        }
    }
}

fn ancestor_button(world: &World, mut entity: Entity) -> Option<Entity> {
    for _ in 0..16 {
        if world.get::<InteractionButton>(entity).is_some() { return Some(entity); }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
    None
}

pub(super) fn pulse_project_toggle(world: &mut World) -> Result<Option<Entity>, String> {
    let candidates: Vec<_> = world.query::<(Entity, &crate::window_chrome::ChromeSlot)>().iter(world)
        .filter_map(|(entity, slot)| (*slot == crate::window_chrome::ChromeSlot::Project
            && world.get::<InteractionButton>(entity).is_some() && visible_button(world, entity)).then_some(entity)).collect();
    if candidates.len() > 1 { return Err("同一作者窗口出现多个可见原工程菜单，拒绝按文字猜测目标".into()); }
    Ok(candidates.first().copied().map(|button|pulse(world, button)))
}

/// 失败只记录有界的真实组件/布局，不改菜单状态；保留隐藏候选便于识别同文案或旧布局误选。
fn diagnostic(world: &mut World) -> Value {
    let labels: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter(|(_,text)|matches!(text.as_str(), "工程" | "导出工程…" | "退出编辑器"))
        .take(24).map(|(entity,text)|(entity,text.0.clone())).collect();
    let buttons: Vec<_> = labels.into_iter().map(|(text,label)| {
        let button = ancestor_button(world,text);
        json!({"label":label,"text_entity":text.to_bits(),"button":button.map(Entity::to_bits),
            "visible":button.is_some_and(|button|visible_button(world,button)),
            "chrome_slot":button.and_then(|button|world.get::<crate::window_chrome::ChromeSlot>(button)).map(|slot|format!("{slot:?}")),
            "computed_size":button.and_then(|button|world.get::<ComputedNode>(button)).map(|node|node.size().to_array())})
    }).collect();
    json!({"native_project_menu_open":world.get_resource::<crate::native_project_menu::NativeProjectMenuState>().map(|menu|menu.is_open()),
        "export_open":world.resource::<ExportState>().open,"buttons":buttons})
}

fn pulse(world: &mut World, entity: Entity) -> Entity {
    world.entity_mut(entity).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    entity
}
