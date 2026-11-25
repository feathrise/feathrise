//! 可选文本用逗号分隔；双引号保留空选项、两端空格或选项本身的逗号。
pub(super) fn show_choices(values: &[String]) -> String {
    values
        .iter()
        .map(|s| {
            if s.is_empty() || s.trim() != s || s.contains([',', '，', '"']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
pub(super) fn parse_choices(value: &str) -> Result<Option<Vec<String>>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let mut result = Vec::new();
    let mut input = value.trim_start();
    loop {
        let (text, tail) = if let Some(mut s) = input.strip_prefix('"') {
            let mut text = String::new();
            loop {
                let i = s.find('"').ok_or("固定选项的双引号未闭合")?;
                text.push_str(&s[..i]);
                s = &s[i + 1..];
                if let Some(rest) = s.strip_prefix('"') {
                    text.push('"');
                    s = rest;
                } else {
                    break;
                }
            }
            (text, s.trim_start())
        } else {
            let end = input.find([',', '，']).unwrap_or(input.len());
            (input[..end].trim().to_owned(), &input[end..])
        };
        result.push(text);
        if tail.is_empty() {
            break;
        }
        input = tail
            .strip_prefix([',', '，'])
            .ok_or("固定选项之间请用逗号分隔")?
            .trim_start();
    }
    Ok(Some(result))
}
pub(super) fn xyz(text: &str) -> Result<[String; 3], String> {
    let values: Vec<_> = text
        .split(|c: char| c.is_whitespace() || c == ',' || c == '，')
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    values
        .try_into()
        .map_err(|_| "地点坐标需要三个数值，例如 0, 1, 5".into())
}
