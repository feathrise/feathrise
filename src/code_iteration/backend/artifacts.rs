//! 将稳定源码身份、精确工具链和完整运行版本绑定；失败/过期结果只返回错误，不触碰旧运行实例。

use super::{
    resources::Shared,
    source,
    types::{BuildIdentity, BuildRequest, PreparedArtifact, PreparedVersion},
};
use crate::{
    build_process,
    compiler_diagnostics::{CompilerDiagnostic, DIAGNOSTIC_LIMIT},
    project::Project,
};
use crate::code_iteration::hotpatch::PatchContext;
use feathrise_editor_core::ProjectBuildProfile;
use std::{
    io::Read,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

pub(super) fn context(
    project: &Project,
    request: &BuildRequest,
    shared: &Shared,
) -> Result<String, String> {
    let cargo = std::env::var_os("EDITOR_HOST_CARGO")
        .or_else(|| std::env::var_os("CARGO"))
        .unwrap_or_else(|| "cargo".into());
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let mut parts = vec![
        request.source.environment_sha256.clone(),
        request.profile.cargo_profile().into(),
        request.build_backend.key().into(),
        crate::native_play::build_feature_identity(),
    ];
    for (program, args) in [
        (cargo, vec!["--version", "--verbose"]),
        (rustc, vec!["--version", "--verbose"]),
    ] {
        let mut command = Command::new(program);
        command.args(args).current_dir(&project.root);
        let output = build_process::run(
            &command,
            &shared.control,
            Some(Duration::from_secs(30)),
            |reader| {
                let mut bytes = Vec::new();
                reader
                    .take(16 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() > 16 * 1024 {
                    return Err("构建工具身份输出超过 16 KiB".into());
                }
                String::from_utf8(bytes).map_err(|error| error.to_string())
            },
        )?;
        if output.exit_code != 0 {
            return Err(format!("构建工具身份探测失败：{}", output.stderr));
        }
        parts.push(output.stdout);
    }
    Ok(crate::build_artifacts::digest(
        &serde_json::to_vec(&parts).map_err(|error| error.to_string())?,
    ))
}

pub(super) fn prepare(
    project: &Project,
    target: &Path,
    request: &BuildRequest,
    identity: &BuildIdentity,
    shared: &Shared,
    patch_context: Option<&PatchContext>,
) -> (
    Result<Arc<PreparedVersion>, String>,
    Vec<CompilerDiagnostic>,
) {
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let writer = diagnostics.clone();
    let callback = Arc::new(move |diagnostic| {
        if let Ok(mut diagnostics) = writer.lock() {
            if diagnostics.len() < DIAGNOSTIC_LIMIT {
                diagnostics.push(diagnostic);
            }
        }
    });
    let result = (|| {
        if !shared.current_build(request.generation, request.profile, request.build_backend, request.backend_revision) {
            return Err("较新的源码已保存，本次旧构建不再采用".into());
        }
        if source::capture(project, target, &shared.control)? != request.source {
            return Err("构建开始前源码已变化，等待最新保存版本".into());
        }
        let artifact = if let Some(context) = patch_context.filter(|context| patch_compatible(project, request, context)) {
            let patch = crate::native_play::prepare_function_patch(
                project, target, context, request.generation, &shared.control, Some(callback),
            )?;
            PreparedArtifact::Patch(Arc::new(patch))
        } else {
            let mut prepared = crate::native_play::prepare_game_with_backend(
                project, target, None, request.profile, request.build_backend, &shared.control, Some(callback),
            )?;
            prepared.bind_build_metadata(serde_json::json!({"build_identity":identity.json(),"source_files":request.source.files,"source_bytes":request.source.bytes}), &shared.control)?;
            PreparedArtifact::Restart(Arc::new(prepared))
        };
        // 全部复制完成后再重扫并核原子代次；源码 ABA 不能只凭前后摘要相同获得采用权。
        if !shared.current_build(request.generation, request.profile, request.build_backend, request.backend_revision)
            || source::capture(project, target, &shared.control)? != request.source
            || !shared.current_build(request.generation, request.profile, request.build_backend, request.backend_revision)
        {
            return Err("构建期间已有较新的源码保存，本次产物已过期".into());
        }
        if !shared.current_build(request.generation, request.profile, request.build_backend, request.backend_revision) {
            return Err("运行版本准备期间已有新保存，拒绝旧产物".into());
        }
        Ok(Arc::new(PreparedVersion {
            identity: identity.clone(),
            artifact,
        }))
    })();
    let diagnostics = diagnostics
        .lock()
        .map(|diagnostics| diagnostics.clone())
        .unwrap_or_default();
    (result, diagnostics)
}

fn patch_compatible(project: &Project, request: &BuildRequest, context: &PatchContext) -> bool {
    request.build_backend == crate::native_play::ProjectBuildBackend::Dx
        && request.profile == ProjectBuildProfile::Debug
        && context.target.project_id == project.id
        && context.target.base_build == context.baseline.base_build
        && inputs_compatible(&request.source, &context.baseline.source)
}

fn inputs_compatible(current: &source::SourceSnapshot, baseline: &source::SourceSnapshot) -> bool {
    current.hotpatch_shape.as_ref().is_some_and(|shape| baseline.hotpatch_shape.as_ref() == Some(shape))
        && current.environment_sha256 == baseline.environment_sha256
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn only_body_hash_changes_keep_patch_eligibility_unknown_structure_or_environment_restarts() {
        let baseline = source::SourceSnapshot { sha256: "body-one".into(), environment_sha256: "same-environment".into(),
            hotpatch_shape: Some("same-complete-shape".into()), files: 3, bytes: 32, watches: BTreeSet::new(), source_watches: BTreeSet::new() };
        let mut current = baseline.clone();
        current.sha256 = "body-two".into();
        current.bytes += 8;
        assert!(inputs_compatible(&current, &baseline));
        current.hotpatch_shape = Some("signature-dependency-or-other-source-changed".into());
        assert!(!inputs_compatible(&current, &baseline));
        current.hotpatch_shape = None;
        assert!(!inputs_compatible(&current, &baseline));
        current = baseline.clone();
        current.environment_sha256 = "changed-environment".into();
        assert!(!inputs_compatible(&current, &baseline));
        current = baseline.clone();
        current.hotpatch_shape = None;
        assert!(!inputs_compatible(&current, &current));
    }
}
