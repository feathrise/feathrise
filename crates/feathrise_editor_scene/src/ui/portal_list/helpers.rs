//! 传送双列表不依赖ECS的目标场景解析和行文案辅助函数。

use bevy::prelude::Assets; // 目标场景只从已经异步载入的正式资产快照读取。
use feathrise_scene_content::{
    SceneContentRegistry, SceneId, SceneManifestAsset, ScenePortalRecord, ScenePortalTarget, ScenePortalsAsset,
}; // 行文本与目标解析直接读取正式作者数据类型。
use feathrise_portal::portal_display_label; // 世界标签和F12列表共用自动默认名与作者自定义名解析规则。

use crate::authoring::EditorAuthoringSession; // 当前场景目标必须优先包含尚未保存的同图草稿传送点。
use crate::resources::SceneBrowserState; // 场景树高亮项是目标场景唯一选择来源。
use crate::types::SceneBrowserItem; // 根、目录和场景三类高亮必须严格区分。

/// 目标场景作者文档解析结果；加载中与目录缺失必须使用不同提示。
pub(super) enum TargetPortalDocuments<'a> {
    Ready {
        manifest: &'a SceneManifestAsset, // 默认出生点与显示名称来自同一清单快照。
        portals: &'a ScenePortalsAsset,   // 具体目标行与arrival来自同一传送图快照。
    },
    Loading, // 目录已有强句柄但清单或传送图尚未进入Assets。
    Missing, // 场景树高亮已经不在正式目录或资产身份损坏。
}

/// 把稳定场景/传送点引用解析成作者可读名称；列表只在签名变化时使用，不进入逐帧全量扫描。
pub(crate) struct PortalDisplayContext<'a> {
    session: &'a EditorAuthoringSession, // 当前草稿中的未保存场景名称优先于正式资产快照。
    registry: Option<&'a SceneContentRegistry>, // 其他场景通过正式目录强句柄解析清单。
    manifests: &'a Assets<SceneManifestAsset>, // 已异步就绪的清单提供唯一正式显示名。
}

impl<'a> PortalDisplayContext<'a> {
    /// 构造一次当前Presentation刷新使用的只读名称解析上下文。
    pub(crate) fn new(
        session: &'a EditorAuthoringSession, // 同图未保存更名必须立即影响传送点名称。
        registry: Option<&'a SceneContentRegistry>, // 目录未就绪时允许显示明确占位名称。
        manifests: &'a Assets<SceneManifestAsset>, // 只做句柄到Ready清单的O(1)查询。
    ) -> Self {
        Self {
            session,   // 保留当前刷新周期的草稿只读引用。
            registry,  // 保留当前目录只读引用，不复制场景表。
            manifests, // 保留当前清单资产集合只读引用。
        }
    }

    /// 返回场景当前作者清单；同图草稿优先，其他场景必须通过正式目录和资产身份校验。
    fn scene_manifest(&self, scene_id: &SceneId) -> Option<&SceneManifestAsset> {
        if let Some(draft) = self
            .session
            .draft
            .as_ref()
            .filter(|draft| &draft.scene_id == scene_id)
        {
            return Some(&draft.manifest); // 同图未保存名称和默认出生点都来自最新草稿。
        }
        let handles = self.registry?.handles(scene_id)?; // 其他场景必须仍位于已采用的正式目录。
        let manifest = self.manifests.get(&handles.manifest)?; // 异步未Ready时由调用侧显示可读占位。
        (manifest.id == *scene_id).then_some(manifest) // 句柄内容身份不一致时禁止借用错误场景数据。
    }

    /// 返回场景当前显示名；稳定编号不得冒充作者名称。
    fn scene_display_name(&self, scene_id: &SceneId) -> Option<&str> {
        let manifest = self.scene_manifest(scene_id)?; // 名称与默认出生点共享同一份已验证清单。
        let name = manifest.display_name.trim(); // 正式清单名称是跨场景显示的唯一权威来源。
        (!name.is_empty()).then_some(name) // 空名称按未就绪处理，不泄露稳定ID兜底。
    }

