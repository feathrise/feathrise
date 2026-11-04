//! 虚拟地图文件夹的格式校验、树整理、会话定位和拖拽阈值纯函数。
//!
//! 本文件不访问ECS或磁盘，加载器、后台事务与UI系统必须复用这里的同一组约束。

use std::collections::{BTreeMap, BTreeSet}; // 稳定查重、父链解析和确定性测试输出。

use bevy::math::Vec2; // 拖拽阈值只比较物理像素向量。
use feathrise_map_content::MapId; // 地图归属键必须继续使用正式稳定身份语法。

use crate::constants::{
    MAP_BROWSER_DRAG_THRESHOLD_PHYSICAL_PIXELS, MAP_FOLDER_MAX_ASSIGNMENTS, MAP_FOLDER_MAX_DEPTH,
    MAP_FOLDER_MAX_FILE_BYTES, MAP_FOLDER_MAX_FOLDERS, MAP_FOLDER_NAME_MAX_CHARS,
    MAP_FOLDER_SCHEMA_VERSION,
}; // 所有入口共享同一组固定文件、层级和交互预算。
use crate::types::{MapBrowserEntry, MapBrowserItem, MapFolderDocument, MapFolderRecord}; // 纯函数只操作普通数据类型。

/// 校验并返回去除首尾空白后的文件夹显示名称。
pub(crate) fn normalize_folder_name(value: &str) -> Result<String, String> {
    let normalized = value.trim(); // 作者无意输入的首尾空格不形成肉眼相同的不同名称。
    if normalized.is_empty() {
        return Err("文件夹名称不能为空".to_owned()); // 空名称无法在固定行池中可靠选择。
    }
    let character_count = normalized.chars().count(); // Unicode名称按可见字符而不是UTF-8字节计数。
    if character_count > MAP_FOLDER_NAME_MAX_CHARS {
        return Err(format!(
            "文件夹名称最多允许{MAP_FOLDER_NAME_MAX_CHARS}个字符"
        )); // 超长作者字符串不得挤坏紧凑左栏。
    }
    if normalized == "." || normalized == ".." {
        return Err("文件夹名称不能是 . 或 ..".to_owned()); // 虚拟结构也不使用文件系统跳转语义。
    }
    if normalized
        .chars()
        .any(|character| character.is_control() || character == '/' || character == '\\')
    {
        return Err("文件夹名称不能包含控制字符、/ 或 \\".to_owned()); // 名称不能冒充路径或破坏JSON回显。
    }
    Ok(normalized.to_owned()) // 后续查重、显示和持久化全部使用同一规范值。
}

/// 判断稳定文件夹ID是否符合编辑器生成的受限ASCII形状。
fn valid_folder_id(value: &str) -> bool {
    value.starts_with("FOLDER-") // 保留明显且可审计的编辑器文件夹身份前缀。
        && value.len() > "FOLDER-".len() // 空后缀不能成为稳定身份。
        && value.len() <= 80 // 异常外部文件不得用超长ID放大路径和诊断字符串。
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') // 只接受路径无关ASCII身份字符。
}

