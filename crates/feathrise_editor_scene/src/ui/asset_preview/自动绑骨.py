# 用途：从静态人形 GLB 估计关节，按确认位置拟合工程动作骨架并生成独立蒙皮 GLB。
# 用法：Blender --background --factory-startup --disable-autoexec --python-exit-code 1
#       --python 自动绑骨.py -- <隔离目录内的绝对请求 JSON 路径>
# 依赖：Blender 4.5+ 内置 bpy、NumPy 和工程已有的保存时自动导出 GLB 插件。

import importlib.util
import json
import math
import struct
import sys
from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector


MAX_GLB_BYTES = 256 * 1024 * 1024
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_VERTICES = 500_000
MAX_MESHES = 32
MAX_JOINTS = 128
SIDES = ("left", "right")
LIMB_MARKERS = ("shoulder", "elbow", "wrist", "hip", "knee", "ankle", "toe")
MARKERS = ("head", "neck", "hips") + tuple(
    f"{side}_{marker}" for side in SIDES for marker in LIMB_MARKERS
)
PREFIX = "mixamorig:"


def glb_document(path):
    """仅读取定长 GLB 头和有界 JSON。导入前拒绝外置文件依赖。"""
    size = path.stat().st_size
    if not 20 <= size <= MAX_GLB_BYTES:
        raise ValueError("GLB 文件为空或超过 256 MiB")
    with path.open("rb") as stream:
        magic, version, declared, length, kind = struct.unpack("<4sIII4s", stream.read(20))
        if (magic, version, declared, kind) != (b"glTF", 2, size, b"JSON"):
            raise ValueError("需要完整的 GLB 2.0 文件")
        if length > min(size - 20, MAX_JSON_BYTES):
            raise ValueError("GLB 元数据超过允许大小")
        document = json.loads(stream.read(length))
    resources = document.get("buffers", []) + document.get("images", [])
    if any(not item.get("uri", "data:").startswith("data:") for item in resources):
        raise ValueError("请先将外部缓冲区和纹理打包进 GLB")
    return document


def staged_path(stage, value, suffix, output=False):
    path = Path(value)
    if not path.is_absolute() or path.suffix.casefold() != suffix:
        raise ValueError(f"需要绝对 {suffix} 路径")
    if path.resolve().parent != stage or path.is_symlink():
        raise ValueError("任务文件必须位于本次隔离目录内")
    if output and path.exists():
        raise ValueError(f"本次任务的暂存输出已存在：{path.name}")
    if not output and not path.is_file():
        raise ValueError(f"未找到任务文件：{path.name}")
    return path


def import_glb(path):
    before = set(bpy.data.objects)
    result = bpy.ops.import_scene.gltf(
        filepath=str(path), import_pack_images=True, bone_heuristic="BLENDER"
    )
    if result != {"FINISHED"}:
        raise RuntimeError(f"Blender 无法导入 {path.name}")
    bpy.context.view_layer.update()
    return set(bpy.data.objects) - before


def select_objects(objects, active):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = active
    bpy.context.view_layer.update()


def load_model(path):
    document = glb_document(path)
    if document.get("skins") or document.get("animations"):
        raise ValueError("请选用没有现有蒙皮或动画的静态人形 GLB")
    scenes = document.get("scenes", [])
    nodes = document.get("nodes", [])
    if len(scenes) != 1 or document.get("scene", 0) != 0:
        raise ValueError("请将人物导出为单一场景 GLB，使绑骨与 Scene0 预览使用同一网格")
    # Blender 会读入未归入场景的孤立网格，Bevy 的 Scene0 不会。预先拒绝两者范围不同的输入。
    visited = set()
    pending = list(scenes[0].get("nodes", []))
    while pending:
        index = pending.pop()
        if index not in visited:
            visited.add(index)
            pending.extend(nodes[index].get("children", []))
    if any(index not in visited and "mesh" in node for index, node in enumerate(nodes)):
        raise ValueError("GLB 包含不在 Scene0 中的网格，请整理后重新导出")
    instances = [node["mesh"] for node in nodes if "mesh" in node]
    declared_vertices = sum(
        document["accessors"][primitive["attributes"]["POSITION"]]["count"]
        for index in instances
        for primitive in document["meshes"][index]["primitives"]
    )
    if not 1 <= len(instances) <= MAX_MESHES or not 1 <= declared_vertices <= MAX_VERTICES:
        raise ValueError("请选择包含网格且不超过 32 个网格对象和 50 万顶点的人形模型")
    objects = import_glb(path)
    meshes = sorted((obj for obj in objects if obj.type == "MESH"), key=lambda obj: obj.name)
    if not meshes or sum(len(obj.data.vertices) for obj in meshes) > MAX_VERTICES:
        raise ValueError("导入后的模型没有可用网格，或超出顶点预算")
    if any(not obj.data.polygons or obj.data.shape_keys for obj in meshes):
        raise ValueError("首版需要三角面网格。请先应用模型形态键并移除空网格")
    # 保留节点世界变换和原材质。Blender 的蒙皮操作要求每个对象有独立的权重数据。
    for mesh in meshes:
        if mesh.data.users > 1:
            mesh.data = mesh.data.copy()
    return meshes


