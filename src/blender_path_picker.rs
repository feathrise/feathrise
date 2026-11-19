//! 在首页和作者窗口共用原生选择器，将选择结果交给现有的作者偏好保存链。

use crate::{folder_picker::FolderPicker, native_project_close::NativeProjectCloseState};
use bevy::{prelude::*, window::{PrimaryWindow, RawHandleWrapper, WindowCloseRequested}};
use feathrise_editor_core::{EditorSettings, SceneEditorSystemSet};
use feathrise_editor_scene::workbench::{BlenderPathPicker, WorkbenchSettingsAccess};
use std::path::{Path, PathBuf};

pub(crate) fn install(app: &mut App) {
    app.init_resource::<BlenderPathPicker>()
        .add_systems(Update, update.after(SceneEditorSystemSet::Input));
}

fn update(
    mut active: Local<Option<FolderPicker>>,
    mut request: ResMut<BlenderPathPicker>,
    mut settings: ResMut<EditorSettings>,
    access: Option<Res<WorkbenchSettingsAccess>>,
    close: Res<NativeProjectCloseState>,
    windows: Query<(Entity, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    mut close_requests: MessageReader<WindowCloseRequested>,
) {
    let window = windows.single().ok();
    let closing = close_requests.read().any(|event| window.is_some_and(|(entity, _)| entity == event.window))
        || close.captures_input();
    let editable = access.is_none_or(|access| access.editable);
    let browse = request.requested;
    if browse {
        request.requested = false;
    }
    if let Some(dialog) = active.as_mut() {
        if !editable {
            // 写权限交给作者后，首页的旧选择不能再改写个人偏好。
            dialog.cancel();
        } else if closing {
            return; // 暂缓取回结果，取消退出后继续处理本次选择。
        }
        if let Some(result) = dialog.poll() {
            *active = None;
            request.busy = false;
            match result {
                Ok(Some(path)) => {
                    let path = PathBuf::from(path);
                    if !path.is_absolute() {
                        request.error = Some("Blender 程序需要使用绝对路径".into());
                    } else if settings.blender_executable.as_ref() != Some(&path) {
                        settings.blender_executable = Some(path);
                    }
                }
                Ok(None) => {}
                Err(error) => request.error = Some(error),
            }
        }
    } else if browse && editable && !closing {
        request.error = None;
        let owner = window.and_then(|(_, raw)| raw).and_then(crate::workspace_window::primary_hwnd).unwrap_or(0);
        let initial = settings.blender_executable.as_deref().filter(|path| path.is_absolute()).and_then(Path::parent)
            .map_or_else(crate::user_preferences::user_home, |path| Ok(path.to_path_buf()));
        match initial.and_then(|path| FolderPicker::start_blender(owner, path)) {
            Ok(dialog) => {
                *active = Some(dialog);
                request.busy = true;
            }
            Err(error) => request.error = Some(error),
        }
    }
}
