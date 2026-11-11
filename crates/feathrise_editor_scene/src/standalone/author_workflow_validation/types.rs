//! 专业作者验收的阶段和有界只读证据。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle,
    Terrain,
    RiverBuilding,
    AnimationAssets,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct AuthorWorkflowProof {
    pub complete: bool,
    pub error: Option<String>,
    pub terrain: Option<serde_json::Value>,
    pub river_building: Option<serde_json::Value>,
    pub animation_assets: Option<serde_json::Value>,
}
