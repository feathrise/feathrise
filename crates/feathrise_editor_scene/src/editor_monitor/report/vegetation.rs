//! F4六种正式植被输出的密度、候选后端与既有预览完成值报告。

use feathrise_scene_content::{SceneVegetationKind, SceneVegetationLayer}; // 正式层类型决定GPU叶片或Mesh实例口径。
use feathrise_fps_display::{LodControlContent, LodControlPresetBindings};
use super::super::vegetation_target::target_layer;

use super::common::{
    append_density_stats, candidate_grid, density_byte, density_percent, format_count,
    no_draft_report, page_header, world_extent,
}; // 候选与R8换算必须保持单一正式口径。
use crate::authoring::{EditorAuthoringSession, EditorSceneDraft, VegetationPreviewState}; // 精确Mesh数量只读取既有散布和活动层缓存。
use crate::monitor_resources::EditorSceneMonitorPage; // 当前植被叶子页选择唯一profile。
use crate::resources::SceneEditorState; // 当前笔刷参数解释R8概率。
use crate::ui::brush_controls::BRUSH_STRENGTH_MIN; // 最低笔刷说明跟随正式控件范围。

/// 一个F4植被页对应的正式层身份与显示单位。
#[derive(Clone, Copy)]
struct VegetationPageSpec {
    title: &'static str,     // 页签和正文首行共用中文名。
    content: LodControlContent,   // 正式运行时profile稳定身份。
    kind: SceneVegetationKind, // Blade与Mesh采用不同数量语义。
    unit: &'static str,      // Mesh精确计数的中文单位。
}

/// 把六个植被页映射到唯一正式输出，不按任意Layer顺序猜测。
const fn page_spec(page: EditorSceneMonitorPage) -> Option<VegetationPageSpec> {
    match page {
        EditorSceneMonitorPage::OrdinaryGrass => Some(VegetationPageSpec {
            title: "普通草",                      // GPU草叶正式页。
            content: LodControlContent::OrdinaryGrass, // 复用项目草地风与四档规格。
            kind: SceneVegetationKind::Blade,       // 不生成全图逐株CPU数组。
            unit: "簇",                           // 仅用于通用文案，Blade不输出伪造精确总数。
        }),
        EditorSceneMonitorPage::Wheat => Some(VegetationPageSpec {
            title: "麦田",                  // GPU麦簇正式页。
            content: LodControlContent::Wheat,    // 复用麦田正式四档profile。
            kind: SceneVegetationKind::Blade, // 相机窗口实时生成。
            unit: "簇",                     // 不冒充独立麦秆数量。
        }),
        EditorSceneMonitorPage::MixedGrain => Some(VegetationPageSpec {
            title: "杂粮田",                   // GPU杂粮簇正式页。
            content: LodControlContent::MixedGrain, // 复用杂粮正式profile。
            kind: SceneVegetationKind::Blade,    // 相机窗口实时生成。
            unit: "簇",                        // 不冒充独立作物数量。
        }),
        EditorSceneMonitorPage::Trees => Some(VegetationPageSpec {
            title: "树木",                 // Mesh树木正式页。
            content: LodControlContent::Trees,    // 四米候选格的正式树木profile。
            kind: SceneVegetationKind::Mesh, // 后台精确散布并共享实例渲染。
            unit: "棵",                    // 用户可直接理解的精确实例单位。
        }),
        EditorSceneMonitorPage::Shrubs => Some(VegetationPageSpec {
            title: "灌木",                 // Mesh灌木正式页。
            content: LodControlContent::Shrubs,   // 两米候选格的正式灌木profile。
            kind: SceneVegetationKind::Mesh, // 后台精确散布并共享实例渲染。
            unit: "株",                    // 灌木实例计数单位。
        }),
        EditorSceneMonitorPage::Rocks => Some(VegetationPageSpec {
            title: "岩石",                 // Mesh岩石正式页。
            content: LodControlContent::Rocks,    // 两米候选格的正式岩石profile。
            kind: SceneVegetationKind::Mesh, // 后台精确散布并共享实例渲染。
            unit: "块",                    // 岩石实例计数单位。
        }),
        _ => None, // 非植被页由统一分派器送往其他报告模块。
    }
}

