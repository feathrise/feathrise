//! 资产预览的轻量会话状态与专用实体标记。

use std::collections::{HashMap, HashSet, VecDeque}; // Scene0队列与GLTF材质/网格身份缓存都按当前文件会话持有。

use bevy::asset::AssetId; // 热更与缓存比较真实资产身份，不依赖作者名称。
use bevy::camera::primitives::Aabb; // Scene0已发布的局部边界可直接复用于单primitive取景。
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh}; // 根元数据及材质/网格身份保持强类型句柄。
use bevy::prelude::*; // Component、Entity、Handle与Resource。
use bevy::world_serialization::WorldAsset; // GLB/GLTF Scene0正式资产类型。
use feathrise_map_content::MapPlacementAssetTarget; // 只向摆放工具公开当前预览的稳定数字目标。

/// 资产预览最外层；没有文件选择时完全退出布局。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPanel;

/// 当前文件名标题。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewTitleText;

/// 图片、模型或说明共用的实际画布；模型取景读取它的真实宽高比。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewCanvas;

/// PNG/JPG/JPEG或二维KTX2的原比例图像节点。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewImage;

/// GLB/GLTF离屏相机输出节点。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewModelViewport;

/// 保留旧占位身份，只向底栏提供中央观察恢复说明，正文始终隐藏。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPlaceholder;

/// 领域加载器维护的操作回执与错误摘要，统一由底栏呈现。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewStatusText;

/// 从已采用图片读取的真实尺寸与格式；与可被复制/错误回执覆盖的状态文本分开。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewMetadataText;

/// 真实加载阶段与具体诊断分开回显，复制成功不能把Failed误标为Ready。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPhaseText;

/// 仅可交互内容显示的缩放和重置工具条。
#[derive(Component, Clone, Copy, Default)] // 只切Display，不随每次选择重建节点。
pub(super) struct AssetPreviewControls;

/// 缩放工具按钮使用独立配色查询，不覆盖子资产当前选中态。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewNavigationButton;

/// 工具条当前缩放倍率文本。
#[derive(Component, Clone, Copy, Default)] // 与加载状态文本使用独立marker。
pub(super) struct AssetPreviewZoomText;

/// 按图片或模型同步准确鼠标操作说明；正文隐藏，由标题旁?承载教程。
#[derive(Component, Clone, Copy, Default)] // 帮助文字不参与鼠标捕获。
pub(super) struct AssetPreviewNavigationHint;

/// GLB/GLTF才显示的固定子资产选择区；图片与数据文件完全退出布局。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewSubAssetPanel;

/// Scene数量、primitive总数和固定预算枚举进度摘要。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewSubAssetSummaryText;

/// 子资产目录尚未Ready时的进度或失败，仅由公共底栏展示。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewSubAssetStatusText;

/// 固定“完整场景 · Scene0”按钮，永远不随primitive数量增删实体。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewSceneRow;

/// 五槽primitive行池中的相对槽位；翻页只原位改写绑定。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AssetPreviewPrimitiveRow(pub(super) usize);

/// primitive行内文字与按钮分开标记，避免依赖Children顺序查找。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AssetPreviewPrimitiveRowText(pub(super) usize);

/// 多页时整体显示的固定分页行。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewSubAssetPagination;

/// 当前primitive页码；列表总量不是产品硬上限。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPageText;

/// 当前完整场景或primitive的主引用文字。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPrimaryReferenceText;

/// 只有primitive带有关联材质时才显示的第二条引用行。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewMaterialReferenceRow;

/// 当前primitive原关联StandardMaterial的完整引用文字。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewMaterialReferenceText;

