# Feathrise 第三方与资源说明

Feathrise 自有源码采用根目录 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)，由使用者任选其一，包含本仓维护的模板源码。自有工具图标和测试素材适用相同双许可；当前公开源码不提供 Feathrise 正式 Logo，既有正式标识副本适用下述独立条件，不包含在源码双许可授权内。模板授权不替使用者选择其工程全部代码和素材的许可证。

## 工具图标与测试素材

以下自有资源递归适用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)，使用者可任选其一：

| 源码路径 | 内容 |
| --- | --- |
| [assets/icons/editor/](assets/icons/editor/) | 22 张工具、目录树及网格状态 PNG 图标 |
| [tests/fixtures/](tests/fixtures/) | 模型与贴图验收资源及说明 |
| [crates/feathrise_editor_map/tests/fixtures/](crates/feathrise_editor_map/tests/fixtures/) | 动画、蒙皮、资产引用和植被模型，以及建筑、院落、地表、植被、水域和笔刷测试参数 |

本项目使用这些素材进行测试，普通新建工程不会复制它们；这项使用方式不限制使用者按双许可复用素材。

## Feathrise 品牌使用条件

当前公开源码不提供 Feathrise 正式 Logo（`feathrise-mark.png` 与 `feathrise.ico`），默认构建不需要这些文件。使用自己有权使用的品牌素材时，可按[可选自有品牌构建](docs/开发与验收.md#可选自有品牌构建)显式提供；该构建入口不授予 Feathrise 正式标识的使用权。

此前已合法取得的正式标识副本继续按取得时适用的 `LicenseRef-Feathrise-Brand` 授权处理：可为构建原版 Feathrise 复制、保留，并随原版源码或构建产物原样分发，分发时须保留本品牌使用条件。已分发的官方原版程序中所含正式标识仍适用这项品牌专用许可，可按相同条件原样分发。

该授权仅用于原版编辑器的构建与原样分发。修改标识，或在修改版、衍生产品及其他产品中使用这些正式标识，须另行取得作者授权；修改版和衍生产品可使用自有标识，并按源码及其他资源各自的许可使用和分发。

停止在当前公开源码提供 Logo 不追溯撤回已取得副本的原有授权，也不表示已收回全部旧副本或清除远端历史。

本授权不授予以 Feathrise／羽立的名称或标识暗示作者认可、官方背书或官方发布的权利。未明确授予的品牌资源权利由作者保留。

## 第三方组件

- 默认中文字体通过 [feathrise_runtime](https://github.com/feathrise/feathrise_runtime) 的 [feathrise_font](https://crates.io/crates/feathrise_font) 提供。字体为未修改的 Noto Sans CJK SC 可变字体，遵循 OFL-1.1；字体包的原 OFL 保留 Adobe 版权及 Reserved Font Name 声明。发行含该字体的编辑器时仍须附带这份许可。
- 编辑器使用 Bevy、公共 SDK 及 Cargo 声明的其他依赖。Cargo 获取的依赖遵循各自原许可；源码使用与程序发行应按实际提供的组件保留对应声明和原文。
- 修改版 Rust 依赖和 FidelityFX 原生补丁材料位于独立 [feathrise_patches](https://github.com/feathrise/feathrise_patches/tree/3c251d3530980b66e49b4748769db36ae9c6bd5a) 仓，保留各组件原许可与修改说明。SDK 的 [第三方说明](https://github.com/feathrise/feathrise_runtime/blob/a476d2374cdc0383efe25129e0003c8fd2733f01/THIRD_PARTY_NOTICES.md) 和 FSR 原生包锁定清单说明对应身份；使用原生包时，须保留其随包许可和修改说明。
- NVIDIA、Microsoft 等外部运行库按实际分发版本处理，Rust 封装的许可不覆盖这些厂商二进制。

本仓不保存开发仓库的全特性依赖审计快照。发行维护流程会提供适用的许可原文及逐包归属表；源码授权和来源说明不代替最终二进制分发范围的核对。
