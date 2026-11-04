//! 工程3D启动编译的独立实窗验收；仅本轮独占夹具安装，旧双模式流程保持独立。

mod driver;
mod evidence;
mod fixture;
mod resources;
mod types;

pub(crate) use driver::install;
pub(crate) use fixture::run;
pub(crate) use resources::Completion;

const ENVIRONMENT: &str = "FEATHRISE_EMBEDDED_BUILD_CANCEL_DIR";
const OWNER_FILE: &str = ".embedded-build-cancel-owner.json";
const PURPOSE: &str = "owned-embedded-startup-build-cancel";
const REPORT_FILE: &str = "build-cancel-editor.json";
const START_FILE: &str = ".validation-build-started.json";
const RETRY_FILE: &str = ".validation-build-retried.json";
const RELEASE_FILE: &str = ".validation-build-release";
const TEMPLATE_BUILD_SCRIPT: &str = include_str!("../../../assets/templates/empty/build.rs");

/// 此源码只写入本轮独占夹具。真实Cargo执行它后才有启动证据，验收不会直接生成该标记。
const BUILD_SCRIPT: &str = r#"// 仅显式停止编译验收的自有夹具延迟；原模板构建参数先由同一入口执行。
use std::{fs, io::Write, path::PathBuf, time::{Duration, Instant}};
fn main() {
    validation_template_build();
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest root"));
    assert_eq!(fs::read_to_string(root.join(".validation-build-token")).unwrap(), "@NONCE@");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.validation-build-release");
    let released = root.join(".validation-build-release").is_file();
    let marker = root.join(if released { ".validation-build-retried.json" } else { ".validation-build-started.json" });
    let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(marker).unwrap();
    writeln!(marker, "{{\"schema\":1,\"nonce\":\"@NONCE@\",\"project_id\":\"@PROJECT_ID@\",\"pid\":{},\"released\":{}}}",
        std::process::id(), released).unwrap();
    marker.sync_all().unwrap();
    if !released {
        // 等待发生在真正的Cargo子进程里；超时必须失败，不能让停止按钮错过窗口却计成功。
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            assert!(!root.join(".validation-build-release").exists(), "首次构建不得由验收提前放行");
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("受控真实编译未在180秒内通过原停止入口取消");
    }
}
"#;

fn script(nonce: &str, project: &str) -> String {
    // 保留模板的Windows开发栈参数，只把本轮真实Cargo延迟包在自有夹具的main外层。
    let original = TEMPLATE_BUILD_SCRIPT.replacen("fn main()", "fn validation_template_build()", 1);
    format!("{original}\n{}", BUILD_SCRIPT.replace("@NONCE@", nonce).replace("@PROJECT_ID@", project))
}