/// 预览按钮不进入全局作者动作枚举，更不会产生地图修改。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)] // BSN模板需要显式安全默认值。
pub(super) enum AssetPreviewAction {
    ZoomOut, // 缩小当前图片或模型。
    #[default] // 默认仅恢复视图，不产生放大或作者数据副作用。
    Reset, // 清除临时观察偏移并恢复完整取景。
    ZoomIn,  // 放大当前图片或模型。
    ShowScene0, // 切回当前GLB的完整Scene0预览。
    ShowPrimitive(usize), // 选择当前页固定槽位绑定的真实primitive。
    PreviousPrimitivePage, // 只改变子资产页，不切换当前预览。
    NextPrimitivePage, // 只改变子资产页，不切换当前预览。
    CopyPrimaryReference, // 复制Scene0或当前primitive的完整Bevy引用。
    CopyMaterialReference, // 复制当前primitive原关联材质的/std引用。
}

/// 按下时冻结的预览手势；移出画布仍由同一动作持有原鼠标键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)] // 小型值状态不引用地图或作者数据。
pub(super) enum AssetPreviewDrag {
    ImagePan(MouseButton), // 图片允许左、右或中键平移。
    ModelOrbit,            // 模型右键只调整轨道方向。
    ModelPan,              // 模型中键只沿当前屏幕平面移动。
}

impl AssetPreviewDrag {
    /// 和弦中的其他按键不允许改变正在执行的手势。
    pub(super) fn button(self) -> MouseButton {
        match self {
            Self::ImagePan(button) => button,       // 图片使用实际起始按键。
            Self::ModelOrbit => MouseButton::Right, // 与地图透视视口一致。
            Self::ModelPan => MouseButton::Middle,  // 与地图平移一致。
        } // 释放判断只读取冻结按键。
    }
}

/// 独立预览Camera3d。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewCamera;

/// 独立预览方向光。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewLight;

/// 当前按需生成的Scene0预览根。
#[derive(Component, Clone, Copy, Default)]
pub struct AssetPreviewModelRoot;

/// 完整作者根撤销发布后先隐藏，等当帧Scene/动画延迟装配落地，再由公共Last阶段统一回收。
#[derive(Component, Clone, Copy, Default)]
pub struct AssetPreviewRetiredRoot;

/// 当前最多一个手工组合的primitive预览实体；与完整Scene0根互斥显示。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AssetPreviewPrimitiveRoot;

/// 文件扩展名决定的可视预览方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetPreviewFileKind {
    Image,       // 常见二维纹理；解码后还会复验是否为平面视图。
    Model,       // GLB或GLTF的Scene0。
    Audio,       // 白名单音频只用于试听，不进入地图摆放协议。
    Document,    // 白名单UTF-8文档、源码与配置，支持分页预览和显式编辑。
    Data,        // 二进制作者数据仅显示格式说明，不把内容误作图像或文字。
    Unsupported, // 其他当前没有专用预览器的文件。
}

/// 图片与模型优先使用专用预览；其余文件按共享文本白名单决定入口。
pub(crate) fn preview_file_kind(path: &str) -> AssetPreviewFileKind {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "ktx2" => AssetPreviewFileKind::Image,
        "glb" | "gltf" => AssetPreviewFileKind::Model,
        "ogg" | "wav" | "mp3" | "flac" => AssetPreviewFileKind::Audio,
        _ if crate::asset_catalog::document_format(path).is_some() => AssetPreviewFileKind::Document,
        "r16" | "bin" => AssetPreviewFileKind::Data,
        _ => AssetPreviewFileKind::Unsupported,
    }
}

/// 图片或模型当前是否已有可提交给UI的完整结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetPreviewPhase {
    Loading,       // AssetServer仍在异步读取或解析。
    Scanning,      // Scene0后代或单primitive顶点正在按固定预算计算完整范围。
    RevealPending, // 所有后代层命令已提交，下一帧才安全解除根隐藏。
    Ready,         // UI可以显示真实内容。
    Failed,        // 加载或格式验证失败；热更Ready仍可恢复。
}

