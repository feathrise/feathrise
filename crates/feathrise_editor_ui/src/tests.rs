//! 验证两套显式色板共存时的主题隔离、输入身份和编辑器独立安装，不依赖原游戏主题。

use super::*;
use feathrise_ui_widgets::{NumericScrubFieldPalette, UiWidgetsPlugin, UiWidgetsSystemSet};

#[derive(Component)]
struct SecondaryField;
#[derive(Component)]
struct EditorField;

struct SecondaryUiPlugin;

impl Plugin for SecondaryUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<UiWidgetsPlugin>() {
            app.add_plugins(UiWidgetsPlugin);
        }
    }
}

fn secondary_palette() -> NumericInputPalette {
    // 只用于验证跨主题焦点的蓝色夹具，不复制任何原游戏配色或外观实现。
    NumericInputPalette {
        normal_bg: Color::srgb(0.08, 0.14, 0.28),
        normal_border: Color::srgb(0.2, 0.4, 0.7),
        hovered_bg: Color::srgb(0.1, 0.2, 0.4),
        active_bg: Color::srgb(0.14, 0.3, 0.6),
        active_border: Color::srgb(0.3, 0.6, 1.0),
        disabled_bg: Color::srgb(0.06, 0.07, 0.1),
        disabled_border: Color::srgb(0.1, 0.12, 0.2),
    }
}

fn input_options() -> NumericScrubFieldOptions {
    NumericScrubFieldOptions::new(
        "数值",
        "",
        NumericValue::U32(8),
        NumericInputSpec::u32()
            .with_min(NumericValue::U32(0))
            .with_max(NumericValue::U32(15)),
        NumericInputScrubConfig::linear(1.0, 1.0, 0.0, 15.0, 1.0),
    )
}

fn setup_pair(editor_first: bool) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>();
    if editor_first {
        app.add_plugins((EditorUiPlugin, SecondaryUiPlugin));
    } else {
        app.add_plugins((SecondaryUiPlugin, EditorUiPlugin));
    }
    app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn_empty().with_children(|parent| {
            feathrise_ui_widgets::spawn_numeric_scrub_field_with_palette(
                parent,
                SecondaryField,
                Handle::default(),
                input_options(),
                NumericScrubFieldPalette {
                    input: secondary_palette(),
                    fill: Color::srgb(0.1, 0.3, 0.6),
                    label: Color::WHITE,
                    value: Color::WHITE,
                    unit: Color::WHITE,
                    shadow: TextShadow::default(),
                },
            );
            spawn_numeric_scrub_field(parent, EditorField, Handle::default(), input_options());
        });
    });
    app.update();
    let game = app
        .world_mut()
        .query_filtered::<Entity, With<SecondaryField>>()
        .single(app.world())
        .unwrap();
    let editor = app
        .world_mut()
        .query_filtered::<Entity, With<EditorField>>()
        .single(app.world())
        .unwrap();
    (app, game, editor)
}

#[test]
fn both_plugin_orders_keep_independent_themes_and_one_focus() {
    for editor_first in [false, true] {
        let (mut app, game, editor) = setup_pair(editor_first);
        // 两个调度别名必须具有同一类型身份，业务不会排在第二套输入系统之后。
        let shared_stage: UiWidgetsSystemSet = EditorUiSystemSet::NumericInput;
        assert_eq!(shared_stage, UiWidgetsSystemSet::NumericInput);
        assert!(app.is_plugin_added::<feathrise_ui_widgets::UiWidgetsPlugin>());
        assert_eq!(
            app.world().get::<BackgroundColor>(game).unwrap().0,
            secondary_palette().normal_bg
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(editor).unwrap().0,
            colors::INPUT_BG
        );
        assert_ne!(numeric_input_palette(), secondary_palette());

        // 编辑器取得唯一焦点后，另一字段仍采用自己的色板，编辑器采用自己的活动边框。
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(editor);
        let mut edit = app
            .world_mut()
            .get_mut::<NumericInputEditState>(editor)
            .unwrap();
        edit.editing = true;
        edit.buffer = "8".into();
        app.update();
        assert_eq!(
            app.world()
                .resource::<feathrise_ui_widgets::ActiveNumericInput>()
                .entity,
            Some(editor)
        );
        assert_eq!(
            app.world().get::<ButtonColors>(game).unwrap().normal,
            secondary_palette().normal_bg
        );
        assert_eq!(
            app.world().get::<ButtonColors>(editor).unwrap().normal,
            colors::INPUT_ACTIVE_BG
        );
        assert_eq!(
            *app.world().get::<BorderColor>(editor).unwrap(),
            BorderColor::all(colors::INPUT_ACTIVE_BORDER)
        );

        // 外部赋值释放焦点时，仍使用当前编辑器色板且遵循原数值范围。
        app.world_mut().write_message(NumericInputSetValue {
            entity: editor,
            value: NumericValue::U32(99),
            cancel_edit: true,
        });
        app.update();
        assert_eq!(app.world().resource::<ActiveNumericInput>().entity, None);
        assert_eq!(
            app.world().get::<NumericInputValue>(editor).unwrap().0,
            NumericValue::U32(15)
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(editor).unwrap().0,
            colors::INPUT_BG
        );

        app.world_mut()
            .entity_mut(editor)
            .insert(NumericInputDisabled);
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(editor).unwrap().0,
            colors::INPUT_DISABLED_BG
        );
    }
}

#[test]
fn editor_installs_with_its_own_theme_and_shared_input_only() {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_plugins(EditorUiPlugin);
    app.update();
    assert!(app.world().contains_resource::<ActiveNumericInput>());
    assert!(app.world().contains_resource::<ActiveNumericInputScrub>());
    assert!(app.is_plugin_added::<EditorThemePlugin>());
    assert!(app.is_plugin_added::<EditorButtonInputPlugin>());
    assert!(!app.is_plugin_added::<SecondaryUiPlugin>());
}
