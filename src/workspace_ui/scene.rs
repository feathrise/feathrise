//! 新增工程页面挂在原工作台静态槽位；只构造新增字段，不复制三栏和作者工具。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{Page, RecentEntry, WorkspaceField, components::*, resources::Presentation};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    text::{EditableTextFilter, LineBreak, TextCursorStyle},
    ui::RelativeCursorPosition,
};
use feathrise_editor_scene::workbench::{
    WorkbenchCentralPageHost, WorkbenchDocumentTabs, WorkbenchHeader, WorkbenchHostTabs,
    WorkbenchSettingsBody,
};
use feathrise_editor_ui::colors as c;
use feathrise_editor_ui::localization::{LocalizedText, editor_label};

fn column() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        flex_direction: FlexDirection::Column,
        row_gap: px(10),
        ..default()
    }
}

fn caption(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, text: &str, size: f32) {
    parent.spawn((caption_bundle(font, text, size), LocalizedText::label(text)));
}

fn caption_bundle(font: &Handle<Font>, text: &str, size: f32) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font: font.clone().into(),
            font_size: size.into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        Node {
            min_width: px(0),
            ..default()
        },
    )
}

pub(super) fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: impl Component,
) -> Entity {
    parent
        .spawn((
            InteractionButton,
            action,
            feathrise_editor_ui::ButtonColors::new(
                c::BUTTON_DEV_SMALL_NORMAL,
                c::BUTTON_DEV_SMALL_HOVERED,
                c::BUTTON_DEV_SMALL_PRESSED,
            ),
            Node {
                min_height: px(26),
                flex_shrink: 0.,
                padding: UiRect::axes(px(9), px(4)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
            BorderColor::all(c::COPPER_BORDER_WEAK),
        ))
        .with_child((
            editor_label(label),
            TextFont {
                font: font.clone().into(),
                font_size: 13.0.into(),
                ..default()
            },
            TextColor(c::TEXT_PRIMARY),
        ))
        .id()
}

fn input(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    kind: Input,
    value: &str,
    limit: usize,
) {
    parent.spawn((
        kind,
        feathrise_editor_core::EditorTextInput,
        feathrise_editor_ui::single_line_input(value, limit),
        EditableTextFilter::new(|c| !c.is_control()),
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(30),
            flex_shrink: 0.,
            padding: UiRect::axes(px(7), px(5)),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip_x(),
            ..default()
        },
        TextFont {
            font: font.clone().into(),
            font_size: 13.0.into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        TextLayout::no_wrap(),
        TextCursorStyle {
            color: c::TEXT_PRIMARY,
            selection_color: c::SELECT_BG_HOVER,
            unfocused_selection_color: c::SELECT_BG,
            selection_radius: 0.0, selected_text_color: Some(c::TEXT_PRIMARY),
        },
        BackgroundColor(c::INPUT_BG),
        BorderColor::all(c::COPPER_BORDER_WEAK),
        TabIndex(kind as i32),
    ));
}

fn field_row(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    kind: Input,
    field: WorkspaceField,
    value: &str,
) {
    parent
        .spawn(Node {
            width: percent(100),
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_grow: 1.,
                min_width: px(0),
                ..default()
            })
            .with_children(|slot| input(slot, font, kind, value, 4095));
            button(row, font, "浏览…", Action::Browse(field));
        });
}

