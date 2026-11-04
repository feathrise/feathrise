//! 后台纯语法兼容门：只擦除显式登记的标量算式函数体，其余 Rust 语义形状必须保持不变。
//! 这不是通用 ABI 分析；未知语法、属性宏、布局或注册变化全部交回完整构建／重启链。

use quote::ToTokens;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use syn::{BinOp, Expr, Item, ItemFn, Lit, ReturnType, Stmt, Type, UnOp};

const MAX_FUNCTIONS: usize = 8;
const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_EXPRESSION_DEPTH: usize = 32;

/// 只接受 Cargo 显式 opt-in 的函数名单；路径是工程内规范的相对 Rust 源码路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Contract {
    functions: BTreeMap<String, BTreeSet<String>>,
}

impl Contract {
    /// 未启用返回 None；启用但合同不可证明有效时给出重启原因，不能降级成已兼容。
    pub(crate) fn parse(manifest: &str) -> Result<Option<Self>, String> {
        if manifest.len() > MAX_MANIFEST_BYTES {
            return Err("热更新合同清单超过 1MiB，需完整构建并重启".into());
        }
        let manifest: toml::Value = toml::from_str(manifest)
            .map_err(|error| format!("热更新合同清单无法解析，需重启：{error}"))?;
        let Some(settings) = manifest.get("package")
            .and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("feathrise"))
            .and_then(|value| value.get("hotpatch")) else { return Ok(None); };
        let settings = settings.as_table().ok_or("热更新合同必须是表，需完整构建并重启")?;
        match settings.get("enabled") {
            None | Some(toml::Value::Boolean(false)) => return Ok(None),
            Some(toml::Value::Boolean(true)) => (),
            _ => return Err("热更新 enabled 必须是显式布尔值，需完整构建并重启".into()),
        }
        if settings.keys().any(|key| !matches!(key.as_str(), "enabled" | "functions")) {
            return Err("热更新合同包含本阶段不支持的字段，需完整构建并重启".into());
        }
        let entries = settings.get("functions").and_then(toml::Value::as_array)
            .ok_or("热更新 functions 必须是字符串数组，需完整构建并重启")?;
        if entries.is_empty() || entries.len() > MAX_FUNCTIONS {
            return Err("热更新首期要求显式登记 1 至 8 个函数，需完整构建并重启".into());
        }
        let mut functions = BTreeMap::<String, BTreeSet<String>>::new();
        for entry in entries {
            let selector = entry.as_str().ok_or("热更新函数条目必须是字符串，需重启")?;
            if selector.len() > 1024 {
                return Err("热更新函数条目过长，需完整构建并重启".into());
            }
            let (path, name) = selector.rsplit_once("::")
                .ok_or("热更新函数须使用 src/file.rs::function 格式，需重启")?;
            if !source_path(path) || syn::parse_str::<syn::Ident>(name).is_err() {
                return Err(format!("热更新函数路径或名称不受支持：{selector}；需完整构建并重启"));
            }
            if !functions.entry(path.to_owned()).or_default().insert(name.to_owned()) {
                return Err(format!("热更新函数重复登记：{selector}；需完整构建并重启"));
            }
        }
        Ok(Some(Self { functions }))
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(String::as_str)
    }

    pub(crate) fn functions_for(&self, path: &str) -> Option<&BTreeSet<String>> {
        self.functions.get(path)
    }
}

fn source_path(path: &str) -> bool {
    path.ends_with(".rs") && !path.starts_with('/')
        && !path.contains(['\\', ':']) && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    pub(crate) digest: String,
    pub(crate) functions: Vec<String>,
}

/// 调用方还须把此摘要放回完整构建输入集合比较，不能只比较名单中的几个源码文件。
/// ToTokens 忽略普通注释／空白，保留文档属性、签名、可见性及文件中全部其他 Rust tokens。
pub(crate) fn shape(contract: &Contract, path: &str, source: &str) -> Result<Shape, String> {
    let wanted = contract.functions_for(path)
        .ok_or_else(|| format!("源码 {path} 未在热更新合同登记，需完整构建并重启"))?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!("热更新源码 {path} 超过 64KiB，需完整构建并重启"));
    }
    let mut file = syn::parse_file(source)
        .map_err(|error| format!("热更新源码 {path} 语法未知或未完成，需完整构建：{error}"))?;
    let mut found = BTreeSet::new();
    for item in &mut file.items {
        if matches!(item, Item::Verbatim(_)) {
            return Err(format!("热更新源码 {path} 含未识别的顶层语法，需重启"));
        }
        let Item::Fn(function) = item else { continue; };
        let name = function.sig.ident.to_string();
        if !wanted.contains(&name) { continue; }
        if !found.insert(name.clone()) {
            return Err(format!("热更新函数 {path}::{name} 有多个定义，需完整构建并重启"));
        }
        scalar_function(function).map_err(|reason| format!("{path}::{name} {reason}；需完整构建并重启"))?;
        // 只有已通过完整约束的 body 可擦除；签名、属性与其余条目完整进入形状摘要。
        function.block = Box::new(syn::parse_quote!({ 0 }));
    }
    if let Some(missing) = wanted.difference(&found).next() {
        return Err(format!("热更新函数 {path}::{missing} 不是唯一顶层函数或已缺失，需重启"));
    }
    let mut digest = Sha256::new();
    digest.update(b"feathrise-scalar-function-shape-v1\0");
    digest.update(path.as_bytes());
    digest.update([0]);
    digest.update(file.into_token_stream().to_string().as_bytes());
    Ok(Shape { digest: digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect(),
        functions: found.into_iter().collect() })
}

