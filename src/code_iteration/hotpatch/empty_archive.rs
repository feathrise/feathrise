//! 规范化 dx 0.7.9 空聚合库，并在 fat 发布前用本次原始链接回执核对其完整依赖清单。

use super::{Context, MAX_COMPILER_OUTPUT_BYTES, small_output};
use crate::{build_process::BuildControl, project::Project};
use serde_json::Value;
use std::{fs::{self, OpenOptions}, io::{Read, Write}, os::windows::fs::{MetadataExt, OpenOptionsExt}, path::{Path, PathBuf}};

const AR_HEADER: &[u8] = b"!<arch>\n";
const MAX_ENTRIES: usize = 4096;
const MAX_SIDECAR: u64 = 128 * 1024;

#[cfg(test)]
#[path = "empty_archive_validation.rs"]
mod private_validation;

pub(super) fn prepare(context: &Context, project: &Project, control: &BuildControl) -> Result<(), String> {
    let profile = context.target.join("x86_64-pc-windows-msvc/render-dev");
    let candidates = empty_candidates(&profile, control)?;
    if candidates.is_empty() { return Ok(()); }
    // 与 dx 的 workspace_dir 判定一致，不能把嵌套 package 根误当 workspace 根。
    let mut command = context.command_for(&context.cargo, project)?;
    command.args(["locate-project", "--workspace", "--message-format", "plain", "--manifest-path"]).arg(&project.manifest);
    let manifest = PathBuf::from(small_output(&command, control)?.trim()).canonicalize().map_err(|error| error.to_string())?;
    let workspace = manifest.parent().ok_or("热更新 workspace 清单缺少父目录")?;
    let repaired = repair_candidates(candidates, workspace, control)?;
    if repaired != 0 && let Some(log) = control.log_sink() {
        log.task_message(&format!("已校验合法成员并补齐{repaired}个 dx 空归档格式头，完整性由本次 fat 发布门核对"));
    }
    Ok(())
}