/// 构建一个正式植被输出页；输入只含常数大小元数据或既有后台完成值。
pub(super) fn build_vegetation_report(
    page: EditorSceneMonitorPage,       // 当前六种植被页之一。
    bindings: Option<&LodControlPresetBindings>,
    editor: &SceneEditorState,          // F12状态和实际笔刷参数。
    session: &EditorAuthoringSession, // 当前作者草稿及编译修订。
    preview: &VegetationPreviewState, // Mesh散布、隐藏安装和完整采用状态。
) -> String {
    let mut spec = page_spec(page).expect("统一分派器只把植被页传入植被报告"); // 内部枚举穷尽保证成立。
    let generic = bindings.and_then(|bindings| bindings.profile(spec.content)).is_none();
    spec.title = page.display_label(bindings);
    if generic { spec.unit = "个"; } // 自定义Mesh可能是水晶或建筑，不能沿用“棵树”的单位。
    let Some(draft) = session.draft.as_ref() else {
        return no_draft_report(spec.title);
    };
    let mut lines = page_header(spec.title, editor, session, draft);
    let Some(layer) = target_layer(draft, editor, bindings, spec.content, spec.kind) else {
        lines.push("本页没有工程绑定的预设或匹配的选中作者层；选中叶片图层后在叶片植被页查看，网格图层在网格植被页查看。".to_owned());
        return lines.join("\n");
    };
    let profile = layer.runtime_profile.as_deref().unwrap_or("未登记");
    if generic {
        lines.push("通用诊断：当前选中预设；统计属于该作者层对应的完整正式输出。".to_owned());
    }
    if let Some(selected) = crate::authoring::selected_vegetation_layer(draft, editor.selected_vegetation_layer)
        && selected.output.id == layer.id
    {
        lines.push(format!("当前作者层：{} [{}]  |  所属组：{} [{}]",
            selected.author.label, selected.author.id, selected.group.label, selected.group.id));
    }

    lines.push(format!(
        "正式输出：{} [{}]  |  profile：{}  |  后端：{}",
        layer.label,
        layer.id,
        profile,
        match spec.kind {
            SceneVegetationKind::Blade => "GPU叶片窗口",
            SceneVegetationKind::Mesh => "空间分块共享实例",
        }
    ));
    let compilation = if session.vegetation_compiled_revision == session.vegetation_revision {
        "已同步"
    } else {
        "后台编译中；统计仍属于上一完整正式修订"
    }; // 作者源变化时绝不把旧统计标成新修订。
    lines.push(format!(
        "植被修订：作者 {}  |  正式 {}  |  {}",
        session.vegetation_revision, session.vegetation_compiled_revision, compilation
    ));
    if let Some((width, length)) = world_extent(draft.manifest.world_min, draft.manifest.world_max)
    {
        lines.push(format!(
            "场景范围：{width:.1}m × {length:.1}m  |  密度图：{} × {} R8",
            layer.resolution[0], layer.resolution[1]
        ));
    } else {
        lines.push(format!(
            "场景范围：无效  |  密度图：{} × {} R8",
            layer.resolution[0], layer.resolution[1]
        ));
    }
    append_candidate_spec(&mut lines, draft, layer, spec); // Blade与Mesh显示各自真实容量语义。
    append_density_stats(&mut lines, draft, layer); // 复用后台编译时顺带归约的常数结构。
    match spec.kind {
        SceneVegetationKind::Blade => append_blade_runtime_status(&mut lines, layer),
        SceneVegetationKind::Mesh => append_mesh_preview_status(
            &mut lines,
            draft,
            layer,
            session.vegetation_compiled_revision,
            preview,
            spec,
        ),
    }
    append_brush_status(&mut lines, editor); // 六种密度层采用同一笔刷R8入口。
    lines.push(match spec.kind {
        SceneVegetationKind::Blade => "统计口径：Blade由GPU按相机窗口实时生成，不存在可扫描的全图逐株ECS集合；F4不伪造“实际株数”。".to_owned(),
        SceneVegetationKind::Mesh => format!(
            "统计口径：只计{}正式密度输出，不混入普通场景摆放；动态场景不存在8,192个实例总量截断。",
            spec.title
        ),
    });
    lines.join("\n")
}

