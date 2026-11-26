//! 从侧栏原生入口编辑两种作用域，再独立重开，验证场景树与属性视图形成完整链路。

use super::*;
use crate::{components::SceneEditorSceneRow, types::SceneBrowserItem};

const SCENE: &str = "M1-SAMPLE";
const EMPTY_SCENE: &str = "M1-EMPTY";
const TREE: EditorSidebarTab = EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree);
const SCENES: EditorSidebarTab = EditorSidebarTab::Navigator(EditorNavigatorTab::Scenes);
const GAMEPLAY: EditorSidebarTab = EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay);
const CONFIG_ROWS: [&str; 3] = ["场景预览", "场景变量", "场景触发"];

pub(super) const AUTHOR: &[Step] = &[
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Selected("a"),
    Field("RulesFieldTag", "World"),
    Button("RulesSave"),
    ScopedStored {
        scene: false,
        id: "a",
    },
    TagMenuLayout(false),
    Button("RulesListTagPicker"),
    TagMenuLayout(true),
    Button("RulesListOption2"),
    ListRows {
        present: "a",
        absent: "gate_open",
    },
    Button("RulesListTagPicker"),
    Button("RulesListOption0"),
    ListRows {
        present: "gate_open",
        absent: "dock_flag",
    },
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    Button("RulesNew"),
    Field("RulesFieldId", "scene_hook"),
    Field("RulesFieldTrigger", "quest.completed"),
    Field("RulesFieldEventTarget", "harbor_entry"),
    Field("RulesFieldActionName", "a"),
    Field("RulesFieldActionValue", "3"),
    Field("RulesFieldTag", "World"),
    Button("RulesSave"),
    ScopedStored {
        scene: false,
        id: "scene_hook",
    },
    Sidebar(TREE),
    ConfigExpanded(false),
    ConfigExpanded(true),
    Button("场景预览"),
    ScenePreviewProperty,
    ScenePropertyCapture("scene-preview-property"),
    Button("场景变量"),
    SceneRulesProperty {
        title: "场景变量",
        scene: SCENE,
    },
    ListRows {
        present: "",
        absent: "gate_open",
    },
    Button("RulesNew"),
    Field("RulesFieldId", "a"),
    Field("RulesFieldDefault", "7"),
    Field("RulesFieldTag", "Harbor"),
    Button("RulesSave"),
    ScopedStored {
        scene: true,
        id: "a",
    },
    Button("RulesNew"),
    Field("RulesFieldId", "dock_flag"),
    Button("RulesTypeBool"),
    Button("RulesSuggestTag"),
    Button("RulesOption0"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesSave"),
    ScopedStored {
        scene: true,
        id: "dock_flag",
    },
    ScenePropertyCapture("scene-variable-property"),
    Button("场景触发"),
    SceneRulesProperty {
        title: "场景触发",
        scene: SCENE,
    },
    Button("RulesNew"),
    Field("RulesFieldId", "scene_hook"),
    Field("RulesFieldTrigger", "quest.completed"),
    Field("RulesFieldEventTarget", "harbor_entry"),
    Field("RulesFieldActionName", "a"),
    Field("RulesFieldActionValue", "7"),
    Field("RulesFieldTag", "Harbor"),
    Button("RulesSave"),
    ScopedStored {
        scene: true,
        id: "scene_hook",
    },
    Button("RulesListTagPicker"),
    Button("RulesListOption1"),
    ListRows {
        present: "",
        absent: "scene_hook",
    },
    Button("RulesListTagPicker"),
    Button("RulesListOption2"),
    ListRows {
        present: "scene_hook",
        absent: "dock_flag",
    },
    ScenePropertyCapture("scene-trigger-property"),
    // 实际双击打开没有场景级记录的第二个场景，再切回原场景，核对列表不会串用。
    Sidebar(SCENES),
    OpenScene(EMPTY_SCENE),
    Sidebar(TREE),
    ConfigExpanded(true),
    Button("场景变量"),
    SceneRulesProperty {
        title: "场景变量",
        scene: EMPTY_SCENE,
    },
    ListRows {
        present: "",
        absent: "a",
    },
    ListRows {
        present: "",
        absent: "dock_flag",
    },
    Button("场景触发"),
    SceneRulesProperty {
        title: "场景触发",
        scene: EMPTY_SCENE,
    },
    ListRows {
        present: "",
        absent: "scene_hook",
    },
    ScenePropertyCapture("scene-alternate-property"),
    Sidebar(SCENES),
    OpenScene(SCENE),
    Sidebar(TREE),
    ConfigExpanded(true),
    Button("场景变量"),
    SceneRulesProperty {
        title: "场景变量",
        scene: SCENE,
    },
    ListRows {
        present: "dock_flag",
        absent: "gate_open",
    },
    Sidebar(GAMEPLAY),
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    ListRows {
        present: "gate_open",
        absent: "dock_flag",
    },
];

pub(super) const REOPEN: &[Step] = &[
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Selected("a"),
    FieldEquals("RulesFieldDefault", "3"),
    FieldEquals("RulesFieldTag", "World"),
    Button("RulesCancel"),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    Selected("scene_hook"),
    FieldEquals("RulesFieldActionValue", "3"),
    FieldEquals("RulesFieldTag", "World"),
    Button("RulesCancel"),
    Sidebar(TREE),
    ConfigExpanded(true),
    Button("场景变量"),
    SceneRulesProperty {
        title: "场景变量",
        scene: SCENE,
    },
    Selected("a"),
    FieldEquals("RulesFieldDefault", "7"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
    Selected("dock_flag"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
    Button("场景触发"),
    SceneRulesProperty {
        title: "场景触发",
        scene: SCENE,
    },
    Selected("scene_hook"),
    FieldEquals("RulesFieldActionValue", "7"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
];

pub(super) const CONTEXT: &[Step] = &[
    Sidebar(TREE),
    ConfigExpanded(true),
    Button("场景预览"),
    ScenePreviewProperty,
    Button("场景变量"),
    SceneRulesProperty {
        title: "场景变量",
        scene: SCENE,
    },
    ListRows {
        present: "dock_flag",
        absent: "gate_open",
    },
    Button("场景触发"),
    SceneRulesProperty {
        title: "场景触发",
        scene: SCENE,
    },
    ListRows {
        present: "scene_hook",
        absent: "",
    },
    ScenePropertyCapture("scene-property-on-document"),
    Sidebar(GAMEPLAY),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
];

pub(super) fn project_page(world: &mut World, run: &mut Run, title: &str) -> bool {
    let pane = entity(world, "RulesWorkbench");
    let tools = entity(world, "交互当前工具内容");
    let at_project = layout::caption(world, "RulesPageTitle").as_deref() == Some(title)
        && pane
            .zip(tools)
            .is_some_and(|(pane, tools)| directory::ancestor(world, pane, tools))
        && [
            "RulesSceneName",
            "RulesScopeProject",
            "RulesScopeScene",
            "RulesScopeScenePicker",
        ]
        .iter()
        .all(|id| entity(world, id).is_none());
    if !at_project {
        run.layout_wait = format!("等待交互下的工程级页面 {title}");
    }
    at_project
}

pub(super) fn open_scene(world: &mut World, run: &mut Run, window: Entity, scene: &str) -> bool {
    // 只有主动打开另一场景才释放中央保留检查；选取任一配置子项期间检查始终有效。
    run.center = Value::Null;
    let editor = world.resource::<SceneEditorState>();
    if editor.scene_transition_in_progress || editor.pending_scene_transition.is_some() {
        return false;
    }
    if world
        .resource::<crate::authoring::EditorAuthoringSession>()
        .draft
        .as_ref()
        .is_some_and(|draft| draft.scene_id.as_str() == scene)
    {
        if let Some(center) = central(world).filter(|value| value["tab"] == "Scene") {
            run.center = center;
            return true;
        }
        return false;
    }
    let row = visible(
        world,
        |row: &SceneEditorSceneRow| matches!(&row.item, Some(SceneBrowserItem::Scene(id)) if id.as_str() == scene),
    );
    if row.is_none() {
        run.layout_wait = format!("等待场景目录中的 {scene}");
    }
    click(world, &mut run.pointer, window, row);
    false
}

pub(super) fn config_expanded(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    expanded: bool,
) -> Result<bool, String> {
    let rows =
        CONFIG_ROWS.map(|name| entity(world, name).and_then(|row| layout::bounds(world, row)));
    if rows.iter().any(Option::is_some) != expanded {
        named_click(world, run, window, "场景配置");
        return Ok(false);
    }
    let Some(root) = entity(world, "场景配置").and_then(|row| layout::bounds(world, row))
    else {
        run.layout_wait = "等待场景树顶部的场景配置".into();
        return Ok(false);
    };
    if !expanded {
        return Ok(true);
    }
    let [Some(preview), Some(variables), Some(triggers)] = rows else {
        return Ok(false);
    };
    if !(root.max.y <= preview.min.y + 1.
        && preview.max.y <= variables.min.y + 1.
        && variables.max.y <= triggers.min.y + 1.)
    {
        return Err("场景配置下面应依次排布场景预览、场景变量、场景触发".into());
    }
    if let Some(main) = entity(world, "主场景根").and_then(|row| layout::bounds(world, row)) {
        if main.min.y + 1. < triggers.max.y {
            return Err("场景配置应在主场景及对象行上方".into());
        }
        let children = [preview, variables, triggers].map(layout::coords);
        let record = json!({"labels":CONFIG_ROWS,"root":layout::coords(root),
            "children":children,"main":layout::coords(main)});
        run.layout.entry("scene-config-tree").or_insert(record);
    }
    Ok(true)
}

pub(super) fn preview_property(world: &mut World, run: &mut Run) -> Result<bool, String> {
    let Some(panel) = entity(world, "场景预览属性") else {
        run.layout_wait = "等待右侧属性的场景预览".into();
        return Ok(false);
    };
    if entity(world, "交互目录条目工程变量").is_some() || entity(world, "RulesPageTitle").is_some()
    {
        return Ok(false);
    }
    let captions = ["平滑", "平直", "显示三角线", "显示碰撞"];
    for label in captions {
        if !visible(world, |text: &Text| text.as_str() == label)
            .is_some_and(|item| directory::ancestor(world, item, panel))
        {
            run.layout_wait = format!("场景预览属性尚未显示 {label}");
            return Ok(false);
        }
    }
    run.layout
        .insert("scene-preview-controls".into(), json!(captions));
    Ok(true)
}

pub(super) fn rules_property(
    world: &mut World,
    run: &mut Run,
    title: &str,
    scene: &str,
) -> Result<bool, String> {
    let pane = entity(world, "RulesWorkbench");
    let host = entity(world, "场景规则属性");
    let page = layout::caption(world, "RulesPageTitle");
    let id = layout::caption(world, "RulesSceneName");
    let ready = page.as_deref() == Some(title)
        && id.as_deref() == Some(format!("场景 · {scene}").as_str())
        && pane
            .zip(host)
            .is_some_and(|(pane, host)| directory::ancestor(world, pane, host))
        && entity(world, "交互目录条目工程变量").is_none();
    if !ready {
        run.layout_wait =
            format!("等待属性中的 {title}（{scene}），当前标题 {page:?}，场景 {id:?}");
    }
    Ok(ready)
}

pub(super) fn capture(world: &mut World, run: &mut Run, name: &str) -> Result<bool, String> {
    let pane = entity(world, "场景预览属性").or_else(|| entity(world, "场景规则属性"));
    let Some(bounds) = pane.and_then(|e| layout::bounds(world, e)) else {
        return Ok(false);
    };
    let title = layout::caption(world, "RulesPageTitle");
    let table = if matches!(title.as_deref(), Some("场景变量" | "场景触发")) {
        match table::measure(world, None) {
            Ok(table) => Some(table),
            Err(error) => {
                run.layout_wait = error;
                return Ok(false);
            }
        }
    } else {
        None
    };
    let path = run.work.join(format!("rules-{name}.png"));
    let Some(shot) = evidence::capture(world, &mut run.capture, &path)? else {
        return Ok(false);
    };
    run.layout.insert(name.into(), json!({
        "screenshot":shot,"center":run.center,"bounds":layout::coords(bounds),
        "title":title,"scene":layout::caption(world, "RulesSceneName"),"table":table,
    }));
    Ok(true)
}

pub(super) fn tag_menu_layout(
    world: &mut World,
    run: &mut Run,
    open: bool,
) -> Result<bool, String> {
    if entity(world, "RulesListOptions").is_some() != open {
        run.layout_wait = button_diagnostic(world, "RulesListOptions");
        return Ok(false);
    }
    let Some(bounds) = entity(world, "RulesList").and_then(|list| layout::bounds(world, list))
    else {
        return Ok(false);
    };
    let rect = json!(layout::coords(bounds));
    if open {
        if run.layout["tag-filter-list-bounds"] != rect {
            return Err("打开标签选项移动了现有列表".into());
        }
        run.layout
            .insert("tag-menu-kept-list-stable".into(), json!(true));
    } else {
        run.layout.insert("tag-filter-list-bounds".into(), rect);
    }
    Ok(true)
}

pub(super) fn stored(
    world: &mut World,
    run: &mut Run,
    scene: bool,
    id: &str,
) -> Result<bool, String> {
    if entity(world, "RulesForm").is_some() {
        return Ok(false);
    }
    let Some((source, false, _, status)) = observe(world) else {
        return Ok(false);
    };
    if !status.contains("已保存") {
        return Ok(false);
    }
    let disk = fs::read_to_string(run.work.join("project/assets").join(PATH))
        .map_err(|e| e.to_string())?;
    if source != disk.replace("\r\n", "\n") {
        return Ok(false);
    }
    let doc: Value = serde_json::from_str(&disk).map_err(|e| e.to_string())?;
    let section = if scene { &doc["scenes"][SCENE] } else { &doc };
    let value = if id == "scene_hook" {
        section["rules"]
            .as_array()
            .and_then(|rules| rules.iter().find(|rule| rule["id"] == id))
            .unwrap_or(&Value::Null)
    } else {
        &section["variables"][id]
    };
    let tag = if scene { "Harbor" } else { "World" };
    if value["tag"] != tag {
        return Ok(false);
    }
    run.direct_saves
        .insert(format!("scope-{scene}-{id}"), value.clone());
    Ok(true)
}