/// 只核对本次 fat 实际使用的空 ar；原始完整 rlib 集合由已校正入口身份的 client 提供。
pub(super) fn verify(context: &Context, client: &Value, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    let session = context.link_manifest.parent().ok_or("fat 链接会话缺少目录")?;
    if session.parent() != Some(context.target.join("hotpatch-tools/function-sessions").as_path())
        || context.session_cache != session.join("session-cache") {
        return Err("空 ar 完整性回执不属于本次 fat 会话".into());
    }
    regular_directory(session)?;
    regular_directory(&context.session_cache)?;
    let bundle = context.session_cache.join("windows");
    regular_directory(&bundle)?;
    let link_args = bundle.join("link_args.json");
    regular_file(&link_args)?;
    regular_file(&context.link_manifest)?;
    if client_environment_path(client, "DX_LINK_ARGS_FILE")? != link_args
        || client_environment_path(client, "FEATHRISE_HOTPATCH_LINK_CONTEXT")? != context.link_manifest {
        return Err("空 ar 完整性回执的入口捕获来自其他 fat 会话".into());
    }
    // dx 的文件名虽以 .json 结尾，原参数实际按行存储；此处直接用 client.link_args，
    // 仅从当前会话的严格响应文件确定实际 /WHOLEARCHIVE 与 /OUT，拒绝借用旧会话或 thin 回执。
    let (_response_lease, response) = read_locked(&bundle.join("windows_command.txt"), MAX_COMPILER_OUTPUT_BYTES as u64, control)?;
    let response = std::str::from_utf8(&response).map_err(|error| error.to_string())?;
    let arguments = response_arguments(response)?;
    let output = response_option(&arguments, "/OUT")?.ok_or("fat 响应文件缺少唯一 /OUT")?;
    let profile = context.target.join("x86_64-pc-windows-msvc/render-dev");
    regular_directory(&profile)?;
    let expected_output = profile.join(format!("{}.exe", context.bin));
    if !Path::new(output).is_absolute()
        || Path::new(output).canonicalize().map_err(|error| error.to_string())? != expected_output {
        return Err("空 ar 完整性回执的输出不是本次 fat 入口 EXE".into());
    }
    let Some(archive) = response_option(&arguments, "/WHOLEARCHIVE")? else { return control.check(); };
    if !Path::new(archive).is_absolute() { return Err("fat 聚合归档必须是实际绝对路径".into()); }
    let archive = Path::new(archive).canonicalize().map_err(|error| error.to_string())?;
    let hash = archive.file_name().and_then(|name| name.to_str()).and_then(archive_hash).ok_or("fat 聚合归档名称不是受检的 libdeps-8hex.a")?;
    if archive.parent() != Some(profile.as_path()) { return Err("fat 聚合归档不属于当前 target 开发配置".into()); }
    let length = regular_file(&archive)?.len();
    if length < AR_HEADER.len() as u64 { return Err("fat 实际选中的聚合归档为空或格式头不完整，未发布基线".into()); }
    if length != AR_HEADER.len() as u64 { return control.check(); }
    let (_archive_lease, bytes) = read_locked(&archive, AR_HEADER.len() as u64, control)?;
    if bytes != AR_HEADER { return Err("fat 的八字节聚合归档不是合法空 ar".into()); }
    let (_sidecar_lease, sidecar) = read_locked(&profile.join(format!("rlibs-{hash}.txt")), MAX_SIDECAR, control)
        .map_err(|error| format!("fat 空 ar 依赖清单缺失或不可校验：{error}"))?;
    let sidecar = std::str::from_utf8(&sidecar).map_err(|error| error.to_string())?;
    let original = client["link_args"].as_array().ok_or("fat client 缺少原始完整链接参数")?;
    if original.len() > MAX_ENTRIES { return Err("fat 原始链接参数超过4096项".into()); }
    let mut expected = Vec::new();
    for argument in original {
        control.check()?;
        let argument = argument.as_str().ok_or("fat 原始链接参数不是字符串")?;
        if argument.chars().any(char::is_control) { return Err("fat 原始链接参数包含控制字符".into()); }
        if argument.ends_with(".rlib") {
            let path = Path::new(argument);
            if !path.is_absolute() || !path.is_file() { return Err("fat 原始 rlib 不是现存绝对路径".into()); }
            expected.push(argument);
        }
    }
    // 官方仅相邻 dedup，不排序或全局去重；相同成员但缺项、插项或改变次序均不是本次完整清单。
    expected.dedup();
    if expected.is_empty() || expected.len() > 1024 { return Err("fat 空 ar 缺少受检的完整 rlib 集合或超过1024项".into()); }
    if sidecar.lines().collect::<Vec<_>>() != expected {
        return Err("fat 空 ar 依赖清单与本次原始完整 rlib 序列不一致，未发布基线".into());
    }
    control.check()
}

fn client_environment_path(client: &Value, name: &str) -> Result<PathBuf, String> {
    let envs = client["rustc_envs"].as_array().ok_or("fat client 缺少受检入口环境")?;
    let mut value = None;
    for pair in envs {
        let pair = pair.as_array().filter(|pair| pair.len() == 2).ok_or("fat 入口环境不是键值对")?;
        if pair[0].as_str() == Some(name) {
            if value.replace(pair[1].as_str().ok_or("fat 入口环境值不是字符串")?).is_some() {
                return Err(format!("fat 入口环境重复{name}"));
            }
        }
    }
    Path::new(value.ok_or_else(|| format!("fat 入口环境缺少{name}"))?).canonicalize().map_err(|error| error.to_string())
}

/// 官方 Windows fat 响应文件逐项加双引号并以一个空格分隔；仅接受这套已实测格式，不猜测转义。
fn response_arguments(mut remaining: &str) -> Result<Vec<&str>, String> {
    if remaining.chars().any(char::is_control) { return Err("fat 响应文件包含控制字符".into()); }
    let mut result = Vec::new();
    loop {
        let quoted = remaining.strip_prefix('"').ok_or("fat 响应文件不是受检的逐项双引号格式")?;
        let (argument, rest) = quoted.split_once('"').ok_or("fat 响应文件双引号未闭合")?;
        if argument.is_empty() || result.len() >= MAX_ENTRIES { return Err("fat 响应参数为空或超过4096项".into()); }
        result.push(argument);
        if rest.is_empty() { return Ok(result); }
        remaining = rest.strip_prefix(' ').ok_or("fat 响应参数没有以单个空格分隔")?;
    }
}

