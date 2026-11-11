//! 自选目录、真实任务取消和再次导出的增量窗口验收；完整包取证留在单飞后台。

use super::{Plan, Phase as EditorPhase, driver, evidence, package_evidence as package,
    resources::PackageCheckpoint};
pub(super) use super::resources::{CustomPhase as Phase, CustomState as State};
use super::super::{components::{ExportAction, ExportDestination}, resources::{ExportRuntime, ExportState}, types::ExportReceipt};
use bevy::{input_focus::{FocusCause, InputFocus}, prelude::*, tasks::{IoTaskPool, futures::check_ready}, text::{EditableText, TextEdit}};
use feathrise_editor_scene::standalone::embedded_validation as author;
use serde_json::json;
use std::{path::Path, sync::Arc};

pub(super) const DIRECTORY: &str = "自选导出 中文包";
pub(super) const REQUIRED_STAGES: &[&str] = &[
    "original_custom_destination_edits_queued", "custom_destination_applied_and_actual_dialog_gpu",
    "original_custom_export_start_pressed", "custom_export_completed_and_author_preserved", "actual_created_custom_dialog_gpu",
    "complete_old_package_captured_before_cancel", "original_update_export_start_pressed",
    "original_cancel_export_pressed_while_real_worker_running", "cancelled_worker_really_drained_and_author_preserved",
    "complete_old_package_unchanged_after_cancel", "original_custom_reopen_menu_pressed", "original_custom_reopen_export_pressed",
    "cancelled_dialog_actual_gpu_and_custom_path_retained", "original_retry_export_start_after_drain",
    "retry_export_completed_and_author_preserved", "complete_retry_package_verified_with_new_export_id", "actual_retry_success_dialog_gpu",
];

impl State {
    pub(super) fn new(root: &Path) -> Self {
        Self { destination:root.join(DIRECTORY), io:None, baseline:None, cancel_control:None, started:false, completed:false }
    }
}

