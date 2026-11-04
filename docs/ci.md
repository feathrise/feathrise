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
