//! 自有工程专业作者验收的串行编排；只调用各域原动作，不另写工具或保存算法。

mod animation_assets;
mod resources;
mod river_building;
mod terrain;
mod types;

use bevy::prelude::*;
use resources::Workflow;
pub use types::AuthorWorkflowProof;
use types::Phase;

/// 正常作者会话没有资源和帧系统；三个显式开关缺一不可。
pub(super) fn install(app: &mut App) {
    if !enabled() {
        return;
    }
    terrain::install(app);
    river_building::install(app);
    animation_assets::install(app);
    app.init_resource::<Workflow>();
}

pub(super) fn enabled() -> bool {
    std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS").as_deref() == Ok("1")
        && std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_some()
        && std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() == Ok("1")
}

pub(super) fn begin(world: &mut World) -> Result<bool, String> {
    if !world.contains_resource::<Workflow>() {
        return Ok(false);
    }
    if world.resource::<Workflow>().phase != Phase::Idle {
        return Err("专业作者验收不能重复执行".into());
    }
    terrain::begin(world)?;
    eprintln!("原作者验收开始：雕刻、地表、Blade；最终保存仍由父原Save负责");
    world.resource_mut::<Workflow>().phase = Phase::Terrain;
    Ok(true)
}

/// 每次只推进一个域；原场景保存统一留到所有域正常返回场景页之后。
pub(super) fn advance(world: &mut World) -> Option<AuthorWorkflowProof> {
    if !world.contains_resource::<Workflow>() {
        return None;
    }
    let phase = world.resource::<Workflow>().phase;
    let result = match phase {
        Phase::Terrain => terrain::advance(world),
        Phase::RiverBuilding => river_building::advance(world),
        Phase::AnimationAssets => animation_assets::advance(world),
        _ => None,
    };
    if let Some(result) = result {
        let transition = match result {
            Err(error) => Err(error),
            Ok(report) => {
                eprintln!("原作者验收阶段完成：{phase:?}");
                match phase {
                    Phase::Terrain => world.resource_mut::<Workflow>().proof.terrain = Some(report),
                    Phase::RiverBuilding => {
                        world.resource_mut::<Workflow>().proof.river_building = Some(report)
                    }
                    Phase::AnimationAssets => {
                        world.resource_mut::<Workflow>().proof.animation_assets = Some(report)
                    }
                    _ => unreachable!(),
                }
                match phase {
                    Phase::Terrain => river_building::begin(world).map(|()| Phase::RiverBuilding),
                    Phase::RiverBuilding => {
                        animation_assets::begin(world).map(|()| Phase::AnimationAssets)
                    }
                    Phase::AnimationAssets => Ok(Phase::Complete),
                    _ => unreachable!(),
                }
            }
        };
        let mut workflow = world.resource_mut::<Workflow>();
        match transition {
            Ok(next) => {
                workflow.phase = next;
                workflow.proof.complete = next == Phase::Complete;
            }
            Err(error) => {
                workflow.phase = Phase::Failed;
                workflow.proof.error = Some(error);
            }
        }
    }
    Some(world.resource::<Workflow>().proof.clone())
}