/// 完整校验虚拟文件夹文档的版本、预算、名称、父链、层级和地图归属。
pub(crate) fn validate_folder_document(document: &MapFolderDocument) -> Result<(), String> {
    if document.schema_version != MAP_FOLDER_SCHEMA_VERSION {
        return Err(format!(
            "地图归类版本{}与编辑器版本{}不一致",
            document.schema_version, MAP_FOLDER_SCHEMA_VERSION
        )); // 不兼容版本必须先迁移，禁止静默覆盖。
    }
    if document.folders.len() > MAP_FOLDER_MAX_FOLDERS {
        return Err(format!(
            "虚拟文件夹数量{}超过{}项上限",
            document.folders.len(),
            MAP_FOLDER_MAX_FOLDERS
        )); // 目录树规模保持固定最坏预算。
    }
    if document.map_folders.len() > MAP_FOLDER_MAX_ASSIGNMENTS {
        return Err(format!(
            "地图归属数量{}超过{}项上限",
            document.map_folders.len(),
            MAP_FOLDER_MAX_ASSIGNMENTS
        )); // 损坏JSON不能形成无界地图键集合。
    }

    let mut folder_by_id = BTreeMap::new(); // 父链与归属验证使用稳定只读索引。
    let mut sibling_names = BTreeSet::new(); // 同级名称按规范化小写键拒绝重复。
    for folder in &document.folders {
        if !valid_folder_id(&folder.id) {
            return Err(format!("虚拟文件夹ID不合法：{}", folder.id)); // 外部文件不得注入路径或不可显示身份。
        }
        let normalized_name = normalize_folder_name(&folder.display_name)?; // 文件中也必须已经保存规范名称。
        if normalized_name != folder.display_name {
            return Err(format!("文件夹“{}”名称包含首尾空白", folder.display_name)); // 加载不静默改写作者字节。
        }
        if folder_by_id.insert(folder.id.as_str(), folder).is_some() {
            return Err(format!("虚拟文件夹ID重复：{}", folder.id)); // 稳定身份必须全局唯一。
        }
        let sibling_key = (
            folder.parent_id.as_deref().unwrap_or("").to_owned(), // 根目录使用空父键参与同级查重。
            normalized_name.to_lowercase(), // 名称大小写不同仍视为同一个作者分类。
        );
        if !sibling_names.insert(sibling_key) {
            return Err(format!("同级虚拟文件夹名称重复：{normalized_name}")); // 固定行中不能出现不可区分同名项。
        }
    }

    for folder in &document.folders {
        if let Some(parent_id) = folder.parent_id.as_deref() {
            if parent_id == folder.id {
                return Err(format!("文件夹{}不能把自己设为父级", folder.id)); // 最短自环明确拒绝。
            }
            if !folder_by_id.contains_key(parent_id) {
                return Err(format!("文件夹{}引用不存在的父级{parent_id}", folder.id)); // 失效父级不能被猜回根目录。
            }
        }
        let mut visited = BTreeSet::new(); // 单条父链最多十六层，局部分配有固定小上限。
        let mut current_id = Some(folder.id.as_str()); // 从当前文件夹自身计入第一层。
        let mut depth = 0_usize; // 根子文件夹深度为一。
        while let Some(folder_id) = current_id {
            if !visited.insert(folder_id) {
                return Err(format!("文件夹{}的父级链形成循环", folder.id)); // 任意长度循环都拒绝采用整份文档。
            }
            depth += 1; // 每经过一个真实文件夹增加一层。
            if depth > MAP_FOLDER_MAX_DEPTH {
                return Err(format!(
                    "文件夹{}层级超过{}层上限",
                    folder.id, MAP_FOLDER_MAX_DEPTH
                )); // 过深结构不能拖慢路径解析或让导航失控。
            }
            current_id = folder_by_id
                .get(folder_id) // 当前ID已在第一阶段全量校验存在。
                .and_then(|current| current.parent_id.as_deref()); // `None`正常结束于虚拟根。
        }
    }

    for (map_id, folder_id) in &document.map_folders {
        MapId::parse(map_id.clone())
            .map_err(|error| format!("地图归属键{map_id}不是合法MapId：{error}"))?; // 归类只引用正式稳定地图身份。
        if !folder_by_id.contains_key(folder_id.as_str()) {
            return Err(format!("地图{map_id}引用不存在的虚拟文件夹{folder_id}")); // 失效归属不能静默挪到根目录。
        }
    }
    Ok(()) // 所有结构约束通过后才允许加载、保存或事务合并。
}

/// 从受限UTF-8字节解码并完整校验归类文档。
pub(crate) fn decode_folder_document(bytes: &[u8]) -> Result<MapFolderDocument, String> {
    if bytes.len() > MAP_FOLDER_MAX_FILE_BYTES {
        return Err(format!(
            "地图归类文件{}字节，超过{}字节上限",
            bytes.len(),
            MAP_FOLDER_MAX_FILE_BYTES
        )); // 在Serde分配任何目录结构前拒绝异常大文件。
    }
    let document = serde_json::from_slice::<MapFolderDocument>(bytes)
        .map_err(|error| format!("解析地图归类JSON失败：{error}"))?; // 损坏JSON绝不能被默认空结构覆盖。
    validate_folder_document(&document)?; // 加载与写入共用完全相同的结构门。
    Ok(document) // 调用方可以原子采用整份有效文档。
}

