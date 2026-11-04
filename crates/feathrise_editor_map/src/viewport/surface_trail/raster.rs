//! 固定256²地表覆盖栅格；连续胶囊柔边、稀疏清理与固定预算脏像素队列。
use super::*; // 只操作覆盖资源，绝不访问作者整图或渲染世界。
impl SurfaceStrokeTrail {
    #[cfg(test)] // 既有输入测试继续验证短点击没有丢失。
    pub(crate) fn has_segments(&self) -> bool {
        !self.touched.is_empty() // 现在表示存在连续填色而非线框段。
    }

    fn dirty_pixel(&mut self, index: usize) {
        let block=index/(DIM*16);
        self.projection_versions[block]=self.projection_versions[block].wrapping_add(1).max(1);
        if !self.dirty_flags[index] {
            // 同一帧同一像素只上传一次。
            self.dirty_flags[index] = true; // 标记已排队。
            self.dirty.push(index); // 队列容量始终不超过PIXELS。
        }
    }

    pub(crate) fn clear(&mut self) {
        while let Some(index) = self.touched.pop() {
            // 只有生命周期变化才清除实际画过的区域。
            self.pixels[index] = [0; 4]; // 图像恢复完全透明。
            self.dirty_pixel(index); // 下次复用前将透明值写回图像。
        }
        self.map_id = None; // 清除旧地图的显示资格。
        self.terrain_id = None;
        self.target_revision = 0; // 不继承已结束笔划的采用目标。
        self.pending_keys = [None; 2]; // 取消后不复用旧受理签名。
    }

    pub(crate) fn reconcile(&mut self, session: &EditorAuthoringSession) {
        if self.map_id.as_ref().is_some_and(|map| {
            // 稳定帧仅比较小型身份，不扫描覆盖图。
            session.draft.as_ref().is_none_or(|draft| &draft.map_id != map)
                || session.draft.as_ref().and_then(|draft| draft.terrain_documents.active.as_ref())
                    .map(|active| &active.definition.id) != self.terrain_id.as_ref()
                || session.revision != self.author_revision // 本笔的合法推进由record同步。
                || session.terrain_revision != self.terrain_revision // 新地形不能使用旧覆盖坐标。
        }) {
            self.clear(); // 撤销、切图和地形变更移除过期覆盖。
        }
    }

