//! 在相同 ID 上通过真实侧栏控件新建场景配置，再独立重开核对两层记录。

use super::*;

const SCENE: &str = "M1-SAMPLE";

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
    Button("RulesScopeScene"),
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
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    Button("RulesScopeProject"),
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
    Button("RulesScopeScene"),
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
];

pub(super) const REOPEN: &[Step] = &[
    Button("交互目录条目工程变量"),
    Page("工程变量"),
    Button("RulesScopeProject"),
    Selected("a"),
    FieldEquals("RulesFieldDefault", "3"),
    FieldEquals("RulesFieldTag", "World"),
    Button("RulesCancel"),
    Button("RulesScopeScene"),
    Selected("a"),
    FieldEquals("RulesFieldDefault", "7"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
    Selected("dock_flag"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
    Button("交互目录条目触发规则"),
    Page("触发规则"),
    Button("RulesScopeProject"),
    Selected("scene_hook"),
    FieldEquals("RulesFieldActionValue", "3"),
    FieldEquals("RulesFieldTag", "World"),
    Button("RulesCancel"),
    Button("RulesScopeScene"),
    Selected("scene_hook"),
    FieldEquals("RulesFieldActionValue", "7"),
    FieldEquals("RulesFieldTag", "Harbor"),
    Button("RulesCancel"),
    ScopePickerLayout,
];

pub(super) fn scope_picker_layout(world: &mut World, run: &mut Run) -> Result<bool, String> {
    let bounds = |world: &mut World, name| {
        let button = entity(world, name)?;
        let children = world.get::<Children>(button)?;
        let arrow = children
            .iter()
            .find(|&child| world.get::<Text>(child).is_some())?;
        let label = children
            .iter()
            .find_map(|child| world.get::<Children>(child)?.iter().next())?;
        let ink = layout::glyph_bounds(world, label)?;
        let clip = world.get::<bevy::ui::CalculatedClip>(label)?;
        let label = feathrise_editor_ui::helpers::clipped_ui_bounds(clip, ink)?;
        Some((
            layout::bounds(world, button)?,
            label,
            layout::glyph_bounds(world, arrow)?,
        ))
    };
    let (Some(tag), Some(scene)) = (
        bounds(world, "RulesListTagPicker"),
        bounds(world, "RulesScopeScenePicker"),
    ) else {
        return Ok(false);
    };
    if (tag.0.width() - scene.0.width()).abs() > 1.
        || [tag, scene]
            .iter()
            .any(|(_, label, arrow)| label.max.x > arrow.min.x)
    {
        return Err("标签与场景选择必须等宽，文字不得遮挡下拉箭头".into());
    }
    run.layout
        .insert("scene-picker-captions-clear".into(), json!(true));
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
