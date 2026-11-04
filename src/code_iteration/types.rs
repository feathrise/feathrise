//! 代码迭代的作者可见阶段；编译、候选版本和实际游戏采用分开呈现。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CodeIterationPhase {
    #[default]
    Watching,
    Waiting,
    Building,
    Failed,
    Ready,
    Applying,
    Applied,
    Stopping,
}
