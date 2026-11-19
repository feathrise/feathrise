# Blender 保存时自动导出 GLB 插件。用途：保存任意目录下的 .blend 时，在同一目录自动导出同名 .glb。
# 使用方法：在 Blender 的“编辑 -> 偏好设置 -> 插件 -> 从磁盘安装”中选择本文件并启用，然后正常保存 .blend。
# 项目后台生成器可临时设置显式目标路径，但普通交互保存不会再执行 assets/raw 到 assets/gltf 的隐式镜像。
# 依赖：仅使用 Blender 内置 bpy 和 Python 标准库；需要 Blender 已启用内置 glTF 2.0 导出能力。

from __future__ import annotations  # 启用延迟解析类型注解，兼容 Blender 自带 Python 的类型提示。

import os  # 提供路径协议与原子替换文件能力，避免 Bevy 读取尚未写完的 GLB。
from pathlib import Path  # 使用跨平台路径对象推导 .blend 同目录下的同名 .glb。

import bpy  # 调用 Blender 保存回调与 glTF 2.0 导出操作。
from bpy.app.handlers import persistent  # 保证打开其他 .blend 文件后保存回调仍然有效。


bl_info = {  # 声明 Blender 插件元数据，供插件管理界面识别和显示。
    "name": "Bevy 保存时自动导出 GLB",  # 插件界面显示名称。
    "author": "bevy_learn5_3d",  # 标记插件属于当前 Bevy 项目工作流。
    "version": (1, 1, 1),  # 1.1起同目录导出，1.1.1补齐旧版插件驻留时的热升级替换。
    "blender": (4, 0, 0),  # 支持 Blender 4.0 及以上版本。
    "location": "保存 .blend 时自动运行",  # 说明插件没有单独操作面板。
    "description": "保存 Blender 文件时在同一目录自动导出同名 GLB",  # 插件功能摘要。
    "category": "Import-Export",  # 将插件归类到导入导出工具。
}  # 结束插件元数据定义。


_export_in_progress = False  # 防重入锁，避免同一次保存意外触发并行导出。
_EXPORT_PATH_OVERRIDE_KEY = "bevy_learn5_glb_export_path_override"  # 后台生成器跨插件模块共享的临时显式目标键。
_HANDLER_NAME = "_export_glb_after_save"  # 跨安装模块和后台临时模块识别本项目保存回调的稳定名称。
_HANDLER_API_MARKER = "_bevy_learn5_glb_handler_api"  # 区分支持同目录规则的新回调与仍驻留的1.0旧回调。
_HANDLER_API_VERSION = 2  # 第二代回调同时具备同目录默认输出和显式后台目标接口。
_registered_handler: object | None = None  # 记录本模块实际注册或复用的回调，供注销时判断所有权。
_owns_registered_handler = False  # 只有本模块追加的回调才能由本模块注销，避免移除用户已启用的插件。


def set_glb_export_path_override(target_path: str | os.PathLike[str]) -> None:  # 供项目后台生成器临时指定唯一GLB运行时目标。
    resolved_target = Path(target_path).expanduser().resolve()  # 立即冻结绝对路径，避免保存期间工作目录变化。
    if resolved_target.suffix.casefold() != ".glb":  # 显式目标同样只允许真正的GLB后缀。
        raise ValueError(f"GLB显式导出目标必须以.glb结尾：{resolved_target}")  # 在写文件前暴露生成脚本配置错误。
    bpy.app.driver_namespace[_EXPORT_PATH_OVERRIDE_KEY] = str(resolved_target)  # 使用Blender进程共享字典兼容已安装或临时加载的插件模块。


def clear_glb_export_path_override() -> None:  # 后台生成器必须在保存完成或失败后清理临时目标。
    bpy.app.driver_namespace.pop(_EXPORT_PATH_OVERRIDE_KEY, None)  # 幂等移除，防止后续人工保存沿用旧运行时路径。


