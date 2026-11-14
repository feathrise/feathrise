//! 独立体素三维涂绘复用持久字段树；只发布新的不可变源引用，密度块和旧字段保持可撤销。
use super::{NativeFieldInput, sources::read_source};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool},
};
use feathrise_scene_content::*;
use feathrise_terrain::volume::sparse::paged::*;
use std::path::PathBuf;

pub(crate) async fn edit_fields(
    root: PathBuf,
    owner: SceneId,
    mut object: SceneTerrainObject,
    input: NativeFieldInput,
    version: String,
) -> Result<SceneTerrainObject, String> {
    let descriptor = object
        .voxel
        .as_ref()
        .ok_or("所选地形没有独立体素作者源")?
        .clone();
    let fields_folder = format!("{}/fields", object.private_folder(&owner));
    let objects = crate::authoring::safe_asset_path(&root, &format!("{fields_folder}/objects"))?;
    let expected = object.id.clone();
    let asset_root = root.clone();
    let fields_root = objects.clone();
    let (mut source, digest, previous) = IoTaskPool::get()
        .spawn(async move {
            let (bytes, digest) =
                read_source(&asset_root, &descriptor.source_path, 32 * 1024 * 1024)?;
            let source: SceneVoxelTerrainSource =
                serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
                    .map_err(|error| error.to_string())?;
            source.validate()?;
            if source.scene_id != owner
                || source.terrain_id != expected
                || source.cell_size_meters != descriptor.cell_size_meters
                || source.min != descriptor.min
                || source.max != descriptor.max
            {
                return Err("体素字段作者源已换代".to_owned());
            }
            let previous = if let Some(path) = &source.surface_fields_path {
                let (bytes, _) = read_source(&asset_root, path, 32 * 1024 * 1024)?;
                // 字段树与密度源共用作者格距，窗口流送可以直接使用同一格点身份。
                decode_paged_volume(&bytes)?.ok_or("体素字段文件不是持久根载体")?
            } else {
                PagedVolume::new(source.cell_size_meters)?
            };
            if previous.cell_size != source.cell_size_meters {
                return Err("体素字段格距与密度作者源不一致".into());
            }
            std::fs::create_dir_all(&fields_root).map_err(|error| error.to_string())?;
            Ok::<_, String>((source, digest, previous))
        })
        .await?;
    let matrix = object.transform.as_transform().to_matrix();
    let inverse = matrix.inverse();
    let mut brush = input.brush;
    let query = input.query.clone();
    let ray = input.ray;
    let to = Vec3::from_array(brush.to);
    let normal = AsyncComputeTaskPool::get()
        .spawn(async move {
            let hit = query
                .ray_surface(ray.0, ray.1, 1_000_000.)
                .ok_or("三维涂绘射线没有真实表面")?;
            if hit.position.distance(to) > 0.05 {
                return Err("三维涂绘光标与冻结表面不一致".to_owned());
            }
            Ok::<_, String>(hit.normal)
        })
        .await?;
    brush.from = inverse
        .transform_point3(Vec3::from_array(brush.from))
        .to_array();
    brush.to = inverse
        .transform_point3(Vec3::from_array(brush.to))
        .to_array();
    brush.normal = matrix
        .transpose()
        .transform_vector3(normal)
        .normalize()
        .to_array();
    brush.source_to_world = Mat3::from_mat4(matrix);
    let io_previous = previous.clone();
    let io_brush = brush.clone();
    let fields_root = objects.clone();
    let window = IoTaskPool::get()
        .spawn(async move { load_field_window(&fields_root, &io_previous, &io_brush) })
        .await?;
    let compute_previous = previous.clone();
    let query = input.query;
    let delta = AsyncComputeTaskPool::get()
        .spawn(async move {
            prepare_field_paint(
                window,
                &compute_previous,
                &brush,
                |point, direction, maximum| {
                    let world_direction = matrix.transform_vector3(direction);
                    let scale = world_direction.length();
                    let hit = query.ray_surface(
                        matrix.transform_point3(point),
                        world_direction / scale,
                        maximum * scale,
                    )?;
                    Some((
                        inverse.transform_point3(hit.position),
                        matrix.transpose().transform_vector3(hit.normal).normalize(),
                    ))
                },
            )
        })
        .await?;
    let fields_root = objects.clone();
    let io_previous = previous.clone();
    let next = IoTaskPool::get()
        .spawn(async move { write_field_paint(&fields_root, &io_previous, delta) })
        .await?;
    if next == previous {
        return Ok(object);
    }
    let (field_bytes, fields_path, bytes, source_scene) = AsyncComputeTaskPool::get()
        .spawn(async move {
            let field_bytes = encode_paged_volume(&next)?;
            let fields_path = format!(
                "{fields_folder}/{}.vpaged",
                authoring_io::bytes_hash(&field_bytes)
            );
            source.surface_fields_path = Some(fields_path.clone());
            let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;
            Ok::<_, String>((field_bytes, fields_path, bytes, source.scene_id))
        })
        .await?;
    let target = format!(
        "{}/versions/{version}.scene_voxel_source.json",
        object.private_folder(&source_scene)
    );
    let original = object.voxel.as_ref().unwrap().source_path.clone();
    let source_target = target.clone();
    IoTaskPool::get()
        .spawn(async move {
            let original = crate::authoring::safe_asset_path(&root, &original)?;
            let mut writes = Vec::new();
            for (path, bytes) in [(fields_path, field_bytes), (source_target, bytes)] {
                let target = crate::authoring::safe_asset_path(&root, &path)?;
                let actual = authoring_io::file_hash(&target)?;
                if actual == "missing" {
                    writes.push(authoring_transaction::PreparedWrite::new(target, bytes));
                } else if actual != authoring_io::bytes_hash(&bytes) {
                    return Err("不可变体素字段版本已有其他内容".into());
                }
            }
            authoring_transaction::commit_prepared_writes_checked(writes, move |_, path| {
                authoring_io::compare_content_hash(&digest, &authoring_io::file_hash(&original)?)?;
                authoring_io::compare_content_hash("missing", &authoring_io::file_hash(path)?)
            })
            .map_err(|failure| failure.message().to_owned())
        })
        .await?;
    object.voxel.as_mut().unwrap().source_path = target;
    Ok(object)
}
