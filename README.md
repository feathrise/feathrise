# Feathrise（羽立）

基于 Bevy 的独立 3D 编辑器，提供工程管理、地图、地形、水域、植被、模型、材质、动画与文档编辑。工程管理与作者工具位于同一窗口，支持外部原生作者插件。

新建工程保持空内容，模板提供自由相机。工程自己的角色控制、玩法与内容通过工程源码或插件接入。

## 从源码启动

需要 Rust/Cargo；Windows 还需要 MSVC x64/x86 工具与 Windows SDK。链接器使用现有 Rust 工具链中的 `rust-lld`。

当前源码构建使用固定修订的公开 SDK 和补丁仓，通过本机路径覆盖接入，无需等待 SDK 发布到 crates.io。将本仓库检出到 `feathrise/`，在其父目录执行：

```powershell
git clone https://github.com/hwyrq/feathrise_runtime.git
git -C feathrise_runtime checkout --detach a476d2374cdc0383efe25129e0003c8fd2733f01
git clone https://github.com/hwyrq/feathrise_patches.git
git -C feathrise_patches checkout --detach 3c251d3530980b66e49b4748769db36ae9c6bd5a
```

三个目录保持同级：

```text
feathrise/
feathrise_runtime/
feathrise_patches/
```

将[完整的 31 项本地覆盖配置](docs/开发与验收.md#本地源码覆盖)保存为编辑器目录下的 `.cargo/local-sdk.toml`。编辑器自己的 Cargo 配置会读取它；此文件已忽略，不需要其他父仓库配置。保持三个目录的相对位置即可整体搬移。

进入 `feathrise/`，首次获取锁定依赖后启动：

```powershell
cargo fetch --locked
cargo run --locked
```

`.cargo/local-sdk.toml` 负责编辑器本身的编译依赖；`sdk-source.toml` 负责之后新建工程的 SDK 来源，默认本地路径与上面的目录布局一致。公开 CI 使用相同的固定修订，见 [Windows CI](docs/ci.md)。

开发产物为 `target/debug/feathrise.exe`，内部 Cargo 包名为 `feathrise_host`。普通用户可使用编译好的编辑器；创建和运行工程仍需要工程所用的 Rust/Cargo 环境。

当前公开源码不提供 Feathrise 正式 Logo，默认构建无需品牌文件。需要使用自己有权使用的品牌素材时，见[可选自有品牌构建](docs/开发与验收.md#可选自有品牌构建)。

地形 GLB 保存默认不调用 Blender，不生成或改写同名 `.blend`。需要同步时，显式配置外部工具目录，见[可选 Blender 地形同步](docs/开发与验收.md#可选-blender-地形同步)；普通构建不包含这两份 Python 脚本。

## 使用与开发

- [使用说明](docs/使用说明.md)：双模式视口、工程运行与导出、SDK 来源和资源位置。
- [作者插件](docs/作者插件.md)：单文件插件、全局／工程安装及作者 ABI。
- [开发与验收](docs/开发与验收.md)：依赖配置、中性回归和可选外部验收适配器。
- [Windows CI](docs/ci.md)：固定 SDK 修订及默认与 validation 检查。

## 源码结构

```text
src/                宿主、工程管理、作者装配与运行进程生命周期
crates/             作者工具、插件 API 及工程管理库
assets/             内嵌工具图标、空工程模板与链接代理源码
build/              运行资源清单
tests/fixtures/     中性测试素材
docs/               使用、开发与 CI 文档
```

`build.rs` 与 `build/` 是编译输入；`Cargo.lock` 随编辑器源码维护。构建缓存 `target/`、本地验收产物 `.local/` 和本机依赖覆盖均已忽略。

自有源码、工具图标和测试素材采用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE) 双许可，可任选其一。Feathrise 正式 Logo 不在该双许可授权范围内。品牌使用条件与第三方归属见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