def mesh_points(meshes):
    parts = []
    for mesh in meshes:
        values = np.empty(len(mesh.data.vertices) * 3, dtype=np.float32)
        mesh.data.vertices.foreach_get("co", values)
        world = np.asarray(mesh.matrix_world, dtype=np.float64)
        parts.append(values.reshape(-1, 3) @ world[:3, :3].T + world[:3, 3])
    if not all(np.isfinite(part).all() for part in parts):
        raise ValueError("模型坐标无效，请检查站立方向和 GLB 的单位")
    return parts


def body_points(parts):
    points = np.concatenate(parts)
    height = float(np.ptp(points[:, 2]))
    if not 0.001 <= height <= 1000:
        raise ValueError("模型高度无效，请检查站立方向和 GLB 的单位")
    dominant = max(parts, key=len)
    dominant_height = float(np.ptp(dominant[:, 2]))
    if len(parts) > 1 and len(dominant) >= len(points) / 2 and dominant_height >= 0.001:
        # 分离武器可能比人物更高。只有独立的明显越界部件从检测中排除，蒙皮仍处理全部原网格。
        margin = 0.16 * dominant_height
        lower = dominant.min(axis=0) - margin
        upper = dominant.max(axis=0) + margin
        close = [part for part in parts if (part.min(axis=0) >= lower).all()
                 and (part.max(axis=0) <= upper).all()]
        if len(close) < len(parts):
            candidate = np.concatenate(close)
            candidate_height = float(np.ptp(candidate[:, 2]))
            try:
                estimate_landmarks(candidate, candidate_height)
            except ValueError:
                pass  # 分件角色的最大网格可能只有躯干，应再尝试用完整模型检测。
            else:
                return candidate, candidate_height, ["超出主体轮廓的独立部件未参与关节估计"]
    return points, height, []