/// 以漂亮UTF-8 JSON和终止换行编码完整归类文档。
pub(crate) fn encode_folder_document(document: &MapFolderDocument) -> Result<Vec<u8>, String> {
    validate_folder_document(document)?; // 任何内存操作遗漏校验都不能越过最终写入门。
    let mut bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("序列化地图归类JSON失败：{error}"))?; // 序列化错误保持旧磁盘文件不变。
    bytes.push(b'\n'); // 与项目其他作者JSON保持一致，便于Git审阅。
    if bytes.len() > MAP_FOLDER_MAX_FILE_BYTES {
        return Err(format!(
            "地图归类文件编码后{}字节，超过{}字节上限",
            bytes.len(),
            MAP_FOLDER_MAX_FILE_BYTES
        )); // 字符串UTF-8体积仍受最终文件硬预算约束。
    }
    Ok(bytes) // 原子写入只接收已经完整编码和验收的字节。
}

/// 查找稳定ID对应的虚拟文件夹。
pub(crate) fn folder_by_id<'a>(
    document: &'a MapFolderDocument, // 当前已验证归类文档。
    folder_id: &str,                 // 需要解析的稳定文件夹身份。
) -> Option<&'a MapFolderRecord> {
    document
        .folders
        .iter()
        .find(|folder| folder.id == folder_id) // 最多512项且只在结构变化或显式操作时调用。
}

/// 根据当前树选择返回新建文件夹或新地图应使用的容器；缺少选择时使用根目录。
pub(crate) fn selected_container_folder_id(
    document: &MapFolderDocument,           // 当前已经严格校验的归类文档。
    selected_item: Option<&MapBrowserItem>, // 根、目录、地图或空选择四种会话上下文。
) -> Option<String> {
    match selected_item {
        Some(MapBrowserItem::Folder(folder_id)) if folder_by_id(document, folder_id).is_some() => {
            Some(folder_id.clone()) // 选择目录时把新内容直接放入该目录。
        }
        Some(MapBrowserItem::Map(map_id)) => document.map_folders.get(map_id.as_str()).cloned(), // 选择地图时沿用它的直接父目录。
        Some(MapBrowserItem::Root) | Some(MapBrowserItem::Folder(_)) | None => None, // 根、失效目录或无选择都安全回到根目录。
    }
}

/// 展开目标文件夹及其全部祖先，使新建或重新定位后的地图能够立即出现在树中。
pub(crate) fn expand_folder_ancestors(
    document: &MapFolderDocument,    // 父链来自当前已经校验的归类文档。
    folder_id: Option<&str>,         // `None`表示根目录，不需要写入展开集合。
    expanded: &mut BTreeSet<String>, // 当前会话展开集合，不写入JSON。
) -> bool {
    let Some(mut current_id) = folder_id else {
        return false; // 根目录固定展开，不需要修改会话集合。
    };
    let mut changed = false; // 只有真实新增身份时才推进展开修订。
    for _ in 0..MAP_FOLDER_MAX_DEPTH {
        let Some(folder) = folder_by_id(document, current_id) else {
            break; // 热更新错帧遇到失效身份时停止，不制造不存在的展开项。
        };
        changed |= expanded.insert(folder.id.clone()); // 当前目录必须展开后才能显示直属地图。
        let Some(parent_id) = folder.parent_id.as_deref() else {
            break; // 已到根目录直属文件夹，完整父链已经展开。
        };
        current_id = parent_id; // 下一轮继续展开父目录。
    }
    changed // 调用方据此只在必要时推进投影身份。
}

