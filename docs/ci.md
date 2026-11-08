# 编辑器 Windows CI

[Windows 工作流](../.github/workflows/windows-check.yml)在推送、Pull Request 或手动触发时，检查 Bevy 0.20.1 的宿主、空工程模板和地图作者库。源码联调包含默认和 `validation` 两个组合，要求 Rust 1.97.1 或更新版本。

## 源码联调

在 GitHub 仓库的 **Settings → Secrets and variables → Actions → Variables** 配置同一轮 0.20.1 迁移的 SDK 和补丁提交。

| 变量 | 值 |
| --- | --- |
| `FEATHRISE_SDK_REPOSITORY` | `feathrise/feathrise_runtime` |
| `FEATHRISE_SDK_REV` | 配套 SDK 的完整 40 位提交 ID，必填 |
| `FEATHRISE_PATCH_REPOSITORY` | `feathrise/feathrise_patches` |
| `FEATHRISE_PATCH_REV` | 配套补丁的完整 40 位提交 ID，必填 |

两个仓库名可以用 `owner/repository` 格式覆盖。缺少完整提交 ID 时，工作流会在检出前提示配置。

工作流将编辑器、指定修订的 SDK 和补丁仓检出到同级目录，核对提交身份，用 SDK 清单生成临时路径覆盖，再运行 `cargo check --locked --tests`。本地复现见 [README](../README.md#从源码启动)与[开发和验收说明](开发与验收.md#本地源码覆盖)。真实窗口另按自动验收入口运行。

## 历史 crates.io 配方

[.cargo/registry-sdk.toml](../.cargo/registry-sdk.toml) 记录 Bevy 0.19.1 和 SDK 0.1.0 的已发布配方，适用于对应历史版本。0.20.1 的 registry 检查依赖新的公开 SDK、补丁提交和配套锁文件。远端验证状态以对应提交的 Actions 记录为准。

## 跨运行缓存

GitHub 托管的 Windows Runner 每次运行都会重新创建。工作流使用固定提交的官方 `actions/cache` 分别保存两类缓存：

- Cargo 下载内容位于 `CARGO_HOME`（默认 `~/.cargo`）。两个组合共用 registry 索引、crate 归档和 Git 依赖数据库缓存。
- 编译产物位于 `feathrise/target`。缓存键绑定操作系统、架构、实际 Rust 工具链、检查组合、SDK 和补丁的固定修订，以及锁文件、清单和工作流。Cargo 根据实际输入决定需要重编译的部分。

缓存键不使用编辑器提交 ID，因此只修改 Rust 源码的后续推送可以复用依赖。缓存命中后仍完整执行原有 `cargo check --locked --tests`，不跳过检查，也不更换工具链或依赖来源。下载和编译缓存会在任务成功结束时保存；首次运行、此前任务失败或取消尚未保存缓存，以及 GitHub 缓存过期／淘汰时，仍需重新下载或编译。多个矩阵首次并行启动时可能都没有缓存，不能共享对方尚未保存的内容。

可在 Actions 日志的 `Restore Cargo dependency downloads`、`Restore editor dependency build artifacts` 及其任务结束后的保存步骤查看命中、恢复和上传情况。真正的跨运行加速需在首次成功保存后再次运行确认，本地 YAML 检查不能证明 GitHub 已命中缓存。
