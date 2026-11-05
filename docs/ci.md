# 编辑器 Windows CI

[Windows 工作流](../.github/workflows/windows-check.yml)在推送、Pull Request 或手动触发时，检查宿主、空工程模板和地图作者库的默认与 validation 组合。

工作流默认使用下面已固定修订的公开依赖，首次推送无需先配置仓库变量：

| 可覆盖变量 | 默认值 |
| --- | --- |
| `FEATHRISE_SDK_REPOSITORY` | `hwyrq/feathrise_runtime` |
| `FEATHRISE_SDK_REV` | `a476d2374cdc0383efe25129e0003c8fd2733f01` |
| `FEATHRISE_PATCH_REPOSITORY` | `hwyrq/feathrise_patches` |
| `FEATHRISE_PATCH_REV` | `3c251d3530980b66e49b4748769db36ae9c6bd5a` |

需要验证其他配套修订时，可在 GitHub 仓库的 **Settings → Secrets and variables → Actions → Variables** 设置对应变量；仓库值使用 `owner/repository`，修订必须为完整 40 位提交 ID。仓库和修订应配套调整，未设置或空值继续使用表中的公开默认值。

工作流将编辑器、指定修订的 SDK 和补丁仓检出到同级目录，核对检出身份，在 Runner 临时目录生成 23 项 SDK 包覆盖，并加载补丁仓的八项覆盖，再运行 `cargo check --locked --tests`。它不依赖私有父仓库、本机 `.cargo/local-sdk.toml` 或 SDK 已发布到 crates.io；本地复现步骤见 [README](../README.md#从源码启动)与[完整覆盖配置](开发与验收.md#本地源码覆盖)。检查只证明对应构建组合，不代替真实窗口验收。

## 跨运行缓存

GitHub 托管的 Windows Runner 每次运行都会重新创建。工作流使用固定提交的官方 `actions/cache` 分别保存两类缓存：

- Cargo 下载内容：当前 `CARGO_HOME`（未设置时为 `~/.cargo`）中的 registry 索引、crate 压缩归档和 Git 依赖数据库。两个检查组合共用下载缓存，锁文件变化时可恢复旧缓存并补充新增依赖；解压源码由 Cargo 从归档重新生成。
- 编译产物：编辑器检出目录内的 `feathrise/target`。缓存按操作系统、架构、实际 Rust 工具链、SDK／补丁仓及其固定修订、`default`／`validation` 组合区分；锁文件、Cargo 清单、配置及工作流变化会更新缓存键。相同工具链和源码组合可恢复上一份缓存，由 Cargo 判断哪些产物需要重编译。

缓存键不使用编辑器提交 ID，因此只修改 Rust 源码的后续推送可以复用依赖。缓存命中后仍完整执行原有 `cargo check --locked --tests`，不跳过检查，也不更换工具链或依赖来源。下载和编译缓存会在任务成功结束时保存；首次运行、此前任务失败或取消尚未保存缓存，以及 GitHub 缓存过期／淘汰时，仍需重新下载或编译。两个矩阵首次并行启动时可能都没有缓存，不能共享对方尚未保存的内容。

可在 Actions 日志的 `Restore Cargo dependency downloads`、`Restore editor dependency build artifacts` 及其任务结束后的保存步骤查看命中、恢复和上传情况。真正的跨运行加速需在首次成功保存后再次运行确认，本地 YAML 检查不能证明 GitHub 已命中缓存。
