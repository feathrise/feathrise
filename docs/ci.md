# 编辑器 Windows CI

[Windows 工作流](../.github/workflows/windows-check.yml)检查 Bevy 0.20.1 的宿主、空工程模板和场景作者库，要求 Rust 1.97.1 或更新版本。每种选定来源均检查默认和 `validation` 两个组合。

| 触发方式 | SDK 来源 |
| --- | --- |
| 普通分支推送、Pull Request | 配套源码 `source` |
| 版本标签推送 | 配套源码 `source` 与 crates.io `registry` |
| 手动运行 | `sdk_source` 可选 `source`、`registry`、`both`，默认 `source` |

手动选择优先于当前分支或标签。开发代码可以先于公开包增加接口，因此日常源码检查通过只证明这组固定源码可编译；版本标签与显式 registry 检查仍核对实际已发布依赖，缺少接口时应失败。

## 源码联调

工作流使用固定的场景 SDK 和 Bevy 0.20.1 补丁提交。源码检查先核对提交，再将 `feathrise_runtime`、`feathrise_scene_content`、`feathrise_terrain` 和 `feathrise_editor_protocol` 的包身份及版本与编辑器锁文件逐一比对。更新配套 SDK 提交时同步更新固定修订，也可在 GitHub 仓库的 **Settings → Secrets and variables → Actions → Variables** 覆盖。

| 变量 | 值 |
| --- | --- |
| `FEATHRISE_SDK_REPOSITORY` | `feathrise/feathrise_runtime` |
| `FEATHRISE_SDK_REV` | 可选，配套 SDK 的完整 40 位提交 ID |
| `FEATHRISE_PATCH_REPOSITORY` | `feathrise/feathrise_patches` |
| `FEATHRISE_PATCH_REV` | 可选，配套补丁的完整 40 位提交 ID |

覆盖来源时使用 `owner/repository` 格式和对应的完整提交 ID。工作流在检出前检查格式，检出后核对实际提交。

工作流将编辑器、指定修订的 SDK 和补丁仓检出到同级目录，核对提交身份，用 SDK 清单生成临时路径覆盖，再运行 `cargo check --locked --tests`。本地复现见 [README](../README.md#从源码启动)与[开发和验收说明](开发与验收.md#本地源码覆盖)。真实窗口另按自动验收入口运行。

## crates.io 配方

显式 registry 检查使用与编辑器配套的已发布 SDK 包和固定的 Bevy 0.20.1 补丁。当前场景开发使用配套源码，公开包验收需要已发布的场景接口。历史发行版的配方随各自 Git 标签保存。 [公开补丁配置](../.cargo/registry-sdk.toml)

## 跨运行缓存

GitHub 托管的 Windows Runner 每次运行都会重新创建。工作流使用固定提交的官方 `actions/cache` 分别保存两类缓存：

- Cargo 下载内容位于 `CARGO_HOME`（默认 `~/.cargo`）。两个组合共用 registry 索引、crate 归档和 Git 依赖数据库缓存。
- 编译产物位于 `feathrise/target`。缓存键绑定操作系统、架构、实际 Rust 工具链、检查组合、SDK 和补丁的固定修订，以及锁文件、清单和工作流。Cargo 根据实际输入决定需要重编译的部分。

缓存键不使用编辑器提交 ID，因此只修改 Rust 源码的后续推送可以复用依赖。缓存命中后仍完整执行原有 `cargo check --locked --tests`，不跳过检查，也不更换工具链或依赖来源。下载和编译缓存会在任务成功结束时保存；首次运行、此前任务失败或取消尚未保存缓存，以及 GitHub 缓存过期／淘汰时，仍需重新下载或编译。多个矩阵首次并行启动时可能都没有缓存，不能共享对方尚未保存的内容。

可在 Actions 日志的 `Restore Cargo dependency downloads`、`Restore editor dependency build artifacts` 及其任务结束后的保存步骤查看命中、恢复和上传情况。真正的跨运行加速需在首次成功保存后再次运行确认，本地 YAML 检查不能证明 GitHub 已命中缓存。
