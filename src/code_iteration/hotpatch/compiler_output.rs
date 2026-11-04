//! dx 0.7.9 后台产物边界：只解析并冻结构建记录和跳表，不执行工具、读文件或采用代码。
//!
//! 官方 CLI 在 --json-output 下把 StructuredOutput 放进末条 tracing 记录的 json 字符串；
//! stdin 热补丁入口只接受 BuildsFinished.client。源码、会话和文件内容身份仍由调用方绑定。

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::{fmt, path::{Component, Path, PathBuf}, sync::Arc};

pub(crate) const MAX_COMPILER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct CompilerArtifacts {
    pub root: PathBuf,
    pub executable: PathBuf,
    /// Windows 原生跳表从初始 EXE 旁的同名 PDB 建立；调用方须一起冻结并校验。
    pub pdb: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct FatBuildOutput {
    pub artifacts: CompilerArtifacts,
    /// 可直接作为 dx tools hotpatch 的 stdin；不包括 BuildsFinished 或 tracing 包装。
    pub client_json: Arc<Vec<u8>>,
}

impl FatBuildOutput {
    /// 调用方完成 EXE/PDB 独占复制后，用实际冻结路径重定向基线；不改变 rustc/link 参数。
    pub(crate) fn client_for_frozen_executable(&self, executable: &Path) -> Result<Arc<Vec<u8>>, String> {
        checked_path(executable.to_str().ok_or("冻结 EXE 路径不是 UTF-8")?, true)?;
        let mut client = parse_json(&self.client_json)?;
        client["exe"] = serde_json::to_value(executable).map_err(|error| error.to_string())?;
        freeze(&client)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HotpatchOutput {
    pub artifacts: CompilerArtifacts,
    pub library: PathBuf,
    pub pdb: PathBuf,
    /// 完整跳表只进入受控产物包，不能复制到普通 IPC 元信息或 UI 状态。
    pub jump_table_json: Arc<Vec<u8>>,
    pub entry_count: usize,
    pub aslr_reference: u64,
    pub new_base_address: u64,
    pub ifunc_count: u64,
    windows_sources_normalized: bool,
}

impl HotpatchOutput {
    /// dx Windows 原表是旧 EXE RVA→新 DLL RVA，CLI 输入却是运行中 main 的 VA。
    /// 协议固定传绝对源地址：宿主只给源键加一次旧模块基址，SDK 的旧位移因此为0；
    /// 目标仍保留 DLL RVA，由 Subsecond 装载后依据实际 DLL main 计算新位移。
    pub(crate) fn normalize_windows_sources(&mut self, runtime_main: u64) -> Result<(), String> {
        let old_main_rva = self.aslr_reference;
        if self.windows_sources_normalized || old_main_rva == 0 || old_main_rva > u64::from(u32::MAX)
            || self.new_base_address == 0 || self.new_base_address > u64::from(u32::MAX) || self.ifunc_count != 0 {
            return Err("Windows 原始补丁必须提供有效 main RVA，且不能重复重定位或包含 IFUNC".into());
        }
        let old_image_base = runtime_main.checked_sub(old_main_rva).ok_or("运行 main VA 小于原始 main RVA")?;
        let mut table = parse_json(&self.jump_table_json)?;
        if table["aslr_reference"].as_u64() != Some(old_main_rva)
            || table["new_base_address"].as_u64() != Some(self.new_base_address) {
            return Err("原始补丁 main 字段与受检回执不一致".into());
        }
        let original = object(&table["map"], "Windows jump_table.map")?;
        if original.get(&old_main_rva.to_string()).and_then(Value::as_u64) != Some(self.new_base_address) {
            return Err("Windows 原始跳表缺少旧 main RVA 到新 main RVA 的映射".into());
        }
        let mut relocated = Map::new();
        for (source, target) in original {
            let source = source.parse::<u64>().map_err(|_| "原始补丁源 RVA 不是整数")?;
            let target_rva = target.as_u64().ok_or("原始补丁目标 RVA 不是整数")?;
            if source > u64::from(u32::MAX) || target_rva > u64::from(u32::MAX) {
                return Err("Windows 原始跳表地址超出 PE RVA 范围".into());
            }
            let address = source.checked_add(old_image_base).ok_or("Windows 补丁源地址重定位溢出")?;
            relocated.insert(address.to_string(), target.clone());
        }
        table["map"] = Value::Object(relocated);
        table["aslr_reference"] = Value::from(runtime_main);
        let bytes = freeze(&table)?;
        // 完整检查后才替换本地回执；失败不留下半份已重定位的候选。
        self.jump_table_json = bytes;
        self.aslr_reference = runtime_main;
        self.windows_sources_normalized = true;
        Ok(())
    }
}

pub(crate) fn parse_fat_output(output: &[u8]) -> Result<FatBuildOutput, String> {
    let result = result_record(output)?;
    let body = variant(&result, "BuildsFinished")?;
    exact_fields(body, &["client", "server"], "BuildsFinished")?;
    let client = body.get("client").ok_or("dx fat 构建缺少 client")?;
    let artifacts = compiler_artifacts(client)?;
    if let Some(server) = body.get("server").filter(|server| !server.is_null()) {
        compiler_artifacts(server)?;
    }
    Ok(FatBuildOutput { artifacts, client_json: freeze(client)? })
}

pub(crate) fn parse_hotpatch_output(output: &[u8]) -> Result<HotpatchOutput, String> {
    let result = result_record(output)?;
    let body = variant(&result, "Hotpatch")?;
    exact_fields(body, &["jump_table", "artifacts"], "Hotpatch")?;
    let artifacts = compiler_artifacts(&body["artifacts"])?;
    let table = object(&body["jump_table"], "jump_table")?;
    exact_fields(table, &["lib", "map", "aslr_reference", "new_base_address", "ifunc_count"], "jump_table")?;
    let library = checked_path(string(table, "lib")?, true)?;
    let map = object(&table["map"], "jump_table.map")?;
    if map.is_empty() { return Err("dx 补丁跳表没有函数入口".into()); }
    for (source, target) in map {
        let address = source.parse::<u64>().map_err(|_| "跳表来源地址不是 u64")?;
        if address.to_string() != *source || target.as_u64().is_none() {
            return Err("跳表地址必须使用精确、规范的 u64 整数".into());
        }
    }
    let integer = |name: &str| table[name].as_u64().ok_or_else(|| format!("jump_table.{name} 不是 u64"));
    Ok(HotpatchOutput {
        pdb: library.with_extension("pdb"), library, artifacts,
        jump_table_json: freeze(&body["jump_table"] )?, entry_count: map.len(),
        aslr_reference: integer("aslr_reference")?, new_base_address: integer("new_base_address")?,
        ifunc_count: integer("ifunc_count")?,
        windows_sources_normalized: false,
    })
}

fn compiler_artifacts(value: &Value) -> Result<CompilerArtifacts, String> {
    let fields = object(value, "artifacts")?;
    exact_fields(fields, &["path", "exe", "rustc_args", "rustc_envs", "link_args", "assets"], "artifacts")?;
    for name in ["rustc_args", "link_args"] {
        let values = fields[name].as_array().ok_or_else(|| format!("artifacts.{name} 不是数组"))?;
        if values.iter().any(|value| value.as_str().is_none_or(|value| value.contains('\0'))) {
            return Err(format!("artifacts.{name} 包含非字符串或 NUL"));
        }
    }
    let envs = fields["rustc_envs"].as_array().ok_or("artifacts.rustc_envs 不是数组")?;
    if envs.iter().any(|entry| entry.as_array().is_none_or(|pair| pair.len() != 2
        || pair.iter().any(|part| part.as_str().is_none_or(|part| part.contains('\0'))))) {
        return Err("artifacts.rustc_envs 必须是字符串键值对数组".into());
    }
    if !fields["assets"].is_array() { return Err("artifacts.assets 不是数组".into()); }
    let executable = checked_path(string(fields, "exe")?, true)?;
    Ok(CompilerArtifacts { root: checked_path(string(fields, "path")?, false)?,
        pdb: executable.with_extension("pdb"), executable })
}

fn result_record(output: &[u8]) -> Result<Value, String> {
    if output.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("dx 输出超过 8 MiB 解析预算".into()); }
    let text = std::str::from_utf8(output).map_err(|_| "dx 输出不是完整 UTF-8")?;
    // 同时接受调用方保存的裸结果或官方 JSON 行日志；末行坏了就失败，不倒找旧成功。
    let record = match parse_json(output) {
        Ok(value) => value,
        Err(_) => parse_json(text.lines().rev().find(|line| !line.trim().is_empty())
            .ok_or("dx 没有最终结果")?.as_bytes())?,
    };
    let fields = object(&record, "dx 最终记录")?;
    if let Some(inner) = fields.get("json") {
        if fields.keys().any(|key| !matches!(key.as_str(), "timestamp" | "level" | "json")) {
            return Err("dx tracing 结果出现未知或混合包装字段".into());
        }
        let text = inner.as_str().ok_or("dx tracing.json 不是字符串")?;
        return parse_json(text.as_bytes());
    }
    Ok(record)
}

fn variant<'a>(result: &'a Value, expected: &str) -> Result<&'a Map<String, Value>, String> {
    let fields = object(result, "dx StructuredOutput")?;
    if fields.len() != 1 || !fields.contains_key(expected) {
        return Err(format!("dx 最终结果不是预期的 {expected}，未采用产物"));
    }
    object(&fields[expected], expected)
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| format!("{label} 不是 JSON 对象"))
}

