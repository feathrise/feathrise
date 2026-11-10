//! 中央标签固定期间的独立文件属性。只读取所选文件目录信息，不创建第二个预览。

use super::super::{AssetPreviewState, AssetPreviewWorkspace, DocumentState, gltf_tree::data};
use crate::{
    MapEditorSystemSet,
    resources::{EditorAssetCatalog, MapEditorLayoutState, MapEditorState},
    ui::{
        scene::FONT_PATH,
        sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
        theme as colors,
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    gltf::Gltf,
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready},
    text::FontSourceTemplate,
};
use feathrise_editor_ui::{localization::EditorLocale, metrics};
use serde::{Deserialize, de::IgnoredAny};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

const IMAGE_HEADER_LIMIT: u64 = 512 * 1024;

#[derive(Component, Clone, Default)]
struct DetailText;

#[derive(Clone, PartialEq, Eq)]
struct FileKey {
    root: PathBuf,
    path: String,
    project: Option<u64>,
    refresh: u64,
    model: Option<u64>,
}

impl FileKey {
    fn identity(&self) -> (&Path, &str, Option<u64>, u64, Option<u64>) {
        (&self.root, &self.path, self.project, self.refresh, self.model)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ModelCounts {
    scenes: usize,
    nodes: usize,
    meshes: usize,
    skins: usize,
    clips: usize,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ModelJson {
    scenes: Vec<IgnoredAny>,
    nodes: Vec<IgnoredAny>,
    meshes: Vec<IgnoredAny>,
    skins: Vec<IgnoredAny>,
    animations: Vec<IgnoredAny>,
}

enum FormatDetails {
    Model(ModelCounts),
    Image(u32, u32),
}

enum Header {
    Model(Vec<u8>),
    Image(Vec<u8>, image::ImageFormat),
    Ktx2(Vec<u8>),
    None,
}

struct Details {
    size: u64,
    format: Result<Option<FormatDetails>, String>,
}

type Outcome = Result<Details, String>;

struct Work {
    key: FileKey,
    cancel: Arc<AtomicBool>,
    task: Task<Outcome>,
}

#[derive(Resource, Default)]
struct State {
    visible: bool,
    selected: Option<FileKey>,
    completed: Option<Outcome>,
    work: Option<Work>,
    revision: u64,
}

pub(in crate::ui::asset_preview) fn install(app: &mut App) {
    app.init_resource::<State>()
        .add_systems(Update, update
            .after(super::super::loading::poll_asset_preview_loading)
            .in_set(MapEditorSystemSet::Tasks))
        .add_systems(Update, present.in_set(MapEditorSystemSet::Presentation));
}

pub(super) fn panel() -> impl Scene {
    bsn! {
        DetailText Text("") TextLayout { linebreak: LineBreak::AnyCharacter }
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_VALUE)
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

fn update(
    mut state: ResMut<State>,
    editor: Res<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    layout: Option<Res<MapEditorLayoutState>>,
    documents: Res<DocumentState>,
    media: Res<AssetPreviewWorkspace>,
    workspace: Res<VegetationPresetEditorState>,
    preview: Res<AssetPreviewState>,
    catalog: Res<EditorAssetCatalog>,
    hosted: Option<Res<crate::workbench::WorkbenchModel>>,
) {
    let root = documents.asset_root();
    let wanted = editor.selected_asset.as_deref().filter(|_| {
        editor.open && !editor.close_requested && editor.pending_map_transition.is_none()
            && media.active(workspace.active_tab) && !root.as_os_str().is_empty()
    }).map(|path| {
        let model = if preview.selected_path.as_deref() == Some(path) && preview.external_preview.is_none() {
            // 纯动作的蒙皮可能没有网格引用，加载器不生成对应句柄。其属性按文件头计数。
            preview.model.as_ref().filter(|model| model.scene_mesh_count > 0).map(|model| model.generation)
        } else {
            None
        };
        (root, path, hosted.as_ref().map(|host| host.context), catalog.applied_revision, model)
    });
    state.visible = wanted.is_some() && sidebar.right == EditorRightSidebarTab::Assets
        && layout.is_none_or(|layout| layout.inspector_shown());
    if state.selected.as_ref().map(FileKey::identity) != wanted {
        state.selected = wanted.map(|(root, path, project, refresh, model)| FileKey {
            root: root.to_owned(), path: path.to_owned(), project, refresh, model,
        });
        state.completed = None;
        state.revision = state.revision.wrapping_add(1);
    }
    if let Some(work) = &mut state.work {
        if Some(work.key.identity()) != wanted {
            work.cancel.store(true, Ordering::Release);
        }
        let Some(outcome) = check_ready(&mut work.task) else { return };
        let work = state.work.take().unwrap();
        if !work.cancel.load(Ordering::Acquire) && Some(&work.key) == state.selected.as_ref() {
            state.completed = Some(outcome);
            state.revision = state.revision.wrapping_add(1);
        }
    }
    if state.visible && state.completed.is_none() && let Some(key) = state.selected.clone() {
        let input = key.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        // 仅一个在途槽。切文件先换身份，待旧读取结束才派发当前选择，迟到值不能串入右栏。
        let task = IoTaskPool::get().spawn(async move {
            let (size, header) = read_header(&input, &worker_cancel)?;
            let format = match header {
                Ok(Header::None) => Ok(None),
                Ok(header) => AsyncComputeTaskPool::get().spawn(async move {
                    if worker_cancel.load(Ordering::Acquire) { return Ok(None); }
                    parse_header(header).map(Some)
                }).await,
                Err(error) => Err(error),
            };
            Ok(Details { size, format })
        });
        state.work = Some(Work { key, cancel, task });
    }
}

fn read_header(key: &FileKey, cancel: &AtomicBool) -> Result<(u64, Result<Header, String>), String> {
    if cancel.load(Ordering::Acquire) { return Err("文件属性读取已取消".into()); }
    let mut file = data::open_asset(&key.root, &key.path)?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    let extension = Path::new(&key.path).extension().and_then(|extension| extension.to_str())
        .unwrap_or_default().to_ascii_lowercase();
    let mut read = |limit, convert: fn(Vec<u8>) -> Header| -> Result<Header, String> {
        let mut bytes = Vec::new();
        (&mut file).take(limit).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        Ok(convert(bytes))
    };
    // 当前画布已持有模型目录；只为没有预览句柄的其他模型读取其 JSON，不加载网格和贴图。
    let header = match extension.as_str() {
        "glb" if key.model.is_none() => data::read_json_file(&mut file, cancel).map(Header::Model),
        "gltf" if key.model.is_none() => {
            if before.len() > data::MAX_JSON as u64 {
                Err("glTF JSON 超过 16 MiB 读取预算".into())
            } else {
                read(data::MAX_JSON as u64, Header::Model)
            }
        }
        "png" => read(IMAGE_HEADER_LIMIT, |bytes| Header::Image(bytes, image::ImageFormat::Png)),
        "jpg" | "jpeg" => read(IMAGE_HEADER_LIMIT, |bytes| Header::Image(bytes, image::ImageFormat::Jpeg)),
        "ktx2" => read(80, Header::Ktx2),
        _ => Ok(Header::None),
    };
    let after = file.metadata().map_err(|error| error.to_string())?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err("文件在读取期间发生变化，请刷新目录".into());
    }
    Ok((before.len(), header))
}

fn parse_header(header: Header) -> Result<FormatDetails, String> {
    match header {
        Header::Model(bytes) => {
            let root: ModelJson = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            Ok(FormatDetails::Model(ModelCounts {
                scenes: root.scenes.len(), nodes: root.nodes.len(), meshes: root.meshes.len(),
                skins: root.skins.len(), clips: root.animations.len(),
            }))
        }
        Header::Image(bytes, format) => image::ImageReader::with_format(Cursor::new(bytes), format)
            .into_dimensions().map(|(width, height)| FormatDetails::Image(width, height))
            .map_err(|error| error.to_string()),
        Header::Ktx2(bytes) => {
            if bytes.len() < 80 || &bytes[..12] != b"\xABKTX 20\xBB\r\n\x1A\n" {
                return Err("KTX2 文件头无效".into());
            }
            let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            Ok(FormatDetails::Image(word(20), word(24).max(1)))
        }
        Header::None => unreachable!("空文件头无额外格式属性"),
    }
}

fn present(
    state: Res<State>,
    preview: Res<AssetPreviewState>,
    gltfs: Option<Res<Assets<Gltf>>>,
    locale: Option<Res<EditorLocale>>,
    mut previous: Local<Option<(Entity, u64, EditorLocale, Option<ModelCounts>)>>,
    mut texts: Query<(Entity, &mut Node, &mut Text), With<DetailText>>,
) {
    let Ok((entity, mut node, mut text)) = texts.single_mut() else { return };
    let display = if state.visible { Display::Flex } else { Display::None };
    if node.display != display { node.display = display; }
    if !state.visible { return; }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let counts = state.selected.as_ref().and_then(|key| {
        let model = preview.model.as_ref()?;
        if key.model != Some(model.generation)
            || preview.selected_path.as_deref() != Some(key.path.as_str())
        {
            return None;
        }
        let gltf = gltfs.as_ref()?.get(&model.catalog.gltf)?;
        Some(ModelCounts {
            scenes: gltf.scenes.len(), nodes: gltf.nodes.len(), meshes: gltf.meshes.len(),
            skins: gltf.skins.len(), clips: gltf.animations.len(),
        })
    });
    let current = (entity, state.revision, locale, counts);
    if *previous == Some(current) { return; }
    *previous = Some(current);
    let zh = locale == EditorLocale::Chinese;
    let wanted = match state.completed.as_ref() {
        None => if zh { "正在读取文件属性…" } else { "Reading file properties…" }.to_owned(),
        Some(Err(error)) => format!("{} · {error}", if zh { "无法读取文件属性" } else { "File properties unavailable" }),
        Some(Ok(details)) => {
            let mut value = format!("{} · {} B", if zh { "文件大小" } else { "File size" }, details.size);
            let loaded = counts.map(FormatDetails::Model);
            match (details.format.as_ref(), loaded.as_ref()) {
                (Err(error), _) => value.push_str(&format!("\n{} · {error}", if zh { "格式详情不可用" } else { "Format details unavailable" })),
                (Ok(format), loaded) => match format.as_ref().or(loaded) {
                    Some(FormatDetails::Model(counts)) => {
                        let ModelCounts { scenes, nodes, meshes, skins, clips } = counts;
                        let line = if zh {
                            format!("场景 {scenes} · 节点 {nodes} · 网格 {meshes}\n蒙皮 {skins} · 动画 {clips}")
                        } else {
                            format!("Scenes {scenes} · Nodes {nodes} · Meshes {meshes}\nSkins {skins} · Animations {clips}")
                        };
                        value.push('\n');
                        value.push_str(&line);
                    }
                    Some(FormatDetails::Image(width, height)) => value.push_str(&format!(
                        "\n{} · {width} × {height} px", if zh { "图像尺寸" } else { "Image dimensions" }
                    )),
                    None => {}
                },
            }
            value
        }
    };
    if text.as_str() != wanted { text.0 = wanted; }
}

#[cfg(feature = "validation")]
pub(crate) fn validation_text(world: &mut World) -> Option<String> {
    let (node, text) = world.query_filtered::<(&Node, &Text), With<DetailText>>().single(world).ok()?;
    (node.display == Display::Flex).then(|| text.0.clone())
}
