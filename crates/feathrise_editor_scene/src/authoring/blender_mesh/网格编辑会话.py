# Feathrise 单对象 Blender 会话。模型只交换标准 GLB，保存使用项目原自动导出插件。
# 用法：blender --disable-autoexec --python-exit-code 1 --python bridge.py -- <会话目录> <输入GLB> [原插件路径]
# 依赖：Blender 4.0+ 内置 glTF 与支持同目录导出的项目插件。

import hashlib
import os
import runpy
import struct
import sys
from pathlib import Path

import addon_utils
import bmesh
import bpy


def load_exporter(configured_path):
    if configured_path is None:
        matches = [
            Path(module.__file__)
            for module in addon_utils.modules()
            if module.bl_info.get("name") == "Bevy 保存时自动导出 GLB"
            and module.bl_info.get("author") == "bevy_learn5_3d"
        ]
        if len(matches) != 1:
            raise RuntimeError("请安装项目的 Blender 保存自动导出插件，或设置 FEATHRISE_BLENDER_TOOLS_DIR。")
        configured_path = matches[0]

    plugin = runpy.run_path(str(configured_path))
    export = plugin.get("_export_glb_after_save")
    saved_path = plugin.get("_saved_blend_path")
    owns_handler = plugin.get("_is_project_export_handler")
    if (not callable(export) or getattr(export, "_bevy_learn5_glb_handler_api", None) != 2
            or not callable(saved_path) or not callable(owns_handler)):
        raise RuntimeError("请使用支持同目录导出的项目 Blender 插件（1.1.1 或兼容版本）。")
    # 只接管项目导出回调。保留正常启动已启用的第三方插件和用户偏好。
    handlers = bpy.app.handlers.save_post
    handlers[:] = [handler for handler in handlers if not owns_handler(handler)]
    return export, saved_path


def import_model(source, builtin_seed):
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
        raise RuntimeError("输入 GLB 没有可建模的网格。")
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


def start_session(directory, source, plugin_path):
    blend_path = directory / "model.blend"
    glb_path = directory / "model.glb"
    if blend_path.exists() or glb_path.exists():
        raise RuntimeError("会话目录已有模型文件。")
    export, saved_path = load_exporter(plugin_path)
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
            # 原导出器可能退出编辑模式。保留用户当前 Blender 建模操作环境。
            if mode != "OBJECT" and active == bpy.context.view_layer.objects.active and active.mode == "OBJECT":
                if bpy.ops.object.mode_set.poll():
                    bpy.ops.object.mode_set(mode=mode)

    handlers.append(saved)
    publish("ready")
    print(f"[Feathrise 建模] 已打开：{blend_path}", flush=True)


if __name__ == "__main__":
    arguments = sys.argv[sys.argv.index("--") + 1 :]
    directory, source = (Path(value).resolve(strict=True) for value in arguments[:2])
    plugin_path = Path(arguments[2]).resolve(strict=True) if len(arguments) == 3 else None
    # 先读取出厂空场景，正常启动加载的用户偏好仍留在进程内。
    if "FINISHED" not in bpy.ops.wm.read_homefile(use_factory_startup=True, use_empty=True, app_template=""):
        raise RuntimeError("Blender 未能准备空白建模场景。")
    window = bpy.context.window or next(iter(bpy.context.window_manager.windows), None)
    with bpy.context.temp_override(window=window):
        start_session(directory, source, plugin_path)
