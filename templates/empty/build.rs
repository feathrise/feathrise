//! 空工程沿用编辑器已使用的Windows开发栈预算，不修改工具链和优化档。
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
        && std::env::var("OPT_LEVEL").as_deref() == Ok("0")
    {
        println!("cargo::rustc-link-arg-bins=/STACK:8388608");
    }
}