    /// 生成单个传送点在世界Billboard与作者列表中共同使用的名称。
    pub(crate) fn portal_label(&self, portal: &ScenePortalRecord) -> String {
        self.scene_display_name(&portal.target.scene_id)
            .map(|target_name| {
                portal_display_label(&portal.label, portal.label_customized, target_name) // 作者自定义区域名优先于自动默认名。
            })
            .unwrap_or_else(|| {
                portal_display_label(&portal.label, portal.label_customized, "目标场景（未就绪）") // 目标未Ready时仍保留合法自定义名。
            })
    }

    /// 返回“显示名 [稳定编号]”；两套传送列表都用这一份场景身份格式。
    pub(super) fn scene_identity_label(&self, scene_id: &SceneId) -> String {
        let display_name = self
            .scene_display_name(scene_id)
            .unwrap_or("目标场景（未就绪）"); // 名称未就绪时仍保留引用中的真实编号供排错。
        format!("{display_name} [{scene_id}]") // 编号只占独立身份段，不混入传送点名称。
    }

    /// 返回当前源点绑定的目标场景身份；调用侧无需从“前往…”名称反向猜场景。
    pub(super) fn target_scene_identity_label(&self, target: &ScenePortalTarget) -> String {
        self.scene_identity_label(&target.scene_id) // 源点引用中的SceneId是第二段的唯一身份来源。
    }

    /// 把当前目标选择解析成简短端点名；完整场景身份和坐标由点击详情提供。
    pub(super) fn target_endpoint_label(
        &self,
        target: &ScenePortalTarget,  // None表示清单默认出生点。
        portals: &ScenePortalsAsset, // 具体ID只在目标传送图内部完成稳定解析。
    ) -> String {
        let Some(portal_id) = target.portal_id.as_deref() else {
            return "默认起始位置".to_owned(); // 默认目标不伪造传送点名称。
        };
        portals
            .portals
            .iter()
            .find(|portal| portal.id == portal_id) // 重名世界标签仍通过内部ID精确命中唯一记录。
            .map(|portal| self.portal_label(portal)) // 目标点使用自身可编辑Billboard名称。
            .unwrap_or_else(|| "目标传送点已失效".to_owned()) // 悬空ID不泄露portal-*内部值。
    }

    /// 返回当前源点在目标场景中的实际安全到达坐标，而不是目标端点的另一段名称。
    pub(super) fn current_target_arrival_label(
        &self,
        portal: &ScenePortalRecord, // 当前源点提供目标场景和可选目标传送点ID。
        portal_assets: &Assets<ScenePortalsAsset>, // 只有具体目标点才读取目标传送图。
    ) -> String {
        if portal.target.portal_id.is_none() {
            return self
                .scene_manifest(&portal.target.scene_id)
                .map(|manifest| scene_position_text(manifest.default_spawn)) // 默认目标取清单安全出生坐标。
                .unwrap_or_else(|| "坐标未就绪".to_owned()); // 不被无关传送图加载状态阻塞。
        }
        match target_portal_documents(
            &portal.target.scene_id, // 只解析该源点实际引用的一张目标场景。
            self.session,          // 同图目标优先使用未保存草稿。
            self.registry,         // 其他场景通过目录句柄读取。
            self.manifests,        // 清单身份与传送图共同校验。
            portal_assets,         // 具体目标ID必须存在于Ready传送图。
        ) {
            TargetPortalDocuments::Ready { portals, .. } => portal
                .target
                .portal_id
                .as_deref()
                .and_then(|portal_id| {
                    portals
                        .portals
                        .iter()
                        .find(|target_portal| target_portal.id == portal_id) // 稳定ID只用于命中实际目标记录。
                })
                .map(|target_portal| scene_position_text(target_portal.arrival)) // 具体目标必须显示运行时实际采用的安全arrival。
                .unwrap_or_else(|| "坐标已失效".to_owned()), // 悬空目标ID不能伪造坐标。
            TargetPortalDocuments::Loading => "坐标加载中".to_owned(), // 异步边界不回显旧缓存值。
            TargetPortalDocuments::Missing => "坐标已失效".to_owned(), // 目录或身份损坏给出明确状态。
        }
    }
}

