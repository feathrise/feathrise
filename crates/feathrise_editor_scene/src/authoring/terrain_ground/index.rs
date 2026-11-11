//! 静态地形对象BVH只过滤空间候选，最终命中始终来自实际GLB/Clipmap/体素三角面。
use bevy::prelude::*;
use feathrise_scene_content::{SceneId, SceneTerrainObject, TerrainId};
use feathrise_scene::{TerrainObjectRaySnapshot, TerrainSurfaceVersion};
use std::sync::Arc;

pub(super) type SurfaceInput = (Arc<SceneTerrainObject>, TerrainObjectRaySnapshot, TerrainSurfaceVersion);
struct Surface { id: TerrainId, definition: Option<Arc<SceneTerrainObject>>, query: TerrainObjectRaySnapshot, min: Vec3, max: Vec3 }
struct Node { min: Vec3, max: Vec3, children: Option<(usize, usize)>, range: std::ops::Range<usize> }

pub struct TerrainGroundSnapshot {
    pub scene_id: SceneId,
    pub revision: u64,
    pub generation: u64,
    pub(super) versions: Vec<TerrainSurfaceVersion>,
    surfaces: Vec<Surface>,
    nodes: Vec<Node>,
    pub bindings: feathrise_scene_content::TerrainBindingIndex,
    by_id: std::collections::BTreeMap<TerrainId, usize>,
}
impl TerrainGroundSnapshot {
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub fn test_meshes(map:SceneId, objects:&[SceneTerrainObject], collider:avian3d::prelude::Collider)->Arc<Self> {
        let mut result=Self {scene_id: map,revision:0,generation:0,versions:Vec::new(),surfaces:Vec::new(),nodes:Vec::new(),bindings:Default::default(),by_id:Default::default()};
        for object in objects {
            let query=TerrainObjectRaySnapshot::Mesh(collider.clone(),feathrise_scene_core::residency::SceneSourceFrame::from_transform(object.transform.as_transform()));
            let (min,max)=query.mesh_bounds().unwrap();
            result.bindings.insert(object.id.clone(),object.transform);
            result.surfaces.push(Surface {id:object.id.clone(),definition:Some(Arc::new(object.clone())),query,min:min-Vec3::splat(0.01),max:max+Vec3::splat(0.01)});
        }
        if !result.surfaces.is_empty() {result.branch(0..result.surfaces.len());}
        result.by_id=result.surfaces.iter().enumerate().map(|(i,s)|(s.id.clone(),i)).collect();Arc::new(result)
    }
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub fn empty(map:SceneId,revision:u64)->Arc<Self> {Self::build(map,revision,Default::default(),Vec::new()).unwrap()}
    /// 回归测试同样使用真实Collider查询，避免用假高度返回值掩盖跨层错误。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub fn test_mesh(map: SceneId, id: TerrainId, pose: feathrise_scene_content::TerrainObjectTransform,
        collider: avian3d::prelude::Collider) -> Arc<Self> {
        let query = TerrainObjectRaySnapshot::Mesh(collider, feathrise_scene_core::residency::SceneSourceFrame::from_transform(pose.as_transform()));
        let (min, max) = query.mesh_bounds().unwrap();
        let mut result = Self {scene_id: map, revision:0, generation:0, versions:Vec::new(),
            surfaces:vec![Surface{id:id.clone(),definition:None,query,min:min-Vec3::splat(0.01),max:max+Vec3::splat(0.01)}],
            nodes:Vec::new(),bindings:Default::default(),by_id:std::collections::BTreeMap::from([(id.clone(),0)])};
        result.bindings.insert(id,pose);result.branch(0..1);Arc::new(result)
    }
    pub(super) fn build(map: SceneId, revision: u64, objects: Arc<[SceneTerrainObject]>, inputs: Vec<SurfaceInput>) -> Result<Arc<Self>, String> {
        let versions = inputs.iter().map(|(_, _, version)| version.clone()).collect();
        let mut indexed = std::collections::BTreeMap::new();
        for (definition, query, version) in inputs { indexed.insert(definition.id.clone(), (definition, query, version)); }
        let mut result = Self { scene_id: map, revision, generation: 0, versions, surfaces: Vec::new(), nodes: Vec::new(), bindings: Default::default(), by_id: Default::default() };
        for object in objects.iter().filter(|object| object.enabled) {
            let (definition, query, _) = indexed.remove(&object.id).ok_or("地形对象尚未完整就绪")?;
            if *definition != *object { return Err("地形对象仍在采用上一作者版本".into()); }
            let matrix = object.transform.as_transform().to_matrix();
            let frame = match &query { TerrainObjectRaySnapshot::Clipmap(_, f) | TerrainObjectRaySnapshot::Mesh(_, f) => f };
            if matrix != frame.source_to_world { return Err("地形姿态尚未完整采用".into()); }
            result.bindings.insert(object.id.clone(), object.transform);
            let local_min = object.voxel.as_ref().map_or(Vec3::new(object.world_min[0], object.terrain.base_height, object.world_min[1]), |v| Vec3::from_array(v.min));
            let local_max = object.voxel.as_ref().map_or(Vec3::new(object.world_max[0], object.terrain.base_height + object.terrain.height_scale, object.world_max[1]), |v| Vec3::from_array(v.max));
            let mut min = Vec3::splat(f32::INFINITY); let mut max = Vec3::splat(f32::NEG_INFINITY);
            for bits in 0..8 {
                let point = matrix.transform_point3(Vec3::new(if bits & 1 == 0 { local_min.x } else { local_max.x },
                    if bits & 2 == 0 { local_min.y } else { local_max.y }, if bits & 4 == 0 { local_min.z } else { local_max.z }));
                min = min.min(point); max = max.max(point);
            }
            if let Some((actual_min,actual_max))=query.mesh_bounds() { min=min.min(actual_min);max=max.max(actual_max); }
            result.surfaces.push(Surface { id: object.id.clone(), definition:Some(definition), query, min: min - Vec3::splat(0.01), max: max + Vec3::splat(0.01) });
        }
        if !result.surfaces.is_empty() { result.branch(0..result.surfaces.len()); }
        result.by_id = result.surfaces.iter().enumerate().map(|(index,surface)|(surface.id.clone(),index)).collect();
        Ok(Arc::new(result))
    }
    fn branch(&mut self, range: std::ops::Range<usize>) -> usize {
        let mut min = Vec3::splat(f32::INFINITY); let mut max = Vec3::splat(f32::NEG_INFINITY);
        for surface in &self.surfaces[range.clone()] { min = min.min(surface.min); max = max.max(surface.max); }
        let index = self.nodes.len();
        self.nodes.push(Node { min, max, children: None, range: range.clone() });
        if range.len() > 4 {
            let extent = max - min;
            let axis = if extent.x >= extent.z { 0 } else { 2 };
            let middle = range.start + range.len() / 2;
            self.surfaces[range.clone()].select_nth_unstable_by(range.len() / 2,
                |a, b| (a.min[axis] + a.max[axis]).total_cmp(&(b.min[axis] + b.max[axis])));
            let left = self.branch(range.start..middle); let right = self.branch(middle..range.end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }
    /// 空场景与对象之间的空隙返回None。此方法供单飞后台光标使用。
    #[cfg(test)]
    pub fn ray(&self, origin: Vec3, direction: Vec3) -> Option<(TerrainId, Vec3)> {
        self.ray_visible(origin,direction,&Default::default())
    }
    /// 编辑器隐藏只裁剪拾取候选；玩法承托查询继续使用完整真实地形集合。
    pub fn ray_visible(&self, origin: Vec3, direction: Vec3, hidden: &std::collections::BTreeSet<TerrainId>) -> Option<(TerrainId, Vec3)> {
        let mut closest = f32::INFINITY; let mut result = None;
        self.visit(origin, direction, |surface| {
            if hidden.contains(&surface.id) {return;}
            if let Some(point) = surface.query.ray(origin, direction) {
                let distance = origin.distance_squared(point);
                if distance < closest { closest = distance; result = Some((surface.id.clone(), point)); }
            }
        });
        result
    }
    /// 批量刷草遇到遮挡候选缺页也必须等待，不能把“尚未读到上层”当作下层可见。
    pub fn ray_visible_checked(&self, origin:Vec3, direction:Vec3, hidden:&std::collections::BTreeSet<TerrainId>)
        -> Result<Option<(TerrainId,Vec3)>,feathrise_scene::ClipmapGroundHit> {
        use feathrise_scene::ClipmapGroundHit;
        let mut closest=f32::INFINITY;let mut result=None;let mut waiting=false;let mut failed=false;
        self.visit(origin,direction,|surface| {
            if hidden.contains(&surface.id) {return;}
            let hit=match &surface.query {
                TerrainObjectRaySnapshot::Mesh(..)=>surface.query.ray(origin,direction).map_or(ClipmapGroundHit::Miss,ClipmapGroundHit::Hit),
                TerrainObjectRaySnapshot::Clipmap(snapshot,frame)=>{
                    let Some(local_direction)=frame.world_to_source.transform_vector3(direction).try_normalize() else {failed=true;return;};
                    match snapshot.ray_intersection(frame.to_local(origin),local_direction) {
                        ClipmapGroundHit::Hit(point)=>ClipmapGroundHit::Hit(frame.to_world(point)),other=>other,
                    }
                }
            };
            match hit {
                ClipmapGroundHit::Hit(point)=>{let distance=origin.distance_squared(point);if distance<closest {closest=distance;result=Some((surface.id.clone(),point));}},
                ClipmapGroundHit::WaitingForHeight=>waiting=true,ClipmapGroundHit::Failed=>failed=true,_=>{},
            }
        });
        if failed {Err(ClipmapGroundHit::Failed)} else if waiting {Err(ClipmapGroundHit::WaitingForHeight)} else {Ok(result)}
    }
    /// 所选落点冻结承托层参考高度；洞底和下层地形不会再次跳到上方地形。
    pub fn support(&self, x: f32, z: f32, reference: Option<f32>) -> Option<f32> {
        match self.support_status(x,z,reference) { feathrise_scene::ClipmapGroundHit::Hit(point)=>Some(point.y), _=>None }
    }
    /// 关联记录只允许原承托对象提供地面；叠层移动、删岛不会重新绑定到同XZ的另一层。
    pub fn support_on(&self, id: &TerrainId, x: f32, z: f32, reference: f32) -> feathrise_scene::ClipmapGroundHit {
        let Some(surface)=self.by_id.get(id).map(|index|&self.surfaces[*index]) else {return feathrise_scene::ClipmapGroundHit::Miss;};
        surface.query.support_below_status(Vec3::new(x,reference,z),(reference-surface.min.y+1.0).max(0.0))
    }
    /// 后台散布与五点地基记录实际命中的稳定对象身份，不根据初始场景范围猜承托物。
    pub fn support_identity(&self, x:f32,z:f32,reference:Option<f32>) -> Result<Option<(TerrainId,f32)>,String> {
        let Some(root)=self.nodes.first() else {return Ok(None);};
        let top=reference.unwrap_or(root.max.y+1.0); let origin=Vec3::new(x,top,z);
        let mut highest:Option<(TerrainId,f32)>=None;let mut waiting=false;
        self.visit(origin,Vec3::NEG_Y,|surface|match surface.query.support_below_status(origin,(top-root.min.y+1.0).max(0.0)) {
            feathrise_scene::ClipmapGroundHit::Hit(point) if highest.as_ref().is_none_or(|(_,y)|point.y>*y)=>highest=Some((surface.id.clone(),point.y)),
            feathrise_scene::ClipmapGroundHit::WaitingForHeight=>waiting=true,
            _=>{},
        });
        if waiting {Err("承托高度页尚未完整就绪".into())} else {Ok(highest)}
    }
    pub fn bind(&self,id:&TerrainId,pose:feathrise_scene_content::TerrainObjectTransform)->Result<feathrise_scene_content::SceneTerrainBinding,String> {
        feathrise_scene_content::SceneTerrainBinding::capture(id.clone(),self.bindings.pose(id).ok_or("所选承托地形已不存在")?,pose)
    }
    /// 冻结单对象真实查询供作者后台投射；只克隆Collider/高度页共享句柄。
    pub fn object_query(&self,id:&TerrainId)->Option<TerrainObjectRaySnapshot> {
        self.by_id.get(id).map(|index|self.surfaces[*index].query.clone())
    }
    /// 由完整查询快照缓存的世界包围换回源坐标；固定八角，不在显示帧扫描高度或网格。
    /// 范围可保守外扩，必须包含超过旧编码域的山顶和深坑，变换预览可再应用新姿态。
    pub fn local_bounds(&self, id: &TerrainId) -> Option<(Vec3, Vec3)> {
        let surface = &self.surfaces[*self.by_id.get(id)?];
        let frame = match &surface.query {
            TerrainObjectRaySnapshot::Clipmap(_, frame) | TerrainObjectRaySnapshot::Mesh(_, frame) => frame,
        };
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for bits in 0..8 {
            let point = frame.to_local(Vec3::new(
                if bits & 1 == 0 { surface.min.x } else { surface.max.x },
                if bits & 2 == 0 { surface.min.y } else { surface.max.y },
                if bits & 4 == 0 { surface.min.z } else { surface.max.z },
            ));
            min = min.min(point); max = max.max(point);
        }
        (min.is_finite() && max.is_finite()).then_some((min, max))
    }
    /// 只在光标后台槽查询射线扇的局部BVH候选；潜在遮挡也断线，禁止穿过另一网格补刷。
    pub(super) fn paint_segment_unobstructed(&self, owner: &TerrainId, from: Vec3, to: Vec3,
        camera: Mat4, orthographic: bool, hidden: &std::collections::BTreeSet<TerrainId>) -> bool {
        let eye = camera.w_axis.truncate();
        let direction = camera.transform_vector3(Vec3::NEG_Z).normalize_or_zero();
        let origin = |point: Vec3| if orthographic {
            point - direction * ((point - eye).dot(direction).max(0.0) + 0.1)
        } else { eye };
        let triangles = [[origin(from), from, to], [origin(from), origin(to), to]];
        let intersects = |min, max| triangles.iter().any(|triangle|
            super::paint_cursor::triangle_intersects_bounds(*triangle, min, max));
        let mut stack = Vec::from([0]);
        while let Some(index) = stack.pop() {
            let Some(node) = self.nodes.get(index) else { continue; };
            if !intersects(node.min, node.max) { continue; }
            if let Some((left, right)) = node.children { stack.push(left); stack.push(right); }
            else if self.surfaces[node.range.clone()].iter().any(|surface|
                surface.id != *owner && !hidden.contains(&surface.id) && intersects(surface.min, surface.max)) {
                return false;
            }
        }
        true
    }
    /// 跨网格起笔按已勾选ID直达同代表面定义，不在输入帧扫描整个场景的网格清单。
    pub fn object_definition(&self,id:&TerrainId)->Option<&Arc<SceneTerrainObject>> {
        self.surfaces.get(*self.by_id.get(id)?)?.definition.as_ref()
    }
    pub fn support_status(&self, x: f32, z: f32, reference: Option<f32>) -> feathrise_scene::ClipmapGroundHit {
        use feathrise_scene::ClipmapGroundHit;
        let Some(root) = self.nodes.first() else { return ClipmapGroundHit::Miss; };
        let top = reference.unwrap_or(root.max.y + 1.0);
        let origin = Vec3::new(x, top, z);
        let depth = (top - root.min.y + 1.0).max(0.0);
        let mut highest: Option<f32> = None;
        let mut waiting=false;
        self.visit(origin, Vec3::NEG_Y, |surface| {
            match surface.query.support_below_status(origin,depth) {
                ClipmapGroundHit::Hit(point) if highest.is_none_or(|height|point.y>height)=>highest=Some(point.y),
                ClipmapGroundHit::WaitingForHeight=>waiting=true,
                _=>{},
            }
        });
        if waiting { ClipmapGroundHit::WaitingForHeight } else { highest.map_or(ClipmapGroundHit::Miss,|y|ClipmapGroundHit::Hit(Vec3::new(x,y,z))) }
    }
    fn visit(&self, origin: Vec3, direction: Vec3, mut use_surface: impl FnMut(&Surface)) {
        if self.nodes.is_empty() || !origin.is_finite() || !direction.is_finite() { return; }
        let mut stack = vec![0];
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index];
            if !ray_bounds(origin, direction, node.min, node.max) { continue; }
            if let Some((left, right)) = node.children { stack.push(right); stack.push(left); }
            else { for surface in &self.surfaces[node.range.clone()] {
                if ray_bounds(origin, direction, surface.min, surface.max) { use_surface(surface); }
            } }
        }
    }
}
fn ray_bounds(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> bool {
    let mut near: f32 = 0.0; let mut far = f32::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-8 { if origin[axis] < min[axis] || origin[axis] > max[axis] { return false; } }
        else {
            let a = (min[axis] - origin[axis]) / direction[axis]; let b = (max[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b)); far = far.min(a.max(b));
            if near > far { return false; }
        }
    }
    true
}

