//! 正式无限网格通道的离屏像素回归；覆盖细线一致性、工作平面、远景延伸与相机隔离。
use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{
        gltf_browser::EditorGltfBrowserState, vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::{multi_select::PlacementSelectionSet, placement_move::PlacementMoveState},
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, visibility::RenderLayers},
    image::ImageSampler,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use feathrise_editor_core::EditorSettings;
use std::{io::Write, time::Duration};

/// 显式验收目录必须为绝对路径，且每份证据只允许新建，防止本轮覆盖旧报告。
fn output_file(name: &str) -> std::fs::File {
    let explicit = std::env::var_os("FEATHRISE_GRID_GPU_REPORT");
    let output = if let Some(path) = explicit.as_ref() {
        let path = std::path::PathBuf::from(path);
        assert!(path.is_absolute(), "FEATHRISE_GRID_GPU_REPORT必须为绝对目录");
        path
    } else {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/infinite-grid-validation")
    };
    std::fs::create_dir_all(&output).unwrap();
    let path = output.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if explicit.is_some() {
        options.create_new(true);
    } else {
        options.create(true).truncate(true); // 不显式设置时保留原取证路径及覆盖行为。
    }
    options.open(&path).unwrap_or_else(|error| panic!("网格证据写入失败 {}：{error}", path.display()))
}

/// 只计真实地面内部的像素；窗口外的网格不能冒充地表可见性，边缘另留半米安全带。
fn ground_mask(app: &App, camera: Entity, ground: Entity) -> Vec<bool> {
    let world = app.world();
    let view = world.get::<Camera>(camera).unwrap();
    let camera_transform = world.get::<GlobalTransform>(camera).unwrap();
    let inverse = world
        .get::<GlobalTransform>(ground)
        .unwrap()
        .to_matrix()
        .inverse();
    (0..640 * 480)
        .map(|index| {
            let pixel = Vec2::new((index % 640) as f32 + 0.5, (index / 640) as f32 + 0.5);
            let Ok(ray) = view.viewport_to_world(camera_transform, pixel) else {
                return false;
            };
            let origin = inverse.transform_point3(ray.origin);
            let direction = inverse.transform_vector3(*ray.direction);
            if direction.y.abs() < 1.0e-6 {
                return false;
            }
            let distance = -origin.y / direction.y;
            let hit = origin + direction * distance;
            distance > 0.0 && hit.x.abs() < 4.5 && hit.z.abs() < 4.5
        })
        .collect()
}

fn visible_ground_pixels(on: &[u8], off: &[u8], mask: &[bool]) -> usize {
    on.chunks_exact(4)
        .zip(off.chunks_exact(4))
        .zip(mask)
        .filter(|((on, off), inside)| {
            **inside && (0..3).any(|channel| on[channel].abs_diff(off[channel]) >= 4)
        })
        .count() // 新合同是统一低对比细线；仍须位于实际地面内部，不能拿窗口外线段通过。
}

