//! 植被作者编译的工作集可寻址校验。

/// 在分配当前阶段整图前检查平台整数可寻址性；不设置人为总内存配额。
pub(super) fn ensure_compile_working_set(
    final_output_bytes: usize,
    temporary_bytes: usize,
    label: &str,
) -> Result<(), String> {
    final_output_bytes
        .checked_add(temporary_bytes)
        .ok_or_else(|| format!("植被编译阶段{label}工作集字节数溢出"))?;
    Ok(()) // 实际分配由当前系统资源决定，内容层不猜测用户硬件配额。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_accepts_large_working_sets_without_an_artificial_budget() {
        assert!(ensure_compile_working_set(24 * 1024 * 1024, 32 * 1024 * 1024, "HN-01").is_ok());
        assert!(
            ensure_compile_working_set(128 * 1024 * 1024, 65 * 1024 * 1024, "大型地图").is_ok()
        );
        assert!(ensure_compile_working_set(usize::MAX, 1, "溢出").is_err()); // 真实整数溢出仍必须被拒绝。
    }
}