/// 解析唯一目标场景文档；当前草稿场景优先使用未保存传送点，其他场景只读正式热加载资产。
pub(super) fn target_portal_documents<'a>(
    scene_id: &SceneId,                             // 场景树当前高亮的稳定目标场景。
    session: &'a EditorAuthoringSession,        // 当前场景可能包含尚未保存的目标传送点。
    registry: Option<&'a SceneContentRegistry>,   // 其他场景通过目录强句柄取得私有资产。
    manifests: &'a Assets<SceneManifestAsset>,    // 已异步解码的目标清单集合。
    portal_assets: &'a Assets<ScenePortalsAsset>, // 已异步解码的目标传送图集合。
) -> TargetPortalDocuments<'a> {
    if let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| &draft.scene_id == scene_id)
    {
        return TargetPortalDocuments::Ready {
            manifest: &draft.manifest, // 同图默认出生点读取当前作者草稿。
            portals: &draft.portals,   // 同图目标列表包含未保存的新传送点。
        };
    }
    let Some(handles) = registry.and_then(|registry| registry.handles(scene_id)) else {
        return TargetPortalDocuments::Missing; // 不能从已经热更移除的目录项猜测路径。
    };
    let (Some(manifest), Some(portals)) = (
        manifests.get(&handles.manifest), // 清单和传送图必须同时就绪才公开可选落点。
        portal_assets.get(&handles.portals), // 半套目标文档不得让作者创建运行时必失败引用。
    ) else {
        return TargetPortalDocuments::Loading; // AssetServer下一帧就绪后Presentation会重试。
    };
    if manifest.id != *scene_id || portals.scene_id != *scene_id {
        return TargetPortalDocuments::Missing; // 跨文件身份不一致时拒绝把错误场景内容列为目标。
    }
    TargetPortalDocuments::Ready { manifest, portals } // 完整同身份快照可以原子用于列表和点击校验。
}

/// 判断目标端点是否仍存在于已经完整解析的目标场景文档。
pub(super) fn target_exists(
    target: &ScenePortalTarget,
    documents: &TargetPortalDocuments<'_>,
) -> bool {
    let TargetPortalDocuments::Ready { portals, .. } = documents else {
        return false; // 加载中或缺失状态不能证明目标有效。
    };
    match target.portal_id.as_deref() {
        None => true, // 默认出生点由同一Ready清单保证存在。
        Some(portal_id) if portal_id.trim().is_empty() => false, // 空字符串是损坏旧引用，不等于默认出生点。
        Some(portal_id) => portals.portals.iter().any(|portal| portal.id == portal_id), // 具体ID必须仍在目标传送图中。
    }
}

/// 场景树只有高亮真实场景行时才提供传送目标场景。
pub(super) fn selected_target_scene(browser: &SceneBrowserState) -> Option<&SceneId> {
    match browser.selected_item.as_ref() {
        Some(SceneBrowserItem::Scene(scene_id)) => Some(scene_id), // 单击场景只选择目标，不触发正式进入。
        Some(SceneBrowserItem::Root | SceneBrowserItem::Folder(_)) | None => None, // 根和目录不能冒充目的场景。
    }
}

/// 把三维位置压缩成固定一位小数，当前列表和默认出生点列表保持同一坐标格式。
fn scene_position_text(position: [f32; 3]) -> String {
    let display_component = |value: f32| {
        if value.abs() < 0.05 { 0.0 } else { value } // 一位小数会归零的负值不得显示成“-0.0”。
    };
    format!(
        "{:.1},{:.1},{:.1}",
        display_component(position[0]),
        display_component(position[1]),
        display_component(position[2]),
    )
}

/// 所有传送行共享“名称 | 场景名称编号 | 到达坐标”骨架，避免两套列表再次发生格式漂移。
fn portal_endpoint_row_text(
    marker: &str,             // 菱形表示传送点，靶心表示默认出生点。
    endpoint_name: &str,      // 第一段是开发者可读名称。
    scene_identity_label: &str, // 第二段是显示名与稳定SceneId。
    arrival_label: &str,      // 第三段是实际安全到达坐标或明确失效状态。
) -> String {
    format!("{marker} {endpoint_name} | {scene_identity_label} | {arrival_label}") // 竖线是固定字段边界。
}