/// 追加Mesh全图网格或Blade正式profile容量字段。
fn append_candidate_spec(
    lines: &mut Vec<String>,    // 当前植被报告行。
    draft: &EditorSceneDraft,     // 当前场景范围决定全图候选网格。
    layer: &SceneVegetationLayer, // 正式层提供作者间距和兼容容量。
    spec: VegetationPageSpec,   // 后端类型决定解释方式。
) {
    if spec.kind == SceneVegetationKind::Blade {
        let compatibility_capacity = if layer.max_instances == u32::MAX {
            "由正式profile决定".to_owned()
        } else {
            format_count(u64::from(layer.max_instances))
        }; // Blade容量由profile解释，不作为Mesh全图限制。
        lines.push(format!(
            "候选配置：间距 {:.2}m  |  profile兼容容量字段 {}  |  四档相机窗口实时采样",
            layer.spacing_meters, compatibility_capacity
        ));
        return;
    }
    let Some((columns, rows, total)) = candidate_grid(
        draft.manifest.world_min,
        draft.manifest.world_max,
        layer.spacing_meters,
    ) else {
        lines.push(format!(
            "候选规格：间距 {:.2}m  |  场景范围或间距无效，无法计算全图网格",
            layer.spacing_meters
        ));
        return;
    };
    lines.push(format!(
        "候选规格：{:.2}m  |  网格 {} × {} = {} 个槽  |  不设全图实例硬上限",
        layer.spacing_meters,
        format_count(u64::from(columns)),
        format_count(u64::from(rows)),
        format_count(total)
    ));
}

/// Blade没有全图逐株数组，只报告真实GPU窗口和作者密度语义。
fn append_blade_runtime_status(lines: &mut Vec<String>, layer: &SceneVegetationLayer) {
    lines.push(format!(
        "GPU叶片：四档正式窗口按相机位置与R8密度实时生成  |  缩放 {:.2}..{:.2}",
        layer.scale_range[0], layer.scale_range[1]
    ));
    lines.push("CPU/ECS：不生成全图逐株数组  |  逐草/逐作物渲染实体 0".to_owned());
}