/// 模型预览目标只属于当前F12会话，不写入地图、预设或设置。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AssetPreviewModelTarget {
    #[default]
    Scene0, // 文件选择后的安全默认值，明确只代表第零个Scene。
    Primitive(AssetPreviewPrimitiveId), // 只显示一个真实MeshN/PrimitiveN。
}

/// 中央页只固定文件身份和轻量观察状态；图片句柄、模型树与RTT仍由唯一预览会话持有。
#[derive(Debug)]
pub(super) struct ExpandedAssetPreview {
    pub(super) id: u64,
    pub(super) path: String,
    pub(super) return_tab: super::super::vegetation_preset_editor::PresetWorkspaceTab,
    pub(super) view: super::view_math::AssetPreviewView,
    pub(super) target: AssetPreviewModelTarget,
    pub(super) audio_position: f64, // 后台标签只记试听秒数，音频输出和解码数据仍由唯一会话持有。
    pub(super) restore_cursor: usize, // 重新加载模型后按固定预算找回原primitive，不同步遍历整个目录。
}

/// primitive编号使用GLTF正式索引，不以候选Vec下标冒充资产标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AssetPreviewPrimitiveId {
    pub(super) mesh: usize,      // GltfPrimitive::parent_mesh_index。
    pub(super) primitive: usize, // GltfPrimitive::index。
}

/// 候选仅保存数字身份、原关联材质编号和常数成本蒙皮判定，不复制模型、顶点或作者长名称。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AssetPreviewPrimitiveChoice {
    pub(super) id: AssetPreviewPrimitiveId, // 完整Mesh/Primitive标签身份。
    pub(super) material: Option<usize>,     // None严格表示GLTF默认材质。
    pub(super) requires_skinning: bool, // 蒙皮primitive保留可见目录行，但禁止脱离骨骼层级直接预览或摆放。
}

/// GLTF根加载与有界元数据枚举阶段；Ready前不向固定行池公开半套候选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AssetPreviewCatalogPhase {
    Loading,    // 等待根和递归依赖完整就绪。
    Collecting, // 每帧最多推进固定数量材质或primitive。
    Ready,      // scene数量和全部primitive已原子发布。
    Failed,     // 元数据失败不伪造子列表，完整Scene0仍保留自身诊断。
}

/// 枚举游标把材质表和primitive表纳入同一个主线程预算。
#[derive(Debug, Default)]
pub(super) struct AssetPreviewCatalogCursor {
    pub(super) material: usize,  // 先为材质句柄建立O(1)真实编号索引。
    pub(super) mesh: usize,      // 当前GltfMesh数组项。
    pub(super) primitive: usize, // 当前网格内部primitive项。
}

/// 当前文件的完整子资产目录；候选总量不限制，UI只使用固定五槽分页。
#[derive(Debug)]
pub(super) struct AssetPreviewModelCatalog {
    pub(super) gltf: Handle<Gltf>, // 强句柄保证枚举和作者选择期间根元数据存活。
    pub(super) phase: AssetPreviewCatalogPhase, // UI只在Ready后展示选择行。
    pub(super) cursor: AssetPreviewCatalogCursor, // 跨帧继续而非每帧从头扫描。
    pub(super) material_indices: HashMap<AssetId<GltfMaterial>, usize>, // primitive关联材质O(1)映射到MaterialN。
    pub(super) mesh_ids: HashSet<AssetId<GltfMesh>>, // 热更事件只使当前GLTF真实网格失效。
    pub(super) choices: Vec<AssetPreviewPrimitiveChoice>, // 完整候选采用后只读。
    pub(super) page: usize,                          // 当前五槽页，从零开始。
    pub(super) scene_count: usize, // 明确告诉作者Scene0不是多Scene文件的全部内容。
    pub(super) status: String,     // 子列表自己的加载或失败诊断，不覆盖画布状态。
}

