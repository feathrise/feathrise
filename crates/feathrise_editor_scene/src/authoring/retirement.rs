//! 大型作者版本在唯一有界后台批次析构；不属于任何游戏领域。

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use std::collections::LinkedList;

#[derive(Resource, Default)]
pub struct AuthoringRetirement {
    pending: LinkedList<Box<dyn Send + Sync>>,
    task: Option<Task<()>>,
}
impl AuthoringRetirement {
    pub fn push<T: Send + Sync + 'static>(&mut self, value: T) {
        self.pending.push_back(Box::new(value));
    }
}
pub(crate) fn poll(mut retire: ResMut<AuthoringRetirement>) {
    if retire
        .task
        .as_mut()
        .is_some_and(|task| check_ready(task).is_some())
    {
        retire.task = None;
    }
    if retire.task.is_none() && !retire.pending.is_empty() {
        let pending = std::mem::take(&mut retire.pending);
        retire.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(pending);
        }));
    }
}
