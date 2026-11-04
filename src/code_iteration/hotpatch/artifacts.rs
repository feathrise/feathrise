//! 将薄DLL及跳表发布到本次fat基线的受控目录；文件IO和SHA全部留在原后台构建槽。

use super::{PatchContext, PreparedPatch, toolchain};
use crate::{build_artifacts, build_process::BuildControl, project::Project};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, path::Path, sync::Arc};

pub(crate) fn prepare(project: &Project, target: &Path, context: &PatchContext, generation: u64,
    control: &BuildControl,
    diagnostics: Option<Arc<dyn Fn(crate::compiler_diagnostics::CompilerDiagnostic) + Send + Sync>>,
) -> Result<PreparedPatch, String> {
    control.check()?;
    let baseline = &context.baseline;
    if context.target.project_id != project.id || baseline.base_build != context.target.base_build || generation == 0 {
        return Err("热补丁基线或工程身份不匹配".into());
    }
    build_artifacts::verify(&baseline.bundle, control)?;
    let output = toolchain::patch(project, target, baseline.client_json.clone(), context.target.aslr_reference,
        control, &baseline.features, diagnostics)?;
    let root = baseline.patch_root.canonicalize().map_err(|error| error.to_string())?;
    if !root.starts_with(&baseline.directory) { return Err("热补丁目录不属于当前基线".into()); }
    let name = format!("generation-{generation}-{}", bevy::asset::uuid::Uuid::new_v4());
    let directory = root.join(&name);
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let library = directory.join("patch.dll");
    let original = build_artifacts::hash_file(&output.library, control)?;
    copy(&output.library, &library, control)?;
    copy(&output.pdb, &directory.join("patch.pdb"), control)?;
    if build_artifacts::hash_file(&library, control)? != original
        || build_artifacts::hash_file(&output.library, control)? != original {
        return Err("热补丁DLL在复制期间变化".into());
    }
    let mut table: serde_json::Value = serde_json::from_slice(&output.jump_table_json).map_err(|error| error.to_string())?;
    table["lib"] = serde_json::to_value(&library).map_err(|error| error.to_string())?;
    let target = &context.target;
    let package = serde_json::json!({"schema":1,"target":{
        "project_id":target.project_id,"session":target.session,"pid":target.pid,"world_id":target.world_id,
        "base_build":target.base_build,"source_generation":generation,"patch_id":generation},
        "jump_table":table,"dll_sha256":original.0});
    let bytes = serde_json::to_vec(&package).map_err(|error| error.to_string())?;
    if bytes.len() > super::compiler_output::MAX_COMPILER_OUTPUT_BYTES { return Err("热补丁包超过8MiB预算".into()); }
    let mut file = OpenOptions::new().create_new(true).write(true).open(directory.join("patch.json")).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    control.check()?;
    Ok(PreparedPatch { target: target.clone(), source_generation:generation,
        artifact:format!("{name}/patch.json"), sha256:build_artifacts::digest(&bytes), baseline:baseline.clone() })
}

fn copy(source: &Path, target: &Path, control: &BuildControl) -> Result<(), String> {
    let mut source = File::open(source).map_err(|error| error.to_string())?;
    let mut target = OpenOptions::new().write(true).create_new(true).open(target).map_err(|error| error.to_string())?;
    let mut buffer = [0; 64 * 1024];
    loop {
        control.check()?;
        let count = source.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        target.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
    }
    target.sync_all().map_err(|error| error.to_string())
}