impl AssetPreviewModelCatalog {
    /// 新文件从真实根加载开始，绝不沿用上一个GLB的候选或页码。
    pub(super) fn new(gltf: Handle<Gltf>) -> Self {
        Self {
            gltf,                                         // 保持根元数据和全部依赖的强引用。
            phase: AssetPreviewCatalogPhase::Loading,     // 首帧只轮询，不同步解析磁盘。
            cursor: AssetPreviewCatalogCursor::default(), // 从材质零号开始有界枚举。
            material_indices: HashMap::new(),             // 真实元数据到达前没有猜测索引。
            mesh_ids: HashSet::new(), // 每次首次访问一个真实GltfMesh时增量登记。
            choices: Vec::new(),      // 候选按实际primitive数量增长，不预设产品上限。
            page: 0,                  // 新文件默认显示第一组。
            scene_count: 0,           // 完成根读取后一次发布真实数量。
            status: "正在加载GLTF子资产目录".to_owned(), // 与Scene0画布加载并行说明。
        }
    }
}

/// 未被Scene0边界缓存覆盖时，单primitive按固定顶点预算继续计算局部Aabb。
#[derive(Debug)]
pub(super) struct AssetPreviewPrimitiveBoundsScan {
    pub(super) cursor: usize, // 下一待处理位置顶点下标。
    pub(super) minimum: Vec3, // 已完成切片的局部最小值。
    pub(super) maximum: Vec3, // 已完成切片的局部最大值。
}

impl Default for AssetPreviewPrimitiveBoundsScan {
    fn default() -> Self {
        Self {
            cursor: 0,                               // 第一帧从首顶点开始。
            minimum: Vec3::splat(f32::INFINITY),     // 任意有限首点都会正确收紧。
            maximum: Vec3::splat(f32::NEG_INFINITY), // 空网格最终会明确失败。
        }
    }
}

/// 当前单primitive的真实Mesh、StandardMaterial和边界进度。
#[derive(Debug)]
pub(super) struct AssetPreviewPrimitiveLoad {
    pub(super) generation: u64, // 换文件或热更后迟到加载不得生成旧实体。
    pub(super) choice: AssetPreviewPrimitiveChoice, // 与当前目标再次核对的数字身份。
    pub(super) mesh: Handle<Mesh>, // 精确#MeshN/PrimitiveN强句柄。
    pub(super) material: Handle<StandardMaterial>, // 原primitive关联的#MaterialN/std或DefaultMaterial/std。
    pub(super) bounds: AssetPreviewPrimitiveBoundsScan, // 不在一帧遍历任意大顶点数组。
}

/// GLB/GLTF文件会话把目录、目标和互斥预览实体绑定到同一代身份。
#[derive(Debug)]
pub(super) struct AssetPreviewModelSession {
    pub(super) generation: u64, // 当前文件或热更新代，零保留为初始无效值。
    pub(super) hot_reload_armed: bool, // 首次Scene与目录完整后再监听Modified，避免把同源标签补载误判为热更。
    pub(super) target: AssetPreviewModelTarget, // 默认Scene0或作者明确选择的primitive。
    pub(super) selected_primitive: Option<AssetPreviewPrimitiveChoice>, // 当前单体的材质身份保持O(1)可复制。
    pub(super) catalog: AssetPreviewModelCatalog,                       // 完整候选和独立分页状态。
    pub(super) primitive_load: Option<AssetPreviewPrimitiveLoad>, // 同时最多一个单体加载/扫描。
    pub(super) primitive_root: Option<Entity>,                    // 同时最多一个单体渲染实体。
    pub(super) scene_ready: bool, // WorldInstanceReady到达后才允许扫描或切回Scene0。
    pub(super) scene_bounds: Option<AssetPreviewModelBounds>, // 切回Scene0复用，不重复扫描稳定层级。
    pub(super) scene_mesh_count: usize,                       // 恢复Scene0时回显原完整网格数。
    pub(super) local_bounds: HashMap<AssetId<Mesh>, Aabb>, // Scene0扫描顺带缓存primitive局部Aabb。
}

