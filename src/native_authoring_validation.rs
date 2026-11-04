//! 显式有限验收：通过普通地表动作、官方文本与实际后台保存留下可重开复核的证据。

#[path = "native_authoring_validation_domains.rs"]
mod domains;
mod resources;
mod types;

#[cfg(test)]
pub(crate) use domains::dirty_surface_fixture;
pub(crate) use resources::NativeAuthoringValidation;
use types::Phase;
pub(crate) use types::{AuthoringReport, PresetProof};
pub(super) use types::{Domain, Pulse};

use bevy::prelude::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) fn install(app: &mut App, project_root: PathBuf, asset_root: PathBuf) {
    let edit = std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHORING").as_deref() == Ok("1");
    let marker = project_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    app.insert_resource(NativeAuthoringValidation {
        report: AuthoringReport {
            schema: 2,
            scope: "ordinary_surface_text_edit_save_close_reopen",
            mode: if edit {
                "ordinary_surface_edit_save_close"
            } else {
                "ordinary_surface_catalog_reopen_observation"
            },
            project_root,
            asset_root,
            complete: false,
            error: None,
            presets: Vec::new(),
        },
        edit,
        marker,
        phase: Phase::Waiting,
        index: 0,
        tab: None,
        pulse: None,
        started: Instant::now(),
        phase_frames: 0,
    });
}

impl NativeAuthoringValidation {
    pub(crate) fn report(&self) -> &AuthoringReport {
        &self.report
    }
    fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_frames = 0;
    }
    fn fail(&mut self, reason: impl Into<String>) {
        self.report.error = Some(reason.into());
        self.advance(Phase::Failed);
    }
}