def _configured_glb_path_override() -> Path | None:  # 读取当前Blender进程内可选的显式导出目标。
    raw_target = bpy.app.driver_namespace.get(_EXPORT_PATH_OVERRIDE_KEY)  # 不消费目标，使重复注册的同插件回调仍指向同一文件。
    if raw_target is None:  # 普通交互保存不会设置该内部键。
        return None  # 交回同目录默认规则。
    try:  # 把外部生成器传入的路径协议对象规范化为Path。
        target_path = Path(os.fspath(raw_target)).expanduser().resolve()  # 支持字符串和标准PathLike值。
    except TypeError as error:  # 非路径对象表示生成器错误使用了内部接口。
        raise ValueError(f"GLB显式导出目标不是有效路径：{raw_target!r}") from error  # 保留原类型错误作为诊断链。
    if target_path.suffix.casefold() != ".glb":  # 即使外部直接改写共享字典也执行相同安全校验。
        raise ValueError(f"GLB显式导出目标必须以.glb结尾：{target_path}")  # 禁止覆盖Blend或其他资源类型。
    return target_path  # 返回经过规范化和后缀校验的唯一目标。


def _derive_glb_path(blend_path: Path) -> Path | None:  # 根据已保存 .blend 路径推导本次唯一 GLB 目标。
    if blend_path.suffix.casefold() != ".blend":  # save_post异常传入非Blend路径时不猜测输出。
        return None  # 防止仅凭同名规则覆盖其他类型文件。
    configured_target = _configured_glb_path_override()  # 后台生成器可显式保留既有运行时资产位置。
    if configured_target is not None:  # 显式目标只影响生成器主动包围的当前保存操作。
        return configured_target  # 不额外生成同目录副本，保证一次保存只有一个正式GLB输出。
    return blend_path.with_suffix(".glb")  # 普通用户保存时在Blend旁生成同名GLB。


def _saved_blend_path(callback_args: tuple[object, ...]) -> Path | None:  # 优先从save_post事件取得本次实际保存或保存副本路径。
    callback_path = callback_args[0] if callback_args else None  # Blender 5.x把刚写入的filepath作为第一个回调参数。
    if not isinstance(callback_path, (str, os.PathLike)) or not os.fspath(callback_path):  # 兼容未传路径的旧版或测试回调。
        callback_path = bpy.data.filepath  # 只有事件没有有效路径时才回退当前主Blend路径。
    if not isinstance(callback_path, (str, os.PathLike)) or not os.fspath(callback_path):  # 尚未得到任何已保存文件身份时安全跳过。
        return None  # 未命名内存场景不创建无法归属的GLB。
    return Path(callback_path).expanduser().resolve()  # 规范化为绝对路径供同目录推导与日志使用。


def _export_current_scene(target_path: Path) -> None:  # 将当前 Blender 场景安全导出到指定 GLB 文件。
    target_path.parent.mkdir(parents=True, exist_ok=True)  # 同目录通常已存在，显式后台目标仍可安全补建父目录。
    temporary_path = target_path.with_name(f".{target_path.stem}.{os.getpid()}.exporting.glb")  # 进程号隔离并发Blender实例，同时避免热加载读到半成品。
    try:  # 确保导出成功后再替换正式文件，并在失败时清理临时文件。
        result = bpy.ops.export_scene.gltf(  # 调用 Blender 内置 glTF 2.0 导出器。
            filepath=str(temporary_path),  # 先写入同目录临时文件，保证最终替换可以原子完成。
            check_existing=False,  # 自动化导出时不弹出覆盖确认框。
            export_format="GLB",  # 将模型、材质和贴图打包为单个二进制 GLB 文件。
            export_apply=True,  # 导出修改器结果，使 Bevy 读取到 Blender 视口中的最终几何体。
            export_cameras=False,  # 排除 Blender 预览相机，游戏相机继续由 Bevy 管理。
            export_lights=False,  # 排除 Blender 预览灯光，运行时光照继续由 Bevy 管理。
            export_extras=True,  # 导出自定义属性，供 Bevy 后续读取出生点、传送点等关卡标记。
        )  # 完成 GLB 导出操作。
        if "FINISHED" not in result:  # 检查 Blender 操作是否真正完成。
            raise RuntimeError(f"glTF 导出器返回未完成状态：{result}")  # 将取消或失败状态转换成明确错误。
        os.replace(temporary_path, target_path)  # 原子覆盖正式 GLB，让 Bevy 只看到完整的新文件。
        print(f"[Bevy GLB 自动导出] 已更新：{target_path}")  # 在 Blender 控制台输出成功路径。
    finally:  # 无论导出成功还是异常都执行临时文件清理。
        if temporary_path.exists():  # 仅在临时文件仍然存在时执行删除。
            temporary_path.unlink()  # 清理由失败或中断遗留的临时 GLB。


