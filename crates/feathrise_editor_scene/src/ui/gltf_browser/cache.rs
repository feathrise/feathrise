//! 展开需求驱动的Scene0强句柄缓存、热更失效与有界采用系统。

use std::collections::BTreeSet; // 已消费的热更路径进入single-flight待失效集合。

use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState}; // Scene0完整依赖失败状态。
use bevy::gltf::GltfAssetLabel; // 每个唯一展开GLB只请求Scene0子资产。
use bevy::prelude::*; // ECS系统参数、资产资源与Handle。
use bevy::world_serialization::WorldAsset; // 浏览缓存只读Scene0原始World。

use super::helpers::mesh_index_from_primitive_label; // Mesh子资产标签只提供非权威诊断索引。
use super::retirement::BrowserRetiredGeneration; // 取消、折叠和热更替换均分帧析构旧代。
use super::scene_build::{BrowserSceneBuild, advance_scene_build}; // Scene0读取不跨线程借World，使用主线程固定预算状态机。
use super::{
    BrowserLoadingTicket, CachedGltfAsset, CachedGltfAssetPhase, EditorGltfBrowserState,
    MAX_GLTF_ASSET_DEMAND_PATHS_PER_FRAME, MAX_GLTF_ASSET_EVENTS_PER_FRAME,
    MAX_GLTF_ASSET_POLLS_PER_FRAME,
}; // 强句柄缓存和全部帧预算集中在模块入口定义。
mod hot_reload; // 资产事件突发、全缓存游标与失效背压独立按路径预算收口。
mod session; // F12关闭、退役推进与各有界状态机编排独立收口。
mod signature_sync; // 摆放结构观察、分帧签名与资产需求过渡独立于Scene0加载缓存。

use hot_reload::collect_invalidated_paths; // 会话编排消费固定预算的热更路径集合。

pub(crate) use session::refresh_gltf_browser_cache; // 模块入口保持原系统路径不变。
#[cfg(test)]
use session::release_browser_session; // 关闭回归直接验证活动字段即时撤下。

/// Scene0本体或任一依赖失败时停止逐帧轮询，等待后续匹配资产事件重试。
fn scene_load_failed(asset_server: &AssetServer, handle: &Handle<WorldAsset>) -> bool {
    asset_server
        .get_load_states(handle.id())
        .is_some_and(|(direct, dependencies, recursive)| {
            matches!(direct, LoadState::Failed(_))
                || matches!(dependencies, DependencyLoadState::Failed(_))
                || matches!(recursive, RecursiveDependencyLoadState::Failed(_))
        }) // Bevy 0.19分别报告直接、直接依赖和递归依赖状态。
}

/// 同一路径、同一内容修订只入公平队列一次；热更后的旧票由固定预算自然丢弃。
fn enqueue_loading_path(state: &mut EditorGltfBrowserState, asset_path: &str) {
    let Some(content_revision) = state
        .assets
        .get(asset_path)
        .map(|cached| cached.content_revision)
    else {
        return; // 已释放路径不产生无主票据。
    };
    if let Some(queued_revision) = state.queued_revision_by_path.get_mut(asset_path) {
        *queued_revision = content_revision; // 已有物理票只更新目标修订，连续热更不增长队列长度。
        return;
    }
    state
        .queued_revision_by_path
        .insert(asset_path.to_owned(), content_revision);
    state.loading_queue.push_back(BrowserLoadingTicket {
        asset_path: asset_path.to_owned(),
        content_revision,
    }); // 每个路径至多一张物理票，出队时从索引读取其最新内容修订。
}

/// 只移除仍属于指定修订的当前排队身份，不能让旧票擦掉热更后的新票。
fn clear_queued_revision(state: &mut EditorGltfBrowserState, asset_path: &str, revision: u64) {
    if state.queued_revision_by_path.get(asset_path) == Some(&revision) {
        state.queued_revision_by_path.remove(asset_path);
    }
}

/// 缓存移除或阶段改变前，从O(1)状态摘要计数中撤销旧值。
fn remove_asset_summary(
    state: &mut EditorGltfBrowserState,
    phase: CachedGltfAssetPhase,
    truncated: bool,
) {
    match phase {
        CachedGltfAssetPhase::Loading => {
            state.loading_asset_count = state.loading_asset_count.saturating_sub(1)
        }
        CachedGltfAssetPhase::Failed => {
            state.failed_asset_count = state.failed_asset_count.saturating_sub(1)
        }
        CachedGltfAssetPhase::Ready => {}
    }
    if truncated {
        state.truncated_asset_count = state.truncated_asset_count.saturating_sub(1);
    }
}

