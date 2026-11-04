//! 大批候选仅在后台使用同代真实查询快照投射；全体成功才回DLL重编码，不用包围盒或旧悬停补高度。
use super::*;
use feathrise_map_content::{MapTerrainKind, TerrainObjectTransform};
use feathrise_scene::ClipmapGroundHit;

pub(super) struct Ground {
    query: Box<dyn Fn(&DomainGroundProbe) -> Result<DomainGroundPatch, String> + Send + Sync>,
}
impl Ground {
    pub(super) fn project(&self, position: [f32; 3]) -> Result<[f32; 3], String> {
        (self.query)(&DomainGroundProbe {
            id: "visual".into(),
            position,
            yaw_degrees: 0.,
        })
        .map(|patch| patch.position)
    }
}
pub(super) fn capture(world: &World, identity: &MapDomainIdentity) -> Option<Ground> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    if draft.map_id.as_str() != identity.map_id
        || session.terrain_revision != identity.terrain_revision
    {
        return None;
    }
    if draft.manifest.terrains.is_some() {
        let ground = draft
            .terrain_ground
            .clone()
            .filter(|g| g.map == draft.map_id && g.revision == identity.terrain_revision)?;
        return Some(Ground {
            query: Box::new(move |probe| {
                let point = Vec3::from_array(probe.position);
                let (terrain, y) = ground
                    .support_identity(point.x, point.z, Some(point.y + 0.5))?
                    .ok_or("角色候选没有真实地形承托，整笔未采用")?;
                let pose = TerrainObjectTransform {
                    translation: [point.x, y, point.z],
                    yaw_degrees: probe.yaw_degrees,
                    scale: [1.; 3],
                    ..default()
                };
                let binding = ground.bind(&terrain, pose)?;
                Ok(DomainGroundPatch {
                    id: probe.id.clone(),
                    position: pose.translation,
                    yaw_degrees: pose.yaw_degrees,
                    binding: Some(serde_json::to_value(binding).map_err(|e| e.to_string())?),
                })
            }),
        });
    }
    if draft.manifest.terrain.kind == MapTerrainKind::SparseClipmap {
        let ground = world
            .get_resource::<feathrise_scene::DynamicClipmapGroundSurface>()?
            .snapshot
            .clone()
            .filter(|g| {
                g.map_id == draft.map_id
                    && Some(g.content_revision_words()) == identity.ground_content
            })?;
        return Some(Ground {
            query: Box::new(move |probe| {
                let point = Vec3::from_array(probe.position);
                let position = match ground.sample_support_below(point + Vec3::Y * 0.5, 100_000.) {
                    ClipmapGroundHit::Hit(position) => position,
                    ClipmapGroundHit::WaitingForHeight => {
                        return Err("角色候选的基础作者高度页尚未就绪，整笔未采用".into());
                    }
                    _ => return Err("角色候选没有真实Clipmap承托，整笔未采用".into()),
                };
                Ok(DomainGroundPatch {
                    id: probe.id.clone(),
                    position: position.to_array(),
                    yaw_degrees: probe.yaw_degrees,
                    binding: None,
                })
            }),
        });
    }
    let (mesh, index) = (
        draft.terrain_mesh.clone()?,
        draft.terrain_editing_index.clone()?,
    );
    Some(Ground {
        query: Box::new(move |probe| {
            let position = index
                .ray_intersection(
                    &mesh.positions,
                    &mesh.indices,
                    [probe.position[0], probe.position[1] + 1., probe.position[2]],
                    [0., -1., 0.],
                )
                .ok_or("角色候选没有实际地形三角承托，整笔未采用")?
                .position;
            Ok(DomainGroundPatch {
                id: probe.id.clone(),
                position,
                yaw_degrees: probe.yaw_degrees,
                binding: None,
            })
        }),
    })
}

/// 候选正文暂未进入宿主草稿；沿用唯一DLL串行队列完成投射回写后才构建最终场景索引。
pub(super) fn resolve(
    sender: &async_channel::Sender<Job>,
    port: &Arc<NativeDomainPort>,
    snapshot: Snapshot,
    ground: Option<Ground>,
) -> Result<Snapshot, String> {
    let Some(wire) = &snapshot.map_domain else {
        return Ok(snapshot);
    };
    let Some(requests) = &wire.ground_requests else {
        return Ok(snapshot);
    };
    let ground = ground.ok_or("当前真实地形查询尚未完整就绪，角色草稿未采用")?;
    let probes: Vec<DomainGroundProbe> =
        serde_json::from_slice(&native_domain::read_blob(&port.project_root, requests)?)
            .map_err(|e| format!("角色地面请求无效：{e}"))?;
    let mut ids = std::collections::HashSet::new();
    let mut patches = Vec::with_capacity(probes.len());
    for probe in probes {
        if probe.id.is_empty()
            || probe.id.len() > 1024
            || !ids.insert(probe.id.clone())
            || probe.position.iter().any(|v| !v.is_finite())
            || !probe.yaw_degrees.is_finite()
        {
            return Err("角色地面请求包含重复身份或非法姿态".into());
        }
        patches.push((ground.query)(&probe)?);
    }
    let patches = native_domain::write_blob(
        &port.project_root,
        port.domain,
        &serde_json::to_vec(&patches).map_err(|e| e.to_string())?,
    )?;
    let identity = wire.identity.clone().ok_or("角色地面请求缺少完整身份")?;
    let document = wire.document.clone().ok_or("角色地面请求缺少完整正文")?;
    let response = super::call(
        sender,
        MapDomainRequest {
            identity,
            command: MapDomainCommand::GroundPatch { document, patches },
        },
    )?;
    if response
        .map_domain
        .as_ref()
        .is_none_or(|wire| wire.ground_requests.is_some())
    {
        return Err("角色地面结果未完成收口".into());
    }
    Ok(response)
}
