# 编辑器 Windows CI

[Windows 工作流](../.github/workflows/windows-check.yml)在推送、Pull Request 或手动触发时，分别以源码联调和 crates.io 消费检查宿主、空工程模板和地图作者库。两种来源各跑默认与 validation，合计四个组合。

## 源码联调

`source` 组合默认使用下面已固定修订的公开依赖，首次推送无需先配置仓库变量：

| 可覆盖变量 | 默认值 |
| --- | --- |
| `FEATHRISE_SDK_REPOSITORY` | `feathrise/feathrise_runtime` |
| `FEATHRISE_SDK_REV` | `a476d2374cdc0383efe25129e0003c8fd2733f01` |
| `FEATHRISE_PATCH_REPOSITORY` | `feathrise/feathrise_patches` |
| `FEATHRISE_PATCH_REV` | `3c251d3530980b66e49b4748769db36ae9c6bd5a` |

需要验证其他配套修订时，可在 GitHub 仓库的 **Settings → Secrets and variables → Actions → Variables** 设置对应变量；仓库值使用 `owner/repository`，修订必须为完整 40 位提交 ID。仓库和修订应配套调整，未设置或空值继续使用表中的公开默认值。

工作流将编辑器、指定修订的 SDK 和补丁仓检出到同级目录，核对检出身份，在 Runner 临时目录生成 23 项 SDK 包覆盖，并加载补丁仓的八项覆盖，再运行 `cargo check --locked --tests`。它不依赖私有父仓库、本机 `.cargo/local-sdk.toml` 或 SDK 已发布到 crates.io；本地复现步骤见 [README](../README.md#从源码启动)与[完整覆盖配置](开发与验收.md#本地源码覆盖)。检查只证明对应构建组合，不代替真实窗口验收。

## crates.io 消费

`registry` 组合只检出编辑器，不检出 SDK 或补丁仓，也不生成本地路径覆盖。它使用 [.cargo/registry-sdk.toml](../.cargo/registry-sdk.toml) 的八项公开 Git 补丁，与源码矩阵的仓库变量互相独立。SDK 使用已发布的 0.1.0，字体总包使用 0.1.1、分片使用 0.1.0。

仓内 `Cargo.lock` 同时服务本机源码联调，其来源与 registry 不同。此组合先在 Runner 的临时检出执行公开配置的 `cargo fetch`，把 SDK 与补丁切换到公开来源；随后执行 `cargo check --locked --tests`。不把重新解析的锁文件写回仓库，不在编译时再次解析来源。

GitHub 托管 Runner 使用全新检出，不携带开发机目录或本地覆盖文件。依赖解析与固定提交由 Cargo 配置及锁文件处理，不另外运行 Python 来源审计或扫描 Cargo 缓存。

本机复现应使用没有 `.cargo/local-sdk.toml`、不会继承父级 Cargo 配置的独立检出，并沿用已有工具链：

```powershell
cargo --config .cargo/registry-sdk.toml fetch
# validation 组合在 check 命令中追加 --features feathrise_host/validation。
cargo --config .cargo/registry-sdk.toml check --locked -p feathrise_host -p feathrise_game_template -p feathrise_editor_map --tests
```

该矩阵检查公开来源的下载、解析和两组编译，不替代真实窗口、编辑保存、试玩、导出或第三方原生 SDK 的运行验收。本地静态检查和编译结果不代表远端 Actions 已运行；远端状态以对应提交的 Actions 记录为准。

## 跨运行缓存

GitHub 托管的 Windows Runner 每次运行都会重新创建。工作流使用固定提交的官方 `actions/cache` 分别保存两类缓存：

- Cargo 下载内容：当前 `CARGO_HOME`（未设置时为 `~/.cargo`）中的 registry 索引、crate 压缩归档和 Git 依赖数据库。四个检查组合共用下载缓存，锁文件变化时可恢复旧缓存并补充新增依赖；解压源码由 Cargo 从归档重新生成。
- 编译产物：编辑器检出目录内的 `feathrise/target`。缓存按操作系统、架构、实际 Rust 工具链、`source`／`registry` 来源和 `default`／`validation` 组合区分；源码组合额外绑定 SDK／补丁仓及其固定修订，registry 组合绑定公开补丁配方。锁文件、Cargo 清单、配置及工作流变化会更新缓存键。相同工具链和来源组合可恢复上一份缓存，由 Cargo 判断哪些产物需要重编译。

源码组合仍允许恢复此前 `editor-check-v1` 的同身份缓存；registry 组合使用独立的编译缓存，避免两种 SDK 来源互相覆盖。

缓存键不使用编辑器提交 ID，因此只修改 Rust 源码的后续推送可以复用依赖。缓存命中后仍完整执行原有 `cargo check --locked --tests`，不跳过检查，也不更换工具链或依赖来源。下载和编译缓存会在任务成功结束时保存；首次运行、此前任务失败或取消尚未保存缓存，以及 GitHub 缓存过期／淘汰时，仍需重新下载或编译。多个矩阵首次并行启动时可能都没有缓存，不能共享对方尚未保存的内容。

可在 Actions 日志的 `Restore Cargo dependency downloads`、`Restore editor dependency build artifacts` 及其任务结束后的保存步骤查看命中、恢复和上传情况。真正的跨运行加速需在首次成功保存后再次运行确认，本地 YAML 检查不能证明 GitHub 已命中缓存。
