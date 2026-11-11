//! F12全屏编辑器BSN壳、动态列表和交互系统。

pub(crate) mod action_availability; // 顶栏与视口命令的样式、说明和输入共享真实可用资格。
pub(crate) mod actions; // 工具按钮、快捷键和选中态回显。
pub(crate) mod external_files; // 资产外部打开共享结构化请求与唯一后台启动槽。
pub(crate) mod asset_context_menu; // 工程与资产行右键复制真实路径，菜单关闭帧仍阻止作者输入点透。
pub(crate) mod asset_shortcuts; // 项目内资产收藏和最近访问，不保存作者内容。
pub(crate) mod asset_preview; // 右侧资产行选择后的二维纹理与独立GLB/GLTF离屏预览。
pub(crate) mod project_browser; // 同一浏览入口的资产与工程文件视图，原资产业务保持独立。
pub(crate) mod continuous_list; // 单外层滚动的完整高度、可见首行与固定窗口池几何。
pub(crate) mod asset_thumbnails; // 列表/缩略图切换、当前页静态截图与有界缓存。
pub(crate) mod brush_controls; // 视口参数区的公共数值控件与原笔刷业务桥。
pub(crate) mod building_presets; // 建筑目录、占地/正门/地基表单与专用精确摆放。
pub(crate) mod building_sidebar; // 单栋/院落顶部工具页和统一反馈，不改变作者事务。
pub(crate) mod close; // 未保存地形画面与碰撞联合完成后再关闭F12。
pub(crate) mod scene_objects; // 当前场景的名称、逻辑组、搜索和编辑器保护状态。
pub(crate) mod scene_context_menu; // 场景树右键冻结单对象，复用各作者域的完整操作。
pub(crate) mod scene_layers; // 统一树补齐水域、植被与角色作者内容。
pub(crate) mod courtyard; // 院落组合名称、多选、印章与整体操作的 UI 输入门。
pub(crate) mod delete_scene_dialog; // 可恢复场景删除的模态确认与冻结目标回显。
pub(crate) mod editor_settings; // 偏好分组、固定动作与独立滚动；沿用编辑器设置模态所有权。
pub(crate) mod environment; // 场景持久参数、世界调试与全局默认独立分区，真实IBL阶段增量回显。
pub(crate) mod gameplay_sidebar; // 资产摆放、传送点与空气墙固定工具页及原状态反馈。
pub(crate) mod gltf_browser; // 已摆放GLB实例的Scene0节点与网格固定行浏览器。
pub(crate) mod ime; // F12全部自定义文本框共享唯一最终IME所有权判定。
pub(crate) mod layer_status; // 地表、植被作者组与独立水位层状态回显。
pub(crate) mod layout; // 两个可调侧栏、可还原专注模式与按任务显隐，不实现任意停靠。
pub(crate) mod list_layout; // 五类固定行池共享零项退出与按末页真实行数增高规则。
pub(crate) mod scene_deletion; // 高亮项删除保护门；先关闭目标标签，不切换到固定场景。
pub(crate) mod scene_documents; // 多场景标签保留独立作者草稿、历史和稀疏高度，活动图复用正式运行时。
pub(crate) mod scene_loading; // 切换场景保留工作台，在中央显示真实进度并锁定作者输入。
pub(crate) mod scene_loading_recovery; // 失败冷重试与取消回工作台，不放弃作者文档。
pub(crate) mod scene_sidebar; // 场景目录、已进入场景及所选对象独立回显，保留原虚拟树和事务。
pub(crate) mod subscenes; // 完整场景引用及整体变换，源场景保留唯一作者内容。
pub(crate) mod terrain_objects; // 独立地形的创建、选择、尺寸与后端参数编辑。
pub(crate) mod management_dialogs; // 管理模态共用有界阅读区、固定操作与危险样式，原事务保持不变。
pub(crate) mod scene_switch_dialog; // 未保存切换场景的三选一确认、保存成功续接与一次性放弃授权。
pub(crate) mod scene_transition; // 新建与显式“进入场景”共用等待热加载的正式切换场景事务。
pub(crate) mod new_scene_dialog; // 名称、米制长宽与创建事务状态弹框。
pub(crate) mod numeric_scrub_mouse_guard; // 数值拖动延迟到三键全松后的静默帧再归还场景编辑器鼠标。
pub(crate) mod placement_transform; // 完整摆放实例的变换、复制、删除与视口移动作者桥。
pub(crate) mod placement_water; // 场景树选中完整Blender网格后的水体语义与参数作者桥。
pub(crate) mod portal_list; // 当前场景源点与场景树目标落点的双完整列表和显式选择。
mod portal_scene; // 玩法页传送双列表BSN静态外壳。
pub(crate) mod rows; // 场景目录独立高亮/显式进入与资产固定行池。
pub(crate) mod scene; // BSN声明静态全屏布局。
pub(crate) mod scene_inspector; // 当前场景选择概览与只读反馈，不接管原作者事务或预览。
pub(crate) mod shortcut_settings; // 场景快捷键的可配置组合键与冲突回显。
pub(crate) mod sidebars; // 左右栏九个一级页签、层级标题、折叠帮助与上下文显隐。
mod static_text_input; // 场景、文件夹与传送名称静态控件的业务绑定。
pub(crate) mod surface_layers; // 当前场景地表作者层固定列表、稳定选择与可撤销删除。
pub(crate) mod surface_preset_editor; // F12全局PBR地表预设、连续列表、中央标签与纹理数组原子派生。
pub(crate) mod status_bar; // 固定底栏、明确回执与非模态诊断，不接管保存或加载事务。
pub(crate) mod context_help; // 唯一问号帮助浮层复用原正文，默认不占作者面板高度。
pub(crate) mod systems; // F12、名称输入和壳体显隐同步。
pub(crate) mod terrain_conflict_dialog; // Blender/F12地形并发修改的模态选择。
pub(crate) mod terrain_shading; // 场景页基础地形的本次运行着色模式与切换场景恢复。
pub(crate) mod terrain_sidebar; // 雕刻/地表/积水顶部工具页及只读作者反馈。
pub(crate) mod text_input; // 官方文本组件的公共身份与候选确认帧保护。
pub(crate) mod theme; // F12 独立工具主题；不改变游戏 HUD、背包和设置界面。
pub(crate) mod tooltips; // 顶栏与视口按钮的只读悬停提示，固定浮层不参与正文测量或输入所有权。
pub(crate) mod vegetation_layers; // 作者图层结构按钮、固定行点击与撤销快照采用。
pub(crate) mod vegetation_preset_editor; // F12全局植被预设的新建、编辑、引用保护删除与原子保存。
pub(crate) mod vegetation_presets; // 无分页三列预设列表的有界实体同步与目录热更重绑。
mod vegetation_scene; // 植被图层面板BSN静态外壳。
pub(crate) mod vegetation_paint; // 场景级绘制组和目标网格勾选。
pub(crate) mod vegetation_sidebar; // 全局库、场景建层目标与真实后台阶段的只读反馈。
pub(crate) mod viewport_toolbar; // 默认单行，右端溢出时可显式展开为多行。
mod water_actions; // 静水体与连续水网按钮分派，避免通用动作系统继续膨胀。
pub(crate) mod water_feature_list; // 静水、河流和瀑布统一固定行池、显式分页与稳定ID直选。
pub(crate) mod water_layers; // 独立水层增删、复制和Arc浅快照撤销。
pub(crate) mod water_level_input; // 公共精确数值输入到稳定水层作者请求的桥。
pub(crate) mod water_preset_editor; // 全局静水外观预设、不分页目录和中央完整波浪表单。
mod water_scene; // 静水体、河流样条与瀑布共享的可滚动作者面板。
pub(crate) mod water_sidebar; // 水域横向工具页、统一列表外壳与只读反馈。
mod workspace_tabs; // 中央场景及三类预设共用文字自适应标签和内嵌小关闭键。
pub(super) mod window_camera; // F12窗口UI固定承载者，独立于切换场景暂停和每图RTT。
pub(crate) mod world_session; // 空世界隐藏场景专属入口，保留预设工作区和独立样品预览。

pub(crate) use water_actions::reject_water_feature_switch; // 中央视口Ctrl吸取与列表切换共用水域跨帧事务保护。
