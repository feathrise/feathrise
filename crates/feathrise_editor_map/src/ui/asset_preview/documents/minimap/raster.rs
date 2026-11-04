//! 纯CPU代码轮廓栅格化；由有界计算池执行，不访问World、Assets或源文件。

use super::super::{constants::*, types::*};

pub(super) fn rasterize(key: MinimapKey, marks: Vec<MinimapMark>, color: [u8; 4]) -> MinimapResult {
    let mut pixels = vec![0; (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize];
    for mark in marks {
        for y in u32::from(mark.min[1])..u32::from(mark.max[1]).min(MINIMAP_HEIGHT) {
            for x in u32::from(mark.min[0])..u32::from(mark.max[0]).min(MINIMAP_WIDTH) {
                let index = ((y * MINIMAP_WIDTH + x) * 4) as usize;
                pixels[index..index + 4].copy_from_slice(&color);
            }
        }
    }
    MinimapResult { key, pixels }
}
