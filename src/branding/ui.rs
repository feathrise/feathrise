//! 将羽毛标识放入现有工程欢迎页和原作者顶栏，保留原控件与交互顺序。
use super::resources::BrandImages;
use bevy::prelude::*;
use feathrise_editor_map::workbench::WorkbenchHeader;

pub(super) fn mount_header(
    mut commands: Commands,
    brand: Res<BrandImages>,
    headers: Query<(Entity, &Children), Added<WorkbenchHeader>>,
    titles: Query<&Text>,
) {
    for (header, children) in &headers {
        let Some(&title) = children.first() else {
            continue;
        };
        if titles.get(title).is_err() {
            continue;
        }
        // 图形与文字各有布局节点，避免向Text节点添加图片后影响文本测量。
        let brand_title = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    flex_shrink: 0.,
                    margin: UiRect::right(px(4)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|parent| {
                parent.spawn((
                    ImageNode::new(brand.mark.clone()),
                    Node {
                        width: px(20),
                        height: px(20),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                // 一体顶栏用羽毛识别应用，工程菜单显示当前工程，保留中间连续的窗口拖动空间。
            })
            .id();
        // 只替换无业务标记的原静态标题；工程菜单仍插在第1位，其他控件及顺序不变。
        commands.entity(header).insert_children(0, &[brand_title]);
        commands.entity(title).despawn();
    }
}