pub(super) fn advance(world: &mut World, plan: &mut Plan, phase: Phase) -> Result<(), String> {
    let destination = plan.custom.as_ref().ok_or("附加导出场景缺少冻结状态")?.destination.clone();
    let displayed = crate::project::display_path(&destination);
    match phase {
        Phase::FillDestination => {
            let fields: Vec<_> = world.query_filtered::<Entity, With<ExportDestination>>().iter(world)
                .filter(|entity|driver::visible_node(world,*entity)
                    && world.get::<bevy::ui::InteractionDisabled>(*entity).is_none()).collect();
            if fields.len() != 1 { return Err("原导出路径输入框并非唯一可编辑控件".into()); }
            let entity = fields[0];
            world.resource_mut::<InputFocus>().set(entity, FocusCause::Navigated);
            let mut input = world.get_mut::<EditableText>(entity).ok_or("原路径输入框缺少EditableText")?;
            // 只排入原生文本编辑队列，不能直写ExportState或绕过官方输入处理。
            input.queue_edit(TextEdit::SelectAll);
            input.queue_edit(TextEdit::Insert(displayed.clone().into()));
            plan.proofs.push(json!({"stage":"original_custom_destination_edits_queued","entity":entity.to_bits(),
                "destination":displayed,"input":"original EditableText SelectAll + Insert"}));
            next(plan, Phase::WaitDestination);
        }
        Phase::WaitDestination if plan.frames > 2 => {
            let input_matches = world.query_filtered::<&EditableText, With<ExportDestination>>().iter(world)
                .any(|input|input.value() == displayed.as_str() && !input.is_composing());
            if input_matches && world.resource::<ExportState>().destination == displayed { next(plan, Phase::CaptureDestination); }
        }
        Phase::CaptureDestination => {
            if let Some(capture) = evidence::screenshot(world, plan, "custom-export-dialog.png")? {
                plan.proofs.push(json!({"stage":"custom_destination_applied_and_actual_dialog_gpu","destination":displayed,"capture":capture}));
                next(plan, Phase::Start);
            }
        }
        Phase::Start | Phase::StartUpdate | Phase::StartRetry => {
            if world.resource::<ExportState>().busy() || world.non_send::<ExportRuntime>().active.is_some() { return Ok(()); }
            if let Some(button) = driver::pulse_export(world, ExportAction::Start) {
                plan.pulse = Some(button);
                plan.custom.as_mut().unwrap().started = false;
                let (stage, following) = match phase {
                    Phase::Start => ("original_custom_export_start_pressed", Phase::Exporting),
                    Phase::StartUpdate => ("original_update_export_start_pressed", Phase::WaitWorker),
                    _ => ("original_retry_export_start_after_drain", Phase::Retrying),
                };
                plan.proofs.push(json!({"stage":stage,"entity":button.to_bits(),"directory":destination,"active_slot_before":false}));
                next(plan, following);
            }
        }
        Phase::Exporting | Phase::Retrying => {
            if let Some(receipt) = completed_receipt(world, plan, &destination)? {
                let stage = if phase == Phase::Exporting { "custom_export_completed_and_author_preserved" }
                    else { "retry_export_completed_and_author_preserved" };
                plan.proofs.push(json!({"stage":stage,"receipt":receipt,"author":author::probe(world)?}));
                plan.receipt = Some(receipt);
                next(plan, if phase == Phase::Exporting { Phase::CaptureCreated } else { Phase::VerifyRetry });
            }
        }
        Phase::CaptureCreated if plan.frames > 4 => {
            if let Some(capture) = evidence::screenshot(world, plan, "custom-export-created.png")? {
                plan.proofs.push(json!({"stage":"actual_created_custom_dialog_gpu","capture":capture}));
                next(plan, Phase::BeforeCancel);
            }
        }
        Phase::BeforeCancel => {
            if let Some(checkpoint) = checkpoint(plan, "before-cancel")? {
                plan.proofs.push(json!({"stage":"complete_old_package_captured_before_cancel","package":checkpoint.proof}));
                plan.custom.as_mut().unwrap().baseline = Some(checkpoint);
                next(plan, Phase::StartUpdate);
            }
        }
        Phase::WaitWorker => {
            let state = world.resource::<ExportState>();
            if let Some(error) = &state.failed { return Err(format!("取消前真实任务已失败：{error}")); }
            if !state.busy() && state.receipt.is_some() && plan.frames > 2 {
                return Err("更新任务已经完成，未取得发布前的真实取消窗口；此轮不计通过".into());
            }
            // 此轮只声明任务准备/构建阶段取消；此文案不能证明Cargo已经启动或复制已经发生。
            if !state.busy() || !state.status.starts_with("正在构建") { return Ok(()); }
            let status = state.status.clone();
            let active = world.non_send::<ExportRuntime>().active.as_ref()
                .filter(|active|!active.handle.is_finished() && !active.control.is_cancelled())
                .map(|active|active.control.clone());
            let Some(control) = active else { return Ok(()); };
            if let Some(button) = driver::pulse_export(world, ExportAction::Cancel) {
                plan.pulse = Some(button);
                plan.custom.as_mut().unwrap().cancel_control = Some(control);
                plan.proofs.push(json!({"stage":"original_cancel_export_pressed_while_real_worker_running","entity":button.to_bits(),
                    "status":status,"active_slot":true,"handle_finished":false,"control_cancelled_before":false,
                    "cancellation_scope":"export task preparation/build; Cargo start and copy/publish rollback not asserted"}));
                next(plan, Phase::Cancelling);
            }
        }
        Phase::Cancelling => {
            let state = world.resource::<ExportState>();
            if state.busy() || world.non_send::<ExportRuntime>().active.is_some() { return Ok(()); }
            let control = plan.custom.as_ref().unwrap().cancel_control.as_ref().ok_or("缺少本次活动任务控制牌")?;
            if !control.is_cancelled() || control.cleanup_failed() || state.receipt.is_some() || state.failed.is_some()
                || !state.status.starts_with("导出已取消") || world.non_send::<ExportRuntime>().failure.is_some() {
                return Err(format!("原取消未产生真正收口的无成功回执任务；可能已迟于发布：{}", state.status));
            }
            ensure_author(world, plan)?;
            plan.proofs.push(json!({"stage":"cancelled_worker_really_drained_and_author_preserved","status":state.status,
                "control_cancelled":true,"cleanup_failed":false,"active_slot":false,"working":false,
                "receipt":state.receipt,"failed":state.failed,"author":author::probe(world)?}));
            next(plan, Phase::AfterCancel);
        }
        Phase::AfterCancel => {
            if let Some(checkpoint) = checkpoint(plan, "after-cancel")? {
                plan.proofs.push(json!({"stage":"complete_old_package_unchanged_after_cancel","package":checkpoint.proof,
                    "entire_inventory_equal":true,"export_id_unchanged":true}));
                next(plan, Phase::ReopenMenu);
            }
        }
        Phase::ReopenMenu if plan.frames > 4 && !world.resource::<ExportState>().open => {
            if let Some(button) = driver::pulse_project_toggle(world)? {
                plan.pulse = Some(button);
                plan.proofs.push(json!({"stage":"original_custom_reopen_menu_pressed","entity":button.to_bits()}));
                next(plan, Phase::ReopenExport);
            }
        }
        Phase::ReopenExport => {
            if world.resource::<crate::native_project_menu::NativeProjectMenuState>().is_open()
                && let Some(button) = driver::pulse_text(world, "导出工程…") {
                plan.pulse = Some(button);
                plan.proofs.push(json!({"stage":"original_custom_reopen_export_pressed","entity":button.to_bits()}));
                next(plan, Phase::CaptureCancelled);
            }
        }
        Phase::CaptureCancelled if plan.frames > 4 && world.resource::<ExportState>().open => {
            let state = world.resource::<ExportState>();
            if state.destination != displayed || !state.status.starts_with("导出已取消") || state.receipt.is_some() {
                return Err("原菜单重开没有保留自选目录和真实取消状态".into());
            }
            if let Some(capture) = evidence::screenshot(world, plan, "custom-export-cancelled.png")? {
                plan.proofs.push(json!({"stage":"cancelled_dialog_actual_gpu_and_custom_path_retained","destination":displayed,"capture":capture}));
                next(plan, Phase::StartRetry);
            }
        }
        Phase::VerifyRetry => {
            if let Some(checkpoint) = checkpoint(plan, "after-retry")? {
                let previous = &plan.custom.as_ref().unwrap().baseline.as_ref().ok_or("重试缺少旧包基线")?.marker;
                plan.proofs.push(json!({"stage":"complete_retry_package_verified_with_new_export_id","package":checkpoint.proof,
                    "previous_export_id":previous["export_id"],"new_export_id":checkpoint.marker["export_id"]}));
                next(plan, Phase::CaptureRetry);
            }
        }
        Phase::CaptureRetry if plan.frames > 4 => {
            if let Some(capture) = evidence::screenshot(world, plan, "custom-export-completed.png")? {
                ensure_author(world, plan)?;
                plan.proofs.push(json!({"stage":"actual_retry_success_dialog_gpu","capture":capture}));
                plan.custom.as_mut().unwrap().completed = true;
                plan.advance(EditorPhase::CloseDialog);
            }
        }
        _ => (),
    }
    Ok(())
}