    pub(crate) fn record(
        &mut self,                        // 唯一覆盖写入者，像素不与PBR后台共享。
        session: &EditorAuthoringSession, // 冻结空间和作者身份。
        from: Vec3,                       // 真实受理起点。
        to: Vec3,                         // 单击允许起终点相同。
        radius: f32,                      // 样本冻结的实际半径。
        erase: bool,                      // 样本冻结的绘制语义。
        amount: f32,                      // 样本冻结强度，不读取后续UI值。
    ) {
        let Some(draft) = session.draft.as_ref() else {
            return;
        }; // 没有草稿就没有有效投影。
        if !from.is_finite()
            || !to.is_finite()
            || !radius.is_finite()
            || radius <= 0.0
            || !amount.is_finite()
            || amount <= 0.0
        {
            return; // 非法或零强度样本不可制造覆盖。
        }
        let metric = crate::authoring::terrain_documents::PaintMetricSpace::from_draft(draft);
        let from = metric.point(from);
        let to = metric.point(to);
        let min = Vec2::from_array(metric.bounds(draft.terrain_manifest().world_min));
        let span = Vec2::from_array(metric.bounds(draft.terrain_manifest().world_max)) - min; // 比例仍为源UV，距离使用世界米数。
        if !span.is_finite() || span.min_element() <= 0.0 {
            return;
        } // 防止退化空间造成除零。
        if self.pixels.is_empty() {
            // 首次使用仅分配固定小画布，不复制作者图。
            self.pixels.resize(PIXELS, [0; 4]); // 256KiB RGBA工作画布。
            self.dirty_flags.resize(PIXELS, false); // 固定去重标记。
            self.touched.reserve_exact(PIXELS); // 首次固定分配避免拖动期间扩容。
            self.dirty.reserve_exact(PIXELS); // 上传队列最多每像素一个位置。
        }
        self.map_id = Some(draft.map_id.clone()); // 小型身份复制不涉及地图像素。
        self.terrain_id = draft
            .terrain_documents
            .active
            .as_ref()
            .map(|active| active.definition.id.clone());
        self.author_revision = session.revision; // 本笔的受理版本已更新。
        self.terrain_revision = session.terrain_revision; // 与当前地形空间一致。
        self.target_revision = self.target_revision.max(session.surface_revision); // 较旧回执不能清最新笔。
        let a = Vec2::new(from.x, from.z); // 投影为地形平面坐标。
        let b = Vec2::new(to.x, to.z); // 胶囊距离连续连接同一个受理段。
        let texel = span / (DIM - 1) as f32; // 估计覆盖像素的世界面积。
        let support = radius + texel.length() * 0.5; // 亚像素笔刷仍有至少一个柔边像素。
        let lower = (((a.min(b) - Vec2::splat(support) - min) / span) * (DIM - 1) as f32)
            .floor()
            .clamp(Vec2::ZERO, Vec2::splat((DIM - 1) as f32)); // 仅处理段的局部包围盒。
        let upper = (((a.max(b) + Vec2::splat(support) - min) / span) * (DIM - 1) as f32)
            .ceil()
            .clamp(Vec2::ZERO, Vec2::splat((DIM - 1) as f32)); // 大笔最大也只有固定256²像素。
        let ab = b - a; // 线段方向。
        let length_squared = ab.length_squared(); // 单击用零长度胶囊处理。
        let color = if erase {
            [255, 112, 80] // 明亮橙红让擦除与绘制明确区分。
        } else {
            [72, 245, 92] // 直接沿用植被覆盖的亮绿色，避免混进草土底色。
        }; // 作者反馈优先保证辨识度，边缘仍使用原柔边衰减。
        let opacity = 160.0 + 64.0 * amount.clamp(0.0, 1.0).sqrt(); // 非零弱刷中心至少约63%不透明度，避免帧时间缩小强度后看不清。
        for z in lower.y as usize..=upper.y as usize {
            // 局部行扫描，没有整图作者采样。
            for x in lower.x as usize..=upper.x as usize {
                // 每段最多PIXELS个简单距离计算。
                let point = min + Vec2::new(x as f32, z as f32) * texel; // 对应地形UV像素。
                let t = if length_squared > f32::EPSILON {
                    ((point - a).dot(ab) / length_squared).clamp(0.0, 1.0)
                } else {
                    0.0
                }; // 单击不除零。
                let distance = point.distance(a + ab * t); // 连续胶囊没有离散刷印圆圈。
                let edge = ((support - distance) / (support * 0.65)).clamp(0.0, 1.0); // 外圈平滑渐隐，中央保留清晰反馈。
                let alpha = (opacity * edge * edge * (3.0 - 2.0 * edge)).round() as u8; // smoothstep柔边不出现硬轮廓。
                if alpha == 0 {
                    continue;
                } // 未触及像素不进入脏队列。
                let index = z * DIM + x; // 固定栅格唯一索引。
                let old = self.pixels[index]; // 重叠取最大透明度，不因重复等待帧越来越亮。
                let rgba = [
                    color[0],
                    color[1],
                    color[2],
                    if old[..3] == color {
                        old[3].max(alpha)
                    } else {
                        alpha
                    },
                ]; // 擦除与绘制切换使用新语义颜色。
                if old != rgba {
                    // 稳定按住且强度不变时不重复触发上传。
                    if old[3] == 0 {
                        self.touched.push(index);
                    } // 每个像素只登记一次，长笔不截断。
                    self.pixels[index] = rgba; // 局部修改唯一工作副本。
                    self.dirty_pixel(index); // 本帧仅上传实际变化。
                }
            }
        }
    }
}
