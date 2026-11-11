//! 场景树高亮目标场景的默认出生点与具体传送点列表回显。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*; // 资产集合、UI查询和动态行变化检测。
use feathrise_scene_content::{
    SceneContentRegistry, SceneId, SceneManifestAsset, ScenePortalTarget, ScenePortalsAsset,
}; // 目标清单和传送图共同组成完整可选落点。
use crate::ui::theme as colors; // 目标行复用统一铜金选择配色。

use crate::authoring::EditorAuthoringSession; // 同图目标优先读取未保存草稿。
use crate::components::{
    SceneEditorPortalTargetText, SceneEditorTargetPortalList, SceneEditorTargetPortalListTitle,
    SceneEditorTargetPortalRow, SceneEditorTargetPortalRowText,
}; // 摘要、标题、容器和动态行查询显式互斥。
use crate::resources::{SceneBrowserState, SceneEditorState}; // 场景树高亮是目标场景唯一来源。
use super::super::navigation::PortalSidebarNavigation; // 传送页在Select也显示目标资产加载状态。

use super::super::helpers::{
    PortalDisplayContext, TargetPortalDocuments, selected_target_scene, target_portal_documents,
}; // 纯目标解析和文案不混入ECS借用。
use super::{PortalRowEntityRevision, fit_portal_list, portal_row_background};

/// 目标列表只跟踪场景树、端点资产与当前选择的紧凑身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TargetPortalSignature {
    open: bool,                               // 编辑器重开后重新读取当时资产状态。
    visible: bool,                            // 切入传送页时初始化目标行。
    target_scene_id: Option<SceneId>,             // 场景树高亮场景决定整个列表身份。
    portal_revision: u64,                     // 同图未保存传送结构变化时刷新。
    catalog_revision: u64,                    // 目录换代可能替换目标资产句柄。
    selected_target: Option<ScenePortalTarget>, // 显式端点选择决定高亮和关系摘要。
}

fn target_signature(
    editor: &SceneEditorState,
    browser: &SceneBrowserState,
    session: &EditorAuthoringSession,
    registry: Option<&SceneContentRegistry>,
    visible: bool,
) -> TargetPortalSignature {
    TargetPortalSignature {
        open: editor.open,                                    // 隐藏期间不格式化目标行。
        visible,                                              // 传送页切换触发一次刷新。
        target_scene_id: selected_target_scene(browser).cloned(), // 根或目录稳定映射为None。
        portal_revision: session.portal_revision, // 当前草稿传送变化只在同图目标时生效，但比较成本固定。
        catalog_revision: registry.map_or(0, |registry| registry.revision), // 正式目录公开修订。
        selected_target: editor.portal_target.clone(), // 只克隆场景ID和可选端点ID。
    }
}