#[cfg(test)] mod tests {
    use super::*;
    use feathrise_scene_core::residency::SceneSourceFrame;
    fn plane(id: &str, x: f32, y: f32) -> Surface {
        let collider=avian3d::prelude::Collider::trimesh(vec![Vec3::new(-1.,0.,-1.),Vec3::new(1.,0.,-1.),Vec3::new(1.,0.,1.),Vec3::new(-1.,0.,1.)],vec![[0,2,1],[0,3,2]]);
        let frame=SceneSourceFrame::from_transform(Transform::from_xyz(x,y,0.).with_scale(Vec3::new(2.,3.,2.)));
        Surface{id:TerrainId::parse(id).unwrap(),definition:None,query:TerrainObjectRaySnapshot::Mesh(collider,frame),min:Vec3::new(x-2.,y-0.1,-2.),max:Vec3::new(x+2.,y+0.1,2.)}
    }
    #[test]
    fn actual_islands_leave_empty_gaps_and_selected_lower_layers_keep_their_floor() {
        let mut ground=TerrainGroundSnapshot{scene_id:SceneId::parse("MAP-ground").unwrap(),revision:1,generation:1,versions:Vec::new(),
            surfaces:vec![plane("lower",0.,4.),plane("upper",0.,10.),plane("island",8.,2.)],nodes:Vec::new(),bindings:Default::default(),by_id:Default::default()};
        ground.branch(0..3);
        ground.by_id=ground.surfaces.iter().enumerate().map(|(index,surface)|(surface.id.clone(),index)).collect();
        assert_eq!(ground.support(0.,0.,None),Some(10.));
        assert_eq!(ground.support(0.,0.,Some(4.5)),Some(4.));
        assert_eq!(ground.support(4.,0.,None),None);
        let (id,point)=ground.ray(Vec3::new(8.,20.,0.),Vec3::NEG_Y).unwrap();
        assert_eq!(id.as_str(),"island");assert!((point.y-2.).abs()<0.001);
        assert!(ground.ray(Vec3::new(4.,20.,0.),Vec3::NEG_Y).is_none());
        let hidden=std::collections::BTreeSet::from([TerrainId::parse("upper").unwrap()]);
        assert_eq!(ground.ray_visible(Vec3::new(0.,20.,0.),Vec3::NEG_Y,&hidden).unwrap().0.as_str(),"lower");
        assert_eq!(ground.support(0.,0.,None),Some(10.)); // 隐藏只影响作者拾取，真实承托不变。
        assert!(matches!(ground.support_on(&TerrainId::parse("lower").unwrap(),0.,0.,20.),feathrise_scene::ClipmapGroundHit::Hit(point) if point.y==4.));
        assert!(ground.object_query(&TerrainId::parse("removed").unwrap()).is_none());
    }
}
