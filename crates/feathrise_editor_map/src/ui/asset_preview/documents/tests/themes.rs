//! 复用真实文档GPU夹具，检查正式设置按钮、绘制色及切换期间的草稿/滚动/选区身份。
use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::{components::*, resources::MapEditorSettingsState, types::EditorSettingsAction};
use bevy::{
    ecs::system::RunSystemOnce,
    render::{Render, RenderApp, sync_world::MainEntity},
    ui_render::{ExtractedUiItem, ExtractedUiNodes},
};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorColorTheme;

#[derive(Resource, Clone, Default)]
struct ThemeProbe(Arc<std::sync::Mutex<(Option<Entity>, Option<LinearRgba>)>>);

#[derive(Resource, Clone, Default)]
struct ThemeTextProbe(Arc<std::sync::Mutex<Vec<ThemeTextSample>>>);
struct ThemeTextSample {
    entity: Entity,
    label: String,
    source: Color,
    actual: Option<LinearRgba>,
    geometry: String,
}

pub(super) fn install(app: &mut App) {
    let probe = ThemeProbe::default();
    app.insert_resource(probe.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(probe)
        .add_systems(Render, capture.before(bevy::ui_render::prepare_uinodes));
    let text = ThemeTextProbe::default();
    app.insert_resource(text.clone());
    app.sub_app_mut(RenderApp).insert_resource(text);
}

fn capture(nodes: Res<ExtractedUiNodes>, probe: Res<ThemeProbe>, text: Res<ThemeTextProbe>) {
    let mut probe = probe.0.lock().unwrap();
    let Some(source) = probe.0 else {
        return;
    };
    let draws = |entity| nodes.uinodes.get(&MainEntity::from(entity)).map(|(_, draws)| draws);
    probe.1 = draws(source).and_then(|draws| draws.values().find_map(|draw| {
        match draw.item {
            ExtractedUiItem::Node { color, .. } if color.alpha > 0.7 => Some(color),
            _ => None,
        }
    }));
    for sample in text.0.lock().unwrap().iter_mut() {
        let glyphs = draws(sample.entity).and_then(|draws| draws.values().find_map(|draw| {
            let ExtractedUiItem::Glyphs { glyphs } = &draw.item else {
                return None;
            };
            let glyph = glyphs.first()?;
            Some((
                glyph.color,
                format!(
                    "clip={:?}, y={}, glyphs={}, transform={:?}",
                    draw.clip, glyph.translation.y, glyphs.len(), draw.transform
                ),
            ))
        }));
        if let Some((color, geometry)) = glyphs {
            sample.actual = Some(color);
            sample.geometry = geometry;
        }
    }
}

pub(super) fn verify(
    app: &mut App,
    window: Entity,
    camera: Entity,
    source: Entity,
    target: Handle<Image>,
    width: u32,
    snapshot: impl Fn(&mut App, Handle<Image>, &str, u32),
) {
    app.world().resource::<ThemeProbe>().0.lock().unwrap().0 = Some(source);
    let world = app.world_mut();
    let mut labels = world.query::<(Entity, &Text, &TextColor, Has<crate::ui::asset_preview::types::AssetPreviewTitleText>)>();
    let samples: Vec<_> = labels
        .iter(world)
        .filter(|(_, text, _, preview_title)| {
            // 预览按稳定marker定位；搜索按钮兼容展开后的“收起内容搜索”文案。
            *preview_title || matches!(text.as_str(), "内容搜索 Ctrl+Shift+F" | "收起内容搜索 Ctrl+Shift+F")
        })
        .map(|(entity, text, color, _)| ThemeTextSample {
            entity,
            label: text.0.clone(),
            source: color.0,
            actual: None,
            geometry: String::new(),
        })
        .collect();
    assert_eq!(samples.len(), 2, "需验证两种动态强调标题，实际匹配: {:?}",
        samples.iter().map(|sample| &sample.label).collect::<Vec<_>>());
    *world.resource::<ThemeTextProbe>().0.lock().unwrap() = samples;
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(bevy::text::TextEdit::TextEnd(false));
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(bevy::text::TextEdit::Insert("\n// 未保存的中文草稿".into()));
    frames(app, 20);
    let before = app.world().resource::<DocumentState>().current().unwrap();
    assert!(before.dirty());
    let identity = (
        before.source.clone(),
        before.revision,
        before.saved_revision,
    );
    let selection = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    let scroll = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .viewport.offset;
    for _ in 0..200 {
        if app
            .world()
            .resource::<super::super::resources::DocumentMinimapState>()
            .picture
            .is_some()
        {
            break;
        }
        frames(app, 1);
    }
    let image = app
        .world()
        .resource::<super::super::resources::DocumentMinimapState>()
        .picture
        .as_ref()
        .expect("主题测试需要真实小地图轮廓")
        .image
        .clone();
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = true;
    frames(app, 4);
    for theme in [
        EditorColorTheme::GraphiteBlue,
        EditorColorTheme::WarmCopper,
        EditorColorTheme::NeutralGray,
    ] {
        click(
            app,
            window,
            camera,
            EditorSettingsAction::SetColorTheme(theme),
        );
        frames(app, 4);
        assert_eq!(
            app.world().resource::<EditorSettings>().editor_color_theme,
            theme
        );
        assert_eq!(
            app.world()
                .resource::<feathrise_editor_ui::EditorPalette>()
                .name(),
            theme.as_str()
        );
        let world = app.world_mut();
        let mut query =
            world.query::<(&MapEditorSettingsButton, &ComputedNode, &UiGlobalTransform)>();
        for (button, node, transform) in query.iter(world) {
            if matches!(button.0, EditorSettingsAction::SetColorTheme(_) | EditorSettingsAction::ResetLayout) {
                assert!(node.size().x > 80.0 && node.size().y > 20.0);
                let position = transform.affine().translation;
                assert!(
                    position.x > 0.0
                        && position.x < width as f32
                        && position.y > 0.0
                        && position.y < 1080.0
                );
            }
        }
        snapshot(
            app,
            target.clone(),
            &format!("editor-theme-{}-settings.png", theme.as_str()),
            width,
        );
        click(app, window, camera, EditorSettingsAction::Close);
        frames(app, 3);
        let actual = app
            .world()
            .resource::<ThemeProbe>()
            .0
            .lock()
            .unwrap()
            .1
            .expect("原生源码画布没有绘制背景");
        let palette = app.world().resource::<feathrise_editor_ui::EditorPalette>();
        let expected = if theme == EditorColorTheme::WarmCopper {
            crate::ui::theme::INPUT_BG
        } else {
            palette.resolve(crate::ui::theme::OVERLAY_BG)
        }
        .to_linear();
        assert_eq!(actual, expected, "正式RenderApp必须应用画布主题");
        for sample in app
            .world()
            .resource::<ThemeTextProbe>()
            .0
            .lock()
            .unwrap()
            .iter()
        {
            assert_eq!(
                sample.actual.unwrap(),
                palette.resolve(sample.source).to_linear(),
                "标题未采用主题: {}，源色 {:?}，{}",
                sample.label,
                sample.source,
                sample.geometry
            );
        }
        let doc = app.world().resource::<DocumentState>().current().unwrap();
        assert_eq!(
            (&doc.source, doc.revision, doc.saved_revision),
            (&identity.0, identity.1, identity.2)
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(source)
                .unwrap()
                .editor
                .raw_selection()
                .text_range(),
            selection
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(source)
                .unwrap()
                .viewport.offset,
            scroll
        );
        assert_eq!(
            app.world()
                .resource::<super::super::resources::DocumentMinimapState>()
                .picture
                .as_ref()
                .unwrap()
                .image,
            image,
            "切换主题不能重生成或上传小地图"
        );
        snapshot(
            app,
            target.clone(),
            &format!("editor-theme-{}-document.png", theme.as_str()),
            width,
        );
        app.world_mut()
            .resource_mut::<MapEditorSettingsState>()
            .open = true;
        frames(app, 3);
    }
    // 真实鼠标应命中新加入的紧凑按钮，默认恢复不能重建未保存文档。
    let preferred = feathrise_editor_core::EditorLayoutPreferences {
        content_width: 320.0, inspector_width: 480.0,
        content_visible: false, ..default()
    };
    app.world_mut().resource_mut::<EditorSettings>().editor_layout = preferred;
    {
        let mut layout = app.world_mut().resource_mut::<crate::resources::MapEditorLayoutState>();
        layout.apply_preferences(preferred);
        layout.toggle(crate::types::EditorLayoutAction::Focus);
    }
    click(app, window, camera, EditorSettingsAction::ResetLayout);
    frames(app, 3);
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, default());
    assert!(app.world().resource::<crate::resources::MapEditorLayoutState>().focus_restore.is_none());
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!((&doc.source, doc.revision, doc.saved_revision), (&identity.0, identity.1, identity.2));
    assert_eq!(app.world().get::<EditableText>(source).unwrap().editor.raw_selection().text_range(), selection);
    assert_eq!(app.world().get::<EditableText>(source).unwrap().viewport.offset, scroll);
    snapshot(app, target, "editor-layout-default-settings.png", width);
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn click(app: &mut App, window: Entity, camera: Entity, action: EditorSettingsAction) {
    let (button, position) = {
        let world = app.world_mut();
        let mut query = world.query::<(
            Entity,
            &MapEditorSettingsButton,
            &UiGlobalTransform,
            &ComputedNode,
        )>();
        let (entity, _, transform, _) = query
            .iter(world)
            .find(|(_, b, _, size)| b.0 == action && size.size().x < 200.0)
            .unwrap();
        (entity, transform.affine().translation)
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<UiInteraction>(button).unwrap(),
        UiInteraction::Pressed,
        "正式鼠标应命中主题按钮"
    );
    app.world_mut()
        .run_system_once(crate::ui::editor_settings::handle_settings_input)
        .unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(crate::ui::editor_settings::handle_settings_input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
}
