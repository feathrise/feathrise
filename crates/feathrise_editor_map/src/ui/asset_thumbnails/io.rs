//! 后台检查当前页文件及GLTF外部依赖；帧系统不读取文件或复制像素。

use super::{constants::*, types::*};
use feathrise_map_content::MapAssetKind;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "文件暂不可用".to_owned())?;
    if !path.starts_with(root) {
        return Err("资产路径超出assets目录".to_owned());
    }
    Ok(path)
}

fn file_stamp(path: &Path) -> Result<FileStamp, String> {
    let metadata = path.metadata().map_err(|_| "文件暂不可用".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
        return Err("资源较大，请选中后预览".to_owned());
    }
    Ok(FileStamp {
        length: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn image_header(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (width, height) = if extension == "ktx2" {
        use std::io::Read;
        let mut header = [0u8; 48];
        std::fs::File::open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|_| "KTX2文件头无效".to_owned())?;
        if &header[..12] != b"\xABKTX 20\xBB\r\n\x1A\n" {
            return Err("KTX2文件头无效".into());
        }
        let value = |start| u32::from_le_bytes(header[start..start + 4].try_into().unwrap());
        if value(28) > 1 || value(32) > 1 || value(36) != 1 {
            return Err("Cube / 数组 · 请在环境页查看".into());
        }
        (value(20), value(24))
    } else {
        image::image_dimensions(path).map_err(|_| "图片文件头无效".to_owned())?
    };
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err("图片较大，请选中后预览".to_owned());
    }
    Ok(())
}

fn scene_dependencies(root: &Path, path: &Path) -> Result<Arc<[String]>, String> {
    let gltf = gltf::Gltf::open(path).map_err(|_| "模型格式无效".to_owned())?;
    if gltf.nodes().len() > MAX_NODES
        || gltf.cameras().len() > 32
        || gltf.meshes().flat_map(|mesh| mesh.primitives()).count() > MAX_MESHES
    {
        return Err("模型较大，请选中后预览".into());
    }
    let mut meshes = 0;
    let mut vertices = 0usize;
    for primitive in gltf
        .nodes()
        .filter_map(|node| node.mesh())
        .flat_map(|mesh| mesh.primitives())
    {
        meshes += 1;
        vertices = vertices.saturating_add(
            primitive
                .get(&gltf::Semantic::Positions)
                .map_or(0, |accessor| accessor.count()),
        );
        if meshes > MAX_MESHES || vertices > MAX_VERTICES {
            return Err("模型较大，请选中后预览".into());
        }
    }
    // 深度上限使相机祖先检查与退役栈都有明确边界。
    let mut stack: Vec<_> = gltf
        .scenes()
        .flat_map(|scene| scene.nodes().map(|node| (node, 1)))
        .collect();
    let mut visits = 0;
    while let Some((node, depth)) = stack.pop() {
        visits += 1;
        if depth > 64 || visits > MAX_NODES * 8 {
            return Err("模型层级较深，请选中后预览".into());
        }
        stack.extend(node.children().map(|child| (child, depth + 1)));
    }
    let uris = gltf
        .buffers()
        .filter_map(|buffer| match buffer.source() {
            gltf::buffer::Source::Uri(uri) => Some(uri),
            _ => None,
        })
        .chain(gltf.images().filter_map(|image| match image.source() {
            gltf::image::Source::Uri { uri, .. } => Some(uri),
            _ => None,
        }));
    let mut dependencies = Vec::new();
    for uri in uris {
        if uri.starts_with("data:") {
            continue;
        }
        if dependencies.len() >= 256 {
            return Err("模型依赖较多，请选中后预览".into());
        }
        let uri = decode_uri(uri)?;
        let dependency = path
            .parent()
            .unwrap_or(root)
            .join(uri)
            .canonicalize()
            .map_err(|_| "模型外部依赖缺失".to_owned())?;
        let relative = dependency
            .strip_prefix(root)
            .map_err(|_| "模型依赖超出assets目录".to_owned())?;
        dependencies.push(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(dependencies.into())
}

fn decode_uri(uri: &str) -> Result<String, String> {
    let bytes = uri.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .ok_or("模型依赖路径编码无效")?;
            let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            result.push(
                digit(hex[0])
                    .zip(digit(hex[1]))
                    .map(|(high, low)| high * 16 + low)
                    .ok_or("模型依赖路径编码无效")?,
            );
            index += 3;
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(result).map_err(|_| "模型依赖路径编码无效".into())
}

fn inspect(root: &Path, input: InspectionInput) -> Result<SourceInfo, String> {
    let path = checked_path(root, &input.path)?;
    let primary = file_stamp(&path)?;
    let dependencies = if let Some(previous) = input
        .previous
        .filter(|previous| previous.primary == primary)
    {
        previous.dependencies // 文件未改时仅stat已知依赖，避免每两秒重复解析GLB。
    } else if input.kind == MapAssetKind::Scene {
        scene_dependencies(root, &path)?
    } else {
        image_header(&path)?;
        Arc::from([])
    };
    let mut hasher = DefaultHasher::new();
    primary.hash(&mut hasher);
    let mut source_bytes = primary.length;
    for relative in dependencies.iter() {
        relative.hash(&mut hasher);
        let stamp = file_stamp(&checked_path(root, relative)?)
            .map_err(|_| "模型外部依赖缺失或过大".to_owned())?;
        source_bytes = source_bytes.saturating_add(stamp.length);
        if source_bytes > MAX_SOURCE_BYTES * 2 {
            return Err("模型依赖较大，请选中后预览".into());
        }
        stamp.hash(&mut hasher);
    }
    Ok(SourceInfo {
        primary,
        stamp: hasher.finish(),
        dependencies,
    })
}

pub(super) fn inspect_page(
    root: PathBuf,
    epoch: u64,
    inputs: Vec<InspectionInput>,
) -> InspectionResult {
    let root = root.canonicalize();
    let sources = inputs
        .into_iter()
        .map(|input| {
            let path = input.path.clone();
            let source = match &root {
                Ok(root) => inspect(root, input),
                Err(_) => Err("assets目录暂不可用".into()),
            };
            (path, source)
        })
        .collect();
    InspectionResult { epoch, sources }
}
