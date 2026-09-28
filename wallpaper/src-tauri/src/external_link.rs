//! 打开会话转写里的外部链接。
//!
//! 单独一个模块、单独一条命令，是因为它和 `client_window::open_loopback_url` 的信任级别不同：
//! 那条只开 `http://127.0.0.1:<port>`，这条开的是**模型输出里的地址**。所以"什么算一个可以交给
//! shell 的地址"写在这里，并且刻意只认 http/https：
//!
//!  - 只放行 `http://` 与 `https://`（大小写不敏感）。`javascript:`、`file:`、`ms-settings:`、
//!    以及任何自定义协议一律拒绝 —— `ShellExecuteW` 会老老实实照协议去解析，不能让它试。
//!  - 只放行可打印 ASCII。非 ASCII 域名要 punycode 才开：`аpple.com`（西里尔 а）这类同形字
//!    在悬停预览里和真域名长得一模一样，肉眼分不出来。
//!  - authority 里不许出现 `user@`：`https://apple.com@evil.test/` 是"看着像 A 其实去 B"的
//!    经典写法，而悬停那一行地址很短，很容易只读到前半段。
//!  - 不许空白与控制字符，长度封顶。
//!
//! 打开方式用 `ShellExecuteW` 的 `open` 动词（项目里已有这条路）：不拼命令行、不起 shell、
//! 不会有第二个进程去解释字符串。

/// 地址长度上限；渲染层 `isExternalLink` 用的是同一个数。
const MAX_EXTERNAL_URL_BYTES: usize = 2048;

/// 判断一个地址能不能交给 shell，并返回去掉首尾空白后的结果。
///
/// 纯函数，规则见模块头部。渲染层用它决定"渲染成链接还是保持字面"，这里决定"能不能真的打开"；
/// 两边不一致时以这里为准 —— 这里才是真正把地址交出去的那一步。
pub fn validate(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("链接为空".into());
    }
    if trimmed.len() > MAX_EXTERNAL_URL_BYTES {
        return Err("链接过长，未打开".into());
    }
    if !trimmed.is_ascii() {
        return Err("链接含非 ASCII 字符，未打开（非英文域名请用 punycode）".into());
    }
    if trimmed
        .chars()
        .any(|character| character.is_ascii_control() || character == ' ')
    {
        return Err("链接含空白或控制字符，未打开".into());
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("只打开 http/https 链接".into());
    }
    // scheme 之后的第一个 `//` 就是 authority 的起点（scheme 本身不含斜杠）。
    let after_scheme = trimmed.find("//").map(|at| &trimmed[at + 2..]).unwrap_or("");
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.is_empty() {
        return Err("链接没有主机名，未打开".into());
    }
    if authority.contains('@') {
        return Err("链接的主机名是 user@ 形式，未打开".into());
    }
    Ok(trimmed.to_string())
}

/// Hand the address to the shell. `validate` runs first and is the only reason this is safe.
#[cfg(windows)]
pub fn open(url: &str) -> Result<String, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target = validate(url)?;
    let operation = HSTRING::from("open");
    let wide = HSTRING::from(target.as_str());
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a value <= 32 on failure; only > 32 is success.
    if result.0 as usize <= 32 {
        return Err("Windows 没有打开这个链接".into());
    }
    // 打开外部地址是一次用户可见的动作，留一条痕：出问题时能对上是哪一次点出来的。
    log::info!("打开外部链接：{target}");
    Ok(target)
}

#[cfg(not(windows))]
pub fn open(url: &str) -> Result<String, String> {
    validate(url)?;
    Err("当前平台不支持打开浏览器".into())
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_http_and_https_addresses() {
        assert_eq!(
            validate("https://www.markdownguide.org/").as_deref(),
            Ok("https://www.markdownguide.org/")
        );
        assert_eq!(
            validate("  http://127.0.0.1:3080/a/b?c=d#e  ").as_deref(),
            Ok("http://127.0.0.1:3080/a/b?c=d#e")
        );
        // 大小写不敏感，但**原样交出去**：地址的大小写可能是有意义的。
        assert_eq!(
            validate("HTTPS://Example.COM/Path").as_deref(),
            Ok("HTTPS://Example.COM/Path")
        );
    }

    #[test]
    fn refuses_every_scheme_that_is_not_http() {
        for url in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:startupapps",
            "shell:AppsFolder\\com.dsh.wallpaper_pdxj8y3r6rm5g!Wallpaper",
            "data:text/html,<h1>x</h1>",
            "www.example.com",
            "//example.com/path",
        ] {
            assert!(validate(url).is_err(), "{url} should not be openable");
        }
    }

    #[test]
    fn refuses_addresses_that_hide_where_they_go() {
        // user@host：悬停那一行地址很短，很容易只读到前半段。
        assert!(validate("https://apple.com@evil.test/").is_err());
        // 非 ASCII 域名：同形字在预览里看不出区别，要 punycode 才开。
        assert!(validate("https://аpple.com/").is_err());
        // 空白与控制字符：地址里出现它们只可能是拼接事故。
        assert!(validate("https://exa mple.com/").is_err());
        assert!(validate("https://example.com/\u{7}").is_err());
    }

    #[test]
    fn refuses_addresses_without_a_host_or_with_an_absurd_length() {
        assert!(validate("").is_err());
        assert!(validate("   ").is_err());
        assert!(validate("http://").is_err());
        assert!(validate("https:///only-a-path").is_err());
        let long = format!("https://example.com/{}", "a".repeat(4096));
        assert!(validate(&long).is_err());
    }
}