impl AssetPreviewModelSession {
    /// 创建只读模型会话；选择文件永远先显示完整Scene0。
    pub(super) fn new(generation: u64, gltf: Handle<Gltf>) -> Self {
        Self {
            generation,                                   // 所有后续primitive加载结果必须匹配这一代。
            hot_reload_armed: false, // 首次加载产生的Added/Modified消息必须先排空，随后才接受真实热更。
            target: AssetPreviewModelTarget::Scene0, // 用户确认的默认完整场景行为。
            selected_primitive: None, // Scene0没有单独的材质引用。
            catalog: AssetPreviewModelCatalog::new(gltf), // 子目录与Scene0加载并行推进。
            primitive_load: None,    // 未明确点选时不加载单体副本。
            primitive_root: None,    // 默认只有Scene0根存在。
            scene_ready: false,      // 必须等待实际World实例通知。
            scene_bounds: None,      // 第一次WorldReady后有界计算。
            scene_mesh_count: 0,     // 尚未完成Scene0扫描。
            local_bounds: HashMap::new(), // 热更或换文件不沿用旧几何范围。
        }
    }
}

/// 当前选择持有的唯一预览资产。
#[derive(Debug)]
pub enum AssetPreviewContent {
    None,
    Image {
        handle: Handle<Image>,
        phase: AssetPreviewPhase,
    },
    Model {
        handle: Handle<WorldAsset>,
        root: Entity,
        phase: AssetPreviewPhase,
    },
    Audio {
        phase: AssetPreviewPhase, // 音频句柄、后台解码与唯一输出由独立试听模块维护。
    },
    Document,
    Data,
    Unsupported,
}

impl Default for AssetPreviewContent {
    fn default() -> Self {
        Self::None
    }
}

/// 一个Scene0联合包围盒的有界主世界扫描。
#[derive(Debug)]
pub(super) struct AssetPreviewBoundsScan {
    pub(super) root: Entity,              // 换选择或热更时严格核对当前根。
    pub(super) pending: VecDeque<Entity>, // 每帧只弹固定数量实体。
    pub(super) minimum: Vec3,             // 已看到网格世界Aabb最小值。
    pub(super) maximum: Vec3,             // 已看到网格世界Aabb最大值。
    pub(super) mesh_count: usize,         // 状态栏只回显实际网格数。
    pub(super) visited_count: usize,      // 进度说明不扫描队列外数据。
}

impl AssetPreviewBoundsScan {
    pub(super) fn new(root: Entity) -> Self {
        Self {
            root,
            pending: VecDeque::from([root]),
            minimum: Vec3::splat(f32::INFINITY),
            maximum: Vec3::splat(f32::NEG_INFINITY),
            mesh_count: 0,
            visited_count: 0,
        }
    }

    pub(super) fn has_bounds(&self) -> bool {
        self.minimum.is_finite() && self.maximum.is_finite() // 空Scene不伪造零尺寸网格。
    }
}

/// 完整Scene0扫描得到的稳定世界包围盒，用于窗口缩放后重新自动取景。
#[derive(Clone, Copy, Debug)]
pub struct AssetPreviewModelBounds {
    pub minimum: Vec3,
    pub maximum: Vec3,
}

