//! 独立编辑器品牌入口；只装饰宿主UI和窗口，不进入共享作者库或工程实例。
mod resources;
#[cfg(feathrise_custom_branding)]
mod ui;
#[cfg(all(target_os = "windows", feathrise_custom_branding))]
mod windows;

pub(crate) use resources::BrandImages;

#[cfg(feathrise_custom_branding)]
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
};
use bevy::prelude::*;

pub(crate) struct EditorBrandingPlugin;

impl Plugin for EditorBrandingPlugin {
    fn build(&self, app: &mut App) {
        // 未显式提供品牌材料时不安装图片资源，欢迎页与关于保留文字，作者顶栏保留原标题。
        #[cfg(feathrise_custom_branding)]
        {
            // 图片在App初始化时解码一次并随exe携带，不受工程资产根和同名文件影响。
            let image = Image::from_buffer(
                include_bytes!(concat!(env!("OUT_DIR"), "/feathrise-mark.png")),
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::linear(),
                RenderAssetUsages::default(),
            )
            .expect("内嵌品牌标识必须是有效PNG");
            let mark = app.world_mut().resource_mut::<Assets<Image>>().add(image);
            app.insert_resource(BrandImages { mark })
                .add_systems(Update, ui::mount_header.before(crate::window_chrome::ChromeMount));
            #[cfg(target_os = "windows")]
            app.add_systems(Update, windows::apply_window_icons);
        }
        app.add_plugins(crate::window_chrome::WindowChromePlugin);
    }
}