/// 判断地图在当前展开集合中是否真正可见，避免删除后把隐藏地图设为操作高亮。
pub(crate) fn map_is_visible_in_tree(
    document: &MapFolderDocument, // 当前完整归类树。
    expanded: &BTreeSet<String>,  // 当前会话展开集合。
    map_id: &MapId,               // 需要检查的正式地图身份。
) -> bool {
    let Some(mut current_id) = document
        .map_folders
        .get(map_id.as_str())
        .map(String::as_str)
    else {
        return true; // 根目录直属地图始终可见。
    };
    for _ in 0..MAP_FOLDER_MAX_DEPTH {
        if !expanded.contains(current_id) {
            return false; // 任一折叠祖先都会隐藏该地图。
        }
        let Some(folder) = folder_by_id(document, current_id) else {
            return false; // 防御性拒绝失效归属，正式文档校验不会到达此处。
        };
        let Some(parent_id) = folder.parent_id.as_deref() else {
            return true; // 已验证到根目录且整条父链均展开。
        };
        current_id = parent_id; // 继续检查上一级是否展开。
    }
    false // 超过深度预算只可能来自未受检输入，安全视为不可见。
}

/// 判断目标文件夹是否没有子文件夹和地图归属。
pub(crate) fn folder_is_empty(document: &MapFolderDocument, folder_id: &str) -> bool {
    !document
        .folders
        .iter()
        .any(|folder| folder.parent_id.as_deref() == Some(folder_id)) // 任何直接子目录都阻止删除。
        && !document
            .map_folders
            .values()
            .any(|assigned_folder| assigned_folder == folder_id) // 任意地图归属都阻止删除。
}

/// 判断同一父级是否已存在目标名称，可排除正在重命名的自身ID。
pub(crate) fn sibling_name_exists(
    document: &MapFolderDocument, // 当前完整文件夹集合。
    parent_id: Option<&str>,      // 新建或目标记录的父目录。
    display_name: &str,           // 已规范化的待检查名称。
    exclude_id: Option<&str>,     // 重命名时忽略目标自身。
) -> bool {
    let folded = display_name.to_lowercase(); // 大小写不同仍属于重复名称。
    document.folders.iter().any(|folder| {
        folder.id.as_str() != exclude_id.unwrap_or("") // 重命名自身保持原名合法。
            && folder.parent_id.as_deref() == parent_id // 只在同一虚拟父目录查重。
            && folder.display_name.to_lowercase() == folded // Unicode小写键用于人类可见名称比较。
    })
}

/// 生成当前文档内唯一且可读的稳定文件夹ID。
pub(crate) fn next_folder_id(
    document: &MapFolderDocument, // 已存在ID集合。
    unix_millis: u128,            // 调用方冻结的当前Unix毫秒。
) -> Result<String, String> {
    let base = format!("FOLDER-{unix_millis}"); // 首选ID与地图创建器同样具备可审计时间来源。
    for suffix in 0_u32..10_000_u32 {
        let candidate = if suffix == 0 {
            base.clone() // 同毫秒首次创建使用最短稳定ID。
        } else {
            format!("{base}-{suffix}") // 极端同毫秒多次创建以有界序号去重。
        };
        if !document.folders.iter().any(|folder| folder.id == candidate) {
            return Ok(candidate); // 不依赖显示名，重命名不会改变身份。
        }
    }
    Err("同一毫秒内生成的文件夹ID已超过上限".to_owned()) // 异常碰撞下安全拒绝而非覆盖旧记录。
}

/// 把地图归属改到目标文件夹；`None`通过删除键表示根目录。
pub(crate) fn assign_map_to_folder(
    document: &mut MapFolderDocument, // 乐观修改的完整文档副本。
    map_id: &MapId,                   // 正式稳定地图身份。
    folder_id: Option<&str>,          // 目标文件夹或虚拟根目录。
) -> Result<(), String> {
    if let Some(folder_id) = folder_id {
        if folder_by_id(document, folder_id).is_none() {
            return Err(format!("目标虚拟文件夹{folder_id}不存在")); // 迟到拖拽不能写入已热更删除的目标。
        }
        document
            .map_folders
            .insert(map_id.as_str().to_owned(), folder_id.to_owned()); // 一个地图始终只有一个归属。
    } else {
        document.map_folders.remove(map_id.as_str()); // 根目录无需保存冗余特殊ID。
    }
    validate_folder_document(document) // 最终预算和结构门仍覆盖本次修改。
}