fn form(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, new: bool, state: &Presentation) {
    caption(parent, font, if new { "新建工程" } else { "打开工程" }, 19.);
    parent
        .spawn(Node {
            width: px(560),
            max_width: percent(100),
            ..column()
        })
        .with_children(|form| {
            if new {
                caption(form, font, "工程名称", 13.);
                input(form, font, Input::Name, &state.name, 128);
                caption(form, font, "初始视角", 13.);
                form.spawn(Node {
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|row| {
                    for view in [
                        feathrise_runtime::GameView::ThirdPerson,
                        feathrise_runtime::GameView::FixedTopDown,
                    ] {
                        button(row, font, view.label(), Action::SelectGameView(view));
                    }
                });
            }
            caption(
                form,
                font,
                if new {
                    "父目录"
                } else {
                    "工程目录或 editor-project.json 清单"
                },
                13.,
            );
            field_row(
                form,
                font,
                if new { Input::Parent } else { Input::Open },
                if new {
                    WorkspaceField::NewParent
                } else {
                    WorkspaceField::OpenPath
                },
                if new { &state.parent } else { &state.open_path },
            );
            if new {
                caption(form, font, "将创建于", 12.);
                form.spawn((
                    TargetText,
                    Text::new(""),
                    TextFont {
                        font: font.clone().into(),
                        font_size: 13.0.into(),
                        ..default()
                    },
                    TextColor(c::TEXT_MUTED),
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        padding: UiRect::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(c::INPUT_BG),
                ));
                caption(
                    form,
                    font,
                    "创建空工程，不附带场景、模型或预设。",
                    12.,
                );
            }
            form.spawn(Node {
                column_gap: px(8),
                row_gap: px(6),
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|row| {
                button(
                    row,
                    font,
                    if new {
                        "创建并打开"
                    } else {
                        "打开工程"
                    },
                    if new { Action::Create } else { Action::Open },
                );
                button(
                    row,
                    font,
                    "取消",
                    Action::CloseTab(if new { Page::New } else { Page::Open }),
                );
            });
            status(form, font);
        });
}

fn status(parent: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    parent.spawn((
        StatusText,
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: 12.0.into(),
            ..default()
        },
        TextColor(c::TEXT_MUTED),
        Node {
            width: percent(100),
            min_width: px(0),
            ..default()
        },
    ));
    button(parent, font, "取消操作", Action::CancelOperation);
}

pub(super) fn mount(
    mut commands: Commands,
    assets: Res<AssetServer>,
    state: Res<Presentation>,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    tabs: Query<Entity, Added<WorkbenchDocumentTabs>>,
    bodies: Query<Entity, Added<WorkbenchCentralPageHost>>,
    settings: Query<Entity, Added<WorkbenchSettingsBody>>,
    brand: Option<Res<crate::branding::BrandImages>>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        let mut toggle = None;
        commands.entity(header).with_children(|p| {
            toggle = Some(button(p, &font, "工程", Action::ToggleMenu));
        });
        if let Some(toggle) = toggle {
            commands.entity(toggle).insert(crate::window_chrome::ChromeSlot::Project);
            commands.entity(header).insert_children(1, &[toggle]);
        }
        commands.entity(header).with_children(|p| {
            p.spawn((
                MenuNode,
                RelativeCursorPosition::default(),
                UiInteraction::None,
                bevy::ui::FocusPolicy::Block,
                GlobalZIndex(4300),
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    left: px(46),
                    top: px(if cfg!(target_os="windows") {36}else{28}), // 菜单对齐实际顶栏高度。
                    width: px(250),
                    max_width: Val::Vw(94.),
                    padding: UiRect::all(px(10)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(c::PANEL_BG_STRONG),
                BorderColor::all(c::COPPER_BORDER_WEAK),
            ))
            .with_children(|menu| {
                button(menu, &font, "开始页", Action::Show(Page::Start));
                button(menu, &font, "新建工程…", Action::Show(Page::New));
                button(menu, &font, "打开工程…", Action::Show(Page::Open));
                button(menu, &font, "当前场景设为启动场景", Action::SetStartup);
                button(menu, &font, "导出工程…", Action::ExportProject);
                button(menu, &font, "插件管理…", Action::Plugins);
                button(menu, &font, "关闭工程", Action::CloseProject);
                button(menu, &font, "关于 Feathrise…", Action::ShowAbout);
                button(menu, &font, "退出 Feathrise", Action::QuitApp);
            });
        });
    }
    for tabbar in &tabs {
        let root = commands
            .spawn((
                WorkbenchHostTabs,
                Node {
                    height: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(2),
                    flex_shrink: 0.,
                    ..default()
                },
            ))
            .with_children(|root| {
                for (page, label) in [
                    (Page::Start, "开始"),
                    (Page::New, "新建工程"),
                    (Page::Open, "打开工程"),
                ] {
                    root.spawn((
                        TabNode(page),
                        Node {
                            height: percent(100),
                            align_items: AlignItems::Center,
                            border: UiRect::bottom(px(2)),
                            ..default()
                        },
                        BackgroundColor(c::PANEL_BG_STRONG),
                        BorderColor::all(c::COPPER_BORDER_WEAK),
                    ))
                    .with_children(|tab| {
                        button(tab, &font, label, Action::Show(page));
                        button(tab, &font, "×", Action::CloseTab(page));
                    });
                }
            })
            .id();
        commands.entity(tabbar).insert_children(0, &[root]);
    }
    for body in &bodies {
        commands.entity(body).with_children(|host| {
            for page in [Page::Start, Page::New, Page::Open, Page::Empty] {
                host.spawn((
                    PageNode(page),
                    RelativeCursorPosition::default(),
                    ScrollPosition::default(),
                    Node {
                        display: Display::None,
                        width: percent(100),
                        flex_grow: 1.,
                        min_height: px(0),
                        min_width: px(0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(20),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|p| match page {
                    Page::Start => {
                        p.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            margin: UiRect::bottom(px(10)),
                            ..default()
                        })
                        .with_children(|brand_row| {
                            if let Some(brand) = &brand {
                                brand_row.spawn((
                                    ImageNode::new(brand.mark.clone()),
                                    Node {
                                        width: px(38),
                                        height: px(38),
                                        ..default()
                                    },
                                ));
                            }
                            brand_row.spawn(column()).with_children(|text| {
                                caption(text, &font, "Feathrise", 24.);
                                caption(text, &font, "羽立 · Bevy 编辑器", 12.);
                            });
                        });
                        p.spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(36),
                            row_gap: px(20),
                            ..default()
                        })
                        .with_children(|columns| {
                            columns
                                .spawn(Node {
                                    width: px(170),
                                    max_width: percent(100),
                                    ..column()
                                })
                                .with_children(|start| {
                                    caption(start, &font, "开始", 17.);
                                    button(start, &font, "新建工程…", Action::Show(Page::New));
                                    button(start, &font, "打开工程…", Action::Show(Page::Open));
                                    caption(start, &font, "拖入工程文件夹即可打开", 12.);
                                });
                            columns
                                .spawn(Node {
                                    min_width: px(180),
                                    flex_grow: 1.,
                                    flex_basis: px(230),
                                    ..column()
                                })
                                .with_children(|recent| {
                                    caption(recent, &font, "最近工程", 17.);
                                    recent.spawn((RecentHost, column()));
                                });
                        });
                        status(p, &font);
                    }
                    Page::New => form(p, &font, true, &state),
                    Page::Open => form(p, &font, false, &state),
                    Page::Empty => {
                        caption(p, &font, "尚未打开工程", 17.);
                        button(p, &font, "开始页", Action::Show(Page::Start));
                    }
                    Page::Editor => {}
                });
            }
        });
    }
    for body in &settings {
        let group = commands
            .spawn(Node {
                padding: UiRect::bottom(px(10)),
                ..column()
            })
            .with_children(|p| {
                caption(p, &font, "工程", 15.);
                caption(p, &font, "默认工程目录", 13.);
                field_row(
                    p,
                    &font,
                    Input::DefaultDirectory,
                    WorkspaceField::DefaultDirectory,
                    &state.default_directory,
                );
                caption(
                    p,
                    &font,
                    "新建工程的父目录；初始值为 ~/feathrise。已有工程不会移动。",
                    12.,
                );
                p.spawn(Node {
                    column_gap: px(8),
                    row_gap: px(5),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|row| {
                    button(row, &font, "保存默认目录", Action::SaveDefault);
                    button(row, &font, "恢复 ~/feathrise", Action::ResetDefault);
                });
                status(p, &font);
            })
            .id();
        commands.entity(body).insert_children(0, &[group]);
    }
}

pub(super) fn recent_rows(
    mut commands: Commands,
    assets: Res<AssetServer>,
    state: Res<Presentation>,
    hosts: Query<Entity, With<RecentHost>>,
    roots: Query<Entity, With<RecentRoot>>,
    mut previous: Local<Option<u64>>,
) {
    if *previous == Some(state.recent_revision) || hosts.is_empty() {
        return;
    }
    *previous = Some(state.recent_revision);
    for old in &roots {
        commands.entity(old).despawn();
    }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for host in &hosts {
        commands.entity(host).with_children(|host| {
            host.spawn((RecentRoot, column())).with_children(|list| {
                if state.recent.is_empty() {
                    caption(list, &font, "还没有最近工程", 12.);
                }
                // 最近记录本身有界；不扫描工程或构建历史列表里的项目。
                for recent in state.recent.iter().take(crate::user_preferences::MAX_RECENT_PROJECTS) {
                    let action = |kind| RecentAction {
                        path: recent.path.clone(),
                        kind,
                    };
                    list.spawn(Node {
                        width: percent(100),
                        column_gap: px(6),
                        row_gap: px(4),
                        align_items: AlignItems::FlexStart,
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            InteractionButton,
                            action(RecentActionKind::Open),
                            Node {
                                flex_grow: 1.,
                                flex_basis: px(110),
                                min_width: px(0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::axes(px(5), px(7)),
                                row_gap: px(3),
                                ..default()
                            },
                            feathrise_editor_ui::ButtonColors::new(
                                c::PANEL_BG_STRONG,
                                c::BUTTON_DEV_SMALL_HOVERED,
                                c::BUTTON_DEV_SMALL_PRESSED,
                            ),
                            BackgroundColor(c::PANEL_BG_STRONG),
                        ))
                        .with_children(|item| {
                            // 工程名和路径即使恰好与词条相同，也以作者原文显示。
                            let mut name = item.spawn(caption_bundle(&font, &recent.name, 14.));
                            if recent.pinned {
                                name.insert(TextColor(c::TEXT_ACCENT));
                            }
                            if let Some(directory) = distinguishing_directory(recent, &state.recent) {
                                item.spawn((
                                    caption_bundle(&font, &directory, 12.),
                                    TextLayout::linebreak(LineBreak::AnyCharacter),
                                )).insert(TextColor(c::TEXT_ACCENT));
                            }
                            // 窄窗仍可换行查看完整目录，短路径只用于快速识别同名工作树。
                            item.spawn((
                                caption_bundle(&font, &crate::project::display_path(&recent.path), 12.),
                                TextLayout::linebreak(LineBreak::AnyCharacter),
                            ));
                            item.spawn(caption_bundle(&font, &recent.last_opened, 11.));
                        });
                        row.spawn(Node {
                            column_gap: px(4),
                            flex_shrink: 0.,
                            ..default()
                        }).with_children(|actions| {
                            let label = if recent.pinned { "取消固定工程" } else { "固定工程" };
                            button(actions, &font, label, action(RecentActionKind::SetPinned(!recent.pinned)));
                            button(actions, &font, "×", action(RecentActionKind::Remove));
                        });
                    });
                }
            });
        });
    }
}

