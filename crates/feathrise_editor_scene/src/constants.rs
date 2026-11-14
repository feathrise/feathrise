//! F12场景编辑器跨界面与作者事务共享的固定输入边界。

// 保存、恢复和鼠标拖动共用设置层边界，避免三处默认宽度各自演变。
pub(crate) const EDITOR_CONTENT_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH;
pub(crate) const EDITOR_INSPECTOR_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::DEFAULT_INSPECTOR_WIDTH;
pub(crate) const EDITOR_CONTENT_MIN_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::CONTENT_MIN_WIDTH;
pub(crate) const EDITOR_INSPECTOR_MIN_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::INSPECTOR_MIN_WIDTH;
pub(crate) const EDITOR_CONTENT_MAX_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::CONTENT_MAX_WIDTH;
pub(crate) const EDITOR_INSPECTOR_MAX_WIDTH: f32 = feathrise_editor_core::EditorLayoutPreferences::INSPECTOR_MAX_WIDTH;
pub(crate) const EDITOR_WORKSPACE_MIN_WIDTH: f32 = 360.0;
pub(crate) const EDITOR_SPLITTER_WIDTH: f32 = 6.0;

/// 场景Clipmap后台入口与添加地形面板共用厘米下限；不足一格仍保留来源两端。
pub(crate) const MIN_NEW_SCENE_EXTENT_METERS: f32 = 0.01;
/// 单轴允许的最大世界尺寸；与受检大场景量级一致，同时限制坐标与派生资源跨度。
pub(crate) const MAX_NEW_SCENE_EXTENT_METERS: f32 = 8_192.0;
/// 新建稀疏作者地形的默认目标格距；显示LOD和作者精度分离。
pub(crate) const TARGET_TERRAIN_CELL_METERS: f32 = 2.0;
/// 作者可输入的最细目标边长；所有合法场景尺寸都允许选择。
pub(crate) const MIN_TERRAIN_CELL_METERS: f32 = 0.125;
/// 作者可输入的最粗目标边长；两轴实际格距与逻辑格点数在弹框同步预览。
pub(crate) const MAX_TERRAIN_CELL_METERS: f32 = 64.0;
/// 水层静止水位允许的最低世界Y；沿用旧按钮作者域的防损坏边界。
pub(crate) const MIN_WATER_LEVEL_METERS: f32 = -10_000.0;
/// 水层静止水位允许的最高世界Y；沿用旧按钮作者域的防损坏边界。
pub(crate) const MAX_WATER_LEVEL_METERS: f32 = 10_000.0;
/// 水位输入保留到毫米；比旧半米步进精确，同时仍处在`f32`大场景坐标的有效精度内。
pub(crate) const WATER_LEVEL_INPUT_DECIMALS: u8 = 3;

/// F12 Mesh 植被预览只在正式运行时层 ID 的最高位增加会话所有权隔离标记。
pub(crate) const EDITOR_VEGETATION_PREVIEW_LAYER_BIT: u64 = 1 << 63;

/// F12虚拟场景文件夹的编辑器专用资产路径；正式运行时不读取该文件。
pub(crate) const SCENE_FOLDER_ASSET_PATH: &str = "scenes/editor.scene_folders.json";
/// 虚拟场景文件夹JSON当前格式版本。
pub(crate) const SCENE_FOLDER_SCHEMA_VERSION: u32 = 1;
/// 虚拟文件夹结构文件最大字节数，损坏或异常大文件不得无界分配。
pub(crate) const SCENE_FOLDER_MAX_FILE_BYTES: usize = 256 * 1024;
/// 单项目最多保存的虚拟文件夹数量。
pub(crate) const SCENE_FOLDER_MAX_FOLDERS: usize = 512;
/// 虚拟文件夹最多允许的嵌套层数。
pub(crate) const SCENE_FOLDER_MAX_DEPTH: usize = 16;
/// 单项目最多保存的场景归属记录数量。
pub(crate) const SCENE_FOLDER_MAX_ASSIGNMENTS: usize = 4_096;
/// 单个虚拟文件夹显示名称最多允许的Unicode字符数。
pub(crate) const SCENE_FOLDER_NAME_MAX_CHARS: usize = 48;
/// 左栏场景树滚动视口的固定虚拟行池容量；只生成可视行和少量越界缓冲。
pub(crate) const SCENE_BROWSER_ROW_POOL_COUNT: usize = 28;
/// 场景树每一行的逻辑像素高度。
pub(crate) const SCENE_BROWSER_ROW_HEIGHT: f32 = 26.0;
/// 场景树相邻行之间的逻辑像素间距。
pub(crate) const SCENE_BROWSER_ROW_GAP: f32 = 0.0;
/// 场景树一项在滚动内容中的完整逻辑像素跨度。
pub(crate) const SCENE_BROWSER_ROW_PITCH: f32 = SCENE_BROWSER_ROW_HEIGHT + SCENE_BROWSER_ROW_GAP;
/// 大目录快照每帧最多读取的文件夹或场景记录数。
pub(crate) const SCENE_BROWSER_INDEX_ITEMS_PER_FRAME: usize = 256;
/// 场景左键移动达到该物理像素距离后才正式进入拖拽。
pub(crate) const SCENE_BROWSER_DRAG_THRESHOLD_PHYSICAL_PIXELS: f32 = 6.0;
/// 拖拽时连续悬停半秒才展开，避免经过目录时误展开整排子树。
pub(crate) const SCENE_BROWSER_DRAG_EXPAND_SECONDS: f64 = 0.5;
/// 场景树上下各28逻辑像素作为滚动带，窄矮视口最多各占一半。
pub(crate) const SCENE_BROWSER_DRAG_SCROLL_EDGE: f32 = 28.0;
/// 鼠标越靠近边缘越快，最高每秒360逻辑像素；只滚场景树，不传递到外栏。
pub(crate) const SCENE_BROWSER_DRAG_SCROLL_SPEED: f32 = 360.0;