fn response_option<'a>(arguments: &[&'a str], name: &str) -> Result<Option<&'a str>, String> {
    let mut found = None;
    for &argument in arguments {
        if argument.eq_ignore_ascii_case(name) { return Err(format!("fat 响应{name}缺少路径")); }
        if let Some((key, value)) = argument.split_once(':') && key.eq_ignore_ascii_case(name) {
            if value.is_empty() || found.replace(value).is_some() { return Err(format!("fat 响应{name}路径为空或不唯一")); }
        }
    }
    Ok(found)
}

fn read_locked(path: &Path, maximum: u64, control: &BuildControl) -> Result<(fs::File, Vec<u8>), String> {
    control.check()?;
    regular_file(path)?;
    let mut file = OpenOptions::new().read(true).share_mode(1).open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    (&mut file).take(maximum + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum { return Err("fat 完整性回执超过固定字节预算".into()); }
    control.check()?;
    Ok((file, bytes))
}

fn empty_candidates(profile: &Path, control: &BuildControl) -> Result<Vec<PathBuf>, String> {
    control.check()?;
    match fs::symlink_metadata(profile) {
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return control.check().map(|_| Vec::new()),
        Err(error) => return Err(error.to_string()),
    }
    regular_directory(profile)?;
    let mut result = Vec::new();
    for (index, entry) in fs::read_dir(profile).map_err(|error| error.to_string())?.enumerate() {
        control.check()?;
        if index >= MAX_ENTRIES { return Err("dx 归档目录超过4096项，未扫描其他目录".into()); }
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if name.to_str().and_then(archive_hash).is_none() { continue; }
        // 共享target中的单个过期/不规则条目不是当前工程失败；根目录与总扫描预算仍是硬门。
        let Ok(metadata) = regular_file(&entry.path()) else { continue; };
        if metadata.len() == 0 { result.push(entry.path()); }
    }
    control.check()?;
    Ok(result)
}

fn archive_hash(name: &str) -> Option<&str> {
    let hash = name.strip_prefix("libdeps-")?.strip_suffix(".a")?;
    (hash.len() == 8 && hash.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))).then_some(hash)
}

#[derive(Debug)]
enum CandidateIssue {
    Skip,
    Fatal(String),
}

struct CandidateLease {
    archive: fs::File,
    // sidecar的只读共享租约覆盖写头阶段，防止校验后清单被并发替换；从不改写该清单。
    _sidecar: fs::File,
}

fn repair_candidates(candidates: Vec<PathBuf>, workspace: &Path, control: &BuildControl) -> Result<usize, String> {
    control.check()?;
    let mut repaired = 0;
    for archive in candidates {
        match checked_candidate(&archive, workspace, control) {
            Ok(Some(lease)) => { write_header(lease, control)?; repaired += 1; }
            Ok(None) | Err(CandidateIssue::Skip) => control.check()?,
            Err(CandidateIssue::Fatal(error)) => return Err(error),
        }
    }
    control.check()?;
    Ok(repaired)
}

/// 此阶段没有写副作用。只有可证明合法且未被占用的候选才取得写租约；失效缓存保留原样。
fn checked_candidate(archive: &Path, workspace: &Path, control: &BuildControl) -> Result<Option<CandidateLease>, CandidateIssue> {
    control.check().map_err(CandidateIssue::Fatal)?;
    let hash = archive.file_name().and_then(|name| name.to_str()).and_then(archive_hash).ok_or(CandidateIssue::Skip)?;
    let parent = archive.parent().ok_or(CandidateIssue::Skip)?;
    regular_directory(parent).map_err(CandidateIssue::Fatal)?;
    if regular_file(archive).map_err(|_| CandidateIssue::Skip)?.len() != 0 { return Ok(None); }
    let sidecar = parent.join(format!("rlibs-{hash}.txt"));
    regular_file(&sidecar).map_err(|_| CandidateIssue::Skip)?;
    // 预构建只校验已列成员的合法性；是否漏项必须在 fat 返回真实原始参数后由 verify 证明。
    let mut list = OpenOptions::new().read(true).share_mode(1).open(&sidecar).map_err(|_| CandidateIssue::Skip)?;
    let mut bytes = Vec::new();
    (&mut list).take(MAX_SIDECAR + 1).read_to_end(&mut bytes).map_err(|_| CandidateIssue::Skip)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_SIDECAR { return Err(CandidateIssue::Skip); }
    let text = std::str::from_utf8(&bytes).map_err(|_| CandidateIssue::Skip)?;
    let mut count = 0;
    for line in text.lines() {
        control.check().map_err(CandidateIssue::Fatal)?;
        count += 1;
        if count > 1024 || line.is_empty() || line.chars().any(char::is_control) { return Err(CandidateIssue::Skip); }
        let path = Path::new(line);
        if !path.is_absolute() || path.extension().is_none_or(|extension| extension != "rlib") {
            return Err(CandidateIssue::Skip);
        }
        let path = path.canonicalize().map_err(|_| CandidateIssue::Skip)?;
        if !path.is_file() || path.starts_with(workspace) { return Err(CandidateIssue::Skip); }
    }
    // dx 0.7.9 link.rs:863–923 在全部 rlib 位于 workspace 外时写出空 Vec；
    // :845/:942 又在暖构建按 exists() 将其加入 /WHOLEARCHIVE。8字节空 ar 不添加或移除任何成员。
    let file = OpenOptions::new().read(true).write(true).share_mode(0).open(archive).map_err(|_| CandidateIssue::Skip)?;
    if file.metadata().map_err(|_| CandidateIssue::Skip)?.len() != 0 { return Ok(None); }
    Ok(Some(CandidateLease { archive: file, _sidecar: list }))
}