/// 目标场景、资产就绪或端点选择变化时刷新全部目标行与关系摘要。
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // 完整目标由两类资产组成，查询用Without证明可变组件互斥。
pub(crate) fn sync_target_portal_rows(
    editor: Res<SceneEditorState>,
    navigation: PortalSidebarNavigation,
    browser: Res<SceneBrowserState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<SceneContentRegistry>>,
    manifests: Res<Assets<SceneManifestAsset>>,
    portal_assets: Res<Assets<ScenePortalsAsset>>,
    row_entity_revision: Res<PortalRowEntityRevision>, // 跨帧新增目标行在创建同帧完成完整目标绑定。
    mut last_signature: Local<Option<TargetPortalSignature>>,
    added_containers: Query<(), Added<SceneEditorTargetPortalList>>,
    mut summaries: Query<
        &mut Text,
        (
            With<SceneEditorPortalTargetText>,
            Without<SceneEditorTargetPortalListTitle>,
            Without<SceneEditorTargetPortalRowText>,
        ),
    >,
    mut containers: Query<
        &mut Node,
        (
            With<SceneEditorTargetPortalList>,
            Without<SceneEditorTargetPortalRow>,
        ),
    >,
    mut rows: Query<
        (
            &mut SceneEditorTargetPortalRow,
            &mut Node,
            &mut BackgroundColor,
            &Children,
            Option<&mut feathrise_editor_ui::EditorButtonClick>,
        ),
        Without<SceneEditorTargetPortalList>,
    >,
    mut row_texts: Query<
        (&mut Text, &mut TextColor),
        (
            With<SceneEditorTargetPortalRowText>,
            Without<SceneEditorTargetPortalListTitle>,
            Without<SceneEditorPortalTargetText>,
        ),
    >,
    mut titles: Query<
        &mut Text,
        (
            With<SceneEditorTargetPortalListTitle>,
            Without<SceneEditorTargetPortalRowText>,
            Without<SceneEditorPortalTargetText>,
        ),
    >,
) {
    let signature = target_signature(
        &editor,
        &browser,
        &session,
        registry.as_deref(),
        navigation.visible(&editor),
    ); // 调用前冻结相关身份。
    let assets_changed = manifests.is_changed() || portal_assets.is_changed(); // 异步加载和热更必须绕过签名早退。
    let row_entities_changed = row_entity_revision.is_changed(); // 只在真实结构Commands排队时刷新新增行。
    let container_added = !added_containers.is_empty(); // 新BSN壳需要初始化完整列表和高度。
    if last_signature.as_ref() == Some(&signature)
        && !assets_changed
        && !row_entities_changed
        && !container_added
    {
        return; // 稳定帧不扫描目标传送数组或重新分配文案。
    }
    if !signature.visible {
        *last_signature = Some(signature); // 切回传送页时可见身份变化触发刷新。
        return;
    }

    let target_scene_id = selected_target_scene(&browser).cloned(); // 冻结场景树目标，避免后续格式化重复解析。
    let documents = target_scene_id.as_ref().map(|scene_id| {
        target_portal_documents(
            scene_id,              // 只解析场景树高亮的一个场景。
            &session,            // 同图优先读取未保存草稿。
            registry.as_deref(), // 其他场景从正式目录句柄读取。
            &manifests,          // 默认出生点来自清单。
            &portal_assets,      // 具体落点来自传送图。
        )
    });
    let display_context = PortalDisplayContext::new(&session, registry.as_deref(), &manifests); // 本次刷新统一从目标清单派生“前往{场景名}”。
    let total = match documents.as_ref() {
        Some(TargetPortalDocuments::Ready { portals, .. }) => portals.portals.len() + 1, // 默认出生点始终是首项。
        Some(TargetPortalDocuments::Loading | TargetPortalDocuments::Missing) | None => 0, // 半套资产不提供可点击行。
    };
    for mut title in &mut titles {
        **title = match (target_scene_id.as_ref(), documents.as_ref()) {
            (None, _) => "目标场景落点 · 请在场景页高亮场景".to_owned(),
            (Some(_), Some(TargetPortalDocuments::Loading)) => {
                "目标场景 · 资产加载中".to_owned() // 清单名称未Ready时不把SceneId冒充场景名。
            }
            (Some(_), Some(TargetPortalDocuments::Missing)) | (Some(_), None) => {
                "目标场景 · 数据不可用".to_owned() // 身份损坏状态不暴露内部SceneId。
            }
            (Some(_), Some(TargetPortalDocuments::Ready { manifest, .. })) => {
                format!("目标场景 · {} · {total}项", manifest.display_name)
            }
        }; // 标题明确场景树选择和资产完整状态。
    }
    let summary = match (
        target_scene_id.as_ref(),
        documents.as_ref(),
        editor.portal_target.as_ref(),
    ) {
        (None, _, _) => "传送关系：先在场景页高亮目标场景，再选择目标落点".to_owned(),
        (Some(_), Some(TargetPortalDocuments::Loading), _) => {
            "传送关系：目标场景落点资产加载中".to_owned() // 名称尚未Ready时也不回退显示MAP-*。
        }
        (Some(_), Some(TargetPortalDocuments::Missing) | None, _) => {
            "传送关系：目标场景数据不可用".to_owned() // 损坏引用给出状态语义而不是内部ID。
        }
        (Some(scene_id), Some(TargetPortalDocuments::Ready { manifest, portals }), Some(target))
            if target.scene_id == *scene_id =>
        {
            format!(
                "新传送点：中央点击位置 → {} / {}",
                manifest.display_name, // 当前选择摘要只保留作者能直接理解的场景名称。
                display_context.target_endpoint_label(target, portals), // 端点名称支持作者自定义区域语义。
            )
        }
        (Some(_), Some(TargetPortalDocuments::Ready { manifest, .. }), _) => {
            format!(
                "新点目标：已选场景{}；请在上方选择落点",
                manifest.display_name, // 标题已经提供场景上下文，不重复SceneId制造噪音。
            )
        }
    };
    for mut text in &mut summaries {
        **text = summary.clone(); // 静态壳通常只有一个摘要，clone保持防御性兼容。
    }

    for mut container in &mut containers {
        fit_portal_list(&mut container, total); // 全部Ready端点进入左栏整体滚动，空态完整退出布局。
    }
    for (mut row, mut node, mut background, children, click) in &mut rows {
        // 只有完整目录/作者签名刷新才进入此循环；旧按住不能借新目标或新修订获得释放资格。
        if let Some(mut click) = click { click.invalidate(); }
        let flat_index = row.slot; // 零是默认出生点，其余索引完整映射具体传送点数组。
        let value = match (target_scene_id.as_ref(), documents.as_ref()) {
            (Some(scene_id), Some(TargetPortalDocuments::Ready { .. }))
                if flat_index == 0 =>
            {
                Some((
                    ScenePortalTarget::default_spawn(scene_id.clone()), // None端点是正式安全出生点语义。
                    "◎ 默认起始位置".to_owned(), // 归属与安全坐标由目标上下文解析，不挤入窄栏单行。
                ))
            }
            (Some(scene_id), Some(TargetPortalDocuments::Ready { portals, .. })) => portals
                .portals
                .get(flat_index.saturating_sub(1))
                .map(|portal| {
                    let portal_label = display_context.portal_label(portal); // 目标点名称按它自己的目标场景显示名即时派生。
                    (
                        ScenePortalTarget::portal(scene_id.clone(), portal.id.clone()), // 点击发布目标场景与稳定端点ID。
                        format!("◇ {portal_label}"), // 名称不换行；引用仍保留完整场景和端点ID。
                    )
                }),
            _ => None, // 加载中、缺失或没有场景时清除旧行身份。
        };
        let Some((target, text_value)) = value else {
            row.target = None; // 结构换代边界清除已经越界的旧目标引用。
            node.display = Display::None; // 等待行实体收缩期间不参与布局或命中。
            continue;
        };
        let selected = editor.portal_target.as_ref() == Some(&target); // 场景ID与可选端点共同决定选择。
        row.target = Some(target); // 输入阶段会按当前资产再次验证该引用。
        node.display = Display::Flex; // 完整Ready端点才允许点击。
        background.0 = portal_row_background(UiInteraction::None, selected); // 恢复稳定背景。
        for child in children.iter() {
            if let Ok((mut text, mut color)) = row_texts.get_mut(child) {
                **text = text_value.clone(); // 每行只有一个文字子节点。
                color.0 = if selected {
                    colors::TEXT_ACCENT
                } else {
                    colors::TEXT_PRIMARY
                }; // 选中目标金色高亮。
            }
        }
    }
    *last_signature = Some(target_signature(
        &editor,
        &browser,
        &session,
        registry.as_deref(),
        navigation.visible(&editor),
    )); // 保存钳制后的最终身份。
}

/// 目标落点行提供即时悬停/按下反馈，并在松开后恢复显式选择。
pub(crate) fn sync_target_portal_row_interactions(
    editor: Res<SceneEditorState>,
    mut rows: Query<
        (
            &UiInteraction,
            &SceneEditorTargetPortalRow,
            &mut BackgroundColor,
        ),
        Changed<UiInteraction>,
    >,
) {
    for (interaction, row, mut background) in &mut rows {
        let Some(target) = row.target.as_ref() else {
            background.0 = colors::SLOT_BG; // 空槽永不保留上一页交互色。
            continue;
        };
        let selected = editor.portal_target.as_ref() == Some(target); // 完整目标身份避免跨场景同ID误高亮。
        background.0 = portal_row_background(*interaction, selected); // 统一四态样式。
    }
}