/// 追加一个Mesh层的CPU散布、隐藏安装或完整活动数量。
fn append_mesh_preview_status(
    lines: &mut Vec<String>,          // 当前植被报告行。
    draft: &EditorSceneDraft,           // 当前场景身份用于拒绝串图预览。
    layer: &SceneVegetationLayer,       // 当前正式Mesh输出ID。
    compiled_revision: u64,           // 精确目标必须属于当前完整正式修订。
    preview: &VegetationPreviewState, // 既有single-flight预览完成值。
    spec: VegetationPageSpec,         // 中文名称和数量单位。
) {
    let active = preview
        .layers
        .get(&layer.id)
        .filter(|active| active.scene_id == draft.scene_id); // 只接受当前场景同层活动预览。
    let installing = preview.install.as_ref().filter(|install| {
        install.scene_id == draft.scene_id
            && install.layer_id == layer.id
            && install.revision == compiled_revision
    }); // 隐藏安装一开始就拥有精确最终Vec长度。
    if let Some(install) = installing {
        let target = install.build.transforms.len() as u64; // 后台散布结果长度就是本修订精确目标。
        lines.push(format!(
            "实际{}：本修订精确目标 {} {}  |  当前可见 {}",
            spec.title,
            format_count(target),
            spec.unit,
            active.map_or_else(
                || "无旧预览".to_owned(),
                |active| format!(
                    "{} {}（修订{}）",
                    format_count(active.instance_count as u64),
                    spec.unit,
                    active.applied_revision
                )
            )
        ));
        lines.push(format!(
            "预览进度：隐藏安装 {} / {} {}；完成后按层原子采用，目标未裁剪",
            format_count(install.cursor as u64),
            format_count(target),
            spec.unit
        ));
        return;
    }
    let computing = preview.task.is_some()
        && preview.desired_scene_id.as_ref() == Some(&draft.scene_id)
        && preview.desired_layer_id.as_deref() == Some(layer.id.as_str())
        && preview.desired_revision == compiled_revision; // 任务句柄不暴露半成品数量。
    if computing {
        lines.push(format!(
            "实际{}：新修订正在后台精确散布  |  当前可见 {}",
            spec.title,
            active.map_or_else(
                || "无旧预览".to_owned(),
                |active| format!(
                    "{} {}（修订{}）",
                    format_count(active.instance_count as u64),
                    spec.unit,
                    active.applied_revision
                )
            )
        ));
        lines
            .push("预览进度：CPU散布中；完成后建立共享实例批次，不生成逐实例渲染实体。".to_owned());
        return;
    }
    if let Some(active) = active {
        if active.applied_revision == compiled_revision {
            lines.push(format!(
                "实际{}：{} {}（正式密度预览已完整采用，精确值）",
                spec.title,
                format_count(active.instance_count as u64),
                spec.unit
            ));
            lines.push(format!("预览进度：已收敛到正式修订 {compiled_revision}"));
            if draft.manifest.is_data_driven() {
                lines.push(format!(
                    "渲染后端：128m分块共享实例  |  非空批次 {}  |  ECS渲染节点 {}  |  逐实例渲染实体 0",
                    format_count(active.batch_count as u64),
                    format_count(active.render_node_count as u64)
                )); // 同时显示作者总数与有限承接规模，避免二者混为一谈。
            }
        } else {
            lines.push(format!(
                "实际{}：当前可见 {} {}（旧修订{}）；正式修订{}尚未生成预览",
                spec.title,
                format_count(active.instance_count as u64),
                spec.unit,
                active.applied_revision,
                compiled_revision
            ));
            lines.push("预览进度：进入对应植被层后继续生成结果。".to_owned());
        }
        return;
    }
    let zero_density = draft
        .vegetation_density_stats
        .get(&layer.id)
        .is_some_and(|stats| stats.nonzero_pixels == 0); // 全零正式图无需散布即可确定目标为零。
    if zero_density {
        lines.push(format!(
            "实际{}：0 {}（当前正式密度图全零）",
            spec.title, spec.unit
        ));
        lines.push("预览进度：等待编辑器应用清理结果；零结果不会加载原型或扫描高度场。".to_owned());
    } else {
        lines.push(format!(
            "实际{}：尚无当前正式修订的精确预览结果",
            spec.title
        ));
        lines.push("预览进度：进入对应植被层后开始后台散布。".to_owned());
    }
}

/// 显示一次单击中心写入和最低档R8量化，解释低强度仍可能覆盖大量候选。
fn append_brush_status(lines: &mut Vec<String>, editor: &SceneEditorState) {
    let current_center_r8 = density_byte(editor.brush_strength); // 当前强度与正式笔刷入口采用同一四舍五入。
    lines.push(format!(
        "当前笔刷：半径 {:.1}m  |  强度 {:.1}%  |  单击中心目标 R8 {} ({:.2}%)",
        editor.brush_radius,
        editor.brush_strength * 100.0,
        current_center_r8,
        density_percent(current_center_r8)
    ));
    let minimum_r8 = density_byte(BRUSH_STRENGTH_MIN); // 最低强度来自公共控件正式范围。
    lines.push(format!(
        "最低笔刷口径：中心 {:.0}% = R8 {} = {:.2}%，沿半径线性衰减到0；Mesh实际数量以正式散布结果为准。",
        BRUSH_STRENGTH_MIN * 100.0,
        minimum_r8,
        density_percent(minimum_r8)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_vegetation_pages_scene_to_unique_diagnostic_slots() {
        let specs = EditorSceneMonitorPage::ALL
            .iter()
            .copied()
            .filter_map(page_spec)
            .collect::<Vec<_>>();
        assert_eq!(specs.len(), 6); // 旧故道专页不得重新混入动态场景植被页。
        for (index, spec) in specs.iter().enumerate() {
            assert!(
                specs[index + 1..]
                    .iter()
                    .all(|other| other.content != spec.content)
            ); // 每页对应独立诊断槽，实际预设由工程映射或当前选中层提供。
        }
        assert_eq!(
            specs
                .iter()
                .filter(|spec| spec.kind == SceneVegetationKind::Blade)
                .count(),
            3
        );
        assert_eq!(
            specs
                .iter()
                .filter(|spec| spec.kind == SceneVegetationKind::Mesh)
                .count(),
            3
        );
    }
}
