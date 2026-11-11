//! GLB浏览器最多两代的single-flight退役背压与固定预算推进。

use std::collections::VecDeque; // 前代完整清空后才推进后代。

use super::{BrowserRetiredGeneration, BrowserRetirementBudget, BrowserRetirementStep}; // 工作代只通过统一三维预算推进。

/// 同一时刻最多保留一代正常换代和一代紧急关闭数据。
const MAX_BROWSER_RETIREMENT_GENERATIONS: usize = 2;
/// 正常发布/热更只占第一个槽，永远为F12关闭保留一个确定槽位。
const MAX_NORMAL_RETIREMENT_GENERATIONS: usize = 1;

/// 最多两代的single-flight退役队列；正常换代在一代未清空时必须背压。
#[derive(Default)]
pub(in crate::ui::gltf_browser) struct BrowserRetirement {
    generations: VecDeque<BrowserRetiredGeneration>, // 前代清空后才处理后代，内存峰值确定有界。
}

impl BrowserRetirement {
    pub(in crate::ui::gltf_browser) fn can_enqueue_normal(&self) -> bool {
        self.generations.len() < MAX_NORMAL_RETIREMENT_GENERATIONS // 第二槽始终留给关闭。
    }

    /// 调用方紧邻地预留正常槽后无条件转移；release构建绝不能省略所有权移动。
    pub(in crate::ui::gltf_browser) fn enqueue_normal_reserved(
        &mut self,
        generation: BrowserRetiredGeneration,
    ) {
        if generation.is_empty() {
            return; // 无深数据无需占用代次槽位。
        }
        debug_assert!(
            self.can_enqueue_normal(),
            "正常退役槽必须在移动活动字段前预留"
        );
        self.generations.push_back(generation); // 副作用不放在debug_assert内部。
        // 即使未来调用方破坏预检，release也宁可暂时超代而不能同步Drop深数据。
    }

    pub(in crate::ui::gltf_browser) fn enqueue_close(
        &mut self,
        generation: BrowserRetiredGeneration,
    ) {
        if generation.is_empty() {
            return; // 空会话关闭无需建立代次。
        }
        debug_assert!(
            self.generations.len() < MAX_BROWSER_RETIREMENT_GENERATIONS,
            "浏览器只允许一代正常退役加一代关闭退役"
        );
        self.generations.push_back(generation); // 第二槽由会话开启背压保证预留，release仍无条件执行。
    }

    pub(in crate::ui::gltf_browser) fn advance_frame(&mut self) {
        let mut budget = BrowserRetirementBudget::default(); // 每个Update重新建立固定预算。
        loop {
            let Some(generation) = self.generations.front_mut() else {
                break; // 稳定空队列为O(1)短路。
            };
            match generation.advance(&mut budget) {
                BrowserRetirementStep::Complete => {
                    self.generations.pop_front(); // 仅空工作队列壳在本帧最终释放。
                }
                BrowserRetirementStep::Progressed => continue, // 尚有预算时继续同代下一元素。
                BrowserRetirementStep::Blocked => break,       // 任一预算耗尽后立即让出本帧。
            }
        }
    }

    pub(in crate::ui::gltf_browser) fn is_empty(&self) -> bool {
        self.generations.is_empty() // 开启新会话前可据此实施背压。
    }

    #[cfg(test)]
    pub(in crate::ui::gltf_browser) fn generation_count(&self) -> usize {
        self.generations.len() // 回归测试确认连续换代不会无界排队。
    }
}