/// 只检查移动涉及的源、同级名称与目标父链；悬停缓存该结果，不扫描地图归属。
pub(crate) fn folder_move_target_allowed(
    document: &MapFolderDocument,
    folder_id: &str,
    parent_id: Option<&str>,
) -> Result<(), String> {
    let folder =
        folder_by_id(document, folder_id).ok_or_else(|| "待移动的文件夹已不存在".to_owned())?;
    let mut current = parent_id;
    for _ in 0..MAP_FOLDER_MAX_DEPTH {
        let Some(id) = current else {
            if sibling_name_exists(document, parent_id, &folder.display_name, Some(folder_id)) {
                return Err("目标目录已有同名文件夹，请先重命名".to_owned());
            }
            return Ok(());
        };
        if id == folder_id {
            return Err("不能把文件夹移入自身或子文件夹".to_owned());
        }
        current = folder_by_id(document, id)
            .ok_or_else(|| "目标文件夹已不存在".to_owned())?
            .parent_id
            .as_deref();
    }
    Err("移动后的文件夹层级超过上限".to_owned())
}

/// 只改父级链接，子目录和地图继续引用原稳定ID，整棵子树自然随父目录移动。
pub(crate) fn move_folder(
    document: &mut MapFolderDocument,
    folder_id: &str,
    parent_id: Option<&str>,
) -> Result<bool, String> {
    folder_move_target_allowed(document, folder_id, parent_id)?;
    let index = document
        .folders
        .iter()
        .position(|folder| folder.id == folder_id)
        .ok_or_else(|| "待移动的文件夹已不存在".to_owned())?;
    if document.folders[index].parent_id.as_deref() == parent_id {
        return Ok(false); // 原父级放回不启动保存，也不推进归类修订。
    }
    let previous = document.folders[index].parent_id.clone();
    document.folders[index].parent_id = parent_id.map(str::to_owned);
    if let Err(error) = validate_folder_document(document) {
        document.folders[index].parent_id = previous; // 子树最深处超限等失败必须保留完整旧结构。
        return Err(error);
    }
    Ok(true)
}

/// 递归追加一个父目录的子树；输入已经严格校验，所以递归深度最多十六层。
fn append_browser_subtree(
    parent_id: Option<String>, // 当前需要展开的父目录；`None`表示虚拟根目录。
    depth: usize,              // 根目录直属项目从深度一开始。
    folder_children: &mut BTreeMap<Option<String>, Vec<MapFolderRecord>>, // 已按同级名称排序的目录分组。
    map_children: &mut BTreeMap<Option<String>, Vec<MapBrowserEntry>>, // 保留catalog顺序的地图分组。
    output: &mut Vec<MapBrowserEntry>, // 后台独占构建的完整深度优先结果。
) {
    for folder in folder_children.remove(&parent_id).unwrap_or_default() {
        let folder_id = folder.id.clone(); // 递归键与公开行各自拥有稳定短身份。
        output.push(MapBrowserEntry {
            item: MapBrowserItem::Folder(folder_id.clone()), // 文件夹点击只携带稳定ID。
            display_name: folder.display_name,               // 作者名称已经通过长度和字符校验。
            published: false,                                // 虚拟文件夹没有运行时发布状态。
            depth,                                           // UI只按该值设置有限缩进。
            parent_folder_id: parent_id.clone(),             // 创建上下文和诊断保留直接父级。
        });
        append_browser_subtree(
            Some(folder_id), // 文件夹之后立即排列其展开时可见的完整子树。
            depth + 1,       // 有效文档保证不会越过十六层。
            folder_children, // 同一后台任务逐组取走，避免重复扫描全部目录。
            map_children,    // 地图在各自父级的所有子目录之后追加。
            output,          // 完整结果只在任务结束后原子发布。
        );
    }
    if let Some(mut maps) = map_children.remove(&parent_id) {
        for map in &mut maps {
            map.depth = depth; // 地图与同一父级的子文件夹使用相同树深度。
            map.parent_folder_id = parent_id.clone(); // 明确保留地图直接所属目录。
        }
        output.extend(maps); // 同一目录的地图严格保持catalog作者顺序。
    }
}

