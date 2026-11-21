# Feathrise 单对象与主场景 Blender 会话。保存均使用项目原自动导出插件。
# 用法：blender --disable-autoexec --python-exit-code 1 --python bridge.py -- <会话目录> <输入GLB> <导出脚本路径> [main_scene <场景ID> <源摘要>]
# 依赖：Blender 4.0+ 内置 glTF 和由编辑器准备的导出脚本。

import hashlib
import json
import os
import runpy
import shutil
import struct
import sys
import uuid
from pathlib import Path

import bmesh
import bpy


def load_exporter(path):
    plugin = runpy.run_path(str(path))
    export = plugin.get("_export_glb_after_save")
    saved_path = plugin.get("_saved_blend_path")
    owns_handler = plugin.get("_is_project_export_handler")
    if (not callable(export) or getattr(export, "_bevy_learn5_glb_handler_api", None) != 2
            or not callable(saved_path) or not callable(owns_handler)):
        raise RuntimeError("请使用支持同目录导出的项目 Blender 插件（1.1.1 或兼容版本）。")
    # 只接管项目导出回调。保留正常启动已启用的第三方插件和用户偏好。
    handlers = bpy.app.handlers.save_post
    handlers[:] = [handler for handler in handlers if not owns_handler(handler)]
    return plugin, export, saved_path