/// 内存中生成明暗土/草交错纹理；细碎明暗模拟草叶和土粒，不写入正式资产目录。
fn textured_ground() -> Image {
    let mut pixels = Vec::with_capacity(256 * 256 * 4);
    for z in 0..256_u32 {
        for x in 0..256_u32 {
            let grass = ((x / 16 + z / 16) & 1) == 0;
            let noise = ((x * 73 + z * 151 + (x ^ z) * 19) % 49) as i32 - 24;
            let blade = if (x + z * 3) % 11 < 2 { 22 } else { 0 };
            let base = if grass { [44, 75, 29] } else { [188, 170, 125] };
            for channel in base {
                pixels.push((channel + noise + blade).clamp(0, 255) as u8);
            }
            pixels.push(255);
        }
    }
    let mut texture = Image::new(
        Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    texture.sampler = ImageSampler::nearest(); // 纹理噪声真实参与渲染，固定采样让跨机器回归可复现。
    texture
}

fn luminance(pixel: &[u8]) -> f32 {
    let linear = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
}

fn lightness(pixel: &[u8]) -> f32 {
    let value = luminance(pixel);
    if value > 216.0 / 24389.0 {
        116.0 * value.cbrt() - 16.0
    } else {
        value * (24389.0 / 27.0)
    }
}

/// 挡块前面的实体表面不能冒充地板；只有射线先命中地板的像素才进入普通线覆盖率。
fn blocker_before_ground(ray: &Ray3d, ground_distance: f32) -> bool {
    let minimum = Vec3::new(5.05, 0.0, 4.3);
    let maximum = Vec3::new(6.55, 3.0, 5.7);
    let mut near = 0.0_f32;
    let mut far = ground_distance;
    for axis in 0..3 {
        let origin = ray.origin[axis];
        let direction = ray.direction[axis];
        if direction.abs() < 1.0e-6 {
            if origin < minimum[axis] || origin > maximum[axis] {
                return false;
            }
            continue;
        }
        let first = (minimum[axis] - origin) / direction;
        let last = (maximum[axis] - origin) / direction;
        near = near.max(first.min(last));
        far = far.min(first.max(last));
        if near > far {
            return false;
        }
    }
    near < ground_distance && far > 0.0
}

/// 固定采样普通格线x=4，验收中性灰的真实混合；同亮度纹理不再被要求产生高对比描边。
fn measure_ordinary_grid_line(
    app: &App,
    camera: Entity,
    ground: Entity,
    sunny: &[u8],
    off: &[u8],
    on: &[u8],
) {
    let view = app.world().get::<Camera>(camera).unwrap();
    let transform = app.world().get::<GlobalTransform>(camera).unwrap();
    let mut mask = ground_mask(app, camera, ground);
    for (index, inside) in mask.iter_mut().enumerate() {
        if !*inside {
            continue;
        }
        let ray = view
            .viewport_to_world(
                transform,
                Vec2::new((index % 640) as f32 + 0.5, (index / 640) as f32 + 0.5),
            )
            .unwrap();
        let distance = -ray.origin.y / ray.direction.y;
        *inside = distance > 0.0 && !blocker_before_ground(&ray, distance);
    } // 邻域也排除被挡块真正遮住的地板，不能从物体轮廓上的描边偷取可读像素。
    let mut visited = std::collections::BTreeSet::new();
    let mut counts = [(0_usize, 0_usize, 0_usize, 0_usize); 4]; // 草/土 × 日照/阴影：总数、亮度可辨、可判混合、正确混合。
    let linear = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    let color = |pixel: &[u8]| Vec3::new(linear(pixel[0]), linear(pixel[1]), linear(pixel[2]));
    let target_gray = Vec3::splat(0.036306); // 正式普通线sRGB(0.21)的线性值；混合发生在输出sRGB编码之前。
    let mut marked = on.to_vec();
    for sample in 0..241 {
        let z = 2.0 + sample as f32 * 0.025;
        if (z - z.round()).abs() < 0.12 {
            continue; // 相交格线会加粗，不能用交点掩盖普通细线的辨认困难。
        }
        let point = Vec3::new(4.0, 0.015, z);
        let Ok(projected) = view.world_to_viewport(transform, point) else {
            continue;
        };
        let x = projected.x.floor() as i32;
        let y = projected.y.floor() as i32;
        if x < 2 || y < 2 || x >= 638 || y >= 478 || !visited.insert((x, y)) {
            continue;
        }
        let index = y as usize * 640 + x as usize;
        if !mask[index] {
            continue;
        }
        let ray = view
            .viewport_to_world(transform, Vec2::new(x as f32 + 0.5, y as f32 + 0.5))
            .unwrap();
        let ground_distance = -ray.origin.y / ray.direction.y;
        if ground_distance <= 0.0 || blocker_before_ground(&ray, ground_distance) {
            continue;
        }
        let ground_point = ray.origin + *ray.direction * ground_distance;
        let tile_x = ground_point.x / 0.625;
        let tile_z = ground_point.z / 0.625;
        if tile_z.fract() < 0.08 || tile_z.fract() > 0.92 {
            continue; // 纹理块边缘的滤波或像素量化不得误分草/土类别。
        }
        let grass = ((tile_x.floor() as i32 + tile_z.floor() as i32) & 1) == 0;
        let pixel = &off[index * 4..index * 4 + 4];
        let sunny_pixel = &sunny[index * 4..index * 4 + 4];
        let shadow = luminance(pixel) < luminance(sunny_pixel) * 0.65
            && lightness(sunny_pixel) - lightness(pixel) > 8.0;
        let bucket = usize::from(!grass) * 2 + usize::from(shadow);
        let mut contrast = 0.0_f32;
        let mut measurable = false;
        let mut mixed = false;
        for dy in -2..=2 {
            for dx in -2..=2 {
                let neighbor = (y + dy) as usize * 640 + (x + dx) as usize;
                if mask[neighbor] {
                    let before = &off[neighbor * 4..neighbor * 4 + 4];
                    let after = &on[neighbor * 4..neighbor * 4 + 4];
                    contrast = contrast.max(
                        (lightness(after) - lightness(before)).abs(),
                    );
                    // 单灰线遇到相同灰底时不会凭空增加亮度；只对与目标灰至少差8阶的RGB分量作混合判定。
                    if before[..3].iter().all(|channel| channel.abs_diff(54) < 8) {
                        continue;
                    }
                    measurable = true;
                    let source = color(before);
                    let direction = target_gray - source;
                    let actual = color(after);
                    let alpha = (actual - source).dot(direction) / direction.length_squared();
                    let residual = actual.distance(source + direction * alpha);
                    let changed = (0..3).any(|channel| before[channel].abs_diff(after[channel]) >= 2);
                    // AA/MSAA允许部分覆盖；必须确有变化且落在“原色→中性灰”的线性混合线上，不能用任意亮暗噪声通过。
                    // 0.004线性残差容纳约2~3阶输出量化，避免RGB逐通道取整被误判为另一种线色。
                    mixed |= changed && (0.08..=1.08).contains(&alpha) && residual <= 0.004;
                }
            }
        } // 两像素容纳一像素线的抗锯齿覆盖；最近其他格线远于此窗口，不累计整幅变化像素。
        let readable = contrast >= 3.0;
        counts[bucket].0 += 1;
        counts[bucket].1 += usize::from(readable);
        counts[bucket].2 += usize::from(measurable);
        counts[bucket].3 += usize::from(measurable && mixed);
        marked[index * 4..index * 4 + 3].copy_from_slice(if measurable && mixed {
            &[40, 235, 70]
        } else if !measurable {
            &[90, 150, 235]
        } else {
            &[240, 35, 35]
        });
    }
    let names = ["草纹理日照", "草纹理阴影", "土纹理日照", "土纹理阴影"];
    let report = names
        .iter()
        .zip(counts)
        .map(|(name, (total, readable, measurable, mixed))| {
            format!(
                "{name}：亮度可辨{readable}/{total}，{:.1}%（ΔL*≥3，仅报告）；中性灰正确混合{mixed}/{measurable}，同灰不可判{}（MSAA4）",
                readable as f32 * 100.0 / total.max(1) as f32,
                total - measurable,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    image::RgbaImage::from_raw(640, 480, marked)
        .unwrap()
        .write_to(&mut output_file("editor-grid-textured-shadow-msaa4-samples.png"), image::ImageFormat::Png)
        .unwrap();
    output_file("editor-grid-textured-shadow-msaa4-coverage.txt").write_all(report.as_bytes()).unwrap();
    println!("{report}");
    for (name, (total, _, measurable, mixed)) in names.iter().zip(counts) {
        assert!(total >= 6, "{name}必须确有普通线采样，实得{total}");
        assert!(measurable >= 6, "{name}必须有足够可判混合样本，不能靠排除全部同灰纹理通过：{measurable}/{total}");
        assert!(
            mixed * 100 >= measurable * 75,
            "{name}普通格线至少75%可判样本应实际混入统一中性灰，实得{mixed}/{measurable}"
        );
    }
}

fn capture(app: &mut App, target: Handle<Image>, name: &str) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "网格离屏回读超时");
        std::thread::sleep(Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), 640 * 480 * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(640, 480, bytes.clone())
        .unwrap()
        .write_to(&mut output_file(name), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// 在同源深灰背景上量普通线宽与对比；正交且关闭MSAA，避免交点或抗锯齿替细线通过。
fn wire_line_profile(app: &App, camera: Entity, on: &[u8], off: &[u8]) -> (usize, u8) {
    let world = app.world();
    let point = world.get::<Camera>(camera).unwrap().world_to_viewport(
        world.get::<GlobalTransform>(camera).unwrap(),
        Vec3::new(1.0, 0.0, 1.35),
    ).unwrap(); // x=1普通格线，z=1.35避开横线与主格；12米视高下邻格相距40像素。
    let (x, y) = (point.x.floor() as i32, point.y.floor() as i32);
    (-6..=6).fold((0, 0), |(width, peak), dx| {
        let index = (y as usize * 640 + (x + dx) as usize) * 4;
        let delta = (0..3).map(|channel| on[index + channel].abs_diff(off[index + channel])).max().unwrap();
        (width + usize::from(delta > 3), peak.max(delta))
    })
}

fn changed_pixels(on: &[u8], off: &[u8]) -> usize {
    on.chunks_exact(4).zip(off.chunks_exact(4)).filter(|(on, off)| {
        (0..3).any(|channel| on[channel].abs_diff(off[channel]) > 3)
    }).count()
}

/// 局部投影验收不取全图最大值，避免远处任意一条格线替指定相位通过。
fn projected_delta(app: &App, camera: Entity, point: Vec3, on: &[u8], off: &[u8]) -> u8 {
    let world = app.world();
    let point = world.get::<Camera>(camera).unwrap().world_to_viewport(
        world.get::<GlobalTransform>(camera).unwrap(), point,
    ).unwrap();
    let (x, y) = (point.x.floor() as i32, point.y.floor() as i32);
    assert!(x > 1 && x < 638 && y > 1 && y < 478, "相位采样必须在视口内：{point:?}");
    (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
        .map(|(x, y)| {
            let index = (y as usize * 640 + x as usize) * 4;
            (0..3).map(|channel| on[index + channel].abs_diff(off[index + channel])).max().unwrap()
        })
        .max().unwrap()
}

/// 保持基础步长不变，单独放大投影；超过旧窗口半径的像素仍须出现网格。
fn distant_grid_pixels(app: &App, camera: Entity, on: &[u8], off: &[u8]) -> usize {
    let world = app.world();
    let view = world.get::<Camera>(camera).unwrap();
    let transform = world.get::<GlobalTransform>(camera).unwrap();
    on.chunks_exact(4).zip(off.chunks_exact(4)).enumerate().filter(|(index, (on, off))| {
        if !(0..3).any(|channel| on[channel].abs_diff(off[channel]) > 3) {
            return false;
        }
        let pixel = Vec2::new((index % 640) as f32 + 0.5, (index / 640) as f32 + 0.5);
        let Ok(ray) = view.viewport_to_world(transform, pixel) else { return false; };
        if ray.direction.y.abs() < 1.0e-6 { return false; }
        let distance = -ray.origin.y / ray.direction.y;
        let hit = ray.origin + *ray.direction * distance;
        distance > 0.0 && hit.is_finite()
            && hit.x.abs() > 1.0 && hit.z.abs() > 1.0
            && (hit.x.abs() > 40.0 || hit.z.abs() > 40.0)
    }).count()
}

/// 必须在真实轴投影附近看到该轴的主色，不能用任意全图有色像素通过。
fn assert_wire_axis(app: &App, camera: Entity, pixels: &[u8], point: Vec3, channel: usize, name: &str) {
    let world = app.world();
    let projected = world.get::<Camera>(camera).unwrap().world_to_viewport(
        world.get::<GlobalTransform>(camera).unwrap(), point,
    ).unwrap();
    let (x, y) = (projected.x.floor() as i32, projected.y.floor() as i32);
    let visible = (-1..=1).any(|dy| (-1..=1).any(|dx| {
        let index = ((y + dy) as usize * 640 + (x + dx) as usize) * 4;
        let color = &pixels[index..index + 3];
        color[channel] as i16 - color[(channel + 1) % 3] as i16 > 12
            && color[channel] as i16 - color[(channel + 2) % 3] as i16 > 12
    }));
    assert!(visible, "{name}坐标轴投影附近必须显示正确主色");
}

/// 复用正式网格和着色通道，核验后还原现场，不改变后续地表/阴影验收。
fn verify_wire_reference(app: &mut App, target: Handle<Image>, camera: Entity, resident: Entity) {
    use crate::viewport::shading::{ShadingMode, ShadingState};
    let previous_shading = app.world_mut().remove_resource::<ShadingState>();
    let previous_snap = app.world().resource::<EditorSettings>().editor_snapping;
    let (previous_open, previous_view, previous_cursor) = {
        let editor = app.world().resource::<SceneEditorState>();
        (editor.open, editor.view, editor.cursor_world)
    };
    let previous_preview = app.world().resource::<MeshSnapCursor>().preview;
    let previous_center = app.world().resource::<SceneEditorViewportState>().center;
    let previous_transform = *app.world().get::<Transform>(camera).unwrap();
    let previous_projection = app.world().get::<Projection>(camera).unwrap().clone();
    let previous_msaa = *app.world().get::<Msaa>(camera).unwrap();
    let previous_clear = app.world().get::<Camera>(camera).unwrap().clear_color.clone();
    let previous_visibility = *app.world().get::<Visibility>(resident).unwrap();
    app.insert_resource(ShadingState::default());
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = true;
        editor.view = crate::types::EditorView::Top;
        editor.cursor_world = None;
    }
    {
        let mut settings = app.world_mut().resource_mut::<EditorSettings>();
        settings.editor_snapping.mode = feathrise_editor_core::EditorSnapMode::WorldGrid;
        settings.editor_snapping.translation_meters = 1.0;
        settings.editor_snapping.show_grid = false;
    }
    app.world_mut().resource_mut::<MeshSnapCursor>().preview = None;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = Vec3::ZERO;
    *app.world_mut().get_mut::<Visibility>(resident).unwrap() = Visibility::Hidden;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(0.0, 18.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
    app.world_mut().entity_mut(camera).insert((
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 12.0 },
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Off,
    ));
    app.world_mut().get_mut::<Camera>(camera).unwrap().clear_color =
        bevy::camera::ClearColorConfig::Custom(Color::linear_rgb(0.055, 0.055, 0.055));
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let off = capture(app, target.clone(), "editor-grid-wireframe-off.png");
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    } // 新正交组合须先让真实GPU管线就绪，不能把空白帧误当作细线。
    let ordinary = capture(app, target.clone(), "editor-grid-wireframe-regular.png");
    let (ordinary_width, ordinary_delta) = wire_line_profile(app, camera, &ordinary, &off);
    assert!(ordinary_width > 0 && ordinary_width <= 2,
        "所有模式普通格线应为约一像素细线，不能继续保留粗描边：{ordinary_width}");
    assert!((4..=45).contains(&ordinary_delta),
        "普通细格在灰底可见并保持低对比：{ordinary_delta}");
    for (mode, name) in [
        (ShadingMode::Wireframe, "wireframe"),
        (ShadingMode::Solid, "solid"),
        (ShadingMode::MaterialPreview, "material-preview"),
        (ShadingMode::Rendered, "rendered"),
    ] {
        app.world_mut().resource_mut::<ShadingState>().mode = mode;
        for _ in 0..40 {
            app.update();
            std::thread::sleep(Duration::from_millis(16));
        } // 真实实体/线框后处理也需完成特化；网格必须画在其后，不能被覆盖。
        let pixels = capture(app, target.clone(), &format!("editor-grid-unified-{name}.png"));
        assert_eq!(pixels, ordinary,
            "同源灰底下{mode:?}参考网格应逐字节一致，着色切换不能替换网格样式");
        assert_wire_axis(app, camera, &pixels, Vec3::new(2.35, 0.0, 0.0), 0, "X红");
        assert_wire_axis(app, camera, &pixels, Vec3::new(0.0, 0.0, 2.35), 2, "Z蓝");
    }
    app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Rendered;
    for _ in 0..4 { app.update(); }
    assert!(capture(app, target.clone(), "editor-grid-wireframe-restored.png") == ordinary,
        "退出线框仍须保持统一网格，不残留额外粗线");
    let marker = Vec3::new(0.5, 0.0, 0.5);
    app.world_mut().resource_mut::<MeshSnapCursor>().preview = Some(marker);
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    } // 此前只有全屏网格；首次真实Gizmo需让芯线/描边管线完成编译，不能回读尚未就绪的帧。
    let marked = capture(app, target.clone(), "editor-grid-unified-cursor.png");
    assert!(projected_delta(app, camera, marker, &marked, &ordinary) > 30,
        "无限细网格不能撤下独立的金色落点十字");
    app.world_mut().resource_mut::<MeshSnapCursor>().preview = None;
    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Front;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(0.0, 0.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y);
    for _ in 0..4 { app.update(); }
    let front = capture(app, target.clone(), "editor-grid-wireframe-front.png");
    assert_wire_axis(app, camera, &front, Vec3::new(2.35, 0.0, 0.0), 0, "X红");
    assert_wire_axis(app, camera, &front, Vec3::new(0.0, 2.35, 0.0), 1, "Y绿");
    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Side;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(18.0, 0.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y);
    for _ in 0..4 { app.update(); }
    let side = capture(app, target.clone(), "editor-grid-unified-side.png");
    assert_wire_axis(app, camera, &side, Vec3::new(0.0, 0.0, 2.35), 2, "Z蓝");
    assert_wire_axis(app, camera, &side, Vec3::new(0.0, 2.35, 0.0), 1, "Y绿");

    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Top;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(0.0, 180.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
    *app.world_mut().get_mut::<Projection>(camera).unwrap() = Projection::Orthographic(OrthographicProjection {
        scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 120.0 },
        ..OrthographicProjection::default_3d()
    }); // extent仍为20：基础格间距不变，旧65×65窗口会在画面中留下明显边界。
    for (orthographic, name) in [(true, "orthographic"), (false, "perspective")] {
        if !orthographic {
            app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Perspective;
            *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::new(0.0, 0.0, -20.0), Vec3::Y);
            *app.world_mut().get_mut::<Projection>(camera).unwrap() =
                Projection::Perspective(PerspectiveProjection::default());
        }
        app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = false;
        for _ in 0..8 { app.update(); }
        let distant_off = capture(app, target.clone(), &format!("editor-grid-infinite-{name}-off.png"));
        app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
        for _ in 0..8 { app.update(); }
        let distant_on = capture(app, target.clone(), &format!("editor-grid-infinite-{name}-on.png"));
        let distant = distant_grid_pixels(app, camera, &distant_on, &distant_off);
        assert!(distant > 500, "{name}旧32格边界以外仍须有连续网格：{distant}");
    }

    // 20000米高空时，先转世界再相减的near/mid会丢失小方向差；用CPU公开投影核验实际主格相位。
    // near=.05使旧f32差分约为.048828，斜离中心NDC≈.64处会漂移约5px，不能被±1px容差掩盖。
    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Top;
    let previous_extent = app.world().resource::<SceneEditorViewportState>().extent;
    app.world_mut().resource_mut::<SceneEditorViewportState>().extent = 20_000.0;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(0.0, 20_000.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
    *app.world_mut().get_mut::<Projection>(camera).unwrap() = Projection::Perspective(PerspectiveProjection {
        fov: 50.0_f32.to_radians(), near: 0.05, far: 100_000.0,
        ..default()
    });
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = false;
    for _ in 0..8 { app.update(); }
    let high_off = capture(app, target.clone(), "editor-grid-infinite-high-perspective-off.png");
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
    for _ in 0..8 { app.update(); }
    let high_on = capture(app, target.clone(), "editor-grid-infinite-high-perspective-on.png");
    let projected = app.world().get::<Camera>(camera).unwrap().world_to_viewport(
        app.world().get::<GlobalTransform>(camera).unwrap(), Vec3::new(8000.0, 0.0, 350.0),
    ).unwrap(); // 1000米主格约隔26px；远离坐标轴及横向主格，最强峰必须来自x=8000。
    assert!((0.5..=0.7).contains(&(projected.x / 320.0 - 1.0)), "高空相位采样必须斜离中心：{projected:?}");
    let (x, y) = (projected.x.floor() as i32, projected.y.floor() as i32);
    let profile = (-6..=6).map(|offset| {
        let pixel_x = x + offset;
        let index = (y as usize * 640 + pixel_x as usize) * 4;
        let delta = (0..3).map(|channel| high_on[index + channel].abs_diff(high_off[index + channel])).max().unwrap();
        (pixel_x as f32 + 0.5, delta)
    }).collect::<Vec<_>>();
    let peak = profile.iter().map(|(_, delta)| *delta).max().unwrap();
    let phase_error = profile.iter().filter(|(_, delta)| *delta == peak)
        .map(|(pixel_x, _)| (pixel_x - projected.x).abs()).fold(f32::INFINITY, f32::min);
    assert!(peak >= 4, "高空主格必须确有可辨峰值，不能用空白或只有次格的图通过：{profile:?}");
    assert!(phase_error <= 1.0,
        "高空主格线峰须落在CPU投影±1px内：投影{projected:?}，误差{phase_error}，剖面{profile:?}");
    app.world_mut().resource_mut::<SceneEditorViewportState>().extent = previous_extent;

    // 平移不重置世界相位，非整数观察中心也不能把网格跟随镜头滑动。
    let center = Vec3::new(173.3, 0.0, -239.7);
    app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Top;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = center;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_translation(center + Vec3::Y * 18.0).looking_at(center, Vec3::NEG_Z);
    *app.world_mut().get_mut::<Projection>(camera).unwrap() = Projection::Orthographic(OrthographicProjection {
        scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 12.0 },
        ..OrthographicProjection::default_3d()
    });
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = false;
    for _ in 0..8 { app.update(); }
    let panned_off = capture(app, target.clone(), "editor-grid-infinite-panned-off.png");
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
    for _ in 0..8 { app.update(); }
    let panned_on = capture(app, target.clone(), "editor-grid-infinite-panned-on.png");
    assert!(projected_delta(app, camera, Vec3::new(174.0, 0.0, -238.65), &panned_on, &panned_off) >= 4);
    assert_eq!(projected_delta(app, camera, Vec3::new(174.5, 0.0, -238.65), &panned_on, &panned_off), 0,
        "平移后的半格处应保持空白，不能将相机中心当作世界原点");

    let origin = Vec3::new(0.3, 4.25, 0.7);
    app.world_mut().resource_scope(|world, session: Mut<EditorAuthoringSession>| {
        world.resource_mut::<PlacementSelectionSet>().publish(vec![
            crate::viewport::multi_select::SelectedMember { id: "grid-a".into(), index: 0, position: origin - Vec3::X },
            crate::viewport::multi_select::SelectedMember { id: "grid-b".into(), index: 1, position: origin + Vec3::X },
        ], &session);
    }); // 使用正式多选发布入口；相对吸附原点与选集高度不能移动无限参考网格。
    {
        let mut settings = app.world_mut().resource_mut::<EditorSettings>();
        settings.editor_snapping.enabled = true;
        settings.editor_snapping.mode = feathrise_editor_core::EditorSnapMode::Relative;
        settings.editor_snapping.show_grid = false;
    }
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_translation(origin + Vec3::Y * 18.0).looking_at(origin, Vec3::NEG_Z);
    *app.world_mut().get_mut::<Projection>(camera).unwrap() = Projection::Perspective(PerspectiveProjection::default());
    for _ in 0..8 { app.update(); }
    let relative_off = capture(app, target.clone(), "editor-grid-infinite-relative-off.png");
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
    for _ in 0..8 { app.update(); }
    let relative_on = capture(app, target, "editor-grid-infinite-relative-on.png");
    assert!(projected_delta(app, camera, Vec3::new(1.0, 0.0, 2.05), &relative_on, &relative_off) >= 4,
        "相对吸附仍须显示y=0处的世界整数格");
    assert_eq!(projected_delta(app, camera, Vec3::new(1.5, 0.0, 2.35), &relative_on, &relative_off), 0,
        "资产的非整数原点与高度不能将半格处移动成参考格线");
    assert_wire_axis(app, camera, &relative_on, Vec3::new(2.35, 0.0, 0.0), 0, "世界X红");
    assert_wire_axis(app, camera, &relative_on, Vec3::new(0.0, 0.0, 2.35), 2, "世界Z蓝");
    app.world_mut().resource_mut::<PlacementSelectionSet>().clear();
    if let Some(previous) = previous_shading {
        app.insert_resource(previous);
    } else {
        let _ = app.world_mut().remove_resource::<ShadingState>();
    }
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping = previous_snap;
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = previous_open;
        editor.view = previous_view;
        editor.cursor_world = previous_cursor;
    }
    app.world_mut().resource_mut::<MeshSnapCursor>().preview = previous_preview;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = previous_center;
    *app.world_mut().get_mut::<Visibility>(resident).unwrap() = previous_visibility;
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = previous_transform;
    *app.world_mut().get_mut::<Projection>(camera).unwrap() = previous_projection;
    app.world_mut().entity_mut(camera).insert(previous_msaa);
    app.world_mut().get_mut::<Camera>(camera).unwrap().clear_color = previous_clear;
}