/// 把全部受检目录和catalog地图整理为“目录优先、地图随后”的完整深度优先树。
pub(crate) fn build_browser_tree(
    folders: Vec<MapFolderRecord>, // 主线程按固定预算冻结的全部文件夹记录。
    maps: Vec<MapBrowserEntry>,    // 主线程按catalog顺序冻结的全部地图记录。
) -> Vec<MapBrowserEntry> {
    let total = folders.len().saturating_add(maps.len()); // 只用于一次性预留最终结果容量。
    let mut folder_children = BTreeMap::<Option<String>, Vec<MapFolderRecord>>::new(); // 父ID分组避免递归重复全表扫描。
    for folder in folders {
        folder_children
            .entry(folder.parent_id.clone()) // 根目录使用`None`键，普通目录使用稳定父ID。
            .or_default()
            .push(folder); // 每条受检文件夹只进入一个父级分组。
    }
    for siblings in folder_children.values_mut() {
        siblings.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase()) // 同级目录首先按不区分大小写名称排序。
                .then_with(|| left.display_name.cmp(&right.display_name)) // 折叠键相同时用原文保持确定性。
                .then_with(|| left.id.cmp(&right.id)) // 防御性稳定尾键不依赖输入数组顺序。
        });
    }
    let mut map_children = BTreeMap::<Option<String>, Vec<MapBrowserEntry>>::new(); // 分组过程保持每组的catalog插入顺序。
    for map in maps {
        map_children
            .entry(map.parent_folder_id.clone()) // 缺失归属键的地图进入根目录分组。
            .or_default()
            .push(map); // 每张catalog地图只进入一个直接父级。
    }
    let mut output = Vec::with_capacity(total); // 后台一次分配完整树结果。
    append_browser_subtree(
        None,                 // 从固定虚拟根目录开始展开。
        1,                    // 根目录行由UI固定显示，直属项目缩进一级。
        &mut folder_children, // 受检父树最终会恰好取空。
        &mut map_children,    // catalog中存在的地图最终会恰好取空。
        &mut output,          // 返回前不向主世界暴露半成品。
    );
    output // 调用方以Arc原子采用完整树。
}

/// 根据会话展开集合从完整深度优先树生成一份可见行投影。
pub(crate) fn project_visible_browser_tree(
    full_entries: &[MapBrowserEntry], // 只读完整树由Arc跨线程共享。
    expanded: &BTreeSet<String>,      // 最多512项的会话展开身份快照。
) -> Vec<MapBrowserEntry> {
    let mut visible = Vec::with_capacity(full_entries.len()); // 最坏全部展开时只分配一次结果。
    let mut hidden_folder_depth = None; // 折叠目录深度用于跳过其连续深度优先后代。
    for entry in full_entries {
        if let Some(hidden_depth) = hidden_folder_depth {
            if entry.depth > hidden_depth {
                continue; // 当前项仍属于折叠目录后代，不复制到可见结果。
            }
            hidden_folder_depth = None; // 已到折叠目录的兄弟或祖先后续项，恢复可见。
        }
        visible.push(entry.clone()); // 只有当前可见行才复制短身份与名称。
        if let MapBrowserItem::Folder(folder_id) = &entry.item
            && !expanded.contains(folder_id)
        {
            hidden_folder_depth = Some(entry.depth); // 后续更深项全部属于本折叠分支。
        }
    }
    visible // 主线程仅在完整投影身份仍匹配时一次替换。
}

/// 判断光标是否已越过六物理像素拖拽阈值。
pub(crate) fn drag_threshold_reached(start_physical: Vec2, current_physical: Vec2) -> bool {
    start_physical.distance_squared(current_physical)
        >= MAP_BROWSER_DRAG_THRESHOLD_PHYSICAL_PIXELS * MAP_BROWSER_DRAG_THRESHOLD_PHYSICAL_PIXELS // 使用平方距离避免每帧开方并精确包含边界值。
}