def estimate_landmarks(points, height):
    """使用臂展主轴和脚尖方向建立水平基底，再用局部横截面估算关节中心。"""
    elevation = (points[:, 2] - points[:, 2].min()) / height
    upper = points[(elevation > 0.45) & (elevation < 0.88), :2]
    if len(upper) < 4:
        raise ValueError("无法辨识站立的人形上身，请使用 T 姿态或 A 姿态")
    spread, axes = np.linalg.eigh(np.cov(upper.T))
    across = axes[:, 1]
    if across[0] < 0:
        across = -across
    forward = np.array((across[1], -across[0]))
    projected = points[:, :2] @ forward
    sole = projected[elevation < 0.035]
    heel = projected[(elevation > 0.045) & (elevation < 0.105)]
    facing = np.median(sole) - np.median(heel) if len(sole) and len(heel) else 0
    warnings = []
    if abs(facing) < 0.006 * height or spread[1] < 1.4 * spread[0]:
        warnings.append("正面朝向依模型坐标估算，请核对左右标记")
    if facing < -0.006 * height:
        across = -across
        forward = -forward
    base = points[:, 2].min()
    origin = np.median(upper, axis=0)
    projected_xy = (points[:, :2] - origin) @ np.column_stack((across, forward))
    x, y = projected_xy.T / height
    # 以头部横截面找中线，避免单边残缺或较宽的手臂把中线拉向一侧。
    core = elevation > 0.88
    center = (np.quantile(x[core], 0.14) + np.quantile(x[core], 0.86)) * 0.5 if core.any() else 0
    x = x - center
    center_xy = origin + across * (center * height)
    arms = (elevation > 0.42) & (elevation < 0.9)
    if np.ptp(x[arms]) < 0.35 or np.ptp(x) > 1.9:
        raise ValueError("未检测到分开的双臂，请使用站立的 T 姿态或 A 姿态")

    def sample(mask, values, fallback):
        return float(np.median(values[mask])) if mask.any() else fallback

    def depth(at, lateral=0.11):
        return sample((np.abs(elevation - at) < 0.038) & (np.abs(x) < lateral), y, 0.0)

    head, neck, hips = 0.86, 0.81, 0.60
    markers = {
        "head": (0, depth(head), head),
        "neck": (0, depth(neck), neck),
        "hips": (0, depth(hips), hips),
    }
    wings = [x[arms & (x * sign > 0.08)] * sign for sign in (1, -1)]
    if any(len(wing) < 4 for wing in wings):
        raise ValueError("未检测到两侧肢体，请确认人物双臂张开且网格完整")
    spans = [float(np.quantile(wing, 0.99)) for wing in wings]
    if min(spans) < 0.16 or min(spans) < max(spans) * 0.52:
        raise ValueError("两侧臂展差异过大或一侧缺少手臂，请检查模型是否为完整 T/A 姿态")
    for side, sign, span in zip(SIDES, (1, -1), spans):
        lateral = x * sign
        shoulder = float(np.clip(span * 0.21, 0.055, 0.13))
        wrist = span * 0.84
        elbow = shoulder + (wrist - shoulder) * 0.53
        shoulder_height = 0.80
        markers[f"{side}_shoulder"] = (sign * shoulder, depth(shoulder_height), shoulder_height)
        for name, at in (("elbow", elbow), ("wrist", wrist)):
            section = arms & (np.abs(lateral - at) < max(0.020, span * 0.055))
            z = sample(section, elevation, 0.73)
            markers[f"{side}_{name}"] = (sign * at, sample(section, y, depth(0.73)), z)

        hip_x, hip_z = 0.048, 0.56
        ankle_section = (lateral > 0) & (elevation > 0.035) & (elevation < 0.082)
        ankle_x = float(np.clip(sample(ankle_section, lateral, 0.053), 0.025, 0.25))
        ankle_y = sample(ankle_section, y, depth(0.065))
        knee_z = 0.31
        leg_axis = (hip_x + ankle_x) * 0.5
        knee_section = (np.abs(lateral - leg_axis) < 0.045) & (np.abs(elevation - knee_z) < 0.028)
        knee_x = sample(knee_section, lateral, leg_axis)
        knee_y = sample(knee_section, y, (depth(hips) + ankle_y) * 0.5)
        toe_section = (np.abs(lateral - ankle_x) < 0.065) & (elevation < 0.055)
        toe_y = float(np.quantile(y[toe_section], 0.8)) if toe_section.any() else ankle_y + 0.045
        toe_y = float(np.clip(toe_y, ankle_y + 0.012, ankle_y + 0.085))
        markers[f"{side}_hip"] = (sign * hip_x, depth(hips), hip_z)
        markers[f"{side}_knee"] = (sign * knee_x, knee_y, knee_z)
        markers[f"{side}_ankle"] = (sign * ankle_x, ankle_y, 0.055)
        markers[f"{side}_toe"] = (sign * ankle_x, toe_y, 0.028)

    def to_gltf(point):
        lx, ly, z = point
        bx, by = center_xy + height * (across * lx + forward * ly)
        return [float(bx), float(base + z * height), float(-by)]

    return {name: to_gltf(markers[name]) for name in MARKERS}, warnings


