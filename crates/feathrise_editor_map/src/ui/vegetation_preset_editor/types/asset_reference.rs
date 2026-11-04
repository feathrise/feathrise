//! 资产引用编辑的纯会话类型；不进入预设JSON或地图存档。

use super::{PresetEditorSection, PresetField};
use bevy::asset::UntypedHandle; // 最终验证仍保留真实资产类型身份和强句柄。
use bevy::gltf::Gltf; // 子资产只在真实GLTF根完整加载后枚举。
use bevy::prelude::*; // Handle、Image和组件所需基础类型。
use std::time::Instant; // 加载超时不受编辑器暂停和游戏时间倍率影响。 // 字段及分区身份由当前详细页提供。

/// 字段允许的资产类型，避免图片、模型、材质互相填错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum ReferenceKind {
    Image,     // Blade二维基础色纹理。
    BladeMesh, // Blade LOD原始网格，继续要求作者应用节点变换。
    Mesh,      // 当前Mesh部件的原始网格，不改部件变换。
    Material,  // 当前部件的StandardMaterial子资产。
}

/// 单字段目标同时冻结标签、分区、LOD/部件稳定ID和原值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct ReferenceTarget {
    pub(in super::super) tab_id: u64, // 多标签之间不因当前焦点改变而串写。
    pub(in super::super) tab_index: usize, // 轮询只按索引复核稳定ID，不逐帧搜索任意长已开标签列表。
    pub(in super::super) field: PresetField, // 只允许修改这一条强类型引用。
    pub(in super::super) section: PresetEditorSection, // 离开分区立即取消待赋值请求。
    pub(in super::super) lod_level: Option<u32>, // 除下标外再次校验LOD稳定级别。
    pub(in super::super) part_id: Option<u32>, // 当前部件的稳定身份。
    pub(in super::super) original_path: String, // 目标已被修改时拒绝迟到覆盖。
    pub(in super::super) description: String, // 给作者明确显示将修改哪个预设、LOD或部件。
}

/// 候选仅保存数字索引，不复制整模型或给每个候选创建实体/字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum SubAssetChoice {
    Mesh { mesh: usize, primitive: usize }, // 真实GLTF Mesh/Primitive编号。
    Material(usize),                        // 真实Material编号，采用时追加Bevy的/std派生标签。
    DefaultMaterial,                        // 只有真实默认材质存在时才提供。
}

/// 枚举状态不保存大模型副本；每帧只推进固定数量元数据项目。
#[derive(Default)]
pub(in super::super) struct ReferenceCursor {
    pub(in super::super) node: usize,            // Blade已验证节点数。
    pub(in super::super) mesh: usize,            // 当前GLTF Mesh数组游标。
    pub(in super::super) primitive: usize,       // 当前网格内primitive游标。
    pub(in super::super) material: usize,        // 材质数组游标。
    pub(in super::super) default_material: bool, // 默认材质已完成唯一一次检查。
}

/// 根加载、候选选择与最终类型验证分开，任何中间状态都不能改写字段。
pub(in super::super) enum ReferencePhase {
    Loading,                                            // AssetServer非阻塞加载根或图片。
    Collecting,                                         // 固定预算枚举全部合法候选。
    Choosing,                                           // 多候选等待作者显式选择，不默认第一个。
    Validating { path: String, handle: UntypedHandle }, // 真实Mesh/StandardMaterial成功加载后才赋值。
}

/// 唯一在途引用请求；旧票据、导航变化或加载失败均可零写入取消。
pub(in super::super) struct AssetReferenceRequest {
    pub(in super::super) ticket: u64, // 子资产按钮属于这一轮请求。
    pub(in super::super) target: ReferenceTarget, // 在点击“使用选中”时冻结完整目标。
    pub(in super::super) kind: ReferenceKind, // 输入和最终采用共同校验的资产类型。
    pub(in super::super) source_path: String, // 冻结右侧选择，不随用户继续浏览而替换。
    pub(in super::super) gltf: Option<Handle<Gltf>>, // 根句柄保活所有GLTF子资产。
    pub(in super::super) image: Option<Handle<Image>>, // 图片采用前验证真实Image而不是仅猜后缀。
    pub(in super::super) phase: ReferencePhase, // 加载、枚举、选择或验证。
    pub(in super::super) cursor: ReferenceCursor, // 有界元数据游标。
    pub(in super::super) choices: Vec<SubAssetChoice>, // POD候选表，释放不逐项析构模型或字符串。
    pub(in super::super) page: usize, // 只实例化当前八条候选。
    pub(in super::super) started: Instant, // 加载与最终验证的真实时间超时门。
}