fn string<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    fields.get(name).and_then(Value::as_str).ok_or_else(|| format!("dx 产物缺少字符串字段 {name}"))
}

fn exact_fields(fields: &Map<String, Value>, expected: &[&str], label: &str) -> Result<(), String> {
    if fields.len() != expected.len() || expected.iter().any(|name| !fields.contains_key(*name)) {
        return Err(format!("{label} 字段不符合固定 dx 0.7.9 合同"));
    }
    Ok(())
}

fn checked_path(value: &str, file: bool) -> Result<PathBuf, String> {
    if value.is_empty() || value.chars().any(char::is_control)
        || value.split(['/', '\\']).any(|part| matches!(part, "." | "..")) {
        return Err("dx 产物路径为空、含控制字符或相对跳转".into());
    }
    let path = Path::new(value);
    if !path.is_absolute() || (file && (value.ends_with(['/', '\\']) || path.file_name().is_none())) {
        return Err("dx 产物必须使用明确的绝对路径".into());
    }
    for component in path.components() {
        match component {
            Component::ParentDir | Component::CurDir => return Err("dx 产物路径包含相对跳转".into()),
            Component::Normal(name) => {
                let name = name.to_str().ok_or("dx 产物路径不是 UTF-8")?;
                #[cfg(windows)]
                if name.ends_with([' ', '.']) || name.contains(['<', '>', ':', '"', '|', '?', '*']) {
                    return Err("dx 产物包含非法 Windows 路径段".into());
                }
                let _ = name;
            }
            _ => (),
        }
    }
    Ok(path.to_owned())
}