@persistent  # 打开新的 Blender 文件后继续保留自动导出回调。
def _export_glb_after_save(*callback_args: object) -> None:  # 响应 Blender save_post，并兼容不同版本的回调参数。
    global _export_in_progress  # 修改模块级防重入状态。
    if _export_in_progress:  # 已有导出任务运行时不再次进入。
        return  # 防止重复导出和目标文件竞争。
    _export_in_progress = True  # 加锁，直到本次导出完成或失败。
    try:  # 路径解析、目标校验与场景导出统一捕获，避免save_post异常逃入Blender保存链。
        blend_path = _saved_blend_path(callback_args)  # Save Copy优先使用事件路径，普通保存必要时回退当前文件路径。
        if blend_path is None:  # 未命名内存场景或异常回调没有可用保存路径。
            print("[Bevy GLB 自动导出] 已跳过，本次保存没有有效的 .blend 路径")  # 给出可诊断的跳过原因。
            return  # 不创建无法与Blend对应的游离GLB，finally仍释放防重入锁。
        target_path = _derive_glb_path(blend_path)  # 默认推导同目录目标，并允许后台生成器显式覆盖。
        if target_path is None:  # 回调路径不是可识别的Blend文件。
            print(f"[Bevy GLB 自动导出] 已跳过，保存路径不是 .blend 文件：{blend_path}")  # 明确拒绝非Blend扩展名。
            return  # 不影响普通Blender文件保存，finally仍恢复插件状态。
        _export_current_scene(target_path)  # 导出当前完整场景并安全覆盖目标 GLB。
    except Exception as error:  # 统一处理路径、导出器和文件替换错误。
        print(f"[Bevy GLB 自动导出] 导出失败：{error}")  # 在 Blender 控制台显示明确错误信息。
    finally:  # 无论成功失败都恢复可导出状态。
        _export_in_progress = False  # 释放防重入锁，允许下一次保存触发导出。


setattr(_export_glb_after_save, _HANDLER_API_MARKER, _HANDLER_API_VERSION)  # 给持久回调附加能力标记，让新版注册能淘汰旧实现。


def _is_project_export_handler(handler: object) -> bool:  # 识别当前版本及1.0遗留的本项目保存回调。
    if getattr(handler, _HANDLER_API_MARKER, None) is not None:  # 新版及未来版本都使用本项目专属属性标明身份。
        return True  # 交给接口版本检查决定复用还是升级替换。
    if getattr(handler, "__name__", "") != _HANDLER_NAME:  # 1.0旧版虽然没有标记，但使用稳定函数名。
        return False  # 函数名不同即可确认不是本项目回调。
    handler_globals = getattr(handler, "__globals__", {})  # 旧版身份必须再由定义它的模块内容共同证明。
    handler_info = handler_globals.get("bl_info")  # 读取Blender插件原有的名称和作者元数据。
    return (  # 只有完整匹配旧项目模块特征时才允许热升级移除。
        isinstance(handler_info, dict)
        and handler_info.get("name") == bl_info["name"]
        and handler_info.get("author") == bl_info["author"]
        and callable(handler_globals.get("_derive_glb_path"))
        and callable(handler_globals.get("_export_current_scene"))
    )  # 仅函数名碰巧相同的第三方save_post处理器会原样保留。


def _is_current_export_handler(handler: object) -> bool:  # 验证候选回调确实提供第二代完整接口。
    if getattr(handler, _HANDLER_API_MARKER, None) != _HANDLER_API_VERSION:  # 旧版或未来不兼容接口不能直接复用。
        return False  # 交给register替换，防止继续执行过期路径规则。
    handler_globals = getattr(handler, "__globals__", {})  # 从真实处理器模块核对后台目标设置和清理能力。
    return (  # 标记和两个公开接口同时满足才认定为当前兼容实现。
        callable(handler_globals.get("set_glb_export_path_override"))
        and callable(handler_globals.get("clear_glb_export_path_override"))
    )  # 避免只因函数同名或伪造单一属性而复用错误回调。