/// 预览只属于当前F12会话；关闭即释放所选图片或Scene0强引用。
#[derive(Resource, Debug, Default)]
pub struct AssetPreviewState {
    pub external_preview: Option<crate::game_authoring::ExternalPreviewIdentity>, // 应用提交完整来源，普通资产不解释领域类型。
    pub(super) external_constructed: bool, // 组合根遵守应用完整Ready门，普通GLB仍保留原子热更与子资产扫描。
    pub(super) external_title: Option<String>, // 领域标题由实际作者应用提供，不在编辑器内绑定游戏术语。
    pub render_visible: bool, // 角色画布滚出裁剪视口后同时暂停动画与额外渲染。
    pub selected_path: Option<String>, // 与MapEditorState做O(1)身份比较。
    pub content: AssetPreviewContent, // 当前唯一强句柄与加载阶段。
    pub(crate) texture_preview: Option<Handle<super::compatible_texture::CompatiblePreviewTexture>>, // 唯一图片解码与安全label发布，元数据和画面属于同一代。
    pub(super) display_image: Option<Handle<Image>>, // 只保存经过过滤采样契约验证的输出，等待/失败立即释放UI绑定。
    pub status: String,       // 画布下方的具体进度或错误。
    pub(super) bounds_scan: Option<AssetPreviewBoundsScan>, // 只在Scene0 Ready后存在。
    pub model_bounds: Option<AssetPreviewModelBounds>, // 窗口尺寸变化只重算相机，不重扫Scene0。
    pub(super) model: Option<AssetPreviewModelSession>, // 仅模型选择持有子资产目录和单体预览会话。
    pub(super) next_model_generation: u64,              // 文件选择与热更新共用的迟到结果身份。
    pub(super) camera: Option<Entity>,                  // 固定离屏相机，从不接管主相机。
    pub(super) light: Option<Entity>,                   // 固定预览灯光，与相机共享专层。
    pub(super) _render_target: Option<Handle<Image>>,   // 显式持有RTT生命周期。
}

impl AssetPreviewState {
    /// 显式验收只读当前唯一RTT句柄；不能创建第二套预览相机或改写就绪状态。
    #[cfg(feature = "validation")]
    pub fn validation_render_target(&self) -> Option<Handle<Image>> { self._render_target.clone() }

    /// 与RTT一起冻结唯一预览相机，验收不能误认另一个指向同纹理的ViewportNode。
    #[cfg(feature = "validation")]
    pub fn validation_camera(&self) -> Option<Entity> { self.camera }

    /// 应用的完整组合根Ready后进入同一有界扫描，不向应用公开普通GLB目录和模型会话。
    pub fn begin_external_bounds_scan(&mut self, root: Entity) -> bool {
        if self.external_preview.is_none() || self.scene_root() != Some(root) {
            return false;
        }
        if let Some(model) = self.model.as_mut() {
            model.scene_ready = true;
            model.target = AssetPreviewModelTarget::Scene0;
            model.scene_bounds = None;
            model.scene_mesh_count = 0;
            model.local_bounds.clear();
        }
        if let AssetPreviewContent::Model { phase, .. } = &mut self.content {
            *phase = AssetPreviewPhase::Scanning;
        }
        self.bounds_scan = Some(AssetPreviewBoundsScan::new(root));
        self.model_bounds = None;
        true
    }