fn freeze(value: &Value) -> Result<Arc<Vec<u8>>, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("dx 冻结产物记录超过 8 MiB".into()); }
    Ok(Arc::new(bytes))
}

fn parse_json(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("dx JSON 超过 8 MiB".into()); }
    serde_json::from_slice::<UniqueValue>(bytes).map(|value| value.0)
        .map_err(|error| format!("dx JSON 无效或包含重复字段：{error}"))
}

/// Value 默认覆盖同名键；跳表地址与构建字段必须拒绝重复，不能让后者偷偷替换前者。
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Unique;
        impl<'de> Visitor<'de> for Unique {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str("没有重复键的 JSON") }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Bool(value))) }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Number(value.into()))) }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Number(value.into()))) }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Number::from_f64(value).map(|value| UniqueValue(Value::Number(value))).ok_or_else(|| E::custom("非有限数字"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> { Ok(UniqueValue(Value::String(value.into()))) }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> { Ok(UniqueValue(Value::String(value))) }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Null)) }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> { Ok(UniqueValue(Value::Null)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? { values.push(value.0); }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(name) = map.next_key::<String>()? {
                    if values.contains_key(&name) { return Err(de::Error::custom(format!("重复字段 {name}"))); }
                    values.insert(name, map.next_value::<UniqueValue>()?.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Unique)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn artifacts() -> Value {
        let root = std::env::temp_dir().join("dx-compiler-output-fixture");
        json!({"path":root,"exe":root.join("game.exe"),"rustc_args":["rustc","--crate-name","game"],
            "rustc_envs":[["CARGO_PKG_NAME","different-package"]],"link_args":["game.obj"],"assets":[]})
    }
    fn fat() -> Value { json!({"BuildsFinished":{"client":artifacts(),"server":null}}) }
    fn patch() -> Value { json!({"Hotpatch":{"artifacts":artifacts(),"jump_table":{
        "lib":std::env::temp_dir().join("dx-compiler-output-fixture/patch.dll"),
        "map":{"9007199254740993":18446744073709551615_u64},"aslr_reference":9007199254740993_u64,
        "new_base_address":18446744073709551615_u64,"ifunc_count":0}}}) }

    #[test]
    fn official_tracing_wrapper_freezes_only_client_and_preserves_bin_metadata() {
        let wrapped = json!({"timestamp":"0.001s","level":"INFO","json":fat().to_string()});
        let stdout = format!("普通诊断行\n{}\n", wrapped);
        let parsed = parse_fat_output(stdout.as_bytes()).unwrap();
        let client: Value = serde_json::from_slice(&parsed.client_json).unwrap();
        assert_eq!(client, artifacts());
        assert_eq!(parsed.artifacts.pdb, parsed.artifacts.executable.with_extension("pdb"));
        assert!(parsed.artifacts.root.is_absolute());
    }
    #[test]
    fn jump_addresses_keep_all_u64_bits_without_a_floating_point_roundtrip() {
        let parsed = parse_hotpatch_output(patch().to_string().as_bytes()).unwrap();
        assert_eq!(parsed.aslr_reference, 9007199254740993);
        assert_eq!(parsed.new_base_address, u64::MAX);
        assert_eq!(parsed.ifunc_count, 0);
        assert_eq!(parsed.entry_count, 1);
        assert_eq!(parsed.pdb, parsed.library.with_extension("pdb"));
        let frozen: Value = serde_json::from_slice(&parsed.jump_table_json).unwrap();
        assert_eq!(frozen["map"]["9007199254740993"].as_u64(), Some(u64::MAX));
    }

    fn windows_patch() -> Value {
        let mut value = patch();
        value["Hotpatch"]["jump_table"]["map"] = json!({"4096":8192,"4352":8448});
        value["Hotpatch"]["jump_table"]["aslr_reference"] = json!(4096);
        value["Hotpatch"]["jump_table"]["new_base_address"] = json!(8192);
        value
    }

    #[test]
    fn windows_source_normalization_is_equivalent_to_subsecond_and_keeps_new_dll_rvas() {
        let mut parsed = parse_hotpatch_output(windows_patch().to_string().as_bytes()).unwrap();
        let runtime_main = 0x7ff6_1000_1000_u64;
        let loaded_dll_main = 0x7ffa_2000_2000_u64;
        parsed.normalize_windows_sources(runtime_main).unwrap();
        let normalized: Value = serde_json::from_slice(&parsed.jump_table_json).unwrap();
        assert_eq!(parsed.aslr_reference, runtime_main);
        assert_eq!(normalized["aslr_reference"].as_u64(), Some(runtime_main));
        assert_eq!(parsed.new_base_address, 8192);
        assert_eq!(parsed.entry_count, 2);
        for (old_rva, new_rva) in [(4096_u64,8192_u64),(4352,8448)] {
            let native_source = old_rva + (runtime_main - 4096);
            let native_target = new_rva + (loaded_dll_main - 8192);
            let received_rva = normalized["map"][native_source.to_string()].as_u64().unwrap();
            assert_eq!(received_rva, new_rva);
            assert_eq!(received_rva + (loaded_dll_main - parsed.new_base_address), native_target);
        }
        assert!(parsed.normalize_windows_sources(runtime_main).is_err());
    }

    #[test]
    fn windows_raw_main_rva_bounds_and_checked_relocation_reject_without_partial_changes() {
        for kind in 0..5 {
            let mut value = windows_patch();
            match kind {
                0 => value["Hotpatch"]["jump_table"]["aslr_reference"] = json!(0),
                1 => value["Hotpatch"]["jump_table"]["new_base_address"] = json!(1_u64 << 32),
                2 => value["Hotpatch"]["jump_table"]["map"]["4096"] = json!(8193),
                3 => value["Hotpatch"]["jump_table"]["map"]["4352"] = json!(1_u64 << 32),
                _ => (),
            }
            let mut parsed = parse_hotpatch_output(value.to_string().as_bytes()).unwrap();
            let before = parsed.jump_table_json.clone();
            let runtime = if kind == 4 { 4095 } else { 0x7ff6_1000_1000 };
            assert!(parsed.normalize_windows_sources(runtime).is_err());
            assert_eq!(parsed.jump_table_json, before);
        }
        let mut overflow = parse_hotpatch_output(windows_patch().to_string().as_bytes()).unwrap();
        assert!(overflow.normalize_windows_sources(u64::MAX).is_err());
        let mut low_address = parse_hotpatch_output(windows_patch().to_string().as_bytes()).unwrap();
        low_address.normalize_windows_sources(8192).unwrap();
        assert!(low_address.normalize_windows_sources(8192).is_err(), "重复转换不能依赖地址是否大于u32来识别");
    }
    #[test]
    fn wrong_kind_invalid_final_line_and_duplicate_fields_never_reuse_old_success() {
        assert!(parse_fat_output(patch().to_string().as_bytes()).is_err());
        assert!(parse_hotpatch_output(fat().to_string().as_bytes()).is_err());
        assert!(parse_fat_output(format!("{}\n结果损坏\n", fat()).as_bytes()).is_err());
        assert!(parse_json(br#"{"x":1,"x":2}"#).is_err());
        assert!(parse_json(br#"{"map":{"1":2,"1":3}}"#).is_err());
    }
    #[test]
    fn relative_parent_control_and_non_integer_patch_inputs_are_rejected() {
        for path in ["relative/game.exe", "../game.exe", "bad\0game.exe"] {
            let mut value = fat(); value["BuildsFinished"]["client"]["exe"] = json!(path);
            assert!(parse_fat_output(value.to_string().as_bytes()).is_err());
        }
        let root = std::env::temp_dir();
        assert!(checked_path(&format!("{}/../game.exe", root.display()), true).is_err());
        for number in [json!(-1), json!(1.5), json!("123")] {
            let mut value = patch(); value["Hotpatch"]["jump_table"]["aslr_reference"] = number;
            assert!(parse_hotpatch_output(value.to_string().as_bytes()).is_err());
        }
    }
    #[test]
    fn oversized_output_and_frozen_base_retargeting_have_explicit_boundaries() {
        assert!(parse_fat_output(&vec![b' '; MAX_COMPILER_OUTPUT_BYTES + 1]).is_err());
        let parsed = parse_fat_output(fat().to_string().as_bytes()).unwrap();
        let destination = std::env::temp_dir().join("dx-frozen-base/game.exe");
        let client: Value = serde_json::from_slice(&parsed.client_for_frozen_executable(&destination).unwrap()).unwrap();
        assert_eq!(client["exe"], json!(destination));
        assert_eq!(client["rustc_args"], artifacts()["rustc_args"]);
        assert_eq!(client["rustc_envs"], artifacts()["rustc_envs"]);
        assert!(parsed.client_for_frozen_executable(Path::new("relative.exe")).is_err());
    }
}
