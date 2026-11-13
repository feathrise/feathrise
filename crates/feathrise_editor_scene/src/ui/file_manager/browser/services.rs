//! 原搜索/收藏面板在文件桌面中移动挂载，始终保持唯一表单、结果槽和个人记录服务。

use super::super::resources::FileManagerState;
use super::{components::*, resources::BrowserSessions};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::InputFocus,
    prelude::*,
    ui::RelativeCursorPosition,
};
use feathrise_editor_documents_model::search::components::{SearchAction, SearchBody, SearchResults};
use feathrise_editor_ui::EditorButtonClick;

/// 固定栏点击相同服务的工具时取回表单；其它工具仍可与浮窗服务同时使用。
pub(super) fn yield_to_sidebar(
    mut browser: ResMut<BrowserSessions>,
    files: Res<FileManagerState>,
    search: Query<(&SearchAction, &EditorButtonClick), (Changed<EditorButtonClick>, Without<BrowserButton>)>,
    shortcuts: Query<(&crate::ui::asset_shortcuts::ShortcutAction, &EditorButtonClick), Changed<EditorButtonClick>>,
) {
    let Some(host) = browser.service.as_ref() else { return; };
    if files.owns_mouse() || files.suspended {
        return;
    }
    let reclaim = if host.search {
        search.iter().any(|(action, click)| click.is_clicked() && matches!(action, SearchAction::Toggle | SearchAction::MissingAssets))
    } else {
        shortcuts.iter().any(|(action, click)| click.is_clicked() && action.is_toolbar())
    };
    if reclaim { browser.service = None; }
}

pub(super) fn mount(world: &mut World) {
    let closed_search = world
        .get_resource::<BrowserSessions>()
        .and_then(|browser| browser.service.as_ref())
        .is_some_and(|host| {
            host.search
                && !world
                    .get_resource::<crate::ui::asset_preview::AssetSearchState>()
                    .is_some_and(|state| state.is_open())
        });
    if closed_search && let Some(mut browser) = world.get_resource_mut::<BrowserSessions>() {
        browser.service = None;
    }
    let target = world
        .get_resource::<BrowserSessions>()
        .and_then(|browser| browser.service.as_ref())
        .map(|host| (host.window, host.search));
    let valid = target.is_some_and(|(id, _)| {
        world
            .resource::<FileManagerState>()
            .windows
            .iter()
            .any(|window| window.id == id && !window.minimized)
    });
    let target = if valid { target } else { None };
    if !valid && let Some(mut browser) = world.get_resource_mut::<BrowserSessions>() {
        browser.service = None;
    }
    let hosted = target.is_some_and(|(_, search)| search);
    if let Some(mut search) = world.get_resource_mut::<crate::ui::asset_preview::AssetSearchState>()
        && search.floating_hosted() != hosted
    {
        // 输入系统先处理通常的交接；本处收口同帧关闭/最小化，归还时不闪出浮窗搜索正文。
        search.set_floating_host(hosted);
    }
    let hosts: Vec<_> = world
        .query::<(Entity, &BrowserOverlay)>()
        .iter(world)
        .filter(|(_, overlay)| overlay.service)
        .map(|(entity, overlay)| (overlay.window, entity))
        .collect();
    let panels: Vec<_> = world
        .query_filtered::<(
            Entity,
            &ChildOf,
            Option<&ServiceOrigin>,
            Option<&SearchBody>,
        ), Or<(
            With<SearchBody>,
            With<crate::ui::asset_shortcuts::ShortcutRoot>,
        )>>()
        .iter(world)
        .map(|(entity, parent, origin, search)| {
            (
                entity,
                parent.parent(),
                origin.map(|origin| origin.0),
                search.is_some(),
            )
        })
        .collect();
    for (entity, parent, origin, search) in panels {
        let original = origin.unwrap_or(parent);
        if origin.is_none() {
            world.entity_mut(entity).insert(ServiceOrigin(original));
        }
        let wanted = target
            .filter(|(_, wanted_search)| *wanted_search == search)
            .and_then(|(id, _)| {
                hosts
                    .iter()
                    .find(|(window, _)| *window == id)
                    .map(|(_, entity)| *entity)
            })
            .unwrap_or(original);
        if wanted != parent && world.entities().contains(wanted) {
            // 关闭文件窗前先归还面板，避免窗口的递归despawn连同唯一搜索表单一起销毁。
            if search && wanted == original {
                let mut focused = world.get_resource::<InputFocus>().and_then(InputFocus::get);
                for _ in 0..24 {
                    let Some(current) = focused else { break; };
                    if current == entity {
                        world.resource_mut::<InputFocus>().clear();
                        break;
                    }
                    focused = world.get::<ChildOf>(current).map(ChildOf::parent);
                }
            }
            world.entity_mut(wanted).add_child(entity);
        }
    }
}

pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    browser: Res<BrowserSessions>,
    results: Query<&RelativeCursorPosition, With<SearchResults>>,
    mut panels: Query<
        (
            &BrowserOverlay,
            &RelativeCursorPosition,
            &ComputedNode,
            &mut ScrollPosition,
        ),
        Without<crate::ui::asset_shortcuts::ShortcutList>,
    >,
    mut shortcuts: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        (
            With<crate::ui::asset_shortcuts::ShortcutList>,
            Without<BrowserOverlay>,
        ),
    >,
) {
    let delta: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * if wheel.unit == MouseScrollUnit::Line {
                    30.
                } else {
                    1.
                }
        })
        .sum();
    if !browser.accepts_service_mouse()
        || delta == 0.
        || results.iter().any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    for (cursor, node, mut scroll) in &mut shortcuts {
        if !cursor.cursor_over() {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.);
        scroll.y = (scroll.y + delta).clamp(0., max);
        return;
    }
    for (panel, cursor, node, mut scroll) in &mut panels {
        if !panel.service || !cursor.cursor_over() || !browser.service_open(panel.window) {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.);
        scroll.y = (scroll.y + delta).clamp(0., max);
    }
}
