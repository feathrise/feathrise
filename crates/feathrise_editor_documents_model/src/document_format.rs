//! 资产浏览、文档IO与预览共享的文本格式白名单；不能按内容猜测并开放任意文件编辑。

use crate::types::DocumentFormat;
use std::path::Path;

pub fn document_format(path: &str) -> Option<DocumentFormat> {
    let path = Path::new(path);
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    // 常见无扩展名说明与配置也可查看；模型、图片、字体等格式不会被文本入口抢占。
    match name.as_str() {
        "readme" | "license" | "licence" | "notice" => return Some(DocumentFormat::Text),
        ".env" | ".gitignore" | ".gitattributes" | ".editorconfig" => {
            return Some(DocumentFormat::Config);
        }
        "makefile" | "dockerfile" => return Some(DocumentFormat::Code),
        _ => (),
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "md" | "markdown" => DocumentFormat::Markdown,
        "txt" | "text" | "log" | "csv" | "tsv" => DocumentFormat::Text,
        "json" | "json5" | "ron" | "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "config"
        | "env" | "xml" | "mtl" | "yarnproject" => DocumentFormat::Config,
        "rs" | "py" | "lua" | "js" | "ts" | "jsx" | "tsx" | "mjs" | "cjs" | "c" | "cpp" | "cc"
        | "cxx" | "h" | "hpp" | "cs" | "java" | "go" | "sh" | "ps1" | "bat" | "cmd" | "html"
        | "htm" | "css" | "scss" | "wgsl" | "glsl" | "hlsl" | "vert" | "frag" | "geom" | "comp"
        | "tesc" | "tese" | "metal" | "yarn" => DocumentFormat::Code,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_document_formats_accept_common_sources_configs_and_named_text_files() {
        for (path, expected) in [
            ("文档/说明.MarkDown", DocumentFormat::Markdown),
            ("data/table.TSV", DocumentFormat::Text),
            ("folder.with.dot/README", DocumentFormat::Text),
            ("licenses/LICENSE", DocumentFormat::Text),
            ("settings/config.JSON5", DocumentFormat::Config),
            ("settings/default.RON", DocumentFormat::Config),
            ("settings/test.yml", DocumentFormat::Config),
            ("dialogue/game.yarnproject", DocumentFormat::Config),
            ("settings/.env", DocumentFormat::Config),
            ("settings/.gitignore", DocumentFormat::Config),
            ("shaders/test.WGSL", DocumentFormat::Code),
            ("shaders/test.frag", DocumentFormat::Code),
            ("scripts/init.PS1", DocumentFormat::Code),
            ("scripts/ui.TSX", DocumentFormat::Code),
            ("dialogue/test.yarn", DocumentFormat::Code),
            ("page/index.html", DocumentFormat::Code),
            ("build/Makefile", DocumentFormat::Code),
        ] {
            assert_eq!(document_format(path), Some(expected), "{path}");
        }
    }

    #[test]
    fn asset_document_formats_do_not_reinterpret_binary_or_special_preview_files() {
        for path in [
            "模型.GLTF",
            "模型.glb",
            "图片.png",
            "纹理.ktx2",
            "高度.r16",
            "数据.bin",
            "程序.exe",
            "字体.ttf",
            "音乐.ogg",
            "文档.pdf",
            "notes.md.png",
            "script.rs.bin",
            "unknown.asset",
            "readme.png",
            "folder.rs/unknown",
        ] {
            assert_eq!(document_format(path), None, "{path}");
        }
    }
}
