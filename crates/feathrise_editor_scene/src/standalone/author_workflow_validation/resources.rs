//! 自有工程验收的单份串行状态；不持有作者草稿或复制完整历史。

use super::types::{AuthorWorkflowProof, Phase};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(super) struct Workflow {
    pub phase: Phase,
    pub proof: AuthorWorkflowProof,
}