fn scalar_function(function: &ItemFn) -> Result<(), &'static str> {
    let signature = &function.sig;
    // const fn 可能参与布局和静态初始化，过程宏也可能把表面数字改写为注册／其他副作用。
    if signature.constness.is_some() || signature.asyncness.is_some() || signature.unsafety.is_some()
        || signature.abi.is_some() || signature.variadic.is_some() || !signature.inputs.is_empty()
        || !signature.generics.params.is_empty() || signature.generics.where_clause.is_some()
    { return Err("仅支持无参、非const／泛型／unsafe／async／extern函数"); }
    if function.attrs.iter().any(|attribute| {
        !["doc", "inline", "cold", "allow", "warn", "deny", "forbid", "expect"]
            .into_iter().any(|name| attribute.path().is_ident(name))
    }) { return Err("含未验证的属性或属性宏"); }
    let ReturnType::Type(_, output) = &signature.output else { return Err("必须返回固定标量类型"); };
    let Type::Path(output) = output.as_ref() else { return Err("返回类型不属于首期标量合同"); };
    if output.qself.is_some() || output.path.leading_colon.is_some() || output.path.segments.len() != 1
        || !output.path.segments[0].arguments.is_none()
        || !matches!(output.path.segments[0].ident.to_string().as_str(), "f32" | "f64" | "u32" | "u64")
    { return Err("返回类型仅支持f32／f64／u32／u64"); }
    let [Stmt::Expr(expression, None)] = function.block.stmts.as_slice() else {
        return Err("函数体只能含一个无副作用的数字算式尾表达式");
    };
    if !scalar_expression(expression, 0) {
        return Err("函数体仅允许数字字面量、括号、加减乘除取余及负号");
    }
    Ok(())
}