def import_model(source, builtin_seed, require_mesh=True):
    for obj in tuple(bpy.context.scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    if "FINISHED" not in bpy.ops.import_scene.gltf(filepath=str(source)):
        raise RuntimeError(f"Blender 无法导入 GLB：{source}")
    if builtin_seed:
        # 内置种子由场景写入器生成。只取实际模型，去掉该种子的场景标记与编辑配方。
        for obj in tuple(bpy.context.scene.objects):
            if obj.type == "EMPTY":
                bpy.data.objects.remove(obj, do_unlink=True)
            elif obj.type == "MESH":
                for key in tuple(obj.keys()):
                    if key.startswith("feathrise_"):
                        del obj[key]
                # 内置 GLB 会为平面法线拆点。Blender 焊接重合顶点后可编辑相连的表面。
                mesh = bmesh.new()
                mesh.from_mesh(obj.data)
                bmesh.ops.remove_doubles(mesh, verts=list(mesh.verts), dist=0.0000001)
                mesh.to_mesh(obj.data)
                mesh.free()
    view_layer = bpy.context.view_layer
    view_layer.update()  # 刷新移除场景标记后的活动对象列表。
    meshes = [obj for obj in view_layer.objects if obj.type == "MESH" and obj.visible_get()]
    if not meshes:
        if require_mesh:
            raise RuntimeError("输入 GLB 没有可建模的网格。")
        return None
    for obj in bpy.context.selected_objects:
        obj.select_set(False)
    for obj in meshes:
        obj.select_set(True)
    view_layer.objects.active = meshes[0]
    return max(obj.dimensions.length for obj in meshes)


def frame_model(span):
    window = bpy.context.window
    if bpy.app.background or window is None or window.screen is None:
        return
    for area in window.screen.areas:
        if area.type == "VIEW_3D":
            region = next(region for region in area.regions if region.type == "WINDOW")
            view = area.spaces.active
            view.clip_start = min(view.clip_start, max(0.000001, span * 0.01))
            view.clip_end = max(view.clip_end, span * 10)
            with bpy.context.temp_override(window=window, area=area, region=region):
                bpy.ops.view3d.view_selected()


def file_identity(path):
    try:
        stat = path.stat()
        return stat.st_ino, stat.st_mtime_ns, stat.st_size
    except FileNotFoundError:
        return None


def check_export(path, previous):
    current = file_identity(path)
    if current is None or current == previous or current[2] < 20:
        raise RuntimeError(f"保存后未生成新的 GLB：{path}")
    with path.open("rb") as file:
        header = struct.unpack("<4sII", file.read(12))
    if header != (b"glTF", 2, current[2]):
        raise RuntimeError(f"导出的 GLB 无效：{path}")


def initial_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        while chunk := file.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def read_glb_json(path):
    with path.open("rb") as stream:
        stream.seek(12)
        size, kind = struct.unpack("<I4s", stream.read(8))
        if kind != b"JSON":
            raise RuntimeError("主场景 GLB 缺少 JSON 块。")
        return json.loads(stream.read(size)), 20 + size


def main_scene_version(data, scene_id):
    # 完整场景保存必须保留目录登记的根标记，否则禁止覆盖可用的游戏场景。
    nodes = data["nodes"]
    roots = data["scenes"][data.get("scene", 0)]["nodes"]
    extras = [nodes[index].get("extras") or {} for index in roots]
    markers = [values for values in extras if "feathrise_scene_id" in values]
    if len(markers) != 1 or markers[0]["feathrise_scene_id"] != scene_id:
        raise RuntimeError(f"主场景根标记丢失或不属于 {scene_id}。")
    return markers[0]["feathrise_scene_version"]


def action_bindings():
    for obj in bpy.context.scene.objects:
        animation = obj.animation_data
        if animation is None:
            continue
        if animation.action is not None:
            yield None, animation.action
        for track in animation.nla_tracks:
            for strip in track.strips:
                if strip.action is not None:
                    yield track.name, strip.action


def tag_scene_animations(source_count, metadata, marker):
    tracks = getattr(bpy.context.scene, "gltf2_animation_tracks", ())
    if len(tracks) != source_count:
        return set()
    # Blender 的导入页签顺序对应原 GLB。动作携带真实来源，即使之后在 Blender 改名。
    identities = {tracks[index].name: index for index in metadata}
    tagged = set()
    for name, action in action_bindings():
        if name in identities:
            index = identities[name]
            action[marker] = index
            tagged.add(index)
    return tagged


def clip_signature(document, clip):
    # 仅对可核对到稳定摆放的通道按名称匹配，避免把播放设置赋给同名新动作。
    name = clip.get("name")
    if not name:
        return None
    channels = []
    for channel in clip["channels"]:
        target = channel["target"]
        if "node" not in target:
            return None
        node = document["nodes"][target["node"]]
        identity = (node.get("extras") or {}).get("feathrise_id")
        if not identity:
            return None
        channels.append((identity, target["path"]))
    return name, tuple(sorted(channels))


def restore_scene_animations(signatures, result, metadata, marker, tagged):
    if not metadata:
        return False
    live = {action.get(marker) for _, action in action_bindings()}
    expected = metadata.keys() - (tagged - live)
    matched = set()
    changed = False
    for clip in result.get("animations", ()):
        extras = clip.get("extras") or {}
        index = extras.pop(marker, None)
        if index is None:
            candidates = signatures.get(clip_signature(result, clip), [])
            if len(candidates) != 1 or candidates[0] not in expected:
                continue
            index = candidates[0]
        if index not in expected or index in matched:
            raise RuntimeError("无法唯一验证主场景动画片段，原 GLB 已保留。")
        matched.add(index)
        extras.update(metadata[index])
        clip["extras"] = extras
        changed = True
    if expected != matched:
        raise RuntimeError("导出缺少仍在使用的主场景动画，原 GLB 已保留。")
    return changed


def write_glb_json(path, data, old_header_size):
    # 只写已验证的暂存 GLB 的 JSON，BIN 和其它块逐字节复制，原采样器无需换算。
    encoded = json.dumps(data, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    encoded += b" " * (-len(encoded) % 4)
    trailing_size = path.stat().st_size - old_header_size
    temporary = path.with_name(f".{uuid.uuid4().hex}.glb")
    try:
        with path.open("rb") as source, temporary.open("xb") as output:
            output.write(struct.pack("<4sIII4s", b"glTF", 2, 20 + len(encoded) + trailing_size, len(encoded), b"JSON"))
            output.write(encoded)
            source.seek(old_header_size)
            shutil.copyfileobj(source, output, 1024 * 1024)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def open_main_scene(directory, source, plugin_path, scene_id, expected):
    if initial_digest(source) != expected:
        raise RuntimeError("主场景 GLB 已换代，请重新打开场景。")
    original, _ = read_glb_json(source)
    version = main_scene_version(original, scene_id)
    plugin, export, saved_path = load_exporter(plugin_path)
    set_target = plugin.get("set_glb_export_path_override")
    clear_target = plugin.get("clear_glb_export_path_override")
    if not callable(set_target) or not callable(clear_target):
        raise RuntimeError("项目 Blender 插件缺少受检导出入口。")

    # 始终编辑当前主 GLB 的完整副本。工程中的同名 Blend 可能早于当前版本。
    span = import_model(source, builtin_seed=False, require_mesh=False)
    if span is not None:
        frame_model(span)
    animations = original.get("animations", ())
    metadata = {}
    signatures = {}
    for index, clip in enumerate(animations):
        extra = {key: value for key, value in (clip.get("extras") or {}).items() if key.startswith("feathrise_")}
        if extra:
            metadata[index] = extra
        signature = clip_signature(original, clip)
        if signature is not None:
            signatures.setdefault(signature, []).append(index)
    marker = f"feathrise_session_{uuid.uuid4().hex}"
    tagged = tag_scene_animations(len(animations), metadata, marker)
    blend = directory / "model.blend"
    if "FINISHED" not in bpy.ops.wm.save_as_mainfile(filepath=str(blend), check_existing=False):
        raise RuntimeError("无法创建主场景工作副本。")
    if initial_digest(source) != expected:
        raise RuntimeError("导入期间主场景 GLB 已换代，请重新打开场景。")

    @bpy.app.handlers.persistent
    def saved(*args):
        if saved_path(args) != blend:
            return
        nonlocal expected
        candidate = source.with_name(f".{source.stem}.{uuid.uuid4().hex}.glb")
        active = bpy.context.view_layer.objects.active
        mode = active.mode if active else "OBJECT"
        try:
            if initial_digest(source) != expected:
                raise RuntimeError("主场景 GLB 已被其他程序修改。")
            # 只在本次保存临时导出。校验来源身份与输出完整性后替换同名主场景。
            set_target(candidate)
            export(*args)
            check_export(candidate, None)
            document, old_header_size = read_glb_json(candidate)
            if main_scene_version(document, scene_id) != version:
                raise RuntimeError("导出的主场景格式版本不同。")
            if restore_scene_animations(signatures, document, metadata, marker, tagged):
                write_glb_json(candidate, document, old_header_size)
            next_digest = initial_digest(candidate)
            if initial_digest(source) != expected:
                raise RuntimeError("导出期间主场景 GLB 已被其他程序修改。")
            os.replace(candidate, source)
            expected = next_digest
            print(f"[Feathrise 建模] 已更新主场景：{source}", flush=True)
        except Exception as error:
            print(f"[Feathrise 建模] 主场景导出失败：{error}", file=sys.stderr, flush=True)
            if not bpy.app.background and bpy.context.window is not None:
                message = str(error)
                bpy.context.window_manager.popup_menu(
                    lambda menu, _context: menu.layout.label(text=message),
                    title="主场景导出失败", icon="ERROR",
                )
        finally:
            clear_target()
            candidate.unlink(missing_ok=True)
            restore_mode(active, mode)

    bpy.app.handlers.save_post.append(saved)
    print(f"[Feathrise 建模] 已打开主场景：{blend}", flush=True)


def restore_mode(active, mode):
    # 原导出器可能退出编辑模式。保留用户当前 Blender 建模操作环境。
    if mode != "OBJECT" and active == bpy.context.view_layer.objects.active and active.mode == "OBJECT":
        if bpy.ops.object.mode_set.poll():
            bpy.ops.object.mode_set(mode=mode)


def start_session(directory, source, plugin_path):
    blend_path = directory / "model.blend"
    glb_path = directory / "model.glb"
    if blend_path.exists() or glb_path.exists():
        raise RuntimeError("会话目录已有模型文件。")
    _, export, saved_path = load_exporter(plugin_path)
    if hasattr(bpy.app, "use_userpref_skip_save_on_exit"):
        bpy.app.use_userpref_skip_save_on_exit = True
    bpy.context.preferences.filepaths.temporary_directory = str(directory)
    frame_model(import_model(source, source == directory / "input.glb"))

    handlers = bpy.app.handlers.save_post
    handlers.append(export)
    try:
        result = bpy.ops.wm.save_as_mainfile(filepath=str(blend_path), check_existing=False)
    finally:
        handlers.remove(export)
    if "FINISHED" not in result or not blend_path.is_file() or blend_path.stat().st_size == 0:
        raise RuntimeError("Blender 未生成会话文件。")
    check_export(glb_path, None)
    baseline = initial_digest(glb_path)
    glb_path.unlink()  # 初始出件只验证插件就绪，不能进入场景历史。
    revision = 0

    def publish(state, detail=""):
        temporary = directory / ".save.version.tmp"
        temporary.write_text(f"{revision}\n{state}\n{baseline}\n{detail}", encoding="utf-8")
        os.replace(temporary, directory / "save.version")

    @bpy.app.handlers.persistent
    def saved(*args):
        if saved_path(args) != blend_path:
            return  # 仅本会话的保存可以返回场景，作者仍可另存长期源稿。
        nonlocal revision
        revision += 1
        active = bpy.context.view_layer.objects.active
        mode = active.mode if active else "OBJECT"
        try:
            publish("writing")
            previous = file_identity(glb_path)
            export(*args)
            check_export(glb_path, previous)
            publish("ready")
        except Exception as error:
            print(f"[Feathrise 建模] 回传失败：{error}", file=sys.stderr, flush=True)
            publish("error", str(error))
        finally:
            restore_mode(active, mode)

    handlers.append(saved)
    publish("ready")
    print(f"[Feathrise 建模] 已打开：{blend_path}", flush=True)


if __name__ == "__main__":
    arguments = sys.argv[sys.argv.index("--") + 1 :]
    directory, source, plugin_path = (Path(value).resolve(strict=True) for value in arguments[:3])
    # 先读取出厂空场景，正常启动加载的用户偏好仍留在进程内。
    if "FINISHED" not in bpy.ops.wm.read_homefile(use_factory_startup=True, use_empty=True, app_template=""):
        raise RuntimeError("Blender 未能准备空白建模场景。")
    if len(arguments) == 6 and arguments[3] == "main_scene":
        open_main_scene(directory, source, plugin_path, arguments[4], arguments[5])
    else:
        window = bpy.context.window or next(iter(bpy.context.window_manager.windows), None)
        with bpy.context.temp_override(window=window):
            start_session(directory, source, plugin_path)