fn distinguishing_directory(entry: &RecentEntry, recent: &[RecentEntry]) -> Option<String> {
    // 每对同名记录从末尾比较一次。首个不同目录出现在摘要开头，窄窗也能优先看到它。
    let common_tail = recent
        .iter()
        .filter(|other| other.name == entry.name && other.path != entry.path)
        .map(|other| {
            entry.path.components().rev().zip(other.path.components().rev())
                .take_while(|(left, right)| left == right)
                .count()
        })
        .max()?;
    let prefix = entry.path.ancestors().nth(common_tail + 1)?;
    let suffix: std::path::PathBuf = entry.path.strip_prefix(prefix).ok()?.components().collect();
    Some(format!("…{}{}", std::path::MAIN_SEPARATOR, suffix.display()))
}

#[cfg(test)]
mod recent_tests {
    use super::*;

    #[test]
    fn same_name_and_leaf_directory_show_distinct_worktree_suffixes() {
        let root = std::env::temp_dir().join("worktrees");
        let entries: Vec<_> = ["824b", "8692"].into_iter().map(|worktree| RecentEntry {
            name: "同名工程".into(),
            path: root.join(worktree).join("project"),
            last_opened: "刚刚".into(),
            pinned: false,
        }).collect();
        assert_eq!(distinguishing_directory(&entries[0], &entries).unwrap(), format!(
            "…{s}824b{s}project", s = std::path::MAIN_SEPARATOR,
        ));
        assert_eq!(distinguishing_directory(&entries[1], &entries).unwrap(), format!(
            "…{s}8692{s}project", s = std::path::MAIN_SEPARATOR,
        ));
        assert!(distinguishing_directory(&entries[0], &entries[..1]).is_none());
    }
}
