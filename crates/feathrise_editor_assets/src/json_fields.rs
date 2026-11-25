//! 从完整验证的 JSON 中定位字符串原文。引用搜索和资产搬移共用字段规则与精确字节范围。

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use std::{cell::Cell, fmt, io::Read, ops::Range};

pub fn asset_path_field(key: &str) -> bool {
    key == "path" || key.ends_with("_path") || key.ends_with("_paths")
}

/// 文件整理和缺失检查共用可写范围。名称、节点路径和任意用户文本仍可用于搜索。
pub fn authored_reference_document(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    name.eq_ignore_ascii_case("scene.glb") || name.ends_with(".json")
        && (name.contains(".scene_") || name.contains(".map_") || name.ends_with("presets.json") || name.ends_with(".preset.json"))
}

pub fn authored_asset_field(key: &str) -> bool {
    let key = key.strip_suffix("_paths").unwrap_or_else(|| key.strip_suffix("_path").unwrap_or(""));
    matches!(key, "asset" | "author_asset" | "model" | "mesh" | "material" | "glb" | "height" | "skybox" |
        "texture" | "source" | "peer_source" | "surface" | "surface_fields" | "vegetation" | "water" | "boundary" | "characters")
        || key.ends_with("_texture") || key.ends_with("_source") || key.ends_with("_array")
}

/// 只识别 glTF 节点上由场景作者保存的摆放 extras。
pub fn placement_node(pointer: &str) -> Option<usize> {
    pointer.strip_prefix("/nodes/")?.strip_suffix("/extras/feathrise_placement")?.parse().ok()
}

/// 数组元素沿用所属字段。调用者可以设置自己的查询预算，或在后台按文件完整处理。
pub fn visit_strings(
    source: &str, pointer_limit: Option<usize>, cancelled: &dyn Fn() -> bool,
    mut visit: impl FnMut(&str, &str, &str, Range<usize>) -> Result<(), String>,
) -> Result<(), String> {
    let position = Cell::new(0);
    let mut input = serde_json::Deserializer::from_reader(Cursor { bytes: source.as_bytes(), position: &position, cancelled });
    let mut walker = Walker { source, position: &position, pointer: String::new(), pointer_limit, visit: &mut visit };
    Seed { walker: &mut walker, key: "", start: 0 }.deserialize(&mut input).map_err(|error| error.to_string())?;
    input.end().map_err(|error| error.to_string())
}

struct Cursor<'a> { bytes: &'a [u8], position: &'a Cell<usize>, cancelled: &'a dyn Fn() -> bool }

impl Read for Cursor<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let at = self.position.get();
        if (at & 4095) == 0 && (self.cancelled)() {
            return Err(std::io::Error::other("引用查询已取消"));
        }
        if buffer.is_empty() || at == self.bytes.len() { return Ok(0); }
        // serde 的 IoRead 单字节预读；不能提前填满 buffer 并把游标误算为当前字符串结尾。
        buffer[0] = self.bytes[at];
        self.position.set(at + 1);
        Ok(1)
    }
}

type FieldVisitor<'a> = dyn FnMut(&str, &str, &str, Range<usize>) -> Result<(), String> + 'a;
struct Walker<'a> {
    source: &'a str, position: &'a Cell<usize>, pointer: String, pointer_limit: Option<usize>, visit: &'a mut FieldVisitor<'a>,
}
struct Seed<'a, 's> { walker: &'a mut Walker<'s>, key: &'a str, start: usize }

impl<'de> DeserializeSeed<'de> for Seed<'_, '_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(mut self, deserializer: D) -> Result<(), D::Error> {
        self.start = self.walker.position.get().saturating_sub(1);
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("有效 JSON 值") }
    fn visit_unit<E: Error>(self) -> Result<(), E> { Ok(()) }
    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> { Ok(()) }
    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> { Ok(()) }
    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> { Ok(()) }
    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> { Ok(()) }
    fn visit_str<E: Error>(self, value: &str) -> Result<(), E> {
        let end = self.walker.position.get();
        let offset = self.walker.source[self.start..end].find('"').ok_or_else(|| E::custom("无法定位 JSON 字符串"))?;
        if self.walker.source.as_bytes().get(end - 1) != Some(&b'"') { return Err(E::custom("JSON 字符串位置无效")); }
        (self.walker.visit)(&self.walker.pointer, self.key, value, self.start + offset..end).map_err(E::custom)
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        while let Some(key) = map.next_key::<String>()? {
            let len = self.walker.pointer.len();
            self.walker.pointer.push('/');
            for ch in key.chars() {
                match ch { '~' => self.walker.pointer.push_str("~0"), '/' => self.walker.pointer.push_str("~1"), ch => self.walker.pointer.push(ch) }
            }
            self.walker.check_limit().map_err(M::Error::custom)?;
            map.next_value_seed(Seed { walker: self.walker, key: &key, start: 0 })?;
            self.walker.pointer.truncate(len);
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        for index in 0usize.. {
            let len = self.walker.pointer.len();
            self.walker.pointer.push_str(&format!("/{index}"));
            self.walker.check_limit().map_err(A::Error::custom)?;
            let next = seq.next_element_seed(Seed { walker: self.walker, key: self.key, start: 0 })?;
            self.walker.pointer.truncate(len);
            if next.is_none() { break; }
        }
        Ok(())
    }
}
impl Walker<'_> {
    fn check_limit(&self) -> Result<(), &'static str> {
        if self.pointer_limit.is_some_and(|limit| self.pointer.len() > limit) { Err("JSON字段路径超过检查预算") } else { Ok(()) }
    }
}