/// 缓存新增或阶段改变后，把新值计入O(1)状态摘要。
fn add_asset_summary(
    state: &mut EditorGltfBrowserState,
    phase: CachedGltfAssetPhase,
    truncated: bool,
) {
    match phase {
        CachedGltfAssetPhase::Loading => {
            state.loading_asset_count = state.loading_asset_count.saturating_add(1)
        }
        CachedGltfAssetPhase::Failed => {
            state.failed_asset_count = state.failed_asset_count.saturating_add(1)
        }
        CachedGltfAssetPhase::Ready => {}
    }
    if truncated {
        state.truncated_asset_count = state.truncated_asset_count.saturating_add(1);
    }
}

/// 只调和一个资产路径；展开/折叠和结构换代都由外层固定路径预算调用。
enum AssetPathReconcile {
    Changed,   // 路径的强句柄生命周期确实发生变化。
    Unchanged, // 当前缓存已经符合最新需求。
    Deferred,  // 正常退役槽占用中，路径已重新排队等待。
}

fn reconcile_asset_path(
    state: &mut EditorGltfBrowserState,
    asset_server: &AssetServer,
    asset_path: &str,
) -> AssetPathReconcile {
    let wanted = state
        .asset_demand_counts
        .get(asset_path)
        .is_some_and(|count| *count > 0); // 共享GLB只要还有一个展开实例就继续持有。
    if wanted {
        if state.assets.contains_key(asset_path) {
            return AssetPathReconcile::Unchanged; // 已有Loading/Ready/Failed缓存均复用原强句柄。
        }
        let handle = asset_server.load(
            GltfAssetLabel::Scene(0).from_asset(asset_path.to_owned()), // 与正式实例使用相同Scene0身份。
        );
        state
            .asset_paths_by_id
            .insert(handle.id(), asset_path.to_owned()); // 热更事件按ID直接O(1)定位。
        state.assets.insert(
            asset_path.to_owned(),
            CachedGltfAsset {
                handle,                               // 仅展开需求期间强持有。
                meshes: Vec::new(),                   // 完整依赖就绪后一次采用。
                active_ready: false,                  // 首次Scene0尚无可保留的完整缓存。
                phase: CachedGltfAssetPhase::Loading, // 固定轮询预算渐进推进。
                truncated: false,                     // 尚未遍历World。
                content_revision: 1,                  // 后续每次相关热更事件确定递增。
            },
        );
        add_asset_summary(state, CachedGltfAssetPhase::Loading, false);
        enqueue_loading_path(state, asset_path);
        return AssetPathReconcile::Changed;
    }

    let has_scene_build = state
        .scene_build
        .as_ref()
        .is_some_and(|build| build.asset_path == asset_path);
    if (state.assets.contains_key(asset_path) || has_scene_build)
        && !state.retirement.can_enqueue_normal()
    {
        return AssetPathReconcile::Deferred; // 保留完整缓存/草稿，不能为连续折叠堆第二退役代。
    }
    let Some((old_phase, old_truncated, old_handle_id)) = state
        .assets
        .get(asset_path)
        .map(|cached| (cached.phase, cached.truncated, cached.handle.id()))
    else {
        state.queued_revision_by_path.remove(asset_path); // 没有缓存时仍清掉可能的旧队列身份。
        return AssetPathReconcile::Unchanged;
    };
    remove_asset_summary(state, old_phase, old_truncated); // 摘要在缓存所有权移动前精确撤销。
    let (owned_path, cached) = state
        .assets
        .remove_entry(asset_path)
        .expect("缓存快照刚从同一BTreeMap路径取得"); // 同时取得原键，避免再复制资产路径。
    let scene_build = has_scene_build.then(|| state.scene_build.take()).flatten(); // 匹配草稿只移动所有权。
    let mut retired = BrowserRetiredGeneration::default();
    retired.scene_build(scene_build); // 折叠取消的DFS不能直接整批Drop。
    retired.cached_asset(owned_path, cached); // 强句柄及旧网格逐项退役。
    state.retirement.enqueue_normal_reserved(retired); // release构建也必须真实转移旧缓存所有权。
    if state
        .asset_paths_by_id
        .get(&old_handle_id)
        .is_some_and(|path| path == asset_path)
    {
        state.asset_paths_by_id.remove(&old_handle_id); // 迟到事件不再唤醒已释放路径。
    }
    state.queued_revision_by_path.remove(asset_path); // 队列中的旧票稍后按固定预算跳过。
    AssetPathReconcile::Changed
}