/// 与原地图夹具串行：保存交接后才编辑普通地表，全部IO仍使用生产后台事务。
pub(crate) fn drive(world: &mut World, start: bool) {
    world.resource_scope(|world, mut fixture: Mut<NativeAuthoringValidation>| {
        if let Some(pulse) = fixture.pulse.take() {
            if pulse.synthetic {
                let _ = world.despawn(pulse.entity);
            } else if let Ok(mut button) = world.get_entity_mut(pulse.entity) {
                button.insert((
                    Interaction::None,
                    feathrise_editor_ui::EditorButtonClick::default(),
                ));
            }
        }
        if matches!(fixture.phase, Phase::Complete | Phase::Failed) {
            return;
        }
        if fixture.phase == Phase::Waiting {
            if !start {
                return;
            }
            // 只有首次进入依赖地图就绪；预设页按原行为暂停地图相机，后续不能以该相机门挂住自己。
            fixture.started = Instant::now();
            let mut presets = Vec::with_capacity(Domain::ALL.len());
            for domain in Domain::ALL {
                match domains::catalog(world, domain) {
                    Ok(Some(fact)) => presets.push(PresetProof {
                        domain,
                        stable_id: fact.id,
                        name_before: fact.name.clone(),
                        name_after: fact.name,
                        expected_name: fixture
                            .edit
                            .then(|| format!("作者验收·{}·{}", fixture.marker, domain.label())),
                        catalog_hash_before: fact.hash.clone(),
                        catalog_hash_after: fact.hash,
                        author_revision_before: 0,
                        author_revision_after: 0,
                        original_text_field_used: false,
                        original_save_button_used: false,
                        write_confirmed: false,
                        tab_closed: false,
                    }),
                    Ok(None) => return,
                    Err(error) => {
                        fixture.fail(error);
                        return;
                    }
                }
            }
            fixture.report.presets = presets;
            if fixture.edit {
                fixture.advance(Phase::Select);
            } else {
                fixture.report.complete = true;
                fixture.advance(Phase::Complete);
            }
            return;
        }
        fixture.phase_frames = fixture.phase_frames.saturating_add(1);
        if fixture.started.elapsed() > Duration::from_secs(180) {
            let domain = Domain::ALL[fixture.index];
            let phase = fixture.phase;
            fixture.fail(format!(
                "原{}作者验收超时，阶段{phase:?}：{}",
                domain.label(),
                domains::status(world, domain)
            ));
            return;
        }
        let domain = Domain::ALL[fixture.index];
        let id = fixture.report.presets[fixture.index].stable_id.clone();
        let expected = fixture.report.presets[fixture.index]
            .expected_name
            .clone()
            .expect("编辑模式冻结预期名称");
        let step = (|| -> Result<(), String> {
            match fixture.phase {
                Phase::Select => {
                    if domains::input_blocked(world) {
                        return Ok(());
                    }
                    fixture.pulse = Some(domains::select(world, domain, &id)?);
                    fixture.advance(Phase::Open);
                }
                Phase::Open => {
                    if domains::input_blocked(world) {
                        return Ok(());
                    }
                    fixture.pulse = Some(domains::open(world, domain, &id)?);
                    fixture.advance(Phase::WaitOpen);
                }
                Phase::WaitOpen => {
                    if let Some(tab) = domains::tab(world, domain, &id) {
                        fixture.tab = Some(tab.id);
                        let index = fixture.index;
                        fixture.report.presets[index].author_revision_before = tab.revision;
                        fixture.advance(Phase::Text);
                    }
                }
                Phase::Text => {
                    if !domains::name_active(world) {
                        fixture.pulse = domains::focus_name(world);
                        return Ok(());
                    }
                    if domains::edit_name(world, domain, fixture.tab.unwrap(), &expected) {
                        let index = fixture.index;
                        fixture.report.presets[index].original_text_field_used = true;
                        fixture.advance(Phase::WaitBuffer);
                    }
                }
                Phase::WaitBuffer => {
                    if domains::buffer_matches(world, domain, fixture.tab.unwrap(), &expected) {
                        domains::submit_name(world)?;
                        fixture.advance(Phase::WaitCommit);
                    }
                }
                Phase::WaitCommit => {
                    let tab = domains::tab(world, domain, &id).ok_or("原预设草稿意外关闭")?;
                    let index = fixture.index;
                    if tab.name == expected
                        && tab.dirty
                        && tab.revision != fixture.report.presets[index].author_revision_before
                    {
                        fixture.report.presets[index].author_revision_after = tab.revision;
                        fixture.advance(Phase::Save);
                    }
                }
                Phase::Save => {
                    if !domains::input_blocked(world)
                        && let Some(pulse) = domains::save(world, domain)
                    {
                        let index = fixture.index;
                        fixture.report.presets[index].original_save_button_used = true;
                        fixture.pulse = Some(pulse);
                        fixture.advance(Phase::WaitSave);
                    }
                }
                Phase::WaitSave => {
                    let tab =
                        domains::tab(world, domain, &id).ok_or("保存中的原预设草稿意外关闭")?;
                    let fact = domains::catalog(world, domain)?.ok_or("保存期间原目录尚未就绪")?;
                    let index = fixture.index;
                    if save_confirmed(&fixture.report.presets[index], &tab, &fact, &expected) {
                        let proof = &mut fixture.report.presets[index];
                        proof.name_after = fact.name;
                        proof.catalog_hash_after = fact.hash;
                        proof.write_confirmed = true;
                        fixture.advance(Phase::Close);
                    } else if !tab.writing && tab.dirty && fixture.phase_frames > 3 {
                        return Err(format!(
                            "原{}保存未完成：{}",
                            domain.label(),
                            domains::status(world, domain)
                        ));
                    }
                }
                Phase::Close => {
                    if domains::input_blocked(world) {
                        return Ok(());
                    }
                    fixture.pulse = Some(domains::close(world, domain, fixture.tab.unwrap()));
                    fixture.advance(Phase::WaitClose);
                }
                Phase::WaitClose => {
                    if domains::tab(world, domain, &id).is_none() {
                        let index = fixture.index;
                        fixture.report.presets[index].tab_closed = true;
                        fixture.index += 1;
                        fixture.tab = None;
                        if fixture.index == Domain::ALL.len() {
                            fixture.report.complete = true;
                            fixture.advance(Phase::Complete);
                        } else {
                            fixture.advance(Phase::Select);
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = step {
            fixture.fail(error);
        }
    });
}

/// 原后台成功回执、当前稳定ID、正式目录新摘要必须同时成立，不能把内存名字变了算作保存。
fn save_confirmed(
    proof: &PresetProof,
    tab: &domains::TabFact,
    fact: &domains::CatalogFact,
    expected: &str,
) -> bool {
    !tab.dirty
        && !tab.writing
        && tab.name == expected
        && fact.name == expected
        && fact.id == proof.stable_id
        && fact.hash != proof.catalog_hash_before
        && tab.hash == fact.hash
        // 地表实际UI修订在保存采用时也会增长；确认完整内容与新基线，不伪造领域修订。
        && tab.revision >= proof.author_revision_after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_proof_rejects_unsaved_stale_and_different_identity() {
        let proof = PresetProof {
            domain: Domain::Surface,
            stable_id: "item".into(),
            name_before: "旧".into(),
            name_after: "新".into(),
            expected_name: Some("新".into()),
            catalog_hash_before: "old".into(),
            catalog_hash_after: "old".into(),
            author_revision_before: 1,
            author_revision_after: 2,
            original_text_field_used: true,
            original_save_button_used: true,
            write_confirmed: false,
            tab_closed: false,
        };
        let mut tab = domains::TabFact {
            id: 1,
            name: "新".into(),
            hash: "new".into(),
            revision: 2,
            dirty: false,
            writing: false,
        };
        let mut fact = domains::CatalogFact {
            id: "item".into(),
            name: "新".into(),
            hash: "new".into(),
        };
        assert!(save_confirmed(&proof, &tab, &fact, "新"));
        tab.writing = true;
        assert!(!save_confirmed(&proof, &tab, &fact, "新"));
        tab.writing = false;
        tab.dirty = true;
        assert!(!save_confirmed(&proof, &tab, &fact, "新"));
        tab.dirty = false;
        fact.id = "other".into();
        assert!(!save_confirmed(&proof, &tab, &fact, "新"));
        fact.id = "item".into();
        fact.hash = "old".into();
        tab.hash = "old".into();
        assert!(!save_confirmed(&proof, &tab, &fact, "新"));
    }
}
