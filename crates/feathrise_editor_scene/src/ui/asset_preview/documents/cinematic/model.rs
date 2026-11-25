//! 演出轨道的可见索引、表单与编辑。解析、全量排序与序列化仅在后台执行。
use bevy::math::{EulerRot, Quat};
use feathrise_cinematic::{CameraKey, Cinematic, Clip, DialogueCue, MotionTrack, Pose, PoseKey};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Lane {
    #[default]
    Camera,
    Actor,
    Object,
    Dialogue,
}
pub(super) const LANES: [Lane; 4] = [Lane::Camera, Lane::Actor, Lane::Object, Lane::Dialogue];
impl Lane {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        ["镜头", "角色", "物件", "对白"][self.index()]
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Item {
    Camera(usize),
    Pose(Lane, usize, usize),
    Clip(Lane, usize, usize),
    Dialogue(usize),
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Kind {
    #[default]
    Pose,
    Clip,
    Subtitle,
    Yarn,
}
#[derive(Clone)]
pub(super) struct Marker {
    pub at: f32,
    pub end: f32,
    pub item: Item,
}
pub(super) struct Timeline {
    pub film: Cinematic,
    pub bytes: Arc<Vec<u8>>,
    pub lanes: [Vec<Marker>; 4],
}
pub(super) fn decode(source: &str) -> Result<Cinematic, String> {
    Cinematic::from_slice(source.as_bytes())
}
impl Timeline {
    pub fn new(film: Cinematic, bytes: Arc<Vec<u8>>) -> Self {
        let mut lanes: [Vec<Marker>; 4] = Default::default();
        for (i, k) in film.camera.iter().enumerate() {
            lanes[0].push(Marker {
                at: k.time,
                end: k.time,
                item: Item::Camera(i),
            });
        }
        for (lane, tracks) in [(Lane::Actor, &film.actors), (Lane::Object, &film.objects)] {
            for (i, track) in tracks.iter().enumerate() {
                for (j, k) in track.keys.iter().enumerate() {
                    lanes[lane.index()].push(Marker {
                        at: k.time,
                        end: k.time,
                        item: Item::Pose(lane, i, j),
                    });
                }
                for (j, k) in track.clips.iter().enumerate() {
                    lanes[lane.index()].push(Marker {
                        at: k.time,
                        end: k.time + k.duration,
                        item: Item::Clip(lane, i, j),
                    });
                }
            }
        }
        for (i, k) in film.dialogue.iter().enumerate() {
            lanes[3].push(Marker {
                at: k.time,
                end: k.time + k.duration,
                item: Item::Dialogue(i),
            });
        }
        for lane in &mut lanes {
            lane.sort_by(|a, b| a.at.total_cmp(&b.at));
        }
        Self { film, lanes, bytes }
    }
    /// 标签由当前固定槽读取，成千上万的关键帧不重复保存同一目标名或字幕文本。
    pub fn label(&self, item: Item) -> std::borrow::Cow<'_, str> {
        match item {
            Item::Camera(i) => format!("机位 {}", i + 1).into(),
            Item::Pose(l, i, _) => tracks(&self.film, l)[i].target.as_str().into(),
            Item::Clip(l, i, j) => tracks(&self.film, l)[i].clips[j].name.as_str().into(),
            Item::Dialogue(i) => {
                let d = &self.film.dialogue[i];
                d.yarn
                    .as_deref()
                    .or(d.text.as_deref())
                    .unwrap_or_default()
                    .into()
            }
        }
    }
}
/// 原生文字输入只携带一个当前标记的属性。任意未显示的政策、条件、收尾动作保留在原数据模型中。
#[derive(Clone)]
pub(super) struct Form {
    pub at: String,
    pub length: String,
    pub target: String,
    pub instance: String,
    pub name: String,
    pub position: String,
    pub rotation: String,
    pub scale: String,
    pub fov: String,
    pub smooth: bool,
    pub wait: bool,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            at: "0".into(),
            length: "1".into(),
            target: "player".into(),
            instance: String::new(),
            name: String::new(),
            position: "0, 0, 0".into(),
            rotation: "0, 0, 0".into(),
            scale: "1, 1, 1".into(),
            fov: "60".into(),
            smooth: true,
            wait: false,
        }
    }
}
pub(super) fn number(s: &str) -> Result<f32, String> {
    s.trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("请输入有限数值：{s}"))
}
fn vector(s: &str) -> Result<[f32; 3], String> {
    let mut values = s
        .split([',', '，', ' '])
        .filter(|s| !s.is_empty())
        .map(number);
    let (Some(x), Some(y), Some(z), None) =
        (values.next(), values.next(), values.next(), values.next())
    else {
        return Err("需要 X, Y, Z 三个数值".into());
    };
    Ok([x?, y?, z?])
}
fn show(v: [f32; 3]) -> String {
    format!("{}, {}, {}", v[0], v[1], v[2])
}
impl Form {
    pub fn with_pose(&mut self, pose: &Pose) {
        let (x, y, z) = Quat::from_array(pose.rotation).to_euler(EulerRot::XYZ);
        self.rotation = show([x.to_degrees(), y.to_degrees(), z.to_degrees()]);
        self.position = show(pose.translation);
        self.scale = show(pose.scale);
    }
    fn pose(&self) -> Result<Pose, String> {
        let [x, y, z] = vector(&self.rotation)?.map(f32::to_radians);
        Ok(Pose {
            translation: vector(&self.position)?,
            rotation: Quat::from_euler(EulerRot::XYZ, x, y, z).to_array(),
            scale: vector(&self.scale)?,
        })
    }
    pub fn from_item(f: &Cinematic, item: Item) -> (Self, Kind) {
        let mut form = Self::default();
        let kind = match item {
            Item::Camera(i) => {
                let k = &f.camera[i];
                form.at = k.time.to_string();
                form.with_pose(&k.pose);
                form.fov = k.fov_y_degrees.to_string();
                form.smooth = k.smooth;
                Kind::Pose
            }
            Item::Pose(l, i, j) | Item::Clip(l, i, j) => {
                let tr = &tracks(f, l)[i];
                form.target = tr.target.clone();
                form.instance = tr.instance.clone().unwrap_or_default();
                if matches!(item, Item::Clip(..)) {
                    let c = &tr.clips[j];
                    form.at = c.time.to_string();
                    form.length = c.duration.to_string();
                    form.name = c.name.clone();
                    Kind::Clip
                } else {
                    let k = &tr.keys[j];
                    form.with_pose(&k.pose);
                    form.at = k.time.to_string();
                    form.smooth = k.smooth;
                    Kind::Pose
                }
            }
            Item::Dialogue(i) => {
                let d = &f.dialogue[i];
                form.at = d.time.to_string();
                form.length = d.duration.to_string();
                form.target = d.id.clone();
                form.wait = d.wait;
                if let Some(yarn) = &d.yarn {
                    form.name = yarn.clone();
                    Kind::Yarn
                } else {
                    form.name = d.text.clone().unwrap_or_default();
                    Kind::Subtitle
                }
            }
        };
        (form, kind)
    }
}
fn tracks(f: &Cinematic, l: Lane) -> &Vec<MotionTrack> {
    if l == Lane::Actor {
        &f.actors
    } else {
        &f.objects
    }
}
fn tracks_mut(f: &mut Cinematic, l: Lane) -> &mut Vec<MotionTrack> {
    if l == Lane::Actor {
        &mut f.actors
    } else {
        &mut f.objects
    }
}
#[derive(Clone)]
pub(super) enum Edit {
    Set {
        lane: Lane,
        kind: Kind,
        form: Form,
        replace: Option<Item>,
        camera: Option<CameraKey>,
    },
    Remove(Item),
    Move {
        item: Item,
        time: f32,
    },
    Settings {
        name: String,
        duration: f32,
        control_camera: bool,
        control_player: bool,
    },
}
fn remove(f: &mut Cinematic, item: Item) {
    match item {
        Item::Camera(i) => {
            f.camera.remove(i);
        }
        Item::Dialogue(i) => {
            f.dialogue.remove(i);
        }
        Item::Pose(l, i, j) | Item::Clip(l, i, j) => {
            let tracks = tracks_mut(f, l);
            let t = &mut tracks[i];
            if matches!(item, Item::Pose(..)) {
                t.keys.remove(j);
            } else {
                t.clips.remove(j);
            }
            if t.keys.is_empty() && t.clips.is_empty() {
                tracks.remove(i);
            }
        }
    }
}
impl Edit {
    pub fn apply(self, f: &mut Cinematic) -> Result<(), String> {
        match self {
            Self::Remove(item) => remove(f, item),
            Self::Move { item, time } => match item {
                Item::Camera(i) => {
                    f.camera[i].time = time;
                    f.camera.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
                Item::Dialogue(i) => {
                    f.dialogue[i].time = time;
                    f.dialogue.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
                Item::Pose(l, i, j) => {
                    let v = &mut tracks_mut(f, l)[i].keys;
                    v[j].time = time;
                    v.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
                Item::Clip(l, i, j) => {
                    let v = &mut tracks_mut(f, l)[i].clips;
                    v[j].time = time;
                    v.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
            },
            Self::Settings {
                name,
                duration,
                control_camera,
                control_player,
            } => {
                f.name = name;
                f.duration = duration;
                f.control_camera = control_camera;
                f.control_player = control_player;
            }
            Self::Set {
                lane,
                kind,
                form,
                replace,
                camera,
            } => {
                if let Some(item) = replace {
                    remove(f, item);
                }
                let time = number(&form.at)?;
                match lane {
                    Lane::Camera => {
                        let key = match camera {
                            Some(key) => key,
                            None => CameraKey {
                                time,
                                pose: form.pose()?,
                                smooth: form.smooth,
                                fov_y_degrees: number(&form.fov)?,
                            },
                        };
                        f.camera.push(key);
                        f.camera.sort_by(|a, b| a.time.total_cmp(&b.time));
                    }
                    Lane::Actor | Lane::Object => {
                        let tracks = tracks_mut(f, lane);
                        let target = form.target.trim();
                        let instance = (!form.instance.trim().is_empty())
                            .then(|| form.instance.trim().to_string());
                        let index = tracks
                            .iter()
                            .position(|t| t.target == target && t.instance == instance)
                            .unwrap_or_else(|| {
                                tracks.push(MotionTrack {
                                    target: target.into(),
                                    instance,
                                    keys: vec![],
                                    clips: vec![],
                                });
                                tracks.len() - 1
                            });
                        let tr = &mut tracks[index];
                        if kind == Kind::Clip {
                            tr.clips.push(Clip {
                                time,
                                duration: number(&form.length)?,
                                name: form.name.trim().into(),
                            });
                            tr.clips.sort_by(|a, b| a.time.total_cmp(&b.time));
                        } else {
                            tr.keys.push(PoseKey {
                                time,
                                pose: form.pose()?,
                                smooth: form.smooth,
                            });
                            tr.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                        }
                    }
                    Lane::Dialogue => {
                        let id = if form.target.trim().is_empty() {
                            format!("line_{}", bevy::asset::uuid::Uuid::new_v4().simple())
                        } else {
                            form.target.trim().into()
                        };
                        let yarn = kind == Kind::Yarn;
                        f.dialogue.push(DialogueCue {
                            id,
                            time,
                            duration: number(&form.length)?,
                            text: (!yarn).then(|| form.name.clone()),
                            yarn: yarn.then_some(form.name),
                            wait: form.wait,
                        });
                        f.dialogue.sort_by(|a, b| a.time.total_cmp(&b.time));
                    }
                }
            }
        }
        f.validate().map_err(|e| e.to_string())
    }
}
pub(super) struct Prepared {
    pub timeline: Arc<Timeline>,
    pub text: Option<(Arc<str>, Vec<Arc<str>>)>,
}
pub(super) fn prepare(
    source: Arc<str>,
    edit: Option<Edit>,
    crlf: bool,
) -> Result<Prepared, String> {
    let mut f = decode(&source)?;
    let text = if let Some(edit) = edit {
        edit.apply(&mut f)?;
        let source = serde_json::to_string_pretty(&f).map_err(|e| e.to_string())? + "\n";
        let source: Arc<str> = source.into();
        let bytes: Arc<str> = if crlf {
            source.replace('\n', "\r\n").into()
        } else {
            source.clone()
        };
        Some((source, vec![bytes]))
    } else {
        None
    };
    let bytes = Arc::new(
        text.as_ref()
            .map_or(&*source, |(s, _): &(Arc<str>, Vec<Arc<str>>)| s.as_ref())
            .as_bytes()
            .to_vec(),
    );
    Ok(Prepared {
        timeline: Arc::new(Timeline::new(f, bytes)),
        text,
    })
}