/// 已可能改变归档后，写入或同步错误必须终止本轮，绝不能降级成候选跳过。
fn write_header(mut lease: CandidateLease, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    lease.archive.write_all(AR_HEADER).and_then(|_| lease.archive.sync_data()).map_err(|error| error.to_string())?;
    control.check()
}

/// 原严格单候选回归保留拒绝语义；生产预扫描必须经repair_candidates区分Skip与Fatal。
#[cfg(test)]
fn normalize(archive: &Path, workspace: &Path, control: &BuildControl) -> Result<bool, String> {
    match checked_candidate(archive, workspace, control) {
        Ok(Some(lease)) => { write_header(lease, control)?; Ok(true) }
        Ok(None) => Ok(false),
        Err(CandidateIssue::Skip) => Err("dx 空归档候选失效、不合规或正被占用，未改写".into()),
        Err(CandidateIssue::Fatal(error)) => Err(error),
    }
}

fn regular_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 || path.canonicalize().map_err(|error| error.to_string())? != path {
        return Err("dx 归档目录不是当前 target 的实际目录".into());
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 || path.canonicalize().map_err(|error| error.to_string())? != path {
        return Err("dx 归档或清单不是当前目录的实际文件".into());
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture { root: PathBuf, workspace: PathBuf, archive: PathBuf, sidecar: PathBuf, dependency: PathBuf }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("feathrise-empty-ar-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("workspace")).unwrap();
            fs::create_dir(root.join("profile")).unwrap();
            let root = root.canonicalize().unwrap();
            let workspace = root.join("workspace");
            let archive = root.join("profile/libdeps-1234abcd.a");
            let sidecar = root.join("profile/rlibs-1234abcd.txt");
            let dependency = root.join("external.rlib");
            fs::write(&archive, b"").unwrap(); fs::write(&dependency, b"fixture-rlib-not-linked").unwrap();
            fs::write(&sidecar, dependency.to_str().unwrap()).unwrap();
            Self { root, workspace, archive, sidecar, dependency }
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }

    struct FatFixture { fixture: Fixture, context: Context, client: Value, response: PathBuf, archive: PathBuf, sidecar: PathBuf }
    impl FatFixture {
        fn new() -> Self {
            let fixture = Fixture::new();
            let target = fixture.root.join("target");
            let profile = target.join("x86_64-pc-windows-msvc/render-dev");
            let session = target.join("hotpatch-tools/function-sessions/current");
            let session_cache = session.join("session-cache");
            let bundle = session_cache.join("windows");
            fs::create_dir_all(&profile).unwrap(); fs::create_dir_all(&bundle).unwrap();
            let output = profile.join("actual-bin.exe"); fs::write(&output, b"fixture-exe-not-executed").unwrap();
            let archive = profile.join("libdeps-1234abcd.a"); fs::write(&archive, AR_HEADER).unwrap();
            let sidecar = profile.join("rlibs-1234abcd.txt");
            let second = fixture.root.join("second-external.rlib"); fs::write(&second, b"second-rlib").unwrap();
            fs::write(&sidecar, format!("{}\n{}", fixture.dependency.display(), second.display())).unwrap();
            let link_args = bundle.join("link_args.json"); fs::write(&link_args, b"fixture-link-args-not-read").unwrap();
            let link_manifest = session.join("link-context.txt"); fs::write(&link_manifest, b"current-session").unwrap();
            let response = bundle.join("windows_command.txt");
            fs::write(&response, format!("\"/WHOLEARCHIVE:{}\" \"/OUT:{}\"", archive.display(), output.display())).unwrap();
            let client = serde_json::json!({"link_args":["/DEBUG", fixture.dependency, fixture.dependency, second],
                "rustc_envs":[["DX_LINK_ARGS_FILE",link_args],["FEATHRISE_HOTPATCH_LINK_CONTEXT",link_manifest]]});
            let context = Context { format:1, project:super::super::ProjectIdentity { id:"fixture".into(), root:fixture.workspace.clone(),
                    manifest:fixture.workspace.join("Cargo.toml"), package:"fixture".into() }, target, features:String::new(), bin:"actual-bin".into(),
                dx:PathBuf::new(), dx_sha256:String::new(), rustc:PathBuf::new(), rustc_sha256:String::new(), rustc_version:String::new(),
                cargo:PathBuf::new(), cargo_sha256:String::new(), proxy:PathBuf::new(), proxy_sha256:String::new(), raw_linker:PathBuf::new(),
                raw_sha256:String::new(), sysroot_lib:PathBuf::new(), session_cache, link_manifest, link_manifest_sha256:String::new(),
                imports_file:session.join("imports.txt"), imports:Vec::new(), environment:Default::default() };
            Self { fixture, context, client, response, archive, sidecar }
        }
    }

    #[test]
    fn fat_publication_requires_every_original_rlib_and_preserves_adjacent_dedup_order() {
        let mut f = FatFixture::new(); let control = BuildControl::default(); let before = fs::read(&f.sidecar).unwrap();
        verify(&f.context, &f.client, &control).unwrap();
        assert_eq!(fs::read(&f.sidecar).unwrap(), before);
        assert_eq!(fs::read(&f.archive).unwrap(), AR_HEADER);
        // 不相邻的重复项不能被当成集合去重，否则无法逐序复现官方参数。
        let dependency = f.fixture.dependency.display().to_string();
        f.client["link_args"].as_array_mut().unwrap().push(serde_json::json!(dependency));
        fs::write(&f.sidecar, format!("{}\n{}", String::from_utf8(before).unwrap(), dependency)).unwrap();
        verify(&f.context, &f.client, &control).unwrap();
    }

    #[test]
    fn legal_sidecar_subset_or_reordered_members_never_publish_an_empty_fat_archive() {
        let f = FatFixture::new(); let control = BuildControl::default();
        let first = f.fixture.dependency.display().to_string(); let second = f.fixture.root.join("second-external.rlib").display().to_string();
        for list in [first.clone(), format!("{second}\n{first}"), format!("{first}\n{second}\n{second}")] {
            fs::write(&f.sidecar, list).unwrap();
            // 这些项全部是合法现存外部 rlib，预构建成员校验通过也不能代替完整性门。
            assert!(verify(&f.context, &f.client, &control).unwrap_err().contains("完整 rlib 序列不一致"));
            assert_eq!(fs::read(&f.archive).unwrap(), AR_HEADER);
        }
    }

    #[test]
    fn fat_verification_rejects_foreign_session_and_missing_sidecar() {
        let f = FatFixture::new(); let control = BuildControl::default();
        let other = f.fixture.root.join("other-link-args.json"); fs::write(&other, b"other-session").unwrap();
        let mut client = f.client.clone(); client["rustc_envs"][0][1] = serde_json::json!(other);
        assert!(verify(&f.context, &client, &control).unwrap_err().contains("其他 fat 会话"));
        fs::remove_file(&f.sidecar).unwrap();
        assert!(verify(&f.context, &f.client, &control).unwrap_err().contains("依赖清单缺失"));
    }

    #[test]
    fn fat_member_archives_and_unused_cold_archives_do_not_require_the_full_sidecar_equation() {
        let f = FatFixture::new(); let control = BuildControl::default();
        fs::write(&f.archive, b"!<arch>\nnonempty-members").unwrap(); fs::remove_file(&f.sidecar).unwrap();
        verify(&f.context, &f.client, &control).unwrap();
        fs::write(&f.archive, b"").unwrap();
        let output = f.context.target.join("x86_64-pc-windows-msvc/render-dev/actual-bin.exe");
        fs::write(&f.response, format!("\"/OUT:{}\"", output.display())).unwrap();
        verify(&f.context, &f.client, &control).unwrap();
    }

    #[test]
    fn fat_response_format_and_unique_options_are_not_guessed() {
        assert_eq!(response_arguments("\"/OUT:中文 路径.exe\" \"/DEBUG\"").unwrap(), ["/OUT:中文 路径.exe", "/DEBUG"]);
        for text in ["", "/OUT:file.exe", "\"/DEBUG", "\"/DEBUG\"  \"/OUT:file.exe\"", "\"/DEBUG\"\n", "\"/DEBUG\"junk"] {
            assert!(response_arguments(text).is_err());
        }
        assert!(response_option(&["/OUT:a.exe", "/out:b.exe"], "/OUT").is_err());
        assert!(response_option(&["/WHOLEARCHIVE:a.a", "/wholearchive:b.a"], "/WHOLEARCHIVE").is_err());
        assert!(response_option(&["/WHOLEARCHIVE"], "/WHOLEARCHIVE").is_err());
    }

    #[test]
    fn valid_empty_cache_keeps_every_dependency_and_becomes_idempotent() {
        let f = Fixture::new(); let control = BuildControl::default(); let list = fs::read(&f.sidecar).unwrap();
        assert!(normalize(&f.archive, &f.workspace, &control).unwrap());
        assert_eq!(fs::read(&f.archive).unwrap(), AR_HEADER);
        assert_eq!(fs::read(&f.sidecar).unwrap(), list);
        assert_eq!(fs::read(&f.dependency).unwrap(), b"fixture-rlib-not-linked");
        assert!(empty_candidates(f.archive.parent().unwrap(), &control).unwrap().is_empty());
        assert!(!normalize(&f.archive, &f.workspace, &control).unwrap());
    }

    #[test]
    fn absent_invalid_or_workspace_sidecar_never_turns_empty_cache_into_success() {
        let f = Fixture::new(); let control = BuildControl::default();
        for list in [String::new(), f.root.join("absent.rlib").display().to_string(), "relative.rlib".into()] {
            fs::write(&f.sidecar, list).unwrap();
            assert!(normalize(&f.archive, &f.workspace, &control).is_err());
            assert_eq!(fs::metadata(&f.archive).unwrap().len(), 0);
        }
        let internal = f.workspace.join("inside.rlib"); fs::write(&internal, b"internal").unwrap();
        fs::write(&f.sidecar, internal.to_str().unwrap()).unwrap();
        assert!(normalize(&f.archive, &f.workspace, &control).is_err());
        assert_eq!(fs::metadata(&f.archive).unwrap().len(), 0);
    }

    #[test]
    fn archive_in_use_and_cancellation_leave_empty_bytes_untouched() {
        let f = Fixture::new(); let control = BuildControl::default();
        let lease = fs::File::open(&f.archive).unwrap();
        assert!(normalize(&f.archive, &f.workspace, &control).is_err());
        assert_eq!(fs::metadata(&f.archive).unwrap().len(), 0); drop(lease);
        control.cancel(); assert!(normalize(&f.archive, &f.workspace, &control).is_err());
        assert_eq!(fs::metadata(&f.archive).unwrap().len(), 0);
    }

    #[test]
    fn mixed_profile_repairs_only_valid_candidates_and_preserves_stale_malformed_and_busy_bytes() {
        let f = Fixture::new(); let profile = f.archive.parent().unwrap();
        let create = |hash: &str, sidecar: Option<&[u8]>| {
            let archive = profile.join(format!("libdeps-{hash}.a"));
            let list = profile.join(format!("rlibs-{hash}.txt"));
            fs::write(&archive, b"").unwrap();
            if let Some(bytes) = sidecar { fs::write(&list, bytes).unwrap(); }
            (archive, list)
        };
        let absent_dependency = f.root.join("retired-fixture-leaf.rlib").display().to_string();
        let valid_list = f.dependency.display().to_string();
        let stale = create("00000001", Some(absent_dependency.as_bytes()));
        let missing_sidecar = create("00000002", None);
        let malformed = create("00000003", Some(&[0xff, 0x00]));
        let held_archive = create("00000004", Some(valid_list.as_bytes()));
        let held_sidecar = create("00000005", Some(valid_list.as_bytes()));
        // 名称虽匹配，目录不是可改写归档，不能从扫描层把整批候选变成失败。
        let irregular = profile.join("libdeps-00000006.a"); fs::create_dir(&irregular).unwrap();
        let mut preserved = vec![f.sidecar.clone(), f.dependency.clone()];
        for (archive, list) in [&stale, &missing_sidecar, &malformed, &held_archive, &held_sidecar] {
            preserved.extend([archive.clone(), list.clone()]);
        }
        let before: Vec<_> = preserved.iter().map(|path| (path.clone(), fs::read(path).ok())).collect();
        let archive_lease = fs::File::open(&held_archive.0).unwrap();
        let sidecar_lease = OpenOptions::new().read(true).share_mode(0).open(&held_sidecar.1).unwrap();
        let control = BuildControl::default();
        let candidates = empty_candidates(profile, &control).unwrap();
        assert_eq!(repair_candidates(candidates, &f.workspace, &control).unwrap(), 1);
        drop(archive_lease); drop(sidecar_lease);
        assert_eq!(fs::read(&f.archive).unwrap(), AR_HEADER);
        assert!(irregular.is_dir());
        for (path, bytes) in before {
            assert_eq!(fs::read(&path).ok(), bytes, "候选跳过不得改写{}", path.display());
        }
        assert!(!f.root.join("retired-fixture-leaf.rlib").exists());
        assert_eq!(repair_candidates(empty_candidates(profile, &control).unwrap(), &f.workspace, &control).unwrap(), 2,
            "解锁后两份合法空归档才可修复，坏候选仍须跳过");
    }

    #[test]
    fn cancellation_and_invalid_profile_roots_remain_fatal_before_any_header_write() {
        let f = Fixture::new(); let control = BuildControl::default();
        let candidates = empty_candidates(f.archive.parent().unwrap(), &control).unwrap();
        control.cancel();
        assert!(repair_candidates(candidates, &f.workspace, &control).is_err());
        assert_eq!(fs::read(&f.archive).unwrap(), b"");
        assert!(empty_candidates(f.archive.parent().unwrap(), &control).is_err());

        // 真实租约取得后发生取消也不能被候选Skip吞掉；写阶段是独立的硬错误边界。
        let control = BuildControl::default();
        let lease = checked_candidate(&f.archive, &f.workspace, &control).unwrap().unwrap();
        control.cancel();
        assert!(write_header(lease, &control).is_err());
        assert_eq!(fs::read(&f.archive).unwrap(), b"");
        // verbatim路径的PathBuf::push会折叠..；一次性从OsString构造，才能真正覆盖非规范输入。
        let mut noncanonical = f.root.as_os_str().to_os_string();
        noncanonical.push(r"\profile\..\profile\libdeps-1234abcd.a");
        let noncanonical = PathBuf::from(noncanonical);
        assert!(noncanonical.to_string_lossy().contains(r"\profile\..\profile\"));
        assert!(repair_candidates(vec![noncanonical], &f.workspace, &BuildControl::default()).is_err());
        assert_eq!(fs::read(&f.archive).unwrap(), b"");
        assert!(empty_candidates(&f.dependency, &BuildControl::default()).is_err());
    }

    #[test]
    fn the_archive_selected_by_the_current_fat_response_cannot_be_empty_or_a_partial_header() {
        let f = FatFixture::new(); let control = BuildControl::default();
        for length in 0..AR_HEADER.len() {
            fs::write(&f.archive, &AR_HEADER[..length]).unwrap();
            assert!(verify(&f.context, &f.client, &control).unwrap_err().contains("格式头不完整"));
            assert_eq!(fs::read(&f.archive).unwrap(), &AR_HEADER[..length]);
        }
        fs::write(&f.archive, AR_HEADER).unwrap();
        verify(&f.context, &f.client, &control).unwrap();
    }

    #[test]
    fn scanning_is_exact_and_does_not_modify_nonempty_or_unrelated_files() {
        let f = Fixture::new(); let profile = f.archive.parent().unwrap();
        fs::write(&f.archive, b"existing-members").unwrap();
        for name in ["libdeps-fedcABCD.a", "libdeps-1234abcd.lib", "libdeps-123.a", "other.a"] { fs::write(profile.join(name), b"").unwrap(); }
        assert!(empty_candidates(profile, &BuildControl::default()).unwrap().is_empty());
        assert_eq!(fs::read(&f.archive).unwrap(), b"existing-members");
    }
}
