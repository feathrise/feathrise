//! 有限双层表面工作流的阶段；完成只由原事务与历史回执推进。

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    #[default]
    Idle,
    CreatePreset,
    WaitPreset,
    SavePreset,
    WaitPresetSave,
    ClosePreset,
    WaitPresetClose,
    Focus,
    WaitRoot,
    AddLayer,
    WaitLayer,
    Hover,
    Plant,
    PlantRelease,
    Erase,
    EraseRelease,
    UndoErase,
    WaitUndoErase,
    UndoPlant,
    WaitUndoPlant,
    RedoPlant,
    WaitRedoPlant,
    Lifecycle,
    Settle,
    Capture,
    Complete,
    Failed,
}

pub(super) const MODEL: &str = "model-surface-fixture";
pub(super) const SOURCE: &str = "scenes/M1-SAMPLE/scene.glb";
pub(super) const VEGETATION_SOURCE: &str = "scene/vegetation-probe/vegetation-probe.glb";