/// 每帧只采用固定数量单路径需求变化，避免8192项结构换代叠成一帧峰值。
fn process_asset_demand_budget(
    state: &mut EditorGltfBrowserState,
    asset_server: &AssetServer,
) -> bool {
    let mut changed = false;
    for _ in 0..MAX_GLTF_ASSET_DEMAND_PATHS_PER_FRAME {
        let path = if let Some(path) = state.asset_demand_changes.pop_first() {
            Some(path) // 直接UI动作优先，点击后尽快请求或释放对应Scene0。
        } else if let Some(transition) = state.asset_demand_transition.as_mut() {
            transition
                .stale_paths
                .pop_first()
                .map(|(path, _)| path)
                .or_else(|| transition.wanted_paths.pop_first())
        } else {
            None
        };
        let Some(path) = path else {
            break; // 本帧已无待调和路径。
        };
        match reconcile_asset_path(state, asset_server, &path) {
            AssetPathReconcile::Changed => changed = true,
            AssetPathReconcile::Unchanged => {}
            AssetPathReconcile::Deferred => {
                state.asset_demand_changes.insert(path); // single-flight集合合并同路径最新需求。
                break; // 退役槽释放前不在同一帧反复pop/insert。
            }
        }
    }
    if state
        .asset_demand_transition
        .as_ref()
        .is_some_and(|transition| {
            transition.stale_paths.is_empty() && transition.wanted_paths.is_empty()
        })
    {
        state.asset_demand_transition = None; // 完整收口后释放空过渡集合。
    }
    changed
}

/// 将固定路径集合切回Loading；上一份完整网格缓存保留到新修订原子采用。
fn invalidate_cached_paths(state: &mut EditorGltfBrowserState, paths: BTreeSet<String>) -> bool {
    state.pending_asset_invalidations.extend(paths); // 已消费事件先并入single-flight路径集合，背压时不会丢失。
    let mut changed = false;
    for _ in 0..MAX_GLTF_ASSET_EVENTS_PER_FRAME {
        let Some(path) = state.pending_asset_invalidations.pop_first() else {
            break; // 本帧已经收口全部待失效路径。
        };
        let Some((old_phase, old_truncated)) = state
            .assets
            .get(&path)
            .map(|cached| (cached.phase, cached.truncated))
        else {
            continue; // 同帧需求调和可能已经释放路径。
        };
        let cancels_scene_build = state
            .scene_build
            .as_ref()
            .is_some_and(|build| build.asset_path == path);
        if cancels_scene_build && !state.retirement.can_enqueue_normal() {
            state.pending_asset_invalidations.insert(path); // 保留最新路径请求等待唯一退役槽。
            break; // 同一活动Scene草稿会阻挡后续采用，避免本帧反复检查。
        }
        if cancels_scene_build {
            let mut retired = BrowserRetiredGeneration::default();
            retired.scene_build(state.scene_build.take()); // 旧World读取草稿逐实体/路径释放。
            state.retirement.enqueue_normal_reserved(retired); // 不把有副作用调用藏进debug_assert。
        }
        remove_asset_summary(state, old_phase, old_truncated);
        let cached = state.assets.get_mut(&path).expect("路径刚从当前缓存读取");
        cached.phase = CachedGltfAssetPhase::Loading;
        cached.content_revision = cached.content_revision.wrapping_add(1); // 完成值必须匹配这一修订。
        add_asset_summary(state, CachedGltfAssetPhase::Loading, old_truncated);
        enqueue_loading_path(state, &path);
        changed = true;
    }
    changed
}