#[test]
#[ignore = "显式执行网格GPU回归；不启动系统窗口或改动场景和设置"]
fn gpu_grid_is_visible_and_close_restores_the_same_resident_scene() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::WgpuSettings {
                    features: bevy::render::render_resource::WgpuFeatures::POLYGON_MODE_LINE
                        | bevy::render::render_resource::WgpuFeatures::IMMEDIATES,
                    ..default()
                }.into(),
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    );
    app.init_gizmo_group::<EditorGridOutlineGizmos>()
        .init_gizmo_group::<EditorGridGizmos>() // 这两组仅保留落点十字；参考网格来自正式GPU通道。
        .add_plugins(super::grid_render::GridRenderPlugin)
        .init_resource::<EditorSettings>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<PlacementMoveState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<MeshSnapCursor>()
        .init_resource::<crate::authoring::placement_batch::PlacementBatchState>()
        .init_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
        .init_resource::<crate::ui::building_presets::BuildingPlacementControls>()
        .insert_resource(bevy::render::error_handler::RenderErrorHandler(
            |error, _, _| panic!("无限参考网格GPU验证失败：{error:?}"),
        ))
        .add_systems(Startup, configure_grid)
        .add_systems(Update, draw_grid);
    crate::viewport::shading::install_grid_gpu_validation(&mut app);
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map = draft.scene_id.clone();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(map.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    app.finish();
    app.cleanup();
    let mut image = Image::new_target_texture(640, 480, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((
            crate::components::SceneEditorCamera,
            Camera3d::default(),
            RenderTarget::from(target.clone()),
            Transform::from_xyz(13.0, 11.0, 16.0).looking_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Y),
            RenderLayers::from_layers(&[0, 31]),
        ))
        .id();
    let mut game_image = Image::new_target_texture(640, 480, TextureFormat::Bgra8UnormSrgb, None);
    game_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let game_target = app.world_mut().resource_mut::<Assets<Image>>().add(game_image);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera { order: 1, ..default() },
        RenderTarget::from(game_target.clone()),
        Transform::from_xyz(13.0, 11.0, 16.0).looking_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Y),
        RenderLayers::from_layers(&[0, 31]),
    )); // 相同场景和编辑层，但没有作者相机身份；全屏通道不能污染玩法视图。
    app.insert_resource(SceneEditorViewportState {
        _image: target.clone(),
        camera,
        center: Vec3::new(5.0, 0.0, 5.0),
        scene_center: Vec3::new(5.0, 0.0, 5.0),
        scene_half_extents: Vec3::new(5.0, 0.0, 5.0),
        extent: 20.0,
        yaw: 0.5,
        pitch: 0.5,
        framed_scene_id: Some(map),
        render_size: UVec2::new(640, 480),
    });
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(10.0, 10.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.13, 0.15),
            unlit: true,
            ..default()
        });
    let resident = app
        .world_mut()
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(5.0, 0.0, 5.0),
        ))
        .id();
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let game_baseline = capture(&mut app, game_target.clone(), "editor-grid-game-baseline.png");
    let enabled = capture(&mut app, target.clone(), "editor-grid-on.png");
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_snapping
        .show_grid = false;
    for _ in 0..4 {
        app.update();
    }
    let disabled = capture(&mut app, target.clone(), "editor-grid-off.png");
    let changed = changed_pixels(&enabled, &disabled);
    assert!(
        changed > 100,
        "正式编辑层必须确实渲染参考网格，变化像素{changed}"
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_snapping
        .show_grid = true;
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    for _ in 0..4 {
        app.update();
    }
    let closed = capture(&mut app, target.clone(), "editor-grid-closed.png");
    assert!(
        closed == disabled,
        "关闭F12必须撤下网格并继续显示同一驻留场景"
    );
    assert!(app.world().get::<Mesh3d>(resident).is_some());
    assert_eq!(capture(&mut app, game_target.clone(), "editor-grid-game-after-close.png"), game_baseline,
        "网格开关与退出编辑器不能修改相同场景的非作者相机像素");

    verify_wire_reference(&mut app, target.clone(), camera, resident);
    assert_eq!(capture(&mut app, game_target, "editor-grid-game-after-modes.png"), game_baseline,
        "四种着色、无限远景和相对吸附切换不能污染玩法相机");
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    let material = app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(resident)
        .unwrap()
        .0
        .clone();
    for (name, dark, raised, slope, below, orthographic) in [
        ("sand-flat", false, false, false, false, false),
        ("sand-raised", false, true, false, false, false),
        ("dark-raised", true, true, false, false, false),
        ("sand-slope", false, true, true, false, false),
        ("sand-back", false, true, false, true, false),
        ("sand-top", false, true, false, false, true),
    ] {
        {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            let mut ground_material = materials.get_mut(&material).unwrap();
            ground_material.base_color = if dark {
                Color::srgb(0.12, 0.13, 0.15)
            } else {
                Color::srgb(0.86, 0.80, 0.68)
            };
            ground_material.cull_mode = None; // 同一真实地板提供正面/背面对照，不能用背面被剔除的空画面通过。
            ground_material.double_sided = true;
        }
        *app.world_mut().get_mut::<Transform>(resident).unwrap() = Transform {
            translation: Vec3::new(5.0, if raised { 2.0 } else { 0.0 }, 5.0),
            rotation: if slope {
                Quat::from_rotation_z(0.25)
            } else {
                Quat::IDENTITY
            },
            ..default()
        };
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = if orthographic {
            Transform::from_xyz(5.0, 18.0, 5.0).looking_at(Vec3::new(5.0, 2.0, 5.0), Vec3::NEG_Z)
        } else {
            Transform::from_xyz(13.0, if below { -11.0 } else { 11.0 }, 16.0)
                .looking_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Y)
        };
        *app.world_mut().get_mut::<Projection>(camera).unwrap() = if orthographic {
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                    viewport_height: 16.0,
                },
                ..OrthographicProjection::default_3d()
            })
        } else {
            Projection::Perspective(PerspectiveProjection::default())
        };
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .editor_snapping
            .show_grid = false;
        for _ in 0..40 {
            app.update();
            std::thread::sleep(Duration::from_millis(16)); // 首次正交/双面特化等待真实GPU管线就绪，不能把空白基准当作地面。
        }
        let off = capture(
            &mut app,
            target.clone(),
            &format!("editor-grid-{name}-off.png"),
        );
        let mask = ground_mask(&app, camera, resident);
        let ground_pixels = mask.iter().filter(|inside| **inside).count();
        assert!(
            ground_pixels > 5000,
            "{name}必须有真实地板内部，而非仅验证平面外的线"
        );
        let rendered_ground = off
            .chunks_exact(4)
            .zip(&mask)
            .filter(|(pixel, inside)| **inside && pixel[..3] != off[..3])
            .count();
        assert!(
            rendered_ground > ground_pixels * 9 / 10,
            "{name}基准必须已经绘出真实地面，不能用未就绪管线的空白帧统计网格"
        );
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .editor_snapping
            .show_grid = true;
        for _ in 0..4 {
            app.update();
        }
        let on = capture(
            &mut app,
            target.clone(),
            &format!("editor-grid-{name}-on.png"),
        );
        let visible = visible_ground_pixels(&on, &off, &mask);
        assert!(
            visible > ground_pixels / 150,
            "{name}地表上仍须绘出统一细线网格：内部{visible}/{ground_pixels}"
        );
        println!("网格可见性 {name}：细线内部像素{visible}/{ground_pixels}");
        app.world_mut().resource_mut::<SceneEditorState>().open = false;
        for _ in 0..4 {
            app.update();
        }
        assert!(
            capture(
                &mut app,
                target.clone(),
                &format!("editor-grid-{name}-closed.png")
            ) == off,
            "{name}关闭F12仍须恢复同一地面"
        );
        assert!(app.world().get::<Mesh3d>(resident).is_some());
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
    }

    // 【纹理与阴影验收】复用同一驻留地面、真实PBR与相机；只新增测试资产，不读取/写回场景或偏好。
    let texture = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(textured_ground());
    {
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let mut ground_material = materials.get_mut(&material).unwrap();
        ground_material.base_color = Color::WHITE;
        ground_material.base_color_texture = Some(texture);
        ground_material.unlit = false;
        ground_material.perceptual_roughness = 1.0;
        ground_material.reflectance = 0.0;
    }
    *app.world_mut().get_mut::<Transform>(resident).unwrap() = Transform::from_xyz(5.0, 0.0, 5.0);
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_xyz(13.0, 11.0, 16.0).looking_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Y);
    app.world_mut().entity_mut(camera).insert((
        Projection::Perspective(PerspectiveProjection::default()),
        Msaa::Sample4,
        AmbientLight {
            brightness: 180.0,
            ..default()
        },
    )); // 显式4×MSAA验证细线不会只在无抗锯齿的理想场景中清楚。
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(10.0, 12.0, -2.0).looking_at(Vec3::ZERO, Vec3::Y),
    )); // 阴影沿-X/+Z投向x=4普通线，与相机遮挡方向分离，让阴影中确有可见地板采样。
    let blocker_mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.5, 3.0, 1.4));
    let blocker_material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.16, 0.12),
            perceptual_roughness: 1.0,
            ..default()
        });
    let blocker = app
        .world_mut()
        .spawn((
            Mesh3d(blocker_mesh),
            MeshMaterial3d(blocker_material),
            Transform::from_xyz(5.8, 1.5, 5.0),
            Visibility::Hidden,
        ))
        .id();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_snapping
        .show_grid = false;
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let sunny = capture(&mut app, target.clone(), "editor-grid-textured-sun-off.png");
    *app.world_mut().get_mut::<Visibility>(blocker).unwrap() = Visibility::Inherited;
    for _ in 0..40 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let textured_off = capture(
        &mut app,
        target.clone(),
        "editor-grid-textured-shadow-msaa4-off.png",
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_snapping
        .show_grid = true;
    for _ in 0..4 {
        app.update();
    }
    let textured_on = capture(
        &mut app,
        target.clone(),
        "editor-grid-textured-shadow-msaa4-on.png",
    );
    measure_ordinary_grid_line(&app, camera, resident, &sunny, &textured_off, &textured_on);
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    for _ in 0..4 {
        app.update();
    }
    assert!(
        capture(
            &mut app,
            target,
            "editor-grid-textured-shadow-msaa4-closed.png"
        ) == textured_off,
        "有纹理/阴影和遮挡物时关闭F12也必须仅撤下网格，不能改变场景像素"
    );
    assert!(app.world().get::<Mesh3d>(resident).is_some());
    assert!(app.world().get::<Mesh3d>(blocker).is_some());
}