fn scalar_expression(expression: &Expr, depth: usize) -> bool {
    if depth > MAX_EXPRESSION_DEPTH { return false; }
    match expression {
        Expr::Lit(value) => value.attrs.is_empty() && matches!(value.lit, Lit::Int(_) | Lit::Float(_)),
        Expr::Paren(value) => value.attrs.is_empty() && scalar_expression(&value.expr, depth + 1),
        Expr::Group(value) => value.attrs.is_empty() && scalar_expression(&value.expr, depth + 1),
        Expr::Unary(value) => value.attrs.is_empty() && matches!(value.op, UnOp::Neg(_))
            && scalar_expression(&value.expr, depth + 1),
        Expr::Binary(value) => value.attrs.is_empty()
            && matches!(value.op, BinOp::Add(_) | BinOp::Sub(_) | BinOp::Mul(_) | BinOp::Div(_) | BinOp::Rem(_))
            && scalar_expression(&value.left, depth + 1) && scalar_expression(&value.right, depth + 1),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(functions: &str) -> String {
        format!("[package]\nname='fixture'\n[package.metadata.feathrise.hotpatch]\nenabled=true\nfunctions=[{functions}]\n")
    }

    fn contract() -> Contract {
        Contract::parse(&manifest("'src/controller.rs::walk_speed'")).unwrap().unwrap()
    }

    fn digest(source: &str) -> String {
        shape(&contract(), "src/controller.rs", source).unwrap().digest
    }

    #[test]
    fn opt_in_is_explicit_and_contract_is_bounded_and_normalized() {
        assert!(Contract::parse("[package]\nname='fixture'").unwrap().is_none());
        assert!(Contract::parse("[package.metadata.feathrise.hotpatch]\nfunctions=['broken']").unwrap().is_none());
        assert!(Contract::parse("[package.metadata.feathrise.hotpatch]\nenabled=false\nfunctions=['broken']").unwrap().is_none());
        for input in [
            "[package.metadata.feathrise.hotpatch]\nenabled='true'\nfunctions=[]".to_owned(),
            manifest(""), manifest("42"), manifest("'src/controller.rs::walk_speed','src/controller.rs::walk_speed'"),
            manifest("'../escape.rs::walk_speed'"), manifest("'/abs.rs::walk_speed'"),
            manifest("'C:/abs.rs::walk_speed'"), manifest("'src\\controller.rs::walk_speed'"),
            manifest("'src/controller.rs::module::walk_speed'"), manifest("'src/./controller.rs::walk_speed'"),
            manifest("'src/controller.rs::invalid-name'"), manifest("'src/controller.txt::walk_speed'"),
            manifest("'src/controller.rs::walk_speed'") + "other=true\n",
        ] { assert!(Contract::parse(&input).is_err(), "应拒绝合同：{input}"); }
        let nine = (0..9).map(|index| format!("'src/{index}.rs::value'")).collect::<Vec<_>>().join(",");
        assert!(Contract::parse(&manifest(&nine)).is_err());
        assert_eq!(contract().files().collect::<Vec<_>>(), vec!["src/controller.rs"]);
    }

    #[test]
    fn only_registered_scalar_bodies_are_erased_from_full_file_shape() {
        let old = "struct State { value:u64 } fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 }";
        let changed = "struct State { value:u64 } fn walk_speed()->f32 { -(2.0+4.0)*3.0/2.0%7.0 } fn other()->u64 { 1 }";
        assert_eq!(digest(old), digest(changed));
        for changed in [
            "struct State { value:u64, extra:u64 } fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 }",
            "struct State { value:u64 } fn walk_speed()->f64 { 12.0 } fn other()->u64 { 1 }",
            "struct State { value:u64 } pub fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 }",
            "struct State { value:u64 } #[inline(never)] fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 }",
            "struct State { value:u64 } fn walk_speed()->f32 { 12.0 } fn other()->u64 { 2 }",
            "#![allow(dead_code)] struct State { value:u64 } fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 }",
            "struct State { value:u64 } fn walk_speed()->f32 { 12.0 } fn other()->u64 { 1 } fn register() {}",
        ] { assert_ne!(digest(old), digest(changed), "函数体外代码不能擦除：{changed}"); }
    }

    #[test]
    fn calls_closures_registration_and_unknown_expressions_require_restart() {
        for expression in [
            "compute()", "{ 12.0 }", "if true {12.0} else {14.0}", "return 12.0", "{ let v=12.0; v }",
            "(||12.0)()", "unsafe {12.0}", "async {12.0}", "12.0_f32.abs()", "VALUE", "dbg!(12.0)",
            "std::f32::consts::PI", "{ static X:f32=12.0; X }", "{ register(); 12.0 }", "{ loop {} }",
            "12u32 << 1", "12u32 & 3", "12.0 as f32", "\"12.0\"", "[12.0][0]", "#[cfg(any())] 12.0",
        ] {
            let source = format!("fn walk_speed()->f32 {{ {expression} }}");
            assert!(shape(&contract(), "src/controller.rs", &source).is_err(), "应重启：{expression}");
        }
    }

    #[test]
    fn signature_and_attribute_macros_are_not_treated_as_scalar_patches() {
        for source in [
            "fn walk_speed(value:f32)->f32 {12.0}", "fn walk_speed<T>()->f32 {12.0}",
            "fn walk_speed()->f32 where (): Sized {12.0}", "const fn walk_speed()->f32 {12.0}",
            "unsafe fn walk_speed()->f32 {12.0}", "async fn walk_speed()->f32 {12.0}",
            "extern \"C\" fn walk_speed()->f32 {12.0}", "fn walk_speed() {12.0}",
            "fn walk_speed()->usize {12}", "fn walk_speed()->Vec<f32> {12.0}",
            "fn walk_speed()->&'static f32 {12.0}", "#[register_system] fn walk_speed()->f32 {12.0}",
            "#[cfg_attr(feature=\"extra\", register_system)] fn walk_speed()->f32 {12.0}",
            "mod nested { fn walk_speed()->f32 {12.0} }", "macro_rules! define {()=>{fn walk_speed()->f32 {12.0}}} define!();",
            "fn walk_speed()->f32 {12.0} fn walk_speed()->f32 {14.0}", "fn walk_speed()->f32 {",
        ] { assert!(shape(&contract(), "src/controller.rs", source).is_err(), "应重启：{source}"); }
    }

    #[test]
    fn multiple_files_and_all_four_scalar_types_keep_exact_function_selection() {
        let contract = Contract::parse(&manifest("'src/one.rs::a','src/one.rs::b','src/two.rs::a','src/two.rs::b'")).unwrap().unwrap();
        let one = "fn a()->f32 {1.0} fn b()->f64 {2.0}";
        let two = "fn a()->u32 {1} fn b()->u64 {2}";
        assert!(shape(&contract, "src/one.rs", one).is_ok());
        assert!(shape(&contract, "src/two.rs", two).is_ok());
        assert!(shape(&contract, "src/two.rs", "fn a()->u32 {1}").is_err());
        assert!(shape(&contract, "src/unknown.rs", one).is_err());
        assert!(shape(&contract, "src/one.rs", &format!("{one}{}", " ".repeat(MAX_SOURCE_BYTES))).is_err());
        let nested = format!("fn a()->f32 {{{}1.0{}}} fn b()->f64 {{2.0}}", "(".repeat(40), ")".repeat(40));
        assert!(shape(&contract, "src/one.rs", &nested).is_err());
    }
}