def register() -> None:  # Blender 启用插件时注册保存完成回调。
    global _registered_handler, _owns_registered_handler  # 更新本模块后续注销所需的真实回调和所有权。
    handlers = bpy.app.handlers.save_post  # 保存Blender的可变处理器列表引用，确保本次收敛操作作用于同一列表。
    project_handlers = [handler for handler in tuple(handlers) if _is_project_export_handler(handler)]  # 快照全部新旧项目回调。
    current_handlers = [handler for handler in project_handlers if _is_current_export_handler(handler)]  # 只保留接口完整的第二代候选。
    if _export_glb_after_save in current_handlers:  # 同一模块重复调用register时继续使用自己的处理器。
        active_handler = _export_glb_after_save  # 保持本模块对既有注册的所有权。
    elif current_handlers:  # 用户插件或另一个临时模块已经注册了兼容的新回调。
        active_handler = current_handlers[0]  # 复用第一个有效回调，避免一次保存导出两次。
    else:  # 当前会话只有旧回调或尚未启用本项目插件。
        active_handler = _export_glb_after_save  # 由当前1.1.1实现接管保存事件。
    removed_count = 0  # 统计被清除的旧版或重复回调，便于控制台诊断热升级结果。
    kept_active_handler = False  # 即使同一函数对象被重复append，也只允许保留第一次出现的一项。
    for handler in project_handlers:  # 收敛同一会话可能累积的全部项目回调条目。
        if handler is active_handler and not kept_active_handler:  # 唯一选中的兼容回调只保留一个列表条目。
            kept_active_handler = True  # 后续相同函数对象同样视为重复注册并移除。
            continue  # 保持首个有效处理器在save_post中的既有顺序。
        if handler in handlers:  # 快照成员可能因异常注册逻辑重复出现，移除前再次确认。
            handlers.remove(handler)  # 清除1.0旧规则和多余新副本，杜绝跳过或双重导出。
            removed_count += 1  # 记录本次确实移除的回调数量。
    if active_handler not in handlers:  # 没有可复用的新回调时把当前实现接入保存流程。
        handlers.append(active_handler)  # 完成旧版到新版的原地热升级或首次启用。
    _registered_handler = active_handler  # 保存实际生效的唯一处理器身份。
    _owns_registered_handler = active_handler is _export_glb_after_save  # 复用其他模块时不得在unregister中移除它。
    if active_handler is not _export_glb_after_save:  # 当前模块只是借用已经启用的兼容用户插件。
        print(f"[Bevy GLB 自动导出] 已复用当前会话中的新版插件回调，清理重复回调 {removed_count} 个")  # 说明所有权仍归原模块。
    elif removed_count > 0:  # 当前实现替换了旧版或清理了重复注册。
        print(f"[Bevy GLB 自动导出] 已升级并启用同目录导出回调，替换旧版或重复回调 {removed_count} 个")  # 明确热升级已经生效。
    else:  # 干净会话首次启用或同一模块重复注册。
        print("[Bevy GLB 自动导出] 插件已启用")  # 在 Blender 控制台确认注册成功。


def unregister() -> None:  # Blender 禁用插件时移除保存完成回调。
    global _registered_handler, _owns_registered_handler  # 清空本模块的注册身份和回调所有权。
    if _owns_registered_handler and _registered_handler is _export_glb_after_save:  # 只有本模块亲自接入的处理器可以注销。
        if _export_glb_after_save in bpy.app.handlers.save_post:  # 热重载期间处理器可能已被更新模块替换。
            bpy.app.handlers.save_post.remove(_export_glb_after_save)  # 停止本模块拥有的保存后自动导出行为。
    _registered_handler = None  # 注销完成后不再持有其他模块或本模块处理器身份。
    _owns_registered_handler = False  # 后续重复注销保持幂等且不会误删复用的用户插件。
    print("[Bevy GLB 自动导出] 插件已停用")  # 在 Blender 控制台确认注销成功。


if __name__ == "__main__":  # 允许从 Blender 文本编辑器直接运行脚本进行临时安装。
    register()  # 直接运行时立即启用保存后自动导出。
