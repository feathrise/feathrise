//! 单个平面凸面的等宽内插：沿各边真实偏移，创建保留外边界的环带与内面。

use super::super::types::MeshElementKind;
use super::mesh::{EPSILON, MeshWork, chosen, planar_normal, positive_amount};
use feathrise_map_content::MapPlacementRecord;

pub(in super::super) fn inset_record(
    before: &MapPlacementRecord,
    kind: MeshElementKind,
    selected: u128,
    width_meters: f32,
) -> Result<MapPlacementRecord, String> {
    let width = positive_amount(width_meters, "内插宽度")?;
    if width == 0.0 {
        return Ok(before.clone());
    }
    if kind != MeshElementKind::Face {
        return Err("内插面需要面模式下只选择一个面".into());
    }
    let mut work = MeshWork::from_record(before)?;
    let selected = chosen(selected, work.faces.len())?;
    if selected.len() != 1 {
        return Err("内插面首版每次处理一个平面凸面".into());
    }
    let index = selected[0];
    let face = work.faces[index].clone();
    let normal = planar_normal(&face, &work.points)?;
    let mut inward = Vec::with_capacity(face.len());
    for i in 0..face.len() {
        let direction = (work.points[face[(i + 1) % face.len()]] - work.points[face[i]])
            .try_normalize()
            .ok_or("面包含零长度边")?;
        inward.push(normal.cross(direction));
    }
    for i in 0..face.len() {
        let previous = inward[(i + face.len() - 1) % face.len()];
        let current = inward[i];
        if normal.dot(previous.cross(current)) < -EPSILON {
            return Err("等宽内插目前需要凸面，请先切分凹面".into());
        }
    }
    let mut inner = Vec::with_capacity(face.len());
    for i in 0..face.len() {
        let previous = inward[(i + face.len() - 1) % face.len()];
        let current = inward[i];
        let denominator = 1.0 + previous.dot(current);
        if denominator <= EPSILON {
            return Err("内插角度过窄，无法形成有限宽度".into());
        }
        let point = work.points[face[i]] + (previous + current) * (width / denominator);
        if inward
            .iter()
            .enumerate()
            .any(|(edge, normal)| normal.dot(point - work.points[face[edge]]) < width - EPSILON)
        {
            return Err("内插宽度使内面翻转或消失，请减小宽度".into());
        }
        inner.push(work.push_point(point)?);
    }
    work.faces[index] = inner.clone(); // cap索引原位，确认后仍能继续内插或挤出该内面。
    for i in 0..face.len() {
        let next = (i + 1) % face.len();
        work.faces
            .push(vec![face[i], face[next], inner[next], inner[i]]);
    }
    work.finish(before)
}