/// 把加载或分帧读取失败收口为Failed；有上一版完整缓存时继续原子保留。
fn mark_asset_failed(state: &mut EditorGltfBrowserState, path: &str) -> bool {
    let Some((old_phase, old_truncated)) = state
        .assets
        .get(path)
        .map(|cached| (cached.phase, cached.truncated))
    else {
        return false;
    };
    remove_asset_summary(state, old_phase, old_truncated);
    let cached = state.assets.get_mut(path).expect("路径刚从当前缓存读取");
    cached.phase = CachedGltfAssetPhase::Failed;
    add_asset_summary(state, CachedGltfAssetPhase::Failed, old_truncated);
    state.queued_revision_by_path.remove(path);
    true
}

/// 只有路径、句柄和内容修订仍完全一致时，一次替换活动Scene0网格缓存。
/// 完成草稿中的根、去重键和DFS壳也与旧网格进入同一退役代，发布帧不析构整份草稿。
fn adopt_scene_build_result(
    state: &mut EditorGltfBrowserState,
    mut build: BrowserSceneBuild,
) -> Result<bool, BrowserSceneBuild> {
    if !state.retirement.can_enqueue_normal() {
        return Err(build); // 完整最新结果保持在唯一任务槽，不能为发布再堆一代退役工作。
    }
    let Some((old_phase, old_truncated)) = state.assets.get(&build.asset_path).and_then(|cached| {
        (cached.handle.id() == build.asset_id
            && cached.content_revision == build.content_revision
            && cached.phase == CachedGltfAssetPhase::Loading)
            .then_some((cached.phase, cached.truncated))
    }) else {
        let mut retired = BrowserRetiredGeneration::default();
        retired.scene_build(Some(build)); // 迟到完成草稿仍可能拥有8192根和2048项去重键。
        state.retirement.enqueue_normal_reserved(retired);
        return Ok(false); // 热更、折叠或换句柄后的过期结果不得触碰上一份活动缓存。
    };
    let result = build.take_result(); // 最终网格移出后，完成草稿壳仍保留给下方退役代。
    remove_asset_summary(state, old_phase, old_truncated);
    let cached = state
        .assets
        .get_mut(&result.asset_path)
        .expect("采用身份刚由当前缓存验证");
    let old_meshes = std::mem::replace(&mut cached.meshes, result.meshes); // 完整新列表一次换指针。
    cached.active_ready = true;
    cached.phase = CachedGltfAssetPhase::Ready;
    cached.truncated = result.truncated;
    add_asset_summary(state, CachedGltfAssetPhase::Ready, result.truncated);
    state.queued_revision_by_path.remove(&result.asset_path);
    let mut retired = BrowserRetiredGeneration::default();
    retired.scene_build(Some(build)); // 发布成功也不能同步Drop根、去重路径和空闲DFS容量。
    retired.cached_meshes(old_meshes); // 上一份完整缓存按网格/路径预算释放。
    state.retirement.enqueue_normal_reserved(retired); // 完成草稿保证非空工作，上方已预留唯一正常槽。
    Ok(true)
}

