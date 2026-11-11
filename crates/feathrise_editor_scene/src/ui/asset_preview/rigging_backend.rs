//! 人形绑骨后台的检查、生成、再验证与发布。调用方只在单个 IO 工作槽运行这些入口。

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

#[path = "rigging_blender.rs"]
mod blender;
#[path = "rigging_files.rs"]
mod files;
#[path = "rigging_native.rs"]
mod native;
#[cfg(test)]
#[path = "rigging_backend_tests.rs"]
mod tests;

pub(super) type Landmarks = BTreeMap<String, [f32; 3]>;

const LANDMARKS: [&str; 17] = [
    "head",
    "neck",
    "hips",
    "left_shoulder",
    "left_elbow",
    "left_wrist",
    "left_hip",
    "left_knee",
    "left_ankle",
    "left_toe",
    "right_shoulder",
    "right_elbow",
    "right_wrist",
    "right_hip",
    "right_knee",
    "right_ankle",
    "right_toe",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RigBackend {
    #[default]
    Native,
    Blender,
}

#[derive(Clone)]
pub(super) struct RigInput {
    root: PathBuf,
    source: files::FrozenAsset,
    backend: RigBackend,
}

pub(super) struct AnimationAsset {
    pub path: String,
    pub clips: Vec<String>, // 顺序与 glTF animations 数组一致，包含未命名动作的真实序号。
    pub can_reference: bool,
}

pub(super) struct Inspection {
    pub input: RigInput,
    pub landmarks: Landmarks,
    pub animations: Vec<AnimationAsset>,
    pub warnings: Vec<String>,
}

pub(super) struct PreparedRig {
    input: RigInput,
    reference: files::FrozenAsset,
    stage: files::Stage,
    pub warnings: Vec<String>,
    pub joints: usize,
    pub vertices: usize,
}

pub(super) struct PublishedRig {
    pub path: String,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Request<'a> {
    Inspect {
        input: &'a Path,
        report: &'a Path,
    },
    Bind {
        input: &'a Path,
        reference: &'a Path,
        report: &'a Path,
        landmarks: &'a Landmarks,
        output_blend: &'a Path,
        export_plugin: &'a Path,
    },
}

#[derive(Deserialize)]
struct Detected {
    landmarks: Landmarks,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
struct Bound {
    warnings: Vec<String>,
    joints: usize,
    vertices: usize,
}

pub(super) fn catalog(root: &Path, cancel: &AtomicBool) -> Result<Vec<AnimationAsset>, String> {
    files::catalog(&files::checked_root(root)?, cancel)
}

pub(super) async fn inspect(
    asset_root: &Path,
    model_path: &str,
    backend: RigBackend,
    cancel: Arc<AtomicBool>,
) -> Result<Inspection, String> {
    let root = files::checked_root(asset_root)?;
    let stage = files::Stage::new(&root)?;
    let input = stage.path("model.glb");
    let source = files::snapshot(&root, model_path, &input, &cancel)?;
    let detected = match backend {
        RigBackend::Native => native::inspect(&input, Arc::clone(&cancel)).await?,
        RigBackend::Blender => {
            files::check_input_model(&input)?;
            let report = stage.path("report.json");
            blender::execute(
                &stage,
                &Request::Inspect {
                    input: &input,
                    report: &report,
                },
                &report,
                false,
                &cancel,
            )?
        }
    };
    check_landmarks(&detected.landmarks)?;
    let animations = files::catalog(&root, &cancel)?;
    Ok(Inspection {
        input: RigInput {
            root,
            source,
            backend,
        },
        landmarks: detected.landmarks,
        animations,
        warnings: detected.warnings,
    })
}

pub(super) async fn bind(
    input: &RigInput,
    reference_path: &str,
    landmarks: &Landmarks,
    cancel: Arc<AtomicBool>,
) -> Result<PreparedRig, String> {
    check_landmarks(landmarks)?;
    let stage = files::Stage::new(&input.root)?;
    let model_path = stage.path("model.glb");
    let reference_snapshot = stage.path("reference.glb");
    let output_glb = stage.path("rigged.glb");
    // 关节点校正期间源资产可能被 Blender 重导出。只对当时查看的精确原件套用关节点。
    let source = files::snapshot(&input.root, &input.source.relative, &model_path, &cancel)?;
    if source != input.source {
        return Err("模型已在外部更新，请重新检查关节点后再绑骨".into());
    }
    let reference = files::snapshot(&input.root, reference_path, &reference_snapshot, &cancel)?;
    let reference_doc = files::glb_json(&reference_snapshot)?;
    let (_, reference_joints) = files::mixamo_skin(&reference_doc)
        .ok_or_else(|| "所选 GLB 缺少可用的 Mixamo 人形参考骨架".to_owned())?;
    // 检测时冻结后端，关节点始终交还产生它们的同一套拟合流程。
    let bound = match input.backend {
        RigBackend::Native => {
            native::bind(&model_path, reference_doc, landmarks, &output_glb, cancel).await?
        }
        RigBackend::Blender => {
            let export_plugin = blender::export_plugin()?;
            let report = stage.path("report.json");
            let output_blend = stage.path("rigged.blend");
            blender::execute(
                &stage,
                &Request::Bind {
                    input: &model_path,
                    reference: &reference_snapshot,
                    report: &report,
                    landmarks,
                    output_blend: &output_blend,
                    export_plugin: &export_plugin,
                },
                &report,
                true,
                &cancel,
            )?
        }
    };
    if bound.joints != reference_joints.len() || !(1..=500_000).contains(&bound.vertices) {
        return Err("蒙皮回执的关节或顶点数量不匹配".into());
    }
    files::check_output_model(&output_glb, &reference_joints)?;
    Ok(PreparedRig {
        input: input.clone(),
        reference,
        stage,
        warnings: bound.warnings,
        joints: bound.joints,
        vertices: bound.vertices,
    })
}

pub(super) fn publish(prepared: PreparedRig, cancel: &AtomicBool) -> Result<PublishedRig, String> {
    let root = &prepared.input.root;
    let input = &prepared.input.source;
    // 完整候选由主线程确认会话后才进入发布工作槽。核对两个输入的完整字节，再无覆盖地落盘。
    files::check_frozen(root, input, cancel)?;
    files::check_frozen(root, &prepared.reference, cancel)?;
    let path = files::publish(&prepared.stage.path("rigged.glb"), root, input, cancel)?;
    Ok(PublishedRig {
        path,
        warnings: prepared.warnings,
    })
}

fn check_landmarks(landmarks: &Landmarks) -> Result<(), String> {
    if landmarks.len() != LANDMARKS.len()
        || LANDMARKS.iter().any(|key| {
            landmarks
                .get(*key)
                .is_none_or(|point| point.iter().any(|value| !value.is_finite()))
        })
    {
        return Err("人形关节点缺失或坐标无效，请重新检查模型".into());
    }
    Ok(())
}
