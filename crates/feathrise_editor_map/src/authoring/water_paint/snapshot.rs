//! 静水整笔快照与共享GLB写侧的单飞后台物化，避免历史Arc阻塞后续雕刻。

use super::*; // 复用水作者会话、单飞任务和Arc输入。

pub(crate) fn poll_terrain_materialization(
    session: &mut EditorAuthoringSession,
    materialize: &mut WaterMaskMaterializeTask,
) {
    let Some(task) = materialize.terrain_task.as_mut() else {
        return;
    }; // 未提交任务时不做工作。
    let Some((map_id, revision, source, mesh)) = check_ready(task) else {
        return;
    }; // 不阻塞主线程。
    materialize.terrain_task = None; // CPU真正结束后才释放并发槽。
    if session.terrain_revision != revision {
        return;
    } // 其他地形操作已换代则丢弃旧副本。
    let Some(current) = session
        .draft
        .as_mut()
        .filter(|draft| draft.map_id == map_id)
        .and_then(|draft| draft.terrain_mesh.as_mut())
    else {
        return;
    }; // 同名层不能跨草稿采用。
    if Arc::ptr_eq(current, &source) {
        *current = Arc::new(mesh);
    } // 源身份与修订同时吻合才原子换指针。
}

pub(crate) fn start_terrain_materialization_if_shared(
    session: &EditorAuthoringSession,
    materialize: &mut WaterMaskMaterializeTask,
) -> bool {
    if materialize.terrain_task.is_some() {
        return true;
    } // 迟到任务也占用唯一CPU槽直到完成。
    let Some(draft) = session.draft.as_ref() else {
        return false;
    }; // 未加载草稿无需准备。
    let Some(source) = draft
        .terrain_mesh
        .as_ref()
        .filter(|mesh| Arc::strong_count(mesh) > 1)
    else {
        return false;
    }; // 独占网格继续零复制局部写入。
    let source = source.clone(); // 后台启动只增加共享引用。
    let map_id = draft.map_id.clone(); // 冻结地图身份。
    let revision = session.terrain_revision; // 冻结地形作者修订。
    materialize.terrain_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mesh = source.as_ref().clone(); // 完整位置与索引复制只在线程池执行。
        (map_id, revision, source, mesh) // 返回完整结果和全部采用身份。
    })); // 一次最多一份后台GLB副本。
    true // 调用方保留待补笔段或撤销请求。
}

pub(super) fn capture_stroke_snapshot(
    session: &EditorAuthoringSession,
    stroke: &mut WaterBrushStroke,
    erase: bool,
) {
    let Some(draft) = session.draft.as_ref() else {
        return;
    }; // 不从失效地图建立历史。
    if stroke.snapshot_mask.is_none() {
        // 第一份R8版本贯穿所有内部检查点。
        stroke.snapshot_mask = stroke
            .layer_id
            .as_ref()
            .and_then(|id| draft.water_masks.get(id))
            .cloned(); // 只增加Arc引用，写侧由后台准备。
    }
    if !erase && stroke.snapshot_terrain.is_none() {
        // 纯擦除不冻结或复制网格。
        stroke.snapshot_terrain = draft.terrain_mesh.clone(); // 首次真正请求下挖前保留网格版本。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 在无窗口环境验证真实后台写侧身份与单飞门。

    fn await_copy(session: &mut EditorAuthoringSession, task: &mut WaterMaskMaterializeTask) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5); // 测试等待有明确截止时间。
        while task.terrain_task.is_some() {
            // 仅测试夹具等待，正式帧只调用一次非阻塞poll。
            poll_terrain_materialization(session, task); // 复用生产采用边界。
            assert!(std::time::Instant::now() < deadline, "后台写侧没有完成"); // 防止错误测试永久挂起。
            std::thread::yield_now(); // 让后台worker完成小网格复制。
        }
    }

    #[test]
    fn terrain_history_reader_gets_a_unique_write_side_without_mutating_snapshot() {
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new); // 无窗口测试初始化共享池。
        let draft =
            crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![32_768; 121])); // 有效小网格。
        let before = draft.terrain_mesh.clone().unwrap(); // 模拟历史长期持有原网格。
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        }; // 独立测试作者会话。
        let mut task = WaterMaskMaterializeTask::default(); // 唯一后台复制槽。
        assert!(start_terrain_materialization_if_shared(&session, &mut task)); // 长期共享触发后台准备。
        assert!(start_terrain_materialization_if_shared(&session, &mut task)); // 重复请求复用同一在途槽。
        await_copy(&mut session, &mut task); // 完整后才采用。
        let current = session
            .draft
            .as_mut()
            .unwrap()
            .terrain_mesh
            .as_mut()
            .unwrap(); // 新作者网格应当独占。
        assert!(!Arc::ptr_eq(current, &before)); // 历史仍保持原版本。
        Arc::get_mut(current).unwrap().positions[0][1] = -12.0; // 后续地形与静水都可以继续编辑。
        assert_ne!(before.positions[0][1], -12.0); // 新写入不得污染整笔撤销起点。
    }

    #[test]
    fn stale_terrain_copy_cannot_overwrite_a_new_revision_or_source() {
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new); // 同一测试池继续处理单飞任务。
        let draft =
            crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![32_768; 121])); // 旧地图作者版本。
        let old = draft.terrain_mesh.clone().unwrap(); // 保持旧网格被历史共享。
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        }; // 独立修订身份。
        let mut task = WaterMaskMaterializeTask::default(); // 最多一项任务。
        assert!(start_terrain_materialization_if_shared(&session, &mut task)); // 提交旧来源副本。
        session.terrain_revision += 1; // 用户在结果到达前已换代。
        await_copy(&mut session, &mut task); // 旧副本结束但不得采用。
        assert!(Arc::ptr_eq(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_mesh
                .as_ref()
                .unwrap(),
            &old
        )); // 拒绝陈旧修订。
        assert!(start_terrain_materialization_if_shared(&session, &mut task)); // 新任务绑定新的修订与旧源。
        let replacement = Arc::new(old.as_ref().clone()); // 模拟同修订下地图源指针替换。
        session.draft.as_mut().unwrap().terrain_mesh = Some(replacement.clone()); // 新权威来源。
        await_copy(&mut session, &mut task); // 旧源副本仍必须被拒绝。
        assert!(Arc::ptr_eq(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_mesh
                .as_ref()
                .unwrap(),
            &replacement
        )); // 来源身份与修订都必须正确。
    }
}