    /// 应用作者只读真实根、渲染资格及失败诊断，不另维护一套加载就绪判断。
    pub fn external_preview_status(&self) -> (Option<Entity>, bool, Option<&str>) {
        let root = self.external_preview.and_then(|_| self.scene_root());
        let ready = root.is_some() && self.render_visible
            && matches!(self.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. });
        let error = matches!(self.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Failed, .. })
            .then_some(self.status.as_str());
        (root, ready, error)
    }

    pub fn preview_rendering(&self) -> bool { self.render_visible }
    /// 把当前预览目标转换为可持久化摆放值；文件身份不匹配时安全回退Scene0。
    pub(crate) fn placement_target_for(
        &self,
        selected_path: &str,
    ) -> Result<MapPlacementAssetTarget, String> {
        if selected_path == crate::authoring::basic_shapes::CUBOID_SELECTION {
            return Ok(MapPlacementAssetTarget::DEFAULT_CUBOID); // 编辑期内建选择沿用普通摆放真实命中，token不进入持久资产路径。
        }
        if self.selected_path.as_deref() != Some(selected_path) {
            return Ok(MapPlacementAssetTarget::Scene0); // 浏览切换中的旧预览不能影响新文件摆放。
        }
        let Some(model) = self.model.as_ref() else {
            return Ok(MapPlacementAssetTarget::Scene0); // 非模型或尚未建立模型会话沿用历史语义。
        };
        match (model.target, model.selected_primitive) {
            (AssetPreviewModelTarget::Primitive(id), Some(choice)) if choice.id == id => {
                if choice.requires_skinning {
                    // 防御绕过点击入口构造出的蒙皮单体会话。
                    return Err("蒙皮primitive依赖骨骼层级，必须选择完整场景Scene0".to_owned()); // 不把缺失Skin的数据写进地图文件。
                } // 静态primitive继续沿用既有精确编号持久化。
                let mesh = u32::try_from(id.mesh)
                    .map_err(|_| "当前Mesh编号超出地图摆放格式范围".to_owned())?;
                let primitive = u32::try_from(id.primitive)
                    .map_err(|_| "当前Primitive编号超出地图摆放格式范围".to_owned())?;
                let material = choice
                    .material
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| "当前材质编号超出地图摆放格式范围".to_owned())?;
                Ok(MapPlacementAssetTarget::Primitive {
                    mesh,
                    primitive,
                    material,
                })
            }
            _ => Ok(MapPlacementAssetTarget::Scene0), // Scene0和任何不完整单体身份都不猜测子资产。
        }
    }

    /// 返回GLTF完整Scene0根，用于换文件时精确回收和WorldReady身份检查。
    pub fn scene_root(&self) -> Option<Entity> {
        match &self.content {
            AssetPreviewContent::Model { root, .. } => Some(*root),
            _ => None,
        }
    }

    /// 返回当前目标实际应该解除隐藏的唯一根。
    pub(super) fn current_model_root(&self) -> Option<Entity> {
        match self.model.as_ref()?.target {
            AssetPreviewModelTarget::Scene0 => self.scene_root(), // 完整模式只显示Scene0。
            AssetPreviewModelTarget::Primitive(_) => self.model.as_ref()?.primitive_root, // 单体模式只显示当前手工实体。
        }
    }

    /// 每次文件选择或模型热更都生成新身份，溢出时跳过零号。
    pub(super) fn allocate_model_generation(&mut self) -> u64 {
        let generation = self.next_model_generation.max(1); // Default的零号不能成为有效会话。
        self.next_model_generation = generation.wrapping_add(1).max(1); // 极端溢出仍回到有效编号。
        generation // 调用方与所有在途子资产共同冻结这一代。
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod placement_target_tests; // 摆放协议映射夹具独立维护，保持状态定义文件低于模块拆分阈值。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_extensions_are_case_insensitive_and_typed() {
        assert_eq!(
            preview_file_kind("textures/leaf.PNG"),
            AssetPreviewFileKind::Image
        );
        assert_eq!(
            preview_file_kind("gltf/tree.GLB"),
            AssetPreviewFileKind::Model
        );
        for path in [
            "maps/a.json", "maps/a.ron", "text/readme.MD", "shaders/test.wgsl",
            "scripts/init.py", "data/table.csv", "folder.with.dot/README",
        ] {
            assert_eq!(
                preview_file_kind(path), AssetPreviewFileKind::Document, "{path}"
            );
        }
        assert_eq!(
            preview_file_kind("models/tree.GLTF"), AssetPreviewFileKind::Model
        );
        assert_eq!(preview_file_kind("maps/height.r16"), AssetPreviewFileKind::Data);
        for path in ["audio/wind.OGG", "audio/剑声.WaV", "audio/music.MP3", "audio/wind.FlAc"] {
            assert_eq!(preview_file_kind(path), AssetPreviewFileKind::Audio, "{path}");
        }
        assert_eq!(preview_file_kind("audio/wind.aac"), AssetPreviewFileKind::Unsupported);
    }
}
