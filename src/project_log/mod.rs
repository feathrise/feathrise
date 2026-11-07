//! 工程构建/游戏实际stdout、stderr的只读终端；输出采集与浮窗生命周期彼此独立。

mod components;
mod display;
mod fonts;
mod resources;
mod scrollbar_feedback;
mod reading;
mod scene;
mod stream;
mod systems;
mod types;
mod wrapping;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod reading_tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
pub(crate) mod gpu_tests;

pub(crate) use resources::{ProjectLogChannel, ProjectLogState};
pub(crate) use stream::{LogDecoder, LogReader, ProjectLogSink};
pub(crate) use types::{LogSource, LogTaskKind};

/// 跨模块回归只拿发送端，邮箱及内部事件类型仍封装在工程日志模块内。
#[cfg(test)]
pub(crate) fn fixture_channel(project: &str) -> ProjectLogChannel { ProjectLogChannel::new(project).0 }

/// 跨模块回放真实工具字节时保留正式有界收件端；不伪造日志或绕过生产发送预算。
#[cfg(test)]
pub(crate) fn fixture_log_probe(project: &str) -> (ProjectLogSink, FixtureLogProbe) {
    let (channel, inbox) = ProjectLogChannel::new(project);
    let sink = channel.begin();
    (sink, FixtureLogProbe { channel, inbox })
}

#[cfg(test)]
pub(crate) struct FixtureLogProbe {
    channel: ProjectLogChannel,
    inbox: resources::ProjectLogInbox,
}

#[cfg(test)]
pub(crate) struct FixtureLogEvent {
    pub kind: &'static str,
    pub text: String,
    pub source: LogSource,
    pub sequence: u64,
    pub task: u64,
    pub target: LogTaskKind,
}

#[cfg(test)]
impl FixtureLogProbe {
    pub(crate) fn drain(&self) -> Vec<FixtureLogEvent> {
        self.inbox.0.try_iter().map(|event| FixtureLogEvent {
            kind: match event.kind { types::LogEventKind::Line => "line", types::LogEventKind::Tail => "tail" },
            text: event.text, source: event.source, sequence: event.sequence,
            task: event.task.number, target: event.task.kind,
        }).collect()
    }

    pub(crate) fn dropped(&self) -> u64 { self.channel.dropped.load(std::sync::atomic::Ordering::Acquire) }
}

use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::localization::{EditorLocale, EditorSystemLocale};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProjectLogInput;

pub(crate) struct ProjectLogPlugin {
    project_id: String,
    project_name: String,
}

impl ProjectLogPlugin {
    pub(crate) fn new(project: &crate::project::Project) -> Self {
        Self { project_id: project.id.clone(), project_name: project.name.clone() }
    }
}

impl Plugin for ProjectLogPlugin {
    fn build(&self, app: &mut App) {
        let (channel, inbox) = ProjectLogChannel::new(&self.project_id);
        let world = app.world();
        channel.set_locale(log_locale(world.get_resource::<EditorSettings>(), world.get_resource::<EditorSystemLocale>()));
        let state = ProjectLogState::new(&channel, &self.project_name);
        app.insert_resource(channel).insert_resource(state).insert_non_send(inbox)
            .add_systems(PreUpdate, sync_log_locale)
            .add_systems(Update, scene::mount.before(systems::present))
            .add_systems(Update, reading::readonly.before(bevy::text::apply_text_edits).before(systems::collect))
            .add_systems(PostUpdate, reading::readonly.before(bevy::text::apply_text_edits))
            .add_systems(Update, reading::remember_selection.after(bevy::text::apply_text_edits).before(systems::collect))
            .add_systems(Update, systems::collect.before(systems::control))
            .add_systems(Update, wrapping::sync.after(systems::collect).before(systems::control))
            .add_systems(Update, systems::control.in_set(ProjectLogInput)
                .after(crate::native_project_close::ProjectCloseInput)
                .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(feathrise_editor_core::MapEditorSystemSet::Input))
            .add_systems(Update, reading::copy.after(systems::control).before(systems::present))
            .add_systems(Update, systems::present.after(systems::control))
            .add_systems(Update, scrollbar_feedback::sync.after(systems::control))
            .add_systems(PostUpdate, fonts::fallback
                .after(bevy::text::load_font_assets_into_font_collection)
                .before(bevy::text::detect_text_needs_rerender)
                .before(bevy::text::apply_text_edits))
            .add_systems(PostUpdate, systems::measure_width.after(bevy::ui::UiSystems::PostLayout));
    }
}

fn sync_log_locale(
    settings: Option<Res<EditorSettings>>,
    system: Option<Res<EditorSystemLocale>>,
    channel: Res<ProjectLogChannel>,
) {
    if channel.is_added() || settings.as_ref().is_some_and(|settings| settings.is_changed())
        || system.as_ref().is_some_and(|system| system.is_changed())
    {
        channel.set_locale(log_locale(settings.as_deref(), system.as_deref()));
    }
}

fn log_locale(settings: Option<&EditorSettings>, system: Option<&EditorSystemLocale>) -> EditorLocale {
    let system = system.copied().unwrap_or_default().0;
    settings.map_or_else(EditorLocale::default, |settings| EditorLocale::resolve(settings.ui_locale, system))
}
