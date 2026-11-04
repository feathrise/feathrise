# 正式作者验收夹具

`scene/probe.glb` 与 `scene/albedo.png` 从原 `feathrise/guest/assets/scene/` 逐字迁入。它们是既有中性模型与贴图，只在显式 `--native-report` 创建的自有验收工程中复制，用于真实模型/材质/贴图采用和热更新检查。普通新建工程保持空内容，不复制这些文件。

本目录自有测试素材采用 [MIT](../../LICENSE-MIT) 或 [Apache-2.0](../../LICENSE-APACHE)，由使用者任选其一；许可覆盖本目录及其子目录。