/// 当前唯一Scene0任务每帧只推进一次固定根发现/DFS/路径预算。
fn process_scene_build_budget(
    state: &mut EditorGltfBrowserState,
    asset_server: &AssetServer,
    world_assets: &Assets<WorldAsset>,
) -> bool {
    let Some(mut build) = state.scene_build.take() else {
        return false;
    };
    let current = state.assets.get(&build.asset_path).is_some_and(|cached| {
        cached.handle.id() == build.asset_id
            && cached.content_revision == build.content_revision
            && cached.phase == CachedGltfAssetPhase::Loading
    });
    if !current {
        if !state.retirement.can_enqueue_normal() {
            state.scene_build = Some(build); // 失效草稿保留唯一所有权，等待退役槽。
            return false;
        }
        let mut retired = BrowserRetiredGeneration::default();
        retired.scene_build(Some(build)); // 不匹配草稿也必须逐项释放。
        state.retirement.enqueue_normal_reserved(retired); // release构建同样逐项退役失效草稿。
        return false; // 折叠或热更已经取消该身份，直接释放未发布草稿。
    }
    let handle = state
        .assets
        .get(&build.asset_path)
        .expect("上方已验证构建缓存存在")
        .handle
        .clone();
    if scene_load_failed(asset_server, &handle) {
        if !state.retirement.can_enqueue_normal() {
            state.scene_build = Some(build); // 失败身份保持完整，下一Update再收口。
            return false;
        }
        let path = build.asset_path.clone(); // 状态切换需要当前路径，长度受资产文档约束。
        let mut retired = BrowserRetiredGeneration::default();
        retired.scene_build(Some(build)); // 已扫描前缀不在失败帧集中析构。
        state.retirement.enqueue_normal_reserved(retired); // 失败帧不能同步Drop已扫描Scene前缀。
        return mark_asset_failed(state, &path); // 旧活动网格仍保留。
    }
    let Some(world_asset) = world_assets.get(build.asset_id) else {
        state.scene_build = Some(build); // Assets采用短暂缺席时下帧继续，不重扫已完成前缀。
        return false;
    };
    let completed = advance_scene_build(&mut build, &world_asset.world, |mesh| {
        asset_server
            .get_path(mesh.0.id())
            .and_then(|asset_path| asset_path.label().and_then(mesh_index_from_primitive_label))
    });
    if completed {
        match adopt_scene_build_result(state, build) {
            Ok(changed) => changed,
            Err(build) => {
                state.status = "Scene0：新缓存已完成，等待上一代分帧释放".to_owned();
                state.scene_build = Some(build); // 完整结果保持在唯一任务内，不重复读取World。
                false
            }
        }
    } else {
        let retained = state
            .assets
            .get(&build.asset_path)
            .is_some_and(|cached| cached.active_ready);
        state.status = format!(
            "Scene0：正在分帧读取 {}{}",
            build.asset_path,
            if retained {
                "（旧缓存继续可用）"
            } else {
                "（完成后显示网格）"
            },
        );
        state.scene_build = Some(build);
        false
    }
}

/// 每帧最多轮询八个句柄，并至多启动一个尚未发布的Scene0读取任务。
fn process_loading_budget(
    state: &mut EditorGltfBrowserState,
    asset_server: &AssetServer,
    world_assets: &Assets<WorldAsset>,
) -> bool {
    let mut changed = process_scene_build_budget(state, asset_server, world_assets);
    let poll_count = state
        .loading_queue
        .len()
        .min(MAX_GLTF_ASSET_POLLS_PER_FRAME);
    for _ in 0..poll_count {
        let Some(ticket) = state.loading_queue.pop_front() else {
            break;
        };
        let path = ticket.asset_path;
        let Some(content_revision) = state.queued_revision_by_path.get(&path).copied() else {
            continue; // 路径已经释放，迟到物理票只消耗一次固定预算。
        };
        let _ticket_was_coalesced = ticket.content_revision != content_revision; // 热更可更新索引而复用同一物理票。
        let Some((phase, handle)) = state
            .assets
            .get(&path)
            .filter(|cached| cached.content_revision == content_revision)
            .map(|cached| (cached.phase, cached.handle.clone()))
        else {
            clear_queued_revision(state, &path, content_revision); // 已释放或身份异常票在预算内丢弃。
            continue;
        };
        if phase != CachedGltfAssetPhase::Loading {
            clear_queued_revision(state, &path, content_revision);
            continue;
        }
        if scene_load_failed(asset_server, &handle) {
            changed |= mark_asset_failed(state, &path);
            continue;
        }
        if !asset_server.is_loaded_with_dependencies(handle.id()) || state.scene_build.is_some() {
            state.loading_queue.push_back(BrowserLoadingTicket {
                asset_path: path,
                content_revision,
            }); // Pending重排原票，不创建同路径同修订副本。
            continue;
        }
        let Some(world_asset) = world_assets.get(&handle) else {
            state.loading_queue.push_back(BrowserLoadingTicket {
                asset_path: path,
                content_revision,
            }); // Assets采用可能比加载状态晚一帧；仍只轮转原票。
            continue;
        };
        let revision = state
            .assets
            .get(&path)
            .expect("加载路径来自当前缓存")
            .content_revision;
        state.scene_build = Some(BrowserSceneBuild::new(
            path.clone(),
            handle.id(),
            revision,
            &world_asset.world,
        )); // 建立任务只读取O(1)槽位长度，下一Update才开始根发现。
        clear_queued_revision(state, &path, content_revision); // 活动Scene构建接管该修订的唯一推进权。
    }
    changed
}

#[cfg(test)]
#[path = "cache/tests.rs"]
mod tests; // 过期Scene0结果拒收与F12关闭取消由无AssetServer测试锁定。