def confirmed_landmarks(values, points, height):
    if not isinstance(values, dict) or set(values) != set(MARKERS):
        raise ValueError("请确认全部 17 个人形关节点")
    lower = points.min(axis=0) - height * 0.15
    upper = points.max(axis=0) + height * 0.15
    markers = {}
    for name, value in values.items():
        if not isinstance(value, list) or len(value) != 3:
            raise ValueError(f"关节点 {name} 缺少三维坐标")
        x, y, z = map(float, value)
        point = (x, -z, y)  # GLB Y-up 转换为 Blender Z-up，与预览使用同一世界坐标。
        if not all(math.isfinite(v) and lower[i] <= v <= upper[i] for i, v in enumerate(point)):
            raise ValueError(f"关节点 {name} 超出模型附近，请检查标记位置")
        markers[name] = Vector(point)

    def check_chain(names):
        for a, b in zip(names, names[1:]):
            if (markers[a] - markers[b]).length < 0.009 * height:
                raise ValueError(f"关节点 {a} 和 {b} 太接近，请拉开距离")

    check_chain(("hips", "neck", "head"))
    if not markers["head"].z > markers["neck"].z > markers["hips"].z:
        raise ValueError("请从上至下标记头部、颈部和胯部")
    for side in SIDES:
        for chain in (("shoulder", "elbow", "wrist"), ("hip", "knee", "ankle", "toe")):
            check_chain(tuple(f"{side}_{name}" for name in chain))
        if not markers[f"{side}_hip"].z > markers[f"{side}_knee"].z > markers[f"{side}_ankle"].z:
            raise ValueError("请从上至下标记髋、膝和脚踝")
    for part in ("shoulder", "hip", "ankle"):
        check_chain((f"left_{part}", f"right_{part}"))
    return markers


def load_reference(path):
    document = glb_document(path)
    if not document.get("animations") or not document.get("skins"):
        raise ValueError("参考动作 GLB 需要携带 Mixamo 人形骨架和动画")
    if len(document.get("nodes", [])) > 512:
        raise ValueError("参考动作的节点数量超过预算")
    objects = import_glb(path)
    rigs = [obj for obj in objects if obj.type == "ARMATURE" and PREFIX + "Hips" in obj.data.bones]
    if len(rigs) != 1 or len(rigs[0].data.bones) > MAX_JOINTS:
        raise ValueError("请选择只包含一套 Mixamo 人形骨架的参考动作")
    rig = rigs[0]
    rig.animation_data_clear()
    for bone in rig.pose.bones:
        bone.custom_shape = None
        bone.matrix_basis = Matrix.Identity(4)
    bpy.context.view_layer.update()
    original_matrix = rig.matrix_world.copy()
    rig.parent = None
    rig.matrix_world = original_matrix
    for obj in objects - {rig}:
        bpy.data.objects.remove(obj, do_unlink=True)
    # 输出根与动作源根不同，旧 RPG 路径也能识别外部动作并保留目标骨长。
    rig.name = "RiggedCharacter"
    return rig


