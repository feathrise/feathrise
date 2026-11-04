//! 首页真实窗口的可选安装证明：仅接受本轮夹具内的DLL，后台核对真实索引、日志和回执。
use super::*;
use crate::native_plugins::management::{packages, types::ManagementCommand};
use bevy::tasks::{Task, futures::check_ready};
use std::path::Path;

#[derive(Resource)]
struct InstallProbe {
    source: PathBuf,
    id: String,
    phase: u8,
    verification: Option<Task<Result<Value, String>>>,
}

pub(super) fn setup(app: &mut App, directory: &Path) {
    let Some(source) = std::env::var_os("FEATHRISE_PLUGIN_HOST_VALIDATION_PACKAGE") else {
        return;
    };
    let source = PathBuf::from(source);
    assert!(
        source.is_absolute()
            && !fs::symlink_metadata(&source)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    let source = source.canonicalize().expect("真实DLL来源必须存在");
    assert!(
        source.starts_with(directory),
        "先把来源只读复制到本轮owner目录，不能使用用户工程或插件根"
    );
    let (metadata, source) =
        packages::inspect_package(&source).expect("本轮来源必须是可静态验证的本平台DLL");
    app.insert_resource(InstallProbe {
        source,
        id: metadata.id,
        phase: 0,
        verification: None,
    });
}

pub(super) fn ready(world: &mut World, plan: &mut PluginHostValidation) -> Result<bool, String> {
    let Some(mut probe) = world.remove_resource::<InstallProbe>() else {
        return Ok(true);
    };
    let result = advance(world, plan, &mut probe);
    world.insert_resource(probe);
    result
}

fn advance(
    world: &mut World,
    plan: &mut PluginHostValidation,
    probe: &mut InstallProbe,
) -> Result<bool, String> {
    if probe.phase == 0 {
        // 仅替代系统文件选择器的来源回填；继续执行真实生产后台校验、提交和UI回执。
        world
            .resource_mut::<PluginManagerState>()
            .request(ManagementCommand::Install {
                source: probe.source.clone(),
                replace: false,
            })?;
        probe.phase = 1;
        return Ok(false);
    }
    if probe.phase == 1 {
        let manager = world.resource::<PluginManagerState>();
        if manager.busy {
            return Ok(false);
        }
        let row = manager
            .rows
            .iter()
            .find(|row| row.id == probe.id)
            .ok_or_else(|| manager.status.clone())?;
        ensure(
            row.pending.is_none() && row.enabled && row.error.is_none(),
            "首页安装必须真实完成，不能仅改状态文字",
        )?;
        ensure(
            manager.status.contains("已生效"),
            "首页安装成功后必须明确显示已生效",
        )?;
        ensure(
            manager.load_states.is_empty(),
            "无工程状态不能伪造DLL已加载",
        )?;
        let global = plan.global.clone();
        let owner_directory = plan.directory.clone();
        let id = probe.id.clone();
        let binary = row.path.clone();
        probe.verification = Some(
            IoTaskPool::get()
                .spawn(async move { verify_disk(owner_directory, global, id, binary) }),
        );
        probe.phase = 2;
    }
    if probe.phase == 2 {
        let Some(result) = probe.verification.as_mut().and_then(check_ready) else {
            return Ok(false);
        };
        let evidence = result?;
        plan.proofs.push(json!({"home_install_committed":evidence,"source":"production PluginManagerState request and background transaction"}));
        probe.phase = 3;
    }
    Ok(probe.phase == 3)
}

fn verify_disk(
    owner_directory: PathBuf,
    global: PathBuf,
    id: String,
    binary: PathBuf,
) -> Result<Value, String> {
    // Windows规范化会补入\\?\前缀；两侧统一真实路径再比较，同时重验本轮owner范围，不能仅去掉前缀后做字符串放行。
    let owner_directory = owner_directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let global = global.canonicalize().map_err(|error| error.to_string())?;
    let binary = binary.canonicalize().map_err(|error| error.to_string())?;
    ensure(
        global.starts_with(&owner_directory) && global != owner_directory,
        "全局插件根必须仍位于本轮归属目录内",
    )?;
    ensure(
        binary.starts_with(&global) && binary.is_file(),
        "真实发布对象必须位于本轮全局根",
    )?;
    let read = |path: PathBuf| -> Result<Value, String> {
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    };
    let digest = packages::binary_digest(&binary)?;
    let index = read(global.join("index.json"))?;
    ensure(
        index["entries"][id.as_str()]["digest"].as_str() == Some(digest.as_str()),
        "活动索引必须绑定实际发布字节",
    )?;
    let journal = read(global.join(".management/pending.json"))?;
    ensure(
        journal["changes"].get(&id).is_none(),
        "首页安装后不能遗留同ID待应用计划",
    )?;
    let mut receipts = 0;
    for item in
        fs::read_dir(global.join(".management/receipts")).map_err(|error| error.to_string())?
    {
        let path = item.map_err(|error| error.to_string())?.path();
        if path.is_file() && read(path)?["record"]["manifest"]["id"].as_str() == Some(id.as_str()) {
            receipts += 1;
        }
    }
    ensure(receipts > 0, "必须存在真实完成回执")?;
    Ok(
        json!({"id":id,"binary":binary,"sha256":digest,"pending":false,"completed_receipts":receipts,"loaded_claimed":false}),
    )
}

pub(super) fn directory_proof(world: &mut World, panel: Entity) -> Result<Value, String> {
    let expected =
        crate::project::display_path(&world.resource::<PluginManagerState>().roots.global);
    let (text_entity, text) = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, text, node, visible)| {
            (visible.get()
                && !node.is_empty()
                && below(world, entity, panel)
                && text.0.contains(&expected))
            .then_some((entity, text.0.clone()))
        })
        .ok_or("首页必须显示实际全局插件绝对目录")?;
    let button = world
        .get::<ChildOf>(text_entity)
        .ok_or("目录文字必须属于可点击控件")?
        .parent();
    ensure(
        world.get::<Button>(button).is_some() && world.get::<InteractionDisabled>(button).is_none(),
        "目录路径必须可点击，不能只是一段说明文字",
    )?;
    Ok(
        json!({"actual_path":expected,"visible_text":text,"clickable":true,"os_directory_window_verified":false}),
    )
}

pub(super) fn visible_proof(world: &mut World, panel: Entity) -> Result<Option<Value>, String> {
    let Some(probe) = world.get_resource::<InstallProbe>() else {
        return Ok(None);
    };
    ensure(probe.phase == 3, "截图前必须完成实际事务校验")?;
    let id = probe.id.clone();
    let texts: Vec<_> = world
        .query::<(Entity, &Text, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(entity, _, node, visible)| {
            visible.get() && !node.is_empty() && below(world, *entity, panel)
        })
        .map(|(_, text, _, _)| text.0.clone())
        .collect();
    ensure(
        texts.iter().any(|text| text.contains(&id)),
        "截图必须包含真实安装的插件行",
    )?;
    ensure(
        texts.iter().any(|text| text.contains("已生效")),
        "截图必须包含已生效，不能停留待安装",
    )?;
    ensure(
        !texts.iter().any(|text| text.contains("重开生效")),
        "首页不能仍显示模糊重开生效提示",
    )?;
    Ok(Some(
        json!({"id":id,"displayed_effective":true,"project_opened":false}),
    ))
}