/// 当前源点点击详情按三段显示名称、目标场景身份和目标安全坐标。
pub(super) fn current_portal_row_text(
    portal_label: &str,              // 第一段使用源点世界Billboard名称。
    target_scene_identity_label: &str, // 第二段使用目标场景名称与编号。
    target_arrival_label: &str,      // 第三段使用目标default_spawn或arrival。
) -> String {
    portal_endpoint_row_text(
        "◇",                       // 普通传送点使用空心菱形。
        portal_label,              // 名称不得回退为传送点流水号。
        target_scene_identity_label, // 场景名和SceneId必须同时可见。
        target_arrival_label,      // 实际到达坐标不得替换成端点类型名。
    )
}

/// 默认出生点点击详情保留安全坐标；紧凑列表独立显示简短名称。
pub(super) fn default_spawn_row_text(default_spawn: [f32; 3]) -> String {
    format!("◎ 默认起始位置 · {}", scene_position_text(default_spawn)) // 不重复场景身份制造噪音。
}

/// 具体目标点的点击详情描述它自己的外传目标，而不是重复当前所在场景。
pub(super) fn target_portal_row_text(
    portal_label: &str,              // 第一段与该点可编辑Billboard名称一致。
    target_scene_identity_label: &str, // 第二段是该点自己的目标场景名称与编号。
    target_arrival_label: &str,      // 第三段是该点实际传往目标场景的坐标。
) -> String {
    portal_endpoint_row_text(
        "◇",                       // 具体传送点使用空心菱形。
        portal_label,              // 例如作者自定义“前往王府”。
        target_scene_identity_label, // 例如目标“a [MAP-a]”，而不是点当前所在的b。
        target_arrival_label,      // 使用该点绑定目标的default_spawn或arrival。
    )
}

#[cfg(test)]
mod tests {
    use super::*; // 行文案回归使用与生产完全相同的格式化入口。

    #[test]
    fn snapshot_portal_uses_actual_default_target_arrival_instead_of_its_source_position() {
        let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
        let mut app = bevy::prelude::App::new();
        app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..Default::default() })
            .init_resource::<crate::resources::SceneEditorState>();
        crate::ui::scene_inspector::set_selected_tool_properties_fixture(&mut app,
            Some(crate::ui::scene_inspector::SelectedToolPropertiesKind::Portal));
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let portal = &draft.portals.portals[0];
        assert_eq!(draft.portals.scene_id, draft.scene_id);
        assert_eq!(portal.id, "portal");
        assert_eq!(portal.position, [1.0, 0.0, 1.0]);
        assert_eq!(portal.arrival, [2.0, 0.0, 2.0]);
        assert_eq!(portal.target, ScenePortalTarget::default_spawn(draft.scene_id.clone()));
        let manifests = Assets::<SceneManifestAsset>::default();
        let portals = Assets::<ScenePortalsAsset>::default();
        let display = PortalDisplayContext::new(session, None, &manifests);
        assert_eq!(display.current_target_arrival_label(portal, &portals), scene_position_text(draft.manifest.default_spawn));
        assert_eq!(draft.manifest.default_spawn, [5.0, 0.0, 5.0]);
        assert!(session.portal_revision > 0);
    }

    #[test]
    fn portal_details_describe_each_portals_own_outbound_route_without_default_noise() {
        assert_eq!(
            current_portal_row_text("前往b", "b [MAP-1788077435114]", "0.0,2.0,0.0"), // 三段分别传入，不互相猜测。
            "◇ 前往b | b [MAP-1788077435114] | 0.0,2.0,0.0", // 作者可以同时核对名称、场景身份和实际到达坐标。
        );
        assert_eq!(
            default_spawn_row_text([0.0, 2.0, 0.0]), // 默认点归属已由目标标题表达，不重复整段场景身份。
            "◎ 默认起始位置 · 0.0,2.0,0.0",
        );
        assert_eq!(
            target_portal_row_text("前往a", "a [MAP-1787934853043]", "8.0,2.0,-3.0"), // b中的点描述它自己传往a的关系。
            "◇ 前往a | a [MAP-1787934853043] | 8.0,2.0,-3.0",
        );
        assert_eq!(scene_position_text([12.34, -0.04, 5.96]), "12.3,0.0,6.0"); // 坐标固定为紧凑一位小数且不产生负零。
    }
}
