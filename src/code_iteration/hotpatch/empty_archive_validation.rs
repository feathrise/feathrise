//! 显式私有实测：在独占纯Rust夹具上走真实DX冷/暖归档门，保留源码与缓存引用，不启动工程。

use super::*;
use super::super::{captured_client, compiler_output, execute, existing_target, hash_file, prepare_context,
    sha, small_output, validate_artifacts, validate_project, write_new};

#[test]
#[ignore = "显式设置FEATHRISE_EMPTY_AR_VALIDATION_ROOT(新目录)与FEATHRISE_EMPTY_AR_VALIDATION_TARGET(原target)"]
fn production_empty_archive_gate_cold_and_warm() {
    let requested = PathBuf::from(std::env::var_os("FEATHRISE_EMPTY_AR_VALIDATION_ROOT").expect("缺少独占夹具新目录"));
    let requested_target = PathBuf::from(std::env::var_os("FEATHRISE_EMPTY_AR_VALIDATION_TARGET").expect("缺少原共享target"));
    assert!(requested.is_absolute() && requested_target.is_absolute());
    assert!(!requested.components().any(|part| matches!(part, std::path::Component::ParentDir | std::path::Component::CurDir)));
    let target = existing_target(&requested_target).unwrap();
    regular_directory(&target).unwrap();
    let parent = requested.parent().unwrap().canonicalize().unwrap();
    regular_directory(&parent).unwrap();
    let root = parent.join(requested.file_name().expect("夹具目录名称无效"));
    assert!(!root.starts_with(&target) && !target.starts_with(&root));
    fs::create_dir(&root).expect("仅允许创建全新夹具，拒绝复用已有目录");
    let token = bevy::asset::uuid::Uuid::new_v4().simple().to_string();
    let bin = format!("feathrise_empty_ar_{token}");
    let leaf = format!("{bin}_leaf");
    let owner = serde_json::to_vec(&serde_json::json!({"schema":1,"scope":"production-empty-archive-gate",
        "token":token,"pid":std::process::id(),"root":root,"target":target})).unwrap();
    write_new(&root.join("owner.json"), &owner).unwrap();
    let control = BuildControl::default();
    // 本次污染现场只读保护；不存在的条目也不得由验收补造，目标根仍来自显式环境。
    let protected = ["libdeps-e218c3b7.a", "rlibs-e218c3b7.txt", "libdeps-e981dab2.a", "rlibs-e981dab2.txt"];
    let fingerprints = || -> Result<Vec<Option<String>>, String> {
        protected.iter().map(|name| {
            let path = target.join("x86_64-pc-windows-msvc/render-dev").join(name);
            if !path.exists() { return Ok(None); }
            regular_file(&path)?;
            hash_file(&path, &control).map(Some)
        }).collect()
    };
    let cache_before = fingerprints().unwrap();
    let mut rounds = Vec::new();
    let mut result = (|| -> Result<(), String> {
        for directory in ["src", "leaf", "leaf/src", "assets", "editor_data"] {
            fs::create_dir(root.join(directory)).map_err(|error| error.to_string())?;
        }
        write_new(&root.join("Cargo.toml"), format!(
            "[package]\nname='{bin}'\nversion='0.1.0'\nedition='2024'\n\
             [workspace]\nresolver='3'\nmembers=['leaf']\n[features]\nfixture=[]\n\
             [dependencies]\n{leaf}={{path='leaf'}}\n[profile.render-dev]\ninherits='dev'\nstrip=false\n"
        ).as_bytes())?;
        write_new(&root.join("src/main.rs"), format!("fn main() {{ println!(\"{{}}\", {leaf}::value()); }}\n").as_bytes())?;
        write_new(&root.join("leaf/Cargo.toml"), format!("[package]\nname='{leaf}'\nversion='0.1.0'\nedition='2024'\n").as_bytes())?;
        write_new(&root.join("leaf/src/lib.rs"), b"pub fn value() -> u32 { 7 }\n")?;
        write_new(&root.join("editor-project.json"), &serde_json::to_vec(&serde_json::json!({
            "format":2,"id":bevy::asset::uuid::Uuid::new_v4().to_string(),"name":"DX空归档归属夹具","cargo_manifest":"Cargo.toml","package":bin,
            "asset_roots":["assets"],"editor_data":"editor_data"
        })).map_err(|error| error.to_string())?)?;
        let project = crate::project::open(&root)?;
        validate_project(&project, &control)?;
        let context = prepare_context(&project, &target, "fixture", &control)
            .map_err(|error| format!("准备归档门真实链接环境：{error}"))?;
        // 只给新建夹具生成离线锁文件；两次真实DX仍沿用生产--locked，不改任何既有工程。
        let mut metadata = context.command_for(&context.cargo, &project)?;
        metadata.args(["generate-lockfile", "--offline", "--manifest-path"])
            .arg(&project.manifest);
        small_output(&metadata, &control).map_err(|error| format!("生成新夹具离线锁：{error}"))?;
        let inputs = ["owner.json", "editor-project.json", "Cargo.toml", "Cargo.lock", "src/main.rs", "leaf/Cargo.toml", "leaf/src/lib.rs"];
        let before = inputs.iter().map(|path| hash_file(&root.join(path), &control)).collect::<Result<Vec<_>, _>>()?;
        // 同一夹具/链接上下文复用原target做暖构建；不删除任何归档，也不按mtime猜当前回执。
        for stage in ["cold", "warm"] {
            let directory = root.join(stage);
            fs::create_dir(&directory).map_err(|error| error.to_string())?;
            let round = (|| -> Result<Value, String> {
                regular_directory(&root)?;
                if fs::read(root.join("owner.json")).map_err(|error| error.to_string())? != owner {
                    return Err("夹具归属marker已变化".into());
                }
                validate_project(&project, &control)?;
                prepare(&context, &project, &control)?;
                let mut command = context.command(&project)?;
                command.args(["build", "--fat-binary"]);
                context.arguments(&mut command);
                let raw = execute(&command, None, &project, &control, None)?;
                write_new(&directory.join("dx.stdout.raw"), &raw)?;
                let fat = compiler_output::parse_fat_output(&raw)?;
                validate_artifacts(&fat.artifacts, &project, &target)?;
                let mut client: Value = serde_json::from_slice(&fat.client_json).map_err(|error| error.to_string())?;
                captured_client::normalize(&mut client, &target, &context.bin, &project.root, &project.package, &context.link_manifest)?;
                write_new(&directory.join("client.json"), &serde_json::to_vec(&client).map_err(|error| error.to_string())?)?;
                let response_path = context.session_cache.join("windows/windows_command.txt");
                let (_lease, response) = read_locked(&response_path, MAX_COMPILER_OUTPUT_BYTES as u64, &control)?;
                write_new(&directory.join("windows_command.txt"), &response)?;
                verify(&context, &client, &control)?;
                let text = std::str::from_utf8(&response).map_err(|error| error.to_string())?;
                let arguments = response_arguments(text)?;
                let selected = response_option(&arguments, "/WHOLEARCHIVE")?.map(PathBuf::from);
                let mut archive = Value::Null;
                if let Some(path) = &selected {
                    let path = path.canonicalize().map_err(|error| error.to_string())?;
                    let length = regular_file(&path)?.len();
                    let mut header = [0_u8; 8];
                    fs::File::open(&path).and_then(|mut file| file.read_exact(&mut header)).map_err(|error| error.to_string())?;
                    archive = serde_json::json!({"path":path,"length":length,"header_sha256":sha(&header)});
                    if stage == "warm" && (length != AR_HEADER.len() as u64 || header.as_slice() != AR_HEADER) {
                        return Err("暖构建本次实际选中归档不是8字节合法空ar".into());
                    }
                } else if stage == "warm" { return Err("暖构建没有实际/WHOLEARCHIVE回执，不能计为暖空ar通过".into()); }
                Ok(serde_json::json!({"stage":stage,"verified":true,"raw_sha256":sha(&raw),
                    "response_sha256":sha(&response),"selected_archive":archive,"executable":fat.artifacts.executable}))
            })();
            match round {
                Ok(row) => rounds.push(row),
                Err(error) => { rounds.push(serde_json::json!({"stage":stage,"verified":false,"error":error})); return Err(error); }
            }
        }
        let after = inputs.iter().map(|path| hash_file(&root.join(path), &control)).collect::<Result<Vec<_>, _>>()?;
        if before != after { return Err("冷暖构建改变了夹具来源或归属marker".into()); }
        Ok(())
    })();
    let cache_after = fingerprints();
    let cache_unchanged = cache_after.as_ref().is_ok_and(|after| *after == cache_before);
    if !cache_unchanged && result.is_ok() { result = Err("原污染现场四份缓存文件发生改变或无法核验".into()); }
    // 仅报告已走过的归档门；不调用collect_imports、不宣称fat发布/SDK握手或运行效果。
    let report = serde_json::json!({"schema":1,"scope":"production-empty-archive-gate-only","token":token,
        "root":root,"shared_target":target,"full_fat_publication":false,"game_executed":false,
        "cold_scope":"new unique fixture; original shared cache preserved","same_context_for_rounds":true,
        "protected_stale_cache":protected,"protected_sha256_before":cache_before,"protected_stale_cache_unchanged":cache_unchanged,
        "passed":result.is_ok(),"error":result.as_ref().err(),"rounds":rounds,"fixture_retained":true});
    let report_path = root.join("report.json");
    write_new(&report_path, &serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("空归档实际冷暖回执：{}", report_path.display());
    result.unwrap();
}