def fit_bones(rig, markers, points, height):
    bones = {bone.name.removeprefix(PREFIX): bone for bone in rig.data.bones}
    if len(bones) != len(rig.data.bones):
        raise ValueError("参考动作包含不能唯一对应的骨骼名称")
    reference_world = rig.matrix_world.copy()
    source = {name: reference_world @ bone.head_local for name, bone in bones.items()}
    directions = {
        name: reference_world.to_3x3() @ (bone.tail_local - bone.head_local)
        for name, bone in bones.items()
    }
    roll_axes = {
        name: (reference_world @ bone.matrix_local).to_3x3().col[2].normalized()
        for name, bone in bones.items()
    }
    heads = {"Hips": markers["hips"], "Neck": markers["neck"], "Head": markers["head"]}
    limb = {
        "Arm": "shoulder", "ForeArm": "elbow", "Hand": "wrist",
        "UpLeg": "hip", "Leg": "knee", "Foot": "ankle", "ToeBase": "toe",
    }
    for side in SIDES:
        for name, marker in limb.items():
            heads[side.capitalize() + name] = markers[f"{side}_{marker}"]
    required = set(heads) | {"Spine", "Spine1", "Spine2", "LeftShoulder", "RightShoulder"}
    missing = sorted(name for name in required if PREFIX + name not in rig.data.bones)
    if missing:
        raise ValueError(f"参考动作骨架缺少关节 {', '.join(missing)}")

    def horizontal(vec):
        return Vector((vec.x, vec.y, 0)).normalized()

    old_side = horizontal(source["LeftArm"] - source["RightArm"])
    new_side = horizontal(heads["LeftArm"] - heads["RightArm"])
    if old_side.length < 0.5 or new_side.length < 0.5:
        raise ValueError("参考骨架或当前标记没有可用的左右肩方向")
    yaw = Quaternion((0, 0, 1), math.atan2(old_side.cross(new_side).z, old_side.dot(new_side)))
    old_torso = source["Neck"] - source["Hips"]
    if old_torso.length < 1e-6:
        raise ValueError("参考骨架的躯干长度无效")
    ratio = (heads["Neck"] - heads["Hips"]).length / old_torso.length
    for name, fraction in zip(("Spine", "Spine1", "Spine2"), (0.24, 0.53, 0.78)):
        heads[name] = heads["Hips"].lerp(heads["Neck"], fraction)
    if "HeadTop_End" in source:
        nearby = np.linalg.norm(points[:, :2] - np.array(heads["Head"][:2]), axis=1) < 0.11 * height
        crown = float(points[nearby, 2].max()) if nearby.any() else float(points[:, 2].max())
        heads["HeadTop_End"] = heads["Head"].copy()
        heads["HeadTop_End"].z = max(crown - 0.015 * height, heads["Head"].z + 0.04 * height)

    main_child = {
        "Hips": "Spine", "Spine": "Spine1", "Spine1": "Spine2", "Spine2": "Neck",
        "Neck": "Head", "Head": "HeadTop_End",
    }
    for side in ("Left", "Right"):
        heads[side + "Shoulder"] = heads["Neck"].lerp(heads[side + "Arm"], 0.45)
        names = (
            ("Shoulder", "Arm", "ForeArm", "Hand", "HandMiddle1"),
            ("UpLeg", "Leg", "Foot", "ToeBase", "Toe_End"),
        )
        for chain in names:
            main_child.update({side + a: side + b for a, b in zip(chain, chain[1:])})
        wrist, forearm = side + "Hand", side + "ForeArm"
        old_dir = source[wrist] - source[forearm]
        new_dir = heads[wrist] - heads[forearm]
        hand_rotation = (yaw @ old_dir).rotation_difference(new_dir) @ yaw
        # 手指使用参考骨架的局部形状，并沿当前前臂朝向调整。由腕端网格估计手长。
        hand_names = [bone.name.removeprefix(PREFIX) for bone in bones[wrist].children_recursive]
        if hand_names:
            hand_length = max((source[name] - source[wrist]).length for name in hand_names)
            forward = np.asarray(new_dir.normalized())
            offsets = points - np.asarray(heads[wrist])
            along = offsets @ forward
            lateral = np.linalg.norm(offsets - along[:, None] * forward, axis=1)
            candidates = along[(along > 0) & (along < 0.22 * height) & (lateral < 0.055 * height)]
            if hand_length > 1e-6 and len(candidates) >= 4:
                hand_scale = float(np.clip(np.quantile(candidates, 0.92) / hand_length, ratio * 0.4, ratio * 2))
            else:
                hand_scale = ratio
            for name in hand_names:
                heads[name] = heads[wrist] + hand_rotation @ (source[name] - source[wrist]) * hand_scale
        toe, toe_end, foot = (side + part for part in ("ToeBase", "Toe_End", "Foot"))
        if toe_end in source:
            heads[toe_end] = heads[toe] + horizontal(heads[toe] - heads[foot]) * (height * 0.026)

    # 末端与非主链辅助骨沿父链迁移。保留原命名与父子层级，不以骨骼下标对应动作。
    def fitted_head(name):
        if name not in heads:
            parent = bones[name].parent
            if parent is None:
                heads[name] = heads["Hips"] + yaw @ (source[name] - source["Hips"]) * ratio
            else:
                parent_name = parent.name.removeprefix(PREFIX)
                heads[name] = fitted_head(parent_name) + yaw @ (source[name] - source[parent_name]) * ratio
        return heads[name]

    for name in bones:
        fitted_head(name)
    tails = {}
    rolls = {}
    for name, bone in bones.items():
        child = main_child.get(name)
        if child not in heads and len(bone.children) == 1:
            child = bone.children[0].name.removeprefix(PREFIX)
        fallback = yaw @ directions[name]
        target = heads[child] - heads[name] if child in heads else fallback * ratio
        if target.length < height * 0.003:
            target = (fallback.normalized() if fallback.length > 1e-7 else Vector((0, 0, 1))) * (height * 0.008)
        tails[name] = heads[name] + target
        rolls[name] = fallback.rotation_difference(target) @ (yaw @ roll_axes[name])

    select_objects((rig,), rig)
    rig.matrix_world = Matrix.Identity(4)
    bpy.ops.object.mode_set(mode="EDIT")
    try:
        # 先断开连接锁定，否则 Blender 会用父骨末端覆盖已确认的分支位置。
        for bone in rig.data.edit_bones:
            bone.use_connect = False
        for bone in rig.data.edit_bones:
            name = bone.name.removeprefix(PREFIX)
            bone.head = heads[name]
            bone.tail = tails[name]
            bone.align_roll(rolls[name])
    finally:
        bpy.ops.object.mode_set(mode="OBJECT")
    bpy.context.view_layer.update()