fn next(plan: &mut Plan, phase: Phase) { plan.advance(EditorPhase::Custom(phase)); }

fn ensure_author(world: &World, plan: &Plan) -> Result<(), String> {
    let current = serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
    if Some(&current) != plan.baseline.as_ref() { return Err("自选导出、取消或重试修改了作者草稿、历史或保存修订".into()); }
    Ok(())
}

fn completed_receipt(world: &World, plan: &mut Plan, destination: &Path) -> Result<Option<ExportReceipt>, String> {
    let state = world.resource::<ExportState>();
    if let Some(error) = &state.failed { return Err(format!("真实自选导出失败：{error}")); }
    if state.busy() {
        if world.non_send::<ExportRuntime>().active.is_some() { plan.custom.as_mut().unwrap().started = true; }
        return Ok(None);
    }
    if !plan.custom.as_ref().unwrap().started { return Ok(None); }
    let Some(receipt) = &state.receipt else { return Ok(None); };
    if world.non_send::<ExportRuntime>().active.is_some() || receipt.directory != destination {
        return Err("自选导出回执未绑定当前目录或真实线程尚未归槽".into());
    }
    ensure_author(world, plan)?;
    Ok(Some(receipt.clone()))
}

/// 基线、取消后和重试后各使用同一个单飞槽，完整哈希／清单读取不进入Last系统。
fn checkpoint(plan: &mut Plan, name: &'static str) -> Result<Option<PackageCheckpoint>, String> {
    let custom = plan.custom.as_mut().ok_or("缺少自选包核验槽")?;
    if let Some(task) = custom.io.as_mut() {
        let Some(result) = check_ready(task) else { return Ok(None); };
        custom.io = None;
        return result.map(Some);
    }
    let destination = custom.destination.clone();
    let old = custom.baseline.as_ref().map(|checkpoint|checkpoint.inventory.clone());
    let old_id = custom.baseline.as_ref().map(|checkpoint|checkpoint.marker["export_id"].clone());
    let project = plan.project.clone();
    let path = plan.report.parent().ok_or("本轮证据目录缺失")?.join(format!("custom-{name}-package.json"));
    custom.io = Some(IoTaskPool::get().spawn(async move {
        let (inventory, marker) = package::verify(&destination, &project.id, &project.package)?;
        let id = marker["export_id"].as_str().ok_or("自选包缺少发布身份")?;
        bevy::asset::uuid::Uuid::parse_str(id).map_err(|_|"自选包发布身份不是UUID")?;
        if name == "after-cancel" {
            if old.as_deref() != Some(&inventory) || old_id.as_ref() != Some(&marker["export_id"]) {
                return Err("取消后旧包的完整路径、目录或任一文件字节变化".into());
            }
            for entry in std::fs::read_dir(destination.parent().ok_or("自选目录缺少父目录")?).map_err(|error|error.to_string())? {
                let entry = entry.map_err(|error|error.to_string())?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".feathrise-export-stage-") || name.starts_with(".feathrise-export-backup-") {
                    return Err("取消任务收口后仍遗留本轮暂存或备份目录".into());
                }
            }
        }
        if name == "after-retry" && (old_id.is_none() || old_id.as_ref() == Some(&marker["export_id"])) {
            return Err("再次导出没有新的发布身份，拒绝沿用旧包回执".into());
        }
        let summary = package::summary(&inventory);
        super::write_new(&path, &serde_json::to_vec_pretty(&json!({"schema":1,"checkpoint":name,
            "project_id":project.id,"directory":destination,"marker":marker,"package":summary})).map_err(|error|error.to_string())?)?;
        let proof = json!({"checkpoint":name,"path":path,"file_identity":package::hash(&path)?,
            "export_id":marker["export_id"],"files":inventory.files.len(),"directories":inventory.directories.len(),
            "bytes":inventory.files.values().map(|file|file.bytes).sum::<u64>()});
        Ok(PackageCheckpoint { inventory:Arc::new(inventory), marker, proof })
    }));
    Ok(None)
}
