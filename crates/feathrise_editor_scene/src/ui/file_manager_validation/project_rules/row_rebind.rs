//! 按住真实行按钮时从外部重排独占夹具，检验自动回读后松手不会操作新占据该槽的记录。
use super::*;
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};

pub(super) struct HeldRow {
    button: Entity,
    name: String,
    original: String,
    swapped: String,
    released_frames: u8,
}

pub(super) fn advance(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    prefix: &str,
    delete: bool,
) -> Result<bool, String> {
    let source = run.work.join("project/assets").join(PATH);
    let first = format!("{prefix}01");
    let second = format!("{prefix}02");
    if run.row_rebind.is_none() {
        let name = if delete {
            "RulesDeleteRow0"
        } else {
            "RulesRow0"
        };
        let Some(button) = form::entity(world, name) else {
            return Ok(false);
        };
        if !layout::reveal(world, run, window, button) {
            return Ok(false);
        }
        let position = layout::bounds(world, button)
            .ok_or("待按住行按钮尚未排版")?
            .center();
        run.wheel_point = Some(position);
        move_cursor(world, window, position.as_dvec2());
        if world.get::<UiInteraction>(button) != Some(&UiInteraction::Hovered) {
            return Ok(false);
        }
        let original = fs::read_to_string(&source).map_err(|e| e.to_string())?;
        let original_id = format!("\"{first}\"");
        let next_id = format!("\"{second}\"");
        if !original.contains(&original_id) || !original.contains(&next_id) {
            return Err("行换绑夹具缺少前两条记录".into());
        }
        // 只改本轮夹具的原始文件，仍由正式文件监听、回读与表单刷新采用。
        let swapped = original
            .replace(&original_id, "\"__held_row__\"")
            .replace(&next_id, &original_id)
            .replace("\"__held_row__\"", &next_id);
        run.row_rebind = Some(HeldRow {
            button,
            name: name.into(),
            original,
            swapped,
            released_frames: 0,
        });
        run.pointer = Some((button, true));
        super::super::mouse(world, window, ButtonState::Pressed);
        run.keys = 1;
        return Ok(false);
    }
    let held = run.row_rebind.as_mut().unwrap();
    match run.keys {
        1 => {
            if world.get::<UiInteraction>(held.button) != Some(&UiInteraction::Pressed) {
                return Ok(false);
            }
            fs::write(&source, &held.swapped).map_err(|e| e.to_string())?;
            crate::ui::asset_preview::documents::validation::recheck_project_rules(world)?;
            run.keys = 2;
        }
        2 => {
            if form::row_slot(world, &second) != Some(0)
                || observe(world).is_none_or(|(text, dirty, _, _)| dirty || text != held.swapped)
            {
                return Ok(false);
            }
            super::super::mouse(world, window, ButtonState::Released);
            run.pointer = None;
            run.keys = 3;
        }
        3 => {
            if world
                .get::<EditorButtonClick>(held.button)
                .is_some_and(EditorButtonClick::is_clicked)
                || form::entity(world, "RulesEditorDialog").is_some()
                || observe(world).is_none_or(|(text, dirty, _, _)| dirty || text != held.swapped)
                || fs::read_to_string(&source).map_err(|e| e.to_string())? != held.swapped
            {
                return Err(format!(
                    "{} 从 {first} 换到 {second} 后，旧松手仍触发了新记录",
                    held.name
                ));
            }
            held.released_frames += 1;
            if held.released_frames < 3 {
                return Ok(false);
            }
            fs::write(&source, &held.original).map_err(|e| e.to_string())?;
            crate::ui::asset_preview::documents::validation::recheck_project_rules(world)?;
            run.keys = 4;
        }
        _ => {
            if form::row_slot(world, &first) != Some(0)
                || observe(world).is_none_or(|(text, dirty, _, _)| dirty || text != held.original)
            {
                return Ok(false);
            }
            let label = if delete { "delete" } else { "edit" };
            run.layout.insert(
                format!("{prefix}-{label}-row-rebind"),
                json!({
                    "before":first, "after":second, "released_frames":held.released_frames,
                    "old_release_canceled":true,
                }),
            );
            run.row_rebind = None;
            return Ok(true);
        }
    }
    Ok(false)
}