def fully_weighted(mesh, rig):
    valid = {group.index for group in mesh.vertex_groups if group.name in rig.data.bones}
    return all(
        any(weight.group in valid and math.isfinite(weight.weight) and weight.weight > 1e-7
            for weight in vertex.groups)
        for vertex in mesh.data.vertices
    )


def auto_weights(meshes, rig):
    select_objects([*meshes, rig], rig)
    try:
        outcome = bpy.ops.object.parent_set(type="ARMATURE_AUTO", keep_transform=True)
    except RuntimeError:
        return False
    # 骨骼热权重失败时 Blender 仍可能返回 FINISHED，必须核对实际顶点。
    return outcome == {"FINISHED"} and all(fully_weighted(mesh, rig) for mesh in meshes)


def weights_from_proxy(meshes, rig, height):
    proxies = []
    for mesh in meshes:
        data = mesh.data.copy()
        data.transform(mesh.matrix_world)
        proxy = bpy.data.objects.new("SkinningProxy", data)
        bpy.context.collection.objects.link(proxy)
        proxies.append(proxy)
    proxy = proxies[0]
    select_objects(proxies, proxy)
    if len(proxies) > 1:
        bpy.ops.object.join()
    proxy.data.remesh_voxel_size = height / 240.0
    if bpy.ops.object.voxel_remesh() != {"FINISHED"} or not proxy.data.polygons:
        raise RuntimeError("无法建立连续蒙皮代理，请清理模型重叠面或破损拓扑")
    if not auto_weights([proxy], rig):
        raise RuntimeError("代理网格也无法计算完整权重，请检查关节点是否位于人体内部")
    # 只把最近表面插值的权重传回原网格。原网格、材质与 UV 不参与体素重建。
    for mesh in meshes:
        world = mesh.matrix_world.copy()
        for modifier in list(mesh.modifiers):
            if modifier.type == "ARMATURE" and modifier.object == rig:
                mesh.modifiers.remove(modifier)
        mesh.vertex_groups.clear()
        for group in proxy.vertex_groups:
            mesh.vertex_groups.new(name=group.name)
        transfer = mesh.modifiers.new("ProxyWeights", "DATA_TRANSFER")
        transfer.object = proxy
        transfer.use_vert_data = True
        transfer.data_types_verts = {"VGROUP_WEIGHTS"}
        transfer.vert_mapping = "POLYINTERP_NEAREST"
        transfer.layers_vgroup_select_src = "ALL"
        transfer.layers_vgroup_select_dst = "NAME"
        select_objects((mesh,), mesh)
        if bpy.ops.object.modifier_apply(modifier=transfer.name) != {"FINISHED"}:
            raise RuntimeError(f"代理权重未能传回网格 {mesh.name}")
        mesh.parent = rig
        mesh.matrix_world = world
        mesh.modifiers.new("CharacterSkin", "ARMATURE").object = rig
    bpy.data.objects.remove(proxy, do_unlink=True)


def generate_weights(meshes, rig, height):
    warnings = []
    # 骨热求解只用负责变形的骨骼，末端提示骨仍会按原名称导出。
    terminal = [
        bone for bone in rig.data.bones
        if bone.use_deform and not bone.children and ("_End" in bone.name or bone.name.endswith("4"))
    ]
    for bone in terminal:
        bone.use_deform = False
    try:
        if not auto_weights(meshes, rig):
            weights_from_proxy(meshes, rig, height)
            warnings.append("原网格的骨热权重不完整，已由体素代理计算并传回原网格")
    finally:
        for bone in terminal:
            bone.use_deform = True

    for mesh in meshes:
        select_objects((mesh,), mesh)
        bpy.ops.object.vertex_group_limit_total(limit=4)
        bpy.ops.object.vertex_group_normalize_all(lock_active=False)
        if not fully_weighted(mesh, rig):
            raise RuntimeError(f"网格 {mesh.name} 仍含未分配权重的顶点")
    return warnings


def save_bound_model(output_blend, plugin_path, joint_names):
    output_glb = output_blend.with_suffix(".glb")
    spec = importlib.util.spec_from_file_location("feathrise_rigging_export", plugin_path)
    if spec is None or spec.loader is None:
        raise ValueError("无法加载项目的 Blender 保存导出插件")
    plugin = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(plugin)
    plugin.register()
    try:
        # Blender 的 glTF 导出器会在目录尾部添加正斜杠。Win32 扩展路径不接受这种写法，
        # 所以仅为保存接口恢复同一路径的普通盘符或 UNC 拼写，任务身份和文件核验仍用原路径。
        destination = str(output_blend)
        drive = output_blend.drive
        if drive.upper().startswith("\\\\?\\UNC\\"):
            destination = "\\\\" + destination[8:]
        elif drive.startswith("\\\\?\\") and len(drive) == 6 and drive.endswith(":"):
            destination = destination[4:]
        result = bpy.ops.wm.save_as_mainfile(filepath=destination, check_existing=False)
    finally:
        plugin.unregister()
    # 导出插件可能记录失败后返回。只有本次独占的 GLB 实际生成且骨架与权重正确才回执。
    if result != {"FINISHED"} or not output_glb.is_file():
        raise RuntimeError("保存完成后未生成可用 GLB，请检查导出插件日志")
    document = glb_document(output_glb)
    skins = document.get("skins", [])
    exported_joints = {
        document["nodes"][node]["name"] for skin in skins for node in skin["joints"]
    }
    primitives = [p for mesh in document.get("meshes", []) for p in mesh["primitives"]]
    if (exported_joints != joint_names or not primitives or document.get("animations")
            or any(not {"JOINTS_0", "WEIGHTS_0"} <= p["attributes"].keys() for p in primitives)):
        raise RuntimeError("导出的 GLB 缺少蒙皮或关节，或包含不应写入的动画")
    print(f"绑骨 GLB 已核验：{output_glb}", flush=True)


def run(request_path):
    if not request_path.is_absolute() or request_path.stat().st_size > 65_536:
        raise ValueError("无效的自动绑骨请求路径或大小")
    stage = request_path.resolve().parent
    request = json.loads(request_path.read_text(encoding="utf-8"))
    operation = request["operation"]
    if operation not in ("inspect", "bind"):
        raise ValueError("未知的自动绑骨操作")
    model = staged_path(stage, request["input"], ".glb")
    report = staged_path(stage, request["report"], ".json", output=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    meshes = load_model(model)
    parts = mesh_points(meshes)
    points, height, warnings = body_points(parts)
    if operation == "inspect":
        landmarks, estimate_warnings = estimate_landmarks(points, height)
        result = {"landmarks": landmarks, "warnings": warnings + estimate_warnings}
    else:
        reference = staged_path(stage, request["reference"], ".glb")
        output = staged_path(stage, request["output_blend"], ".blend", output=True)
        staged_path(stage, output.with_suffix(".glb"), ".glb", output=True)
        plugin = Path(request["export_plugin"])
        if not plugin.is_absolute() or plugin.suffix != ".py" or not plugin.is_file():
            raise ValueError("缺少项目的 Blender 保存时自动导出 GLB 插件")
        markers = confirmed_landmarks(request["landmarks"], points, height)
        rig = load_reference(reference)
        fit_bones(rig, markers, points, height)
        warnings.extend(generate_weights(meshes, rig, height))
        names = {bone.name for bone in rig.data.bones}
        save_bound_model(output, plugin, names)
        result = {"warnings": warnings, "joints": len(names), "vertices": sum(map(len, parts))}
    with report.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, allow_nan=False)


if __name__ == "__main__":
    try:
        run(Path(sys.argv[sys.argv.index("--") + 1]))
    except Exception as error:
        print(f"自动绑骨失败：{error}", file=sys.stderr, flush=True)
        raise
