//! 崩溃自证：让进程在自己崩溃的那一刻，留下一份能定位问题的报告。
//!
//! 真机上壁纸会「莫名其妙自动退出」，Windows 事件日志只给到「出错模块 + 异常码 + 偏移」
//! （`0xc0000005` @ `0x353d17` 之类），而 `%LOCALAPPDATA%\CrashDumps` 里的 WER 转储要
//! 调试器 + PDB 才能读。偏移没有符号就没有意义，所以这里让**应用自己**在崩溃现场把
//! 「谁崩的、崩在哪、出错线程的调用链是什么、当时日志的最后几行是什么」写成一份纯文本，
//! 放在用户找得到的 logs 目录里（与壁纸自己的日志同一个目录）。
//!
//! # 三个入口，各管一段
//!
//! * `AddVectoredExceptionHandler` —— 第一机会（first-chance）：异常刚抛出、故障现场最完整
//!   时就被叫到。只看**致命异常码**（见 [`is_fatal_exception_code`]），命中就立刻落盘；
//!   于是即使后面有人把我们的顶层过滤器顶掉、或异常被别处处理掉，现场也已经留下来了。
//! * `SetUnhandledExceptionFilter` —— 顶层过滤器：异常确认无人处理、进程即将退出时被叫到。
//!   它对**任何**未处理异常都落盘（不只是致命码），并在与第一机会那条同源时去重。
//! * `std::panic::set_hook` —— Rust 侧的 panic。**panic hook 抓不到访问违例**：访问违例是
//!   SEH 异常、不是 Rust panic，所以两者必须都装。
//!
//! # 三条硬约束
//!
//! 1. **绝不吞异常**：两个原生处理器都返回 `EXCEPTION_CONTINUE_SEARCH`，异常照原样交回系统
//!    ——WER 转储、Windows 的错误提示、之前装过的过滤器都不受影响。
//! 2. **绝不递归崩溃**：所有入口先抢一把「正在写报告」的旗（[`enter`]），抢不到就直接返回；
//!    写报告只用预置的静态缓冲与栈缓冲，**不分配堆内存**（此刻堆可能已经坏了），任何一步
//!    失败就放弃——报告可以没有，进程不能因此再崩一次。
//! 3. **正常路径零开销**：异常处理器只在异常时被调用；安装时只做一次目录与路径准备。
//!
//! # 为什么报告里最要紧的是模块表
//!
//! 帧是用 `RtlCaptureStackBackTrace` 取的**返回地址**——裸地址（ASLR 之下每次运行都不同）
//! 单独没有任何意义。模块表同时给出**基址与大小**，事后才能把每个地址换算成
//! `模块名+偏移`，再拿偏移去配那个模块的 PDB 做符号化。这两样东西合起来，偏移才有意义。
//! 报告正文里已经算好了 `模块名+偏移`（见 [`locate_address`]），事件日志里的「异常偏移」
//! 就是同一个数。
//!
//! # 报告之外，还要一份全量转储（2026-10-05 补）
//!
//! 文本报告只有「谁崩的、崩在哪」，答不了「那个指针为什么是垃圾」——坏掉的那一页内存
//! 不在报告里。所以同一个处理器在做完现场记录之后，还会调 `MiniDumpWriteDump` 写一份
//! **全量**转储（`MiniDumpWithFullMemory` 一族，见 `native::write_dump`）到
//! `%LOCALAPPDATA%\com.dsh.wallpaper\crashes\`：
//!
//! * 与文本报告**同一个**现场、**同一把**重入旗：一次崩溃最多产出一份转储；
//! * 转储几百 MB，所以有保留策略（只留最近 `KEEP_DUMP_FILES` 份，写新的之前先删最旧的）；
//! * 设 `DSH_WALLPAPER_NO_CRASH_DUMP=1` 就整个关掉（写转储要花时间与磁盘）；
//! * 写失败**只记在文本报告里**（`dump.error`），绝不因为写转储把进程再拖死一次；
//! * 转储之后仍然照常把异常交回系统（返回值不变，`EXCEPTION_CONTINUE_SEARCH`）。
//!
//! 全量转储的价值取决于**符号**：读它的调试器要拿同一次构建的 PDB 才能把地址变成函数名。
//! 因此发布路径必须留 PDB（见 `docs/diagnostics/crash-dumps.md`，以及
//! `scripts/publish-local-nsis.ps1`）。**没有 PDB，全量转储也读不出东西。**
//!
//! # 两处落点的唯一出处
//!
//! 报告在 `<标识符目录>\logs\`、转储在 `<标识符目录>\crashes\`（平级，两个版本各一套），
//! 加上应用日志与 PDB 留档，四处落点的完整表格只在**一处**：
//! `docs/diagnostics/crash-dumps.md` 第 1 节。代码侧对应 [`layout_for`]（纯函数，单测把
//! 报告目录 / 转储目录 / 日志文件三处一起钉住）；报告正文里 `dump.directory` 无论写没写成
//! 转储都会写出来，所以读报告的人不必回头查文档也知道转储落在哪。

// 这套东西只有 Windows 用得上（SEH 是那边的机制）。非 Windows 仍然编译，只是没有调用者：
// 与其让一堆「从未使用」的警告淹掉真正有用的信号，不如在这里明确关掉。
#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// 崩溃转储目录名（挂在应用本地数据目录下）：`%LOCALAPPDATA%\com.dsh.wallpaper\crashes\`。
/// Lite 版是同一个布局（`com.dsh.wallpaper.lite\crashes\`）。
pub(crate) const CRASH_DUMP_DIRECTORY: &str = "crashes";

/// 目录里最多保留几份转储。写新的之前会先把更旧的删掉，所以峰值是
/// [`KEEP_DUMP_FILES`] + 1（正在写的这一份），不会无上限地吃掉用户磁盘。
pub(crate) const KEEP_DUMP_FILES: usize = 3;

/// 关掉转储的环境变量：值为 `1`（或 `true` / `yes` / `on`，大小写无所谓）时不再写转储。
/// 文本报告不受它影响——那只有几十 KB，没有理由一起关掉。安装时读一次，之后不再读
/// （崩溃路径上不碰环境变量，那里连分配都不做）。
pub(crate) const DUMP_DISABLE_ENV: &str = "DSH_WALLPAPER_NO_CRASH_DUMP";

/// 换一个转储目录的环境变量（给「转储要放到别的盘」这种需求留的口子）。
pub(crate) const DUMP_DIRECTORY_ENV: &str = "DSH_WALLPAPER_CRASH_DUMP_DIR";

/// 一次枚举里最多认多少份转储文件。超出部分本次不参与保留策略（也就不会被删）——
/// 宁可多留，也不要在一次枚举里把用户目录翻个底朝天。
pub(crate) const DUMP_LIST_CAP: usize = 64;

/// 转储文件名里写几位十六进制异常码。异常码是 32 位，所以是 8。
const DUMP_CODE_DIGITS: usize = 8;

/// 转储文件名的定长上限（`crash-YYYYMMDD-HHMMSS-mmm-c0000409-16.dmp` 是 43 字节）。
pub(crate) const DUMP_NAME_CAP: usize = 64;

/// 转储文件名的时间戳前缀与后缀（拼名字和被单测钉住的那一段）。
const DUMP_PREFIX: &str = "crash-";
const DUMP_SUFFIX: &str = ".dmp";

/// `FILETIME` 的 100 纳秒单位（保留策略按它排序；纯数字，不依赖任何系统调用）。
pub(crate) type DumpFileTime = u64;

/// 出错线程最多取多少帧返回地址。
pub(crate) const MAX_FRAMES: usize = 64;

/// 模块表最多列多少条。真实条数另记在 `modules.loaded` 里，报告会说明是否截断。
pub(crate) const MODULE_CAP: usize = 384;

/// 模块名（基名）在报告里最多保留的字节数。
pub(crate) const MODULE_NAME_CAP: usize = 64;

/// 报告自带的应用日志尾部行数。
pub(crate) const LOG_TAIL_LINES: usize = 40;

/// 从日志文件里最多回读的字节数。40 行通常远小于它；回读是定长的，不随日志增长。
pub(crate) const LOG_TAIL_BYTES: usize = 8192;

/// 同一个进程最多写多少份报告。panic 可能在循环里反复触发，需要有个上限；
/// 达到上限后仍然照常把 panic 交给上一个 hook（只是不再落盘）。
pub(crate) const MAX_REPORTS_PER_PROCESS: u32 = 8;

/// 路径缓冲的 UTF-16 码元数（含结尾 NUL）。`%LOCALAPPDATA%\com.dsh.wallpaper\logs\...`
/// 远小于它；装不下时报告功能整体放弃，而不是截断成一条错的路径。
const PATH_UNITS: usize = 384;

/// 报告里每一种「谁写的这份报告」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReportOrigin {
    /// 向量化异常处理器：第一机会捕获，进程随后是否被处理掉未知。
    VectoredFirstChance,
    /// 顶层未处理异常过滤器：异常无人处理，进程即将退出。
    UnhandledFilter,
    /// Rust panic hook。
    RustPanic,
}

impl ReportOrigin {
    fn label(self) -> &'static str {
        match self {
            Self::VectoredFirstChance => {
                "native-exception / first-chance（AddVectoredExceptionHandler：异常刚抛出时就抓到了；随后是否被别处处理掉未知）"
            }
            Self::UnhandledFilter => {
                "native-exception / unhandled（SetUnhandledExceptionFilter：异常无人处理，进程即将退出）"
            }
            Self::RustPanic => {
                "rust-panic（std::panic::set_hook；panic 默认 unwind，进程未必退出）"
            }
        }
    }
}

/// 一个时间戳，拆成字段便于格式化（不引入日期库，也不做任何分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millis: u16,
}

impl LocalTime {
    /// `2025-10-04 20:39:12.473`（报告正文里的人读形式）。
    pub(crate) fn write_report_timestamp(&self, out: &mut [u8]) -> usize {
        let mut cursor = 0usize;
        push_decimal(out, &mut cursor, u32::from(self.year), 4);
        push_byte(out, &mut cursor, b'-');
        push_decimal(out, &mut cursor, u32::from(self.month), 2);
        push_byte(out, &mut cursor, b'-');
        push_decimal(out, &mut cursor, u32::from(self.day), 2);
        push_byte(out, &mut cursor, b' ');
        push_decimal(out, &mut cursor, u32::from(self.hour), 2);
        push_byte(out, &mut cursor, b':');
        push_decimal(out, &mut cursor, u32::from(self.minute), 2);
        push_byte(out, &mut cursor, b':');
        push_decimal(out, &mut cursor, u32::from(self.second), 2);
        push_byte(out, &mut cursor, b'.');
        push_decimal(out, &mut cursor, u32::from(self.millis), 3);
        cursor
    }

    /// `20251004-203912-473`（文件名用：可按名字排序、含毫秒、无非法字符）。
    ///
    /// 带毫秒是刻意的：同一秒内先 panic 再真崩（或 panic 循环）也不会互相覆盖。
    pub(crate) fn write_file_stamp(&self, out: &mut [u8]) -> usize {
        let mut cursor = 0usize;
        push_decimal(out, &mut cursor, u32::from(self.year), 4);
        push_decimal(out, &mut cursor, u32::from(self.month), 2);
        push_decimal(out, &mut cursor, u32::from(self.day), 2);
        push_byte(out, &mut cursor, b'-');
        push_decimal(out, &mut cursor, u32::from(self.hour), 2);
        push_decimal(out, &mut cursor, u32::from(self.minute), 2);
        push_decimal(out, &mut cursor, u32::from(self.second), 2);
        push_byte(out, &mut cursor, b'-');
        push_decimal(out, &mut cursor, u32::from(self.millis), 3);
        cursor
    }
}

/// 一个已加载模块：基址、大小、**基名**（`kernel32.dll`，不带路径）。
///
/// 定长内联的名字是刻意的：整张模块表因此可以是一个静态数组，崩溃时不需要堆。
/// 「模块名+0x偏移」要的就是基名；基址加基名已经足以在事后找到那个二进制与它的 PDB。
#[derive(Clone, Copy)]
pub(crate) struct ModuleRecord {
    base: u64,
    size: u64,
    name: [u8; MODULE_NAME_CAP],
    name_len: u8,
}

impl ModuleRecord {
    /// 空记录：只用来初始化静态数组。
    pub(crate) const EMPTY: Self = Self {
        base: 0,
        size: 0,
        name: [0; MODULE_NAME_CAP],
        name_len: 0,
    };

    /// 从 UTF-16（Windows 的模块名）取基名。非 ASCII 码元写成 `?`：模块基名在 Windows 上
    /// 一律是 ASCII，真出现别的字符也只能退化，绝不能因此少写一条模块。
    pub(crate) fn from_wide(base: u64, size: u64, wide: &[u16]) -> Self {
        let mut record = Self {
            base,
            size,
            ..Self::EMPTY
        };
        let mut length = 0usize;
        for unit in wide {
            if *unit == 0 || length + 1 >= MODULE_NAME_CAP {
                break;
            }
            record.name[length] = if *unit < 0x80 { *unit as u8 } else { b'?' };
            length += 1;
        }
        record.name_len = length as u8;
        record
    }

    pub(crate) fn base(&self) -> u64 {
        self.base
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// 基名。空名字（采集失败的那一条）返回空串，不 panic。
    pub(crate) fn name(&self) -> &str {
        ascii_str(&self.name[..usize::from(self.name_len).min(MODULE_NAME_CAP)])
    }
}

/// 一个地址落在哪个模块里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModuleLocation {
    /// 模块表里的下标。
    pub index: usize,
    /// 相对该模块基址的偏移。
    pub offset: u64,
    /// 偏移是否落在模块大小之内。`false` 表示这个地址在模块末尾之后（即时生成 / 动态代码区）。
    pub inside: bool,
}

/// 把地址换算成「哪个模块 + 多少偏移」。
///
/// 取**基址最大且不超过该地址**的那个模块（重名/重叠时以基址更大的为准），这也是调试器
/// 的做法。基址为 0 的记录（采集失败留下的空记录）不参与。
pub(crate) fn locate_address(address: u64, modules: &[ModuleRecord]) -> Option<ModuleLocation> {
    if address == 0 {
        return None;
    }
    let mut best: Option<(usize, u64)> = None;
    for (index, module) in modules.iter().enumerate() {
        let base = module.base();
        if base == 0 || base > address {
            continue;
        }
        let offset = address - base;
        match best {
            // 已有候选的偏移更小，说明它的基址更大 —— 保持它。
            Some((_, best_offset)) if best_offset <= offset => {}
            _ => best = Some((index, offset)),
        }
    }
    best.map(|(index, offset)| ModuleLocation {
        index,
        offset,
        // 大小为 0 的模块（采集不到大小时）只能尽力而为地认下：偏移仍然是有用的。
        inside: modules[index].size() == 0 || offset < modules[index].size(),
    })
}

/// 异常码 → 名字。用事件日志/调试器里同样的英文名，方便对齐官方文档。
pub(crate) fn exception_code_name(code: u32) -> Option<&'static str> {
    Some(match code {
        0xc000_0005 => "ACCESS_VIOLATION",
        0xc000_0006 => "IN_PAGE_ERROR",
        0xc000_0017 => "NO_MEMORY",
        0xc000_001d => "ILLEGAL_INSTRUCTION",
        0xc000_008c => "ARRAY_BOUNDS_EXCEEDED",
        0xc000_008d => "FLOAT_DENORMAL_OPERAND",
        0xc000_008e => "FLOAT_DIVIDE_BY_ZERO",
        0xc000_0094 => "INTEGER_DIVIDE_BY_ZERO",
        0xc000_0095 => "INTEGER_OVERFLOW",
        0xc000_0096 => "PRIVILEGED_INSTRUCTION",
        0xc000_00fd => "STACK_OVERFLOW",
        0xc000_0135 => "DLL_NOT_FOUND",
        0xc000_0138 => "ORDINAL_NOT_FOUND",
        0xc000_0139 => "ENTRYPOINT_NOT_FOUND",
        0xc000_0142 => "DLL_INIT_FAILED",
        0xc000_02b4 => "FLOAT_MULTIPLE_FAULTS",
        0xc000_0374 => "HEAP_CORRUPTION",
        // 现代「快速失败」多半绕过所有异常处理，真的收到它也比没有强。
        0xc000_0409 => "STACK_BUFFER_OVERRUN（fast-fail）",
        _ => return None,
    })
}

/// 是否属于「收到就意味着进程活不下去」的异常码。
///
/// 第一机会处理器只对这些码动手：第一机会异常里混着 C++ EH（`0xE06D7363`）、.NET
/// （`0xE0434352`）、调试断点（`0x80000003`）这些**用于控制流**的异常，把那些也当崩溃
/// 记下来只会污染目录。上面这些码没有一个是拿来做控制流的。
///
/// 顶层过滤器不受这个白名单限制：它被叫到时进程已经确定要死了，任何码都值得留证据。
pub(crate) fn is_fatal_exception_code(code: u32) -> bool {
    exception_code_name(code).is_some()
}

/// 访问违例的 `ExceptionInformation[0]`：访问类型。其它异常码没有这层含义。
pub(crate) fn access_violation_operation(value: usize) -> Option<&'static str> {
    Some(match value {
        0 => "读取",
        1 => "写入",
        8 => "执行（DEP/NX）",
        _ => return None,
    })
}

/// 把一段字节修成合法 UTF-8：非法处整段替换为 `?`，长度不变。
///
/// 日志尾部是从文件里按字节回读的，可能正好从某个多字节字符中间截断。崩溃处理器不能
/// 分配内存，所以这里原地修补，而不是 `to_string_lossy()`。
pub(crate) fn scrub_utf8(bytes: &mut [u8]) -> &str {
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        match std::str::from_utf8(&bytes[cursor..]) {
            Ok(_) => break,
            Err(error) => {
                let bad_at = cursor + error.valid_up_to();
                let bad_len = error
                    .error_len()
                    .unwrap_or_else(|| bytes.len() - bad_at)
                    .max(1);
                let end = (bad_at + bad_len).min(bytes.len());
                for byte in &mut bytes[bad_at..end] {
                    *byte = b'?';
                }
                cursor = end;
            }
        }
    }
    ascii_str(bytes)
}

/// 取一段文本的最后 `lines` 行，按原顺序写进 `out`，返回写了几行。**不分配**。
pub(crate) fn collect_tail<'a>(text: &'a str, out: &mut [&'a str]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let capacity = out.len();
    let mut total = 0usize;
    for candidate in text.lines() {
        if total < capacity {
            out[total] = candidate;
        } else {
            out[total % capacity] = candidate;
        }
        total += 1;
    }
    let stored = total.min(capacity);
    if total > capacity {
        // 环形缓冲里现在的顺序是「从 total % capacity 开始」。
        out[..stored].rotate_left(total % capacity);
    }
    stored
}

/// 异常现场（致命异常的全部可读字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExceptionFacts<'a> {
    pub code: u32,
    pub address: u64,
    pub flags: u32,
    pub thread_id: u32,
    /// `ExceptionInformation[..NumberParameters]`。
    pub parameters: &'a [usize],
    /// `EXCEPTION_POINTERS.ContextRecord` 的裸指针：只有写全量转储时才用得上
    /// （`MiniDumpWriteDump` 靠它拿到出错线程的完整上下文）。
    pub thread_context: *mut core::ffi::c_void,
}

/// panic 现场。
#[derive(Clone, Copy)]
pub(crate) struct PanicFacts<'a> {
    pub message: &'a str,
    pub location: Option<&'a str>,
    pub backtrace: &'a str,
}

/// 报告正文的全部输入。把它摊平成一个结构体，是为了让正文拼装是一个**纯函数**
/// （见 [`compose_report`]），格式可以被单测钉死。
pub(crate) struct ReportFacts<'a> {
    pub product: &'a str,
    pub edition: &'a str,
    pub app_version: &'a str,
    pub os_version: &'a str,
    pub os_arch: &'a str,
    pub process_id: u32,
    pub process_exe: &'a str,
    pub thread_id: u32,
    pub origin: ReportOrigin,
    pub file_name: &'a str,
    pub directory_display: &'a str,
    /// 全量转储的落点与结局（没写出时只留 `error`，报告里说明为什么）。
    pub dump: Option<DumpFacts<'a>>,
    pub local_time: LocalTime,
    pub utc_time: LocalTime,
    pub exception: Option<ExceptionFacts<'a>>,
    pub panic: Option<PanicFacts<'a>>,
    pub frames: &'a [u64],
    pub modules: &'a [ModuleRecord],
    /// 实际枚举到的模块数（可能大于 `modules.len()`，即列表被截断）。
    pub module_total: usize,
    pub log_display: &'a str,
    pub log_tail: &'a str,
}

/// 报告正文的去处。崩溃路径写的是文件（定长缓冲 + `WriteFile`），单测写的是 `String`。
pub(crate) trait ReportSink {
    fn write_str(&mut self, text: &str);
}

impl ReportSink for String {
    fn write_str(&mut self, text: &str) {
        self.push_str(text);
    }
}

/// 报告正文的纯函数入口：**只**由单测调用。
///
/// 生产路径（崩溃处理器）用的是同一个 [`write_report`]，只是把内容直接写进文件、中途不经过
/// `String`——在可能已经坏掉的堆上，一次多余的分配就是一次多余的失败机会。
#[cfg(test)]
pub(crate) fn compose_report(facts: &ReportFacts<'_>) -> String {
    let mut out = String::new();
    write_report(&mut out, facts);
    out
}

/// 拼出报告正文。全程不分配内存、不 panic、不依赖系统状态。
pub(crate) fn write_report<S: ReportSink>(sink: &mut S, facts: &ReportFacts<'_>) {
    sink.write_str(facts.product);
    line(sink, " 崩溃报告（进程自己在崩溃现场写下的，不需要调试器）");
    line(sink, "=====================================================");
    text_field(sink, "report.file", facts.file_name);
    text_field(sink, "report.directory", facts.directory_display);
    text_field(sink, "report.kind", facts.origin.label());

    field_start(sink, "time.local");
    write_timestamp(sink, &facts.local_time);
    sink.write_str("\n");
    field_start(sink, "time.utc");
    write_timestamp(sink, &facts.utc_time);
    line(sink, "（应用日志里的时间是 UTC）");

    field_start(sink, "app");
    sink.write_str(facts.product);
    sink.write_str(" ");
    sink.write_str(facts.app_version);
    sink.write_str(" (");
    sink.write_str(facts.edition);
    line(sink, ")");
    field_start(sink, "os");
    sink.write_str(facts.os_version);
    sink.write_str(" ");
    sink.write_str(facts.os_arch);
    sink.write_str("\n");
    decimal_field(sink, "process.id", facts.process_id);
    text_field(sink, "process.exe", facts.process_exe);
    decimal_field(sink, "thread.id", facts.thread_id);

    if let Some(exception) = facts.exception {
        line(sink, "");
        line(sink, "[exception]");
        field_start(sink, "exception.code");
        write_hex(sink, u64::from(exception.code), 8);
        match exception_code_name(exception.code) {
            Some(name) => {
                sink.write_str(" (");
                sink.write_str(name);
                sink.write_str(")");
            }
            None => sink.write_str(" (未知异常码)"),
        }
        sink.write_str("\n");
        field_start(sink, "exception.address");
        write_hex(sink, exception.address, 16);
        sink.write_str(" ");
        write_address_location(sink, exception.address, facts.modules);
        sink.write_str("\n");
        hex_field(sink, "exception.flags", u64::from(exception.flags), 8);
        decimal_field(sink, "exception.thread.id", exception.thread_id);
        decimal_field(
            sink,
            "exception.parameter.count",
            exception.parameters.len() as u32,
        );
        for (index, value) in exception.parameters.iter().enumerate() {
            sink.write_str("exception.parameter.");
            write_decimal(sink, index as u32);
            sink.write_str(": ");
            write_hex(sink, *value as u64, 16);
            if exception.code == 0xc000_0005 {
                match index {
                    0 => {
                        sink.write_str(" → 访问类型：");
                        sink.write_str(access_violation_operation(*value).unwrap_or("未知"));
                    }
                    1 => {
                        sink.write_str(" → 访问地址：");
                        write_hex(sink, *value as u64, 16);
                        sink.write_str(" ");
                        write_address_location(sink, *value as u64, facts.modules);
                    }
                    _ => {}
                }
            }
            sink.write_str("\n");
        }
    }

    if let Some(dump) = facts.dump.as_ref() {
        line(sink, "");
        line(sink, "[dump]");
        text_field(sink, "dump.directory", dump.directory);
        match dump.error {
            Some(reason) => {
                line(sink, "dump.written: 否");
                text_field(sink, "dump.error", reason);
                if dump.error_code != 0 {
                    hex_field(sink, "dump.error.code", u64::from(dump.error_code), 8);
                }
            }
            None => {
                line(sink, "dump.written: 是");
                text_field(sink, "dump.file", dump.file_name);
                text_field(sink, "dump.path", dump.path);
                if let Some(size) = dump.size {
                    field_start(sink, "dump.size");
                    let mut buffer = [0u8; 32];
                    let length = dump_size_into(&mut buffer, size);
                    sink.write_str(ascii_str(&buffer[..length.min(buffer.len())]));
                    sink.write_str("\n");
                }
                decimal_field(sink, "dump.retention.kept", KEEP_DUMP_FILES as u32);
                decimal_field(sink, "dump.retention.pruned", dump.pruned as u32);
            }
        }
    }

    if let Some(panic) = facts.panic {
        line(sink, "");
        line(sink, "[panic]");
        sink.write_str("panic.message:\n");
        indented_block(sink, panic.message);
        field_start(sink, "panic.location");
        sink.write_str(panic.location.unwrap_or("（没有位置信息）"));
        sink.write_str("\n");
        sink.write_str("panic.backtrace:\n");
        indented_block(sink, panic.backtrace);
    }

    line(sink, "");
    line(sink, "[stack]");
    decimal_field(sink, "stack.thread.id", facts.thread_id);
    decimal_field(sink, "stack.frame.count", facts.frames.len() as u32);
    if facts.frames.is_empty() {
        line(sink, "  （没有帧：这不是原生异常，调用链看上面的 panic.backtrace）");
    }
    for (index, frame) in facts.frames.iter().enumerate() {
        sink.write_str("  #");
        write_decimal_padded(sink, index as u32, 2);
        sink.write_str(" ");
        write_hex(sink, *frame, 16);
        sink.write_str(" ");
        write_address_location(sink, *frame, facts.modules);
        sink.write_str("\n");
    }

    line(sink, "");
    line(sink, "[modules]");
    decimal_field(sink, "modules.loaded", facts.module_total as u32);
    decimal_field(sink, "modules.listed", facts.modules.len() as u32);
    if facts.modules.is_empty() {
        line(
            sink,
            "  （模块表不可用：一个模块都没枚举到 —— 只能靠 exception.address 的数值本身）",
        );
    }
    for module in facts.modules {
        sink.write_str("  ");
        write_hex(sink, module.base(), 16);
        sink.write_str(" ");
        write_hex(sink, module.size(), 16);
        sink.write_str(" ");
        sink.write_str(module.name());
        sink.write_str("\n");
    }
    if facts.module_total > facts.modules.len() {
        line(sink, "  （截断：上面是基址最小的那些，其余模块未列出）");
    }

    line(sink, "");
    line(sink, "[log]");
    text_field(sink, "log.path", facts.log_display);
    field_start(sink, "log.tail.max.lines");
    write_decimal(sink, LOG_TAIL_LINES as u32);
    sink.write_str("\n");
    line(sink, "log.tail:");
    let mut ring: [&str; LOG_TAIL_LINES] = [""; LOG_TAIL_LINES];
    let stored = collect_tail(facts.log_tail, &mut ring);
    if stored == 0 {
        line(
            sink,
            "  （空：崩溃那一刻日志里没有内容 —— 应用日志超过 40 000 字节会被删档重建，见下面的 notes）",
        );
    }
    for text in &ring[..stored] {
        sink.write_str("  ");
        sink.write_str(text);
        sink.write_str("\n");
    }

    line(sink, "");
    line(sink, "[notes]");
    let mut index = 0u32;
    note(sink, &mut index, NOTE_WER);
    note(sink, &mut index, NOTE_FRAMES);
    note(sink, &mut index, NOTE_EVENT_LOG);
    note(sink, &mut index, NOTE_NO_HEAP);
    note(sink, &mut index, NOTE_LOG_ROTATION);
    note(sink, &mut index, NOTE_MODULES);
    if facts.origin == ReportOrigin::RustPanic {
        note(sink, &mut index, NOTE_PANIC);
    }
    note(sink, &mut index, NOTE_UNVERIFIED);
    if let Some(dump) = facts.dump.as_ref() {
        if dump.error.is_none() {
            note(sink, &mut index, NOTE_DUMP);
        }
    }

    line(sink, "");
    sink.write_str("（报告结束：以上内容由进程在 ");
    sink.write_str(facts.origin.label());
    sink.write_str(" 中写出）\n");
}

const NOTE_WER: &str = "本报告是进程自己在异常处理里写下的，和 %LOCALAPPDATA%\\CrashDumps 里的 WER 转储互补：转储要有调试器 + PDB 才能读，本报告要的是模块基址与偏移，两样都在上面。";
const NOTE_FRAMES: &str = "帧是返回地址；“分发路径”的那几帧（KiUserExceptionDispatcher / UnhandledExceptionFilter / 本报告自己的处理器）在列表最前面，真正的出错指令看 exception.address。";
const NOTE_EVENT_LOG: &str = "Windows 事件日志里的“异常偏移”（例如 0x353d17）就是 exception.address 减去所属模块基址 —— 上面的“模块名+偏移”已经算好，可以直接对着事件日志核。";
const NOTE_NO_HEAP: &str = "崩溃处理器不分配堆内存、不递归（重入立刻返回），写完报告仍把异常交回系统（EXCEPTION_CONTINUE_SEARCH），所以 WER 转储和系统错误提示都不受影响。";
const NOTE_LOG_ROTATION: &str = "应用日志按 tauri-plugin-log 的默认值轮换（KeepOne，40 000 字节阈值）：一超阈值就把旧文件删掉重开 —— 日志变成 0 字节就是这么来的。所以本报告自带尾部，但崩溃前刚轮换过时它可能是空的。";
const NOTE_MODULES: &str = "模块名截断到 63 字节，最多列 384 条；每行是“基址 大小 模块名”，按基址升序。";
const NOTE_PANIC: &str = "这是一份 Rust panic 报告，不是致命异常：panic 默认 unwind，进程可能继续跑（同一进程最多写 8 份报告）。";
const NOTE_UNVERIFIED: &str = "采集路径（SEH 处理器）在开发机上无法用真机崩溃验证，格式由 cargo test --lib 的单测钉住；各字段的含义见上面的前缀。";
const NOTE_DUMP: &str = "同一目录（dump.path 所在的 crashes 目录）里最多保留最近三份全量转储，写这一份之前已经删掉了更旧的；不想要转储就设 DSH_WALLPAPER_NO_CRASH_DUMP=1。读它要用 cdb.exe，符号路径里必须有一次构建出的同名 PDB（见 docs/diagnostics/crash-dumps.md）。";

fn note<S: ReportSink>(sink: &mut S, index: &mut u32, text: &str) {
    *index += 1;
    sink.write_str("note.");
    write_decimal(sink, *index);
    sink.write_str(": ");
    sink.write_str(text);
    sink.write_str("\n");
}

/// 把一个地址写成 `模块名+0x偏移`；认不出模块时写出裸地址并说明。
fn write_address_location<S: ReportSink>(sink: &mut S, address: u64, modules: &[ModuleRecord]) {
    match locate_address(address, modules) {
        Some(location) if location.inside => {
            sink.write_str(modules[location.index].name());
            sink.write_str("+");
            write_hex(sink, location.offset, 0);
        }
        Some(location) => {
            sink.write_str(modules[location.index].name());
            sink.write_str("+");
            write_hex(sink, location.offset, 0);
            sink.write_str("（超出该模块大小 ");
            write_hex(sink, modules[location.index].size(), 0);
            sink.write_str("，可能是即时生成/动态代码）");
        }
        None => {
            write_hex(sink, address, 16);
            sink.write_str("（没有落在任何已枚举模块里）");
        }
    }
}

/// 逐行缩进一块多行文本（panic 消息、Backtrace）。
fn indented_block<S: ReportSink>(sink: &mut S, text: &str) {
    if text.trim().is_empty() {
        line(sink, "  （空）");
        return;
    }
    for inner in text.lines() {
        sink.write_str("  ");
        sink.write_str(inner);
        sink.write_str("\n");
    }
}

fn line<S: ReportSink>(sink: &mut S, text: &str) {
    sink.write_str(text);
    sink.write_str("\n");
}

fn text_field<S: ReportSink>(sink: &mut S, name: &str, value: &str) {
    field_start(sink, name);
    sink.write_str(value);
    sink.write_str("\n");
}

fn field_start<S: ReportSink>(sink: &mut S, name: &str) {
    sink.write_str(name);
    sink.write_str(": ");
}

fn decimal_field<S: ReportSink>(sink: &mut S, name: &str, value: u32) {
    field_start(sink, name);
    write_decimal(sink, value);
    sink.write_str("\n");
}

fn hex_field<S: ReportSink>(sink: &mut S, name: &str, value: u64, digits: usize) {
    field_start(sink, name);
    write_hex(sink, value, digits);
    sink.write_str("\n");
}

fn write_timestamp<S: ReportSink>(sink: &mut S, time: &LocalTime) {
    let mut buffer = [0u8; 32];
    let length = time.write_report_timestamp(&mut buffer);
    sink.write_str(ascii_str(&buffer[..length.min(buffer.len())]));
}

fn write_decimal<S: ReportSink>(sink: &mut S, value: u32) {
    write_decimal_padded(sink, value, 0);
}

/// 十进制输出，左补零到 `width` 位（`width` 为 0 表示用最少位数）。
fn write_decimal_padded<S: ReportSink>(sink: &mut S, value: u32, width: usize) {
    let mut buffer = [0u8; 20];
    let mut cursor = 0usize;
    push_decimal(&mut buffer, &mut cursor, value, width);
    sink.write_str(ascii_str(&buffer[..cursor]));
}

/// 写 `0x` 加十六进制。`digits` 为 0 表示用最少位数，否则左补零到这么多位。
fn write_hex<S: ReportSink>(sink: &mut S, value: u64, digits: usize) {
    let width = if digits == 0 {
        hex_digits(value)
    } else {
        digits.min(16)
    };
    let mut buffer = [0u8; 16];
    let mut remaining = value;
    let mut index = width;
    while index > 0 {
        index -= 1;
        let digit = (remaining & 0xf) as u8;
        buffer[index] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        remaining >>= 4;
    }
    sink.write_str("0x");
    sink.write_str(ascii_str(&buffer[..width]));
}

fn hex_digits(value: u64) -> usize {
    let mut digits = 1usize;
    let mut remaining = value >> 4;
    while remaining != 0 {
        digits += 1;
        remaining >>= 4;
    }
    digits
}

/// 十进制写入（左补零到 `width`）。越界一律丢弃，绝不 panic —— 这里跑在崩溃处理器里。
fn push_decimal(out: &mut [u8], cursor: &mut usize, value: u32, width: usize) {
    let mut digits = [b'0'; 10];
    let mut remaining = value;
    let mut count = 0usize;
    while count < digits.len() {
        digits[digits.len() - 1 - count] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        count += 1;
        if remaining == 0 {
            break;
        }
    }
    let start = digits.len() - count;
    for _ in count..width.max(count) {
        push_byte(out, cursor, b'0');
    }
    for index in start..digits.len() {
        push_byte(out, cursor, digits[index]);
    }
}

fn push_byte(out: &mut [u8], cursor: &mut usize, byte: u8) {
    if *cursor < out.len() {
        out[*cursor] = byte;
        *cursor += 1;
    }
}

fn push_bytes(out: &mut [u8], cursor: &mut usize, bytes: &[u8]) {
    for byte in bytes {
        push_byte(out, cursor, *byte);
    }
}

/// 已经是合法 UTF-8 就给出 `&str`；不是就返回空串（绝不 panic）。
fn ascii_str(bytes: &[u8]) -> &str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => "",
    }
}

/// 安装时的入参：崩溃时只读、不含任何需要分配的东西。
pub(crate) struct ReportEnvironment<'a> {
    pub product: &'a str,
    pub edition: &'a str,
    pub app_version: &'a str,
    pub process_exe: &'a str,
    /// 报告目录（UTF-16，结尾带 NUL），`CreateDirectoryW` / `CreateFileW` 直接用。
    pub directory_wide: &'a [u16],
    pub directory_display: &'a str,
    /// 应用日志文件（UTF-16，结尾带 NUL），只用来回读尾部。
    pub log_wide: &'a [u16],
    pub log_display: &'a str,
    /// 全量转储目录（UTF-16，结尾带 NUL），转储文件与旧的转储都在这里。
    pub dump_directory_wide: &'a [u16],
    pub dump_directory_display: &'a str,
    /// 本次进程是否写转储（`DSH_WALLPAPER_NO_CRASH_DUMP` 安装时读一次的结果）。
    pub dump_enabled: bool,
}

/// 把目录与文件名拼成一条 NUL 结尾的 UTF-16 路径，返回含 NUL 的长度；放不下返回 0。
///
/// 纯函数，可单测：报告落在哪个文件，就是这一行代码说了算。
pub(crate) fn join_path(directory: &[u16], file_name: &str, out: &mut [u16]) -> usize {
    let mut length = 0usize;
    for unit in directory {
        if *unit == 0 {
            break;
        }
        if length + 2 >= out.len() {
            return 0;
        }
        out[length] = *unit;
        length += 1;
    }
    if length > 0 && out[length - 1] != u16::from(b'\\') && out[length - 1] != u16::from(b'/') {
        out[length] = u16::from(b'\\');
        length += 1;
    }
    for byte in file_name.as_bytes() {
        if length + 2 >= out.len() {
            return 0;
        }
        out[length] = u16::from(*byte);
        length += 1;
    }
    out[length] = 0;
    length + 1
}

/// 报告文件名：`crash-<时间戳>.txt`。
///
/// `ordinal` 大于 1 时在后面加序号（`crash-<时间戳>-2.txt`）：同一毫秒里可能不止一份报告
/// ——多线程同时崩，或者 panic 之后紧接着真崩。撞名会让 `CREATE_ALWAYS` 把前一份
/// （很可能是**另一条线程**的现场）盖掉，那正是最不该丢的东西。返回写出的字节数。
pub(crate) fn write_report_file_name(out: &mut [u8], stamp: &str, ordinal: u32) -> usize {
    let mut cursor = 0usize;
    push_bytes(out, &mut cursor, REPORT_PREFIX.as_bytes());
    push_bytes(out, &mut cursor, stamp.as_bytes());
    if ordinal > 1 {
        push_byte(out, &mut cursor, b'-');
        push_decimal(out, &mut cursor, ordinal, 0);
    }
    push_bytes(out, &mut cursor, REPORT_SUFFIX.as_bytes());
    cursor
}

/// 报告文件名的时间戳前缀与后缀（拼名字和被单测钉住的那一段）。
const REPORT_PREFIX: &str = "crash-";
const REPORT_SUFFIX: &str = ".txt";

/// 一份报告在磁盘上的落点。
pub(crate) struct Layout {
    /// 报告头里写的产品名（也是日志文件名的来源）。
    pub product: &'static str,
    /// `full` / `lite`。
    pub edition: &'static str,
    /// 报告目录。
    pub directory: std::path::PathBuf,
    /// 应用日志文件（报告要从中带回读尾部的那一个）。
    pub log_file: std::path::PathBuf,
    /// 全量转储目录。与报告目录**平级**（`logs` 与 `crashes` 同一个标识符目录下）：
    /// 报告必须跟应用日志待在一起（用户从日志目录出发找），转储几百 MB 且有保留策略
    /// （会删东西），而且用户可以按环境变量把转储整个甩到别的盘 —— 那时报告仍要留在
    /// 日志旁边。所以两处落点不合并，但**规则只写在这里**。
    pub dump_directory: std::path::PathBuf,
}

/// 报告目录、转储目录与日志文件的规则只有一条：全都在同一个标识符目录下（`logs` 与
/// `crashes` 平级）。
///
/// Tauri 的规则是 `%LOCALAPPDATA%\<标识符>\logs`，日志文件名是 `<productName>.log`
/// （tauri-plugin-log 拿 `package_info().name` 当文件名）。两个版本各有自己的标识符与产品名，
/// 所以这里由 `lite` 一次定型。纯函数，可单测 —— `%LOCALAPPDATA%\com.dsh.wallpaper\logs`
/// 这个落点是需求里逐字写明的，测试把**三处落点**一起钉住。
pub(crate) fn layout_for(local_app_data: &std::path::Path, lite: bool) -> Layout {
    let (product, edition, identifier) = if lite {
        ("dsh-wallpaper-lite", "lite", "com.dsh.wallpaper.lite")
    } else {
        ("dsh-wallpaper", "full", "com.dsh.wallpaper")
    };
    let root = local_app_data.join(identifier);
    let directory = root.join("logs");
    let log_file = directory.join(format!("{product}.log"));
    let dump_directory = root.join(CRASH_DUMP_DIRECTORY);
    Layout {
        product,
        edition,
        directory,
        log_file,
        dump_directory,
    }
}

/// 报告里那份全量转储的落点与结局。
///
/// `file_name` 与 `path` 都为空表示**没有**转储：这时报告只解释原因（`error`），
/// 让读报告的人知道不必去 crashes 目录里找。
pub(crate) struct DumpFacts<'a> {
    /// 转储目录。**写没写成都要写出来**：没写成时它就是「别去别处找，去这里找」的那句话，
    /// 读报告的人不必再回文档里核对转储落在哪。
    pub directory: &'a str,
    pub file_name: &'a str,
    /// 转储的完整路径（给人照着去打开的东西）。
    pub path: &'a str,
    /// 字节数。`None` 表示取不到大小（文件系统调失败之类），报告里就不写这一行。
    pub size: Option<u64>,
    /// 写之前删掉了几份旧转储（保留策略），0 表示没删。
    pub pruned: usize,
    /// 没写出转储的原因；`Some` 时前面三个字段都无意义。
    pub error: Option<&'a str>,
    /// 失败时的 `GetLastError()`（0 表示不是系统调用失败）。
    pub error_code: u32,
}

/// 一次转储尝试的三种结局。写文本报告前先拿它，报告里才写得出「dump 在哪 / 为什么没有」。
#[derive(Clone, Copy)]
pub(crate) enum DumpOutcome {
    /// 硬盘上已经躺着一份全量转储。
    Written,
    /// 没尝试写，也不会写：非原生异常、已经被环境变量关掉、或者正在跑单测。
    NotAttempted,
    /// 试过了，失败了。文本报告里记 `dump.error`。
    Failed,
}

/// 一份转储在枚举里留下的记录。**全部定长内联**：崩溃路径不分配堆。
///
/// `name` 是文件**名**（不含目录），因为保留策略只按名字与写入时间比大小；
/// 真正要删的路径在删之前才拼出来。
#[derive(Clone, Copy)]
pub(crate) struct DumpEntry {
    name: [u8; DUMP_NAME_CAP],
    name_len: u8,
    time: DumpFileTime,
    /// 名字像不像我们自己写的转储（不像的**绝不删**：那可能是用户自己放的文件）。
    is_dump: bool,
}

impl DumpEntry {
    pub(crate) const EMPTY: Self = Self {
        name: [0; DUMP_NAME_CAP],
        name_len: 0,
        time: 0,
        is_dump: false,
    };

    /// 从文件名与写入时间造一条记录；文件名太长就整条放弃（两个断言都由单测钉住）。
    pub(crate) fn new(name: &str, time: DumpFileTime) -> Option<Self> {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.len() >= DUMP_NAME_CAP || name.len() > bytes.len() {
            // 非 ASCII 名字的 `name.len()`（字节或码点）与 `bytes.len()` 不等，
            // 那种名字绝不会是我们写的（我们自己只写 ASCII），直接放弃。
            return None;
        }
        let mut entry = Self {
            time,
            is_dump: is_dump_name(name),
            ..Self::EMPTY
        };
        entry.name[..bytes.len()].copy_from_slice(bytes);
        entry.name_len = bytes.len() as u8;
        Some(entry)
    }

    pub(crate) fn name(&self) -> &str {
        ascii_str(&self.name[..usize::from(self.name_len).min(DUMP_NAME_CAP)])
    }

    pub(crate) fn time(&self) -> DumpFileTime {
        self.time
    }

    /// 名字是我们自己写的那种（`crash-<时间戳>-<异常码>.dmp` / 带序号的同款）。
    pub(crate) fn is_dump(&self) -> bool {
        self.is_dump
    }
}

/// 名字像不像我们自己写的转储。
///
/// 只认「`crash-` 打头、`.dmp` 结尾」：目录可能是用户指定的（`DSH_WALLPAPER_CRASH_DUMP_DIR`），
/// 那里很可能还有别人的文件 —— 那种文件一个都不能删。
pub(crate) fn is_dump_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() > DUMP_PREFIX.len() + DUMP_SUFFIX.len() + 1
        && name.starts_with(DUMP_PREFIX)
        && name.ends_with(DUMP_SUFFIX)
}

/// 保留策略：把 `entries` 重排成「先保留的、后被删的」，返回**被删的份数**。
///
/// 它是纯函数（不认识文件系统），因为「哪几份删、哪几份留」正是最需要被单测钉住的一条。
/// 规则只有两条：
///
/// 1. **只有名字像我们自己的转储才参与**（`crash-*.dmp`）；不像的一个都不动，
///    所以给 `DSH_WALLPAPER_CRASH_DUMP_DIR` 指一个放着别的东西的目录也不会误删。
/// 2. **按写入时间从新到旧，保留 `keep` 份**；被删的那些按「最旧在前」返回，
///    这样调用方的删除顺序就是确定的（测试也能钉住）。
///
/// `keep` 为 0 时全部候选都算要删（用于「一次清空」这种显式调用）。
pub(crate) fn partition_dumps_for_retention(
    entries: &mut [DumpEntry],
    keep: usize,
) -> usize {
    let length = entries.len().min(DUMP_LIST_CAP);
    if length == 0 {
        return 0;
    }
    // 插入排序：按「写入时间新→旧」，同一时刻按名字定序（同一毫秒内写完的两份也能定序）。
    for index in 1..length {
        let mut position = index;
        while position > 0 && dump_should_come_first(&entries[position], &entries[position - 1]) {
            entries.swap(position, position - 1);
            position -= 1;
        }
    }
    // 第一趟：定序之后，前面的 `keep` 份转储留下；不是转储的一律留下（它们不在候选里）。
    let mut kept: [DumpEntry; DUMP_LIST_CAP] = [DumpEntry::EMPTY; DUMP_LIST_CAP];
    let mut deleted: [DumpEntry; DUMP_LIST_CAP] = [DumpEntry::EMPTY; DUMP_LIST_CAP];
    let mut kept_len = 0usize;
    let mut deleted_len = 0usize;
    let mut seen_dumps = 0usize;
    for entry in entries[..length].iter() {
        let survives = if entry.is_dump() {
            let index = seen_dumps;
            seen_dumps += 1;
            index < keep
        } else {
            true
        };
        if survives && kept_len < kept.len() {
            kept[kept_len] = *entry;
            kept_len += 1;
        } else if deleted_len < deleted.len() {
            deleted[deleted_len] = *entry;
            deleted_len += 1;
        }
    }
    // 第二趟：保留段在前、待删段在后，两段各自保持原来的先后顺序。
    let mut write = 0usize;
    for entry in kept[..kept_len].iter().chain(deleted[..deleted_len].iter()) {
        entries[write] = *entry;
        write += 1;
    }
    deleted_len
}

/// 排序用的比较：`left` 应当排在 `right` **前面**（也就是更新，或者同一时刻名字更靠后）。
///
/// 同一时刻按名字**降序**是有意的：我们的文件名带毫秒时间戳，降序就是「同毫秒里
/// 序号更大的（后写的）排前面」，与「文件系统里更晚出现的更值得留」一致。
fn dump_should_come_first(left: &DumpEntry, right: &DumpEntry) -> bool {
    if left.time != right.time {
        return left.time > right.time;
    }
    left.name() > right.name()
}

/// 转储文件名：`crash-<时间戳>.dmp`，有异常码时写成 `crash-<时间戳>-<异常码 8 位>.dmp`。
///
/// 时间戳与文本报告同源（同一毫秒、同一现场），异常码写进名字是为了在一堆转储里一眼认出
/// 哪一份对应事件日志里的 `0xc0000005`。返回写出的字节数；`DUMP_NAME_CAP` 装不下时返回 0。
pub(crate) fn write_dump_file_name(out: &mut [u8], stamp: &str, code: Option<u32>) -> usize {
    let mut cursor = 0usize;
    push_bytes(out, &mut cursor, DUMP_PREFIX.as_bytes());
    push_bytes(out, &mut cursor, stamp.as_bytes());
    if let Some(code) = code {
        push_byte(out, &mut cursor, b'-');
        if !push_hex_at_end(out, &mut cursor, code, DUMP_CODE_DIGITS) {
            return 0;
        }
    }
    push_bytes(out, &mut cursor, DUMP_SUFFIX.as_bytes());
    if cursor > out.len() {
        return 0;
    }
    cursor
}

/// 在 `cursor` 处续写十六进制（小写，左补零到 `digits` 位）。返回是否写下去了。
///
/// 与 `write_hex` 的区别只在「不写 `0x`、写在中间、装不下就整段放弃」。
fn push_hex_at_end(out: &mut [u8], cursor: &mut usize, value: u32, digits: usize) -> bool {
    let width = digits.min(8).max(1);
    if *cursor + width > out.len() {
        return false;
    }
    let mut remaining = value;
    let mut index = *cursor + width;
    while index > *cursor {
        index -= 1;
        let digit = (remaining & 0xf) as u8;
        out[index] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        remaining >>= 4;
    }
    *cursor += width;
    true
}

/// 把字节数写成报告里的 `dump.size`：小于 1 MB 写字节，否则写「多少 MB.多少（保留一位）」。
///
/// 不分配：整数与小数用 `push_decimal` 写在调用方给的栈缓冲里。转储动辄几百 MB，
/// 写成 `524288000` 不好读，写成 `500.0 MB` 一眼就知道。
pub(crate) fn dump_size_into(out: &mut [u8], bytes: u64) -> usize {
    const MB: u64 = 1024 * 1024;
    let mut cursor = 0usize;
    if bytes >= MB {
        let whole = bytes / MB;
        let tenth = (bytes % MB) * 10 / MB;
        push_decimal_u64(out, &mut cursor, whole, 0);
        push_byte(out, &mut cursor, b'.');
        push_decimal(out, &mut cursor, tenth as u32, 1);
        push_bytes(out, &mut cursor, b" MB");
    } else {
        push_decimal_u64(out, &mut cursor, bytes, 0);
        push_bytes(out, &mut cursor, b" \xe5\xad\x97\xe8\x8a\x82"); // " 字节"（UTF-8：E5 AD 97 E8 8A 82）
    }
    cursor
}

/// 64 位十进制写入（转储大小能到 GB，32 位不够；其余格式与 `push_decimal` 一致）。
fn push_decimal_u64(out: &mut [u8], cursor: &mut usize, value: u64, width: usize) {
    let mut digits = [b'0'; 20];
    let mut remaining = value;
    let mut count = 0usize;
    while count < digits.len() {
        digits[digits.len() - 1 - count] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        count += 1;
        if remaining == 0 {
            break;
        }
    }
    let start = digits.len() - count;
    for _ in count..width.max(count) {
        push_byte(out, cursor, b'0');
    }
    for index in start..digits.len() {
        push_byte(out, cursor, digits[index]);
    }
}

/// [`DUMP_DISABLE_ENV`] 的取值判定：`1` / `true` / `yes` / `on`（大小写与首尾空白都不计）
/// 表示「不要写转储」。安装时用它；崩溃路径上不再读环境变量。
pub(crate) fn dump_disabled_by_value(value: Option<&str>) -> bool {
    match value {
        Some(text) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => false,
    }
}

/// 「正在写报告」的旗：抢不到就说明已经有（可能是同一个）处理器在跑，立刻返回。
///
/// 这把旗同时保证对 `SCRATCH` 里那些 `UnsafeCell` 的访问是排他的 —— 所以崩溃处理器里可以
/// 放心拿 `&mut`，不需要 Mutex（在崩溃现场等锁可能永远等不到）。
static REPORTING: AtomicBool = AtomicBool::new(false);

/// 已经写出去的报告数（上限见 [`MAX_REPORTS_PER_PROCESS`]）。
static REPORT_COUNT: AtomicU32 = AtomicU32::new(0);

struct ReportGuard;

/// 抢到「正在写报告」的旗。返回的守卫在离开作用域时放旗。
///
/// 用 `compare_exchange` 而不是「读-判断-写」：两个线程同时崩的时候，也只允许一个动手。
fn enter() -> Option<ReportGuard> {
    if REPORTING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        Some(ReportGuard)
    } else {
        None
    }
}

impl Drop for ReportGuard {
    fn drop(&mut self) {
        REPORTING.store(false, Ordering::Release);
    }
}

fn report_budget_left() -> bool {
    REPORT_COUNT.load(Ordering::Acquire) < MAX_REPORTS_PER_PROCESS
}

fn spend_report_budget() {
    REPORT_COUNT.fetch_add(1, Ordering::AcqRel);
}

/// 报告写到哪个目录（供启动日志用）。`None` 表示这次进程里没装上（非 Windows、
/// 或本地数据目录不可用）。
pub(crate) fn report_directory() -> Option<&'static str> {
    #[cfg(windows)]
    {
        native::report_directory()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

// ---------------------------------------------------------------------------
// 原生部分：只有 Windows 才有 SEH
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod native {
    use super::*;
    use core::ffi::c_void;
    use core::mem::size_of;
    use std::sync::OnceLock;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, DeleteFileW, FindClose, FindFirstFileW, FindNextFileW,
        GetFileAttributesW, GetFileSizeEx, ReadFile, SetFilePointerEx, WriteFile, CREATE_ALWAYS,
        FILE_ATTRIBUTE_NORMAL, FILE_BEGIN, FILE_END, FILE_SHARE_DELETE, FILE_SHARE_MODE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING, WIN32_FIND_DATAW,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, CREATE_TOOLHELP_SNAPSHOT_FLAGS,
        MODULEENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId};

    /// 报告目录用 Tauri 的标识符规则（标识符目录下的 `logs`），也就是**应用日志所在的那个
    /// 目录**：%LOCALAPPDATA%\com.dsh.wallpaper\logs（Lite 版是 ...\.lite\logs）。
    /// 这两个落点由 [`layout_for`] 决定，并被单测钉住。

    /// 报告文件是否已经存在（同一毫秒内的撞名避让用）。
    fn file_exists(path: &[u16]) -> bool {
        unsafe { GetFileAttributesW(PCWSTR::from_raw(path.as_ptr())) != INVALID_FILE_ATTRIBUTES }
    }

    /// 异常处理器的返回码：**继续搜索**，也就是「我不处理，原样交回系统」。
    /// 绝不能返回 `EXCEPTION_EXECUTE_HANDLER`（1，那会吞掉异常、连 WER 都不跑）。
    pub(crate) const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    /// `EXCEPTION_RECORD`（winnt.h）：报告要用到的字段一个不少，布局与系统一致。
    #[repr(C)]
    pub(crate) struct ExceptionRecord {
        code: u32,
        flags: u32,
        record: *mut ExceptionRecord,
        address: *mut c_void,
        number_parameters: u32,
        information: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    /// `MINIDUMP_EXCEPTION_INFORMATION`（minidumpapiset.h）：告诉 `MiniDumpWriteDump`
    /// 「这次转储是为哪一条异常写的」——有了它，转储里才带上**出错线程的完整上下文**，
    /// 调试器打开时能直接停在故障指令上，而不是让人自己去猜哪条线程。
    ///
    /// 布局与系统一致：`ThreadId`（4 字节）+ 结构体里那对指针（x64 是 8 字节对齐，
    /// 于是 `ClientPointers` 落在 +16）。`packed(4)` 就是官方头文件的效果。
    #[repr(C, packed(4))]
    struct MiniDumpExceptionInformation {
        thread_id: u32,
        exception_pointers: *mut ExceptionPointers,
        client_pointers: i32,
    }

    /// `MINIDUMP_TYPE` 的位。
    ///
    /// * `MINIDUMP_NORMAL`（0）——位图里没有这一位，`0` 表示「基础转储」；
    /// * `MINIDUMP_WITH_FULL_MEMORY`（2）——**全量内存**：坏掉的那一页在这里，
    ///   「那个指针为什么是垃圾」只能靠它回答，也正是 2 MB 迷你转储缺的东西；
    /// * `MINIDUMP_WITH_PROCESS_THREAD_DATA`（256）——所有线程的 TEB/栈数据；
    /// * `MINIDUMP_WITH_THREAD_INFO`（4096）——每条线程的起始地址与运行时间。
    ///
    /// 不取 `MiniDumpWithHandleData`（句柄表）与 `MiniDumpWithFullMemoryInfo`（内存布局表）：
    /// 它们只会让文件更大、写得更慢，而对这一族「内存破坏」问题的价值有限。
    const DUMP_TYPE_FLAGS: i32 = 0 | 2 | 256 | 4096;

    /// `MiniDumpWriteDump` 的 32 位十六进制格式码（它的返回值只有这一个错误码有意义）。
    const E_INVALIDARG: u32 = 0x8007_0057;

    // 转储走**直接链接 dbghelp**，理由与上面那几个 kernel32 入口一样：一个函数、签名一眼看完，
    // 不为它把 `windows` crate 的 feature 再打开一片。dbghelp.dll 自 Windows 10 起随系统提供，
    // 不需要随包分发。
    #[link(name = "dbghelp")]
    extern "system" {
        fn MiniDumpWriteDump(
            process: HANDLE,
            process_id: u32,
            file: HANDLE,
            dump_type: i32,
            exception_information: *const MiniDumpExceptionInformation,
            user_stream: *const c_void,
            callback: *const c_void,
        ) -> i32;
    }

    /// `SYSTEMTIME`（minwinbase.h）：只要日期与时间字段。
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NativeSystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    /// `OSVERSIONINFOW`（winnt.h）：只要版本号字段。
    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        csd_version: [u16; 128],
    }

    type VectoredHandler = Option<unsafe extern "system" fn(*mut ExceptionPointers) -> i32>;
    type TopLevelFilter = Option<unsafe extern "system" fn(*const ExceptionPointers) -> i32>;

    // 这几个入口都用**直接链接 kernel32**，而不是给 `windows` crate 打开新的 feature：
    // 仓库里本来就有直接 `#[link]` 的先例（client_window.rs 的 winmm），而这些声明的 ABI
    // 几十年没变过，签名也一眼看得完。
    #[link(name = "kernel32")]
    extern "system" {
        fn AddVectoredExceptionHandler(first: u32, handler: VectoredHandler) -> *mut c_void;
        fn SetUnhandledExceptionFilter(filter: TopLevelFilter) -> TopLevelFilter;
        fn RtlCaptureStackBackTrace(
            frames_to_skip: u32,
            frames_to_capture: u32,
            backtrace: *mut *mut c_void,
            backtrace_hash: *mut u32,
        ) -> u16;
        fn GetLocalTime(system_time: *mut NativeSystemTime);
        fn GetSystemTime(system_time: *mut NativeSystemTime);
    }

    // `RtlGetVersion` 是唯一一个不理会兼容性清单、永远说真话的版本查询
    // （`GetVersionEx` 在没有清单时会谎报 6.2）。
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(version_info: *mut OsVersionInfoW) -> i32;
    }

    /// 安装时算好、崩溃时只读的东西。`OnceLock` 的读取是原子的、不取锁。
    pub(super) struct InstalledPaths {
        product: &'static str,
        edition: &'static str,
        app_version: &'static str,
        process_exe: String,
        directory: [u16; PATH_UNITS],
        directory_len: usize,
        directory_display: String,
        log_file: [u16; PATH_UNITS],
        log_file_len: usize,
        log_display: String,
        /// 全量转储目录（`...\crashes`）。
        dump_directory: [u16; PATH_UNITS],
        dump_directory_len: usize,
        dump_directory_display: String,
        /// `DSH_WALLPAPER_NO_CRASH_DUMP` 安装时读一次的结果。
        dump_enabled: bool,
    }

    impl InstalledPaths {
        fn environment(&self) -> ReportEnvironment<'_> {
            ReportEnvironment {
                product: self.product,
                edition: self.edition,
                app_version: self.app_version,
                process_exe: &self.process_exe,
                directory_wide: &self.directory[..self.directory_len],
                directory_display: &self.directory_display,
                log_wide: &self.log_file[..self.log_file_len],
                log_display: &self.log_display,
                dump_directory_wide: &self.dump_directory[..self.dump_directory_len],
                dump_directory_display: &self.dump_directory_display,
                dump_enabled: self.dump_enabled,
            }
        }
    }

    static PATHS: OnceLock<InstalledPaths> = OnceLock::new();

    /// 之前装过的顶层过滤器（如果有）。我们照样调用它并把它的返回值原样返回，
    /// 这样「装我们之前的行为」一点没变。
    static PREVIOUS_FILTER: OnceLock<TopLevelFilter> = OnceLock::new();

    /// 上一次记下来的致命现场。第一机会处理器写它，顶层过滤器用它去重。
    #[derive(Clone, Copy)]
    struct Fault {
        found: bool,
        reported: bool,
        code: u32,
        address: u64,
        thread_id: u32,
    }

    impl Fault {
        const EMPTY: Self = Self {
            found: false,
            reported: false,
            code: 0,
            address: 0,
            thread_id: 0,
        };
    }

    /// 崩溃专用暂存区：**全是静态内存**（BSS），因为崩溃那一刻堆、栈都可能已经不可靠，
    /// 而为了省这点内存去调用会分配的 API 也不值得赌。
    ///
    /// 访问一律在 `ReportGuard` 保护之下（见 [`enter`]），所以同一时刻只有一个执行流碰它，
    /// 各字段之间也没有别名 —— `unsafe impl Sync` 的依据就在这里。
    struct Scratch {
        modules: std::cell::UnsafeCell<[ModuleRecord; MODULE_CAP]>,
        frames: std::cell::UnsafeCell<[u64; MAX_FRAMES]>,
        log_bytes: std::cell::UnsafeCell<[u8; LOG_TAIL_BYTES]>,
        report_path: std::cell::UnsafeCell<[u16; PATH_UNITS]>,
        /// 全量转储的 UTF-16 路径（文本报告里的 `dump.path` 就指着它）。
        dump_path: std::cell::UnsafeCell<[u16; PATH_UNITS]>,
        /// 全量转储的 ASCII 文件名（报告里的 `dump.file`）。
        dump_name: std::cell::UnsafeCell<[u8; DUMP_NAME_CAP]>,
        out: std::cell::UnsafeCell<[u8; 4096]>,
        fault: std::cell::UnsafeCell<Fault>,
    }

    unsafe impl Sync for Scratch {}

    static SCRATCH: Scratch = Scratch {
        modules: std::cell::UnsafeCell::new([ModuleRecord::EMPTY; MODULE_CAP]),
        frames: std::cell::UnsafeCell::new([0u64; MAX_FRAMES]),
        log_bytes: std::cell::UnsafeCell::new([0u8; LOG_TAIL_BYTES]),
        report_path: std::cell::UnsafeCell::new([0u16; PATH_UNITS]),
        dump_path: std::cell::UnsafeCell::new([0u16; PATH_UNITS]),
        dump_name: std::cell::UnsafeCell::new([0u8; DUMP_NAME_CAP]),
        out: std::cell::UnsafeCell::new([0u8; 4096]),
        fault: std::cell::UnsafeCell::new(Fault::EMPTY),
    };

    pub(super) fn report_directory() -> Option<&'static str> {
        PATHS.get().map(|paths| paths.directory_display.as_str())
    }

    /// 装处理器。进程里只调用一次；要在 Tauri/WebView2 起来之前调用。
    pub(super) fn install(lite: bool) {
        let Some(local_app_data) = dirs::data_local_dir() else {
            return;
        };
        let layout = layout_for(&local_app_data, lite);
        let product = layout.product;
        let edition = layout.edition;
        // 转储目录的默认值来自 `layout_for`（标识符目录下的 `crashes`，与 `logs` 平级）：
        // 两处落点的规则只写在那个纯函数里。先取出来，因为下面要把其余字段移进 `InstalledPaths`。
        let dump_default_directory = layout.dump_directory;
        let directory = layout.directory;
        let log_file = layout.log_file;
        // 目录先建出来：日志插件起来之前就可能崩，那时它还没建。失败也只在心里记一笔 ——
        // 报告是尽力而为的东西，绝不拖住启动。
        let _ = std::fs::create_dir_all(&directory);
        let process_exe = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "（未知）".to_string());

        let mut directory_wide = [0u16; PATH_UNITS];
        let mut log_file_wide = [0u16; PATH_UNITS];
        let Some(directory_len) = wide_path(&directory, &mut directory_wide) else {
            return;
        };
        let Some(log_file_len) = wide_path(&log_file, &mut log_file_wide) else {
            return;
        };
        // 全量转储目录：默认在标识符目录下的 `crashes\`（与 logs 平级）；
        // `DSH_WALLPAPER_CRASH_DUMP_DIR` 可以把它整个换掉（F: 盘之类）。
        // 启用与否、落在哪里，都只在这里读一次 —— 崩溃路径上不碰环境变量。
        let dump_directory = match std::env::var(DUMP_DIRECTORY_ENV) {
            Ok(text) if !text.trim().is_empty() => std::path::PathBuf::from(text.trim()),
            _ => dump_default_directory,
        };
        let dump_enabled = !dump_disabled_by_value(
            std::env::var(DUMP_DISABLE_ENV).ok().as_deref(),
        );
        let mut dump_directory_wide = [0u16; PATH_UNITS];
        let Some(dump_directory_len) = wide_path(&dump_directory, &mut dump_directory_wide) else {
            return;
        };
        if dump_enabled {
            let _ = std::fs::create_dir_all(&dump_directory);
        }
        let paths = InstalledPaths {
            product,
            edition,
            app_version: env!("CARGO_PKG_VERSION"),
            process_exe,
            directory: directory_wide,
            directory_len,
            directory_display: directory.display().to_string(),
            log_file: log_file_wide,
            log_file_len,
            log_display: log_file.display().to_string(),
            dump_directory: dump_directory_wide,
            dump_directory_len,
            dump_directory_display: dump_directory.display().to_string(),
            dump_enabled,
        };
        if PATHS.set(paths).is_err() {
            return;
        }

        // 保留策略：启动时先按写入时间把「最近三份之外」的旧转储删掉。放在启动时（而不是
        // 崩溃时）是有意的：删文件要走文件系统，而崩溃现场每一分把握都值得留着。
        if dump_enabled {
            if let Some(installed) = PATHS.get() {
                let environment = installed.environment();
                prune_dumps(&environment);
            }
        }

        install_panic_hook();
        let previous = unsafe { SetUnhandledExceptionFilter(Some(unhandled_filter)) };
        let _ = PREVIOUS_FILTER.set(previous);
        unsafe {
            AddVectoredExceptionHandler(1, Some(vectored_handler));
        }
    }

    /// UTF-16 路径写进定长缓冲，并补结尾 NUL。返回含 NUL 的长度；放不下返回 `None`。
    fn wide_path(path: &std::path::Path, out: &mut [u16]) -> Option<usize> {
        use std::os::windows::ffi::OsStrExt;
        let mut length = 0usize;
        for unit in path.as_os_str().encode_wide() {
            if length + 1 >= out.len() {
                return None;
            }
            out[length] = unit;
            length += 1;
        }
        out[length] = 0;
        length += 1;
        Some(length)
    }

    fn install_panic_hook() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // panic hook 里绝不碰 log：崩溃可能正发生在日志锁里面，那会把自己挂死。
            // 报告写失败也绝不影响 panic 的既有行为 —— 包一层 catch_unwind，然后无论成败
            // 都把控制权交回上一个 hook（默认 hook 照常往 stderr 打印）。
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if !report_budget_left() {
                    return;
                }
                let message = match info.payload().downcast_ref::<&str>() {
                    Some(text) => *text,
                    None => match info.payload().downcast_ref::<String>() {
                        Some(text) => text.as_str(),
                        None => "（读不出 panic 消息）",
                    },
                };
                let location = info.location().map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                });
                // panic 这条路径堆是好的，但 Backtrace 只在真需要时才抓：`force_capture`
                // 不受 RUST_BACKTRACE 影响，报告的价值就在这里。
                let backtrace = std::backtrace::Backtrace::force_capture().to_string();
                let Some(_guard) = enter() else {
                    return;
                };
                let thread_id = unsafe { GetCurrentThreadId() };
                let facts = PanicFacts {
                    message,
                    location: location.as_deref(),
                    backtrace: &backtrace,
                };
                // 报告配额由 `emit_from_installed` → `emit_with_dump` 自己记账。
                unsafe {
                    emit_from_installed(ReportOrigin::RustPanic, None, Some(&facts), &[], thread_id);
                }
            }));
            previous(info);
        }));
    }

    /// 第一机会：只在致命异常码上落盘。
    unsafe extern "system" fn vectored_handler(pointers: *mut ExceptionPointers) -> i32 {
        if pointers.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let record = (*pointers).record;
        if record.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let Some(paths) = PATHS.get() else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        let environment = paths.environment();
        handle_native(
            &environment,
            ReportOrigin::VectoredFirstChance,
            record,
            (*pointers).context,
            GetCurrentThreadId(),
        )
    }

    /// 顶层过滤器：进程确定要死了。任何未处理异常都落盘，除非第一机会那条已经写过同一处。
    unsafe extern "system" fn unhandled_filter(pointers: *const ExceptionPointers) -> i32 {
        if !pointers.is_null() {
            let record = (*pointers).record;
            if !record.is_null() {
                if let Some(paths) = PATHS.get() {
                    let environment = paths.environment();
                    handle_native(
                        &environment,
                        ReportOrigin::UnhandledFilter,
                        record,
                        (*pointers).context,
                        GetCurrentThreadId(),
                    );
                }
            }
        }
        // 交给之前装过的过滤器（返回值也照它给），没有就继续搜索 —— 两种情况都等价于
        // 「装我们之前的默认行为」，WER 与系统错误提示照旧。
        match PREVIOUS_FILTER.get().copied().flatten() {
            Some(previous) => previous(pointers),
            None => EXCEPTION_CONTINUE_SEARCH,
        }
    }

    /// 两个原生入口的共同本体：抢旗（重入保护）→ 过滤 →（去重）→ 抓栈 → 记现场 → 写报告。
    ///
    /// **返回值永远、也只能是 [`EXCEPTION_CONTINUE_SEARCH`]**：它由这一个函数决定，
    /// 单测直接钉住这个数。返回 `EXCEPTION_EXECUTE_HANDLER` 会吞掉异常、连 WER 都不跑，
    /// 那正是这里绝不能发生的事。
    pub(crate) unsafe fn handle_native(
        environment: &ReportEnvironment<'_>,
        origin: ReportOrigin,
        record: *mut ExceptionRecord,
        context: *mut c_void,
        thread_id: u32,
    ) -> i32 {
        // 重入保护：已经（或正在，可能是另一条线程）写报告就直接收手 —— 绝不递归。
        let Some(_guard) = enter() else {
            return EXCEPTION_CONTINUE_SEARCH;
        };
        let code = (*record).code;
        let address = (*record).address as u64;
        // 第一机会那条只看致命码；顶层过滤器被叫到时进程已经要死了，任何码都留证据。
        if origin == ReportOrigin::VectoredFirstChance && !is_fatal_exception_code(code) {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        if origin == ReportOrigin::UnhandledFilter {
            let fault = &*SCRATCH.fault.get();
            // 第一机会已经为**同一条**异常写过一份；再写一份只是同一个现场的副本。
            if fault.found
                && fault.reported
                && fault.code == code
                && fault.address == address
                && fault.thread_id == thread_id
            {
                return EXCEPTION_CONTINUE_SEARCH;
            }
        }
        if origin == ReportOrigin::VectoredFirstChance {
            *SCRATCH.fault.get() = Fault {
                found: true,
                reported: false,
                code,
                address,
                thread_id,
            };
        }
        if report_budget_left() {
            let frames_buffer: &mut [u64] = &mut *SCRATCH.frames.get();
            let frames = capture_frames(frames_buffer);
            let facts = exception_facts(record, context, thread_id);
            if emit_with_dump(environment, origin, Some(&facts), None, frames, thread_id) {
                let fault = &mut *SCRATCH.fault.get();
                if origin == ReportOrigin::VectoredFirstChance {
                    fault.reported = true;
                } else {
                    fault.found = true;
                }
                // 报告配额由 `emit_with_dump` 自己记账（它是唯一知道报告有没有真的写出去的人）。
            }
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    /// 测试用：造一条假的异常记录，好把处理器的语义（过滤、去重、返回码）钉住。
    #[cfg(test)]
    pub(crate) fn exception_record(
        code: u32,
        address: usize,
        parameters: &[usize],
    ) -> ExceptionRecord {
        let count = parameters.len().min(15);
        let mut record = ExceptionRecord {
            code,
            flags: 0,
            record: std::ptr::null_mut(),
            address: address as *mut c_void,
            number_parameters: count as u32,
            information: [0usize; 15],
        };
        record.information[..count].copy_from_slice(&parameters[..count]);
        record
    }

    /// 把异常记录摊成报告要的字段。参数只取真正有意义的那些（其余是未初始化的槽位）。
    unsafe fn exception_facts(
        record: *const ExceptionRecord,
        context: *mut c_void,
        thread_id: u32,
    ) -> ExceptionFacts<'static> {
        let record: &'static ExceptionRecord = &*record;
        let count = (record.number_parameters as usize).min(record.information.len());
        // 异常记录在分发期间一直有效（它由系统放在栈上），所以这里可以借用整段生命周期。
        let parameters: &'static [usize] = &record.information[..count];
        ExceptionFacts {
            code: record.code,
            address: record.address as u64,
            flags: record.flags,
            thread_id,
            parameters,
            thread_context: context,
        }
    }

    /// 抓出错线程的调用栈（返回地址）。返回抓到的那一段。
    pub(crate) fn capture_frames(out: &mut [u64]) -> &[u64] {
        let count = out.len().min(MAX_FRAMES);
        if count == 0 {
            return &[];
        }
        let captured = unsafe {
            RtlCaptureStackBackTrace(
                0,
                count as u32,
                out.as_mut_ptr() as *mut *mut c_void,
                std::ptr::null_mut(),
            )
        } as usize;
        let captured = captured.min(count);
        &out[..captured]
    }

    /// 枚举已加载模块，写进 `out`，返回 `(实际加载数, 写入条数)`。
    ///
    /// 先走 PEB 里的加载器链表（只读内存、不分配、不取加载器锁 —— 崩溃发生在加载器锁被持有
    /// 的时候也不至于把自己挂死），失败再退回 ToolHelp 快照。
    pub(crate) fn collect_modules(out: &mut [ModuleRecord]) -> (usize, usize) {
        let (total, written) = unsafe { walk_loader_modules(out) };
        if written > 0 {
            return (total, written);
        }
        unsafe { toolhelp_modules(out) }
    }

    /// 走 PEB → `Ldr` → `InMemoryOrderModuleList`。
    ///
    /// 这些偏移来自 winternl.h 的结构定义，几十年没变过；为了少写一份拿不准的偏移表，
    /// 这里**只声明 x64 的那一份**，其它架构直接交给 ToolHelp（本项目发布的就是 x64）。
    #[cfg(target_arch = "x86_64")]
    unsafe fn walk_loader_modules(out: &mut [ModuleRecord]) -> (usize, usize) {
        // x64：`gs:[0x30]` 是 TEB 自身，TEB+0x60 是 PEB；PEB+0x18 是 Ldr；
        // PEB_LDR_DATA+0x20 是 InMemoryOrderModuleList；LDR_DATA_TABLE_ENTRY 里
        // +0x10 是 InMemoryOrderLinks、+0x30 DllBase、+0x40 SizeOfImage、+0x58 BaseDllName。
        const TEB_SELF: usize = 0x30;
        const TEB_PEB: usize = 0x60;
        const PEB_LDR: usize = 0x18;
        const LDR_IN_MEMORY_ORDER_LIST: usize = 0x20;
        const ENTRY_IN_MEMORY_ORDER_LINKS: usize = 0x10;
        const ENTRY_DLL_BASE: usize = 0x30;
        const ENTRY_SIZE_OF_IMAGE: usize = 0x40;
        const ENTRY_BASE_DLL_NAME: usize = 0x58;

        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            buffer: *const u16,
        }

        let teb: usize;
        core::arch::asm!(
            "mov {}, gs:[{offset}]",
            out(reg) teb,
            offset = const TEB_SELF,
            options(nomem, nostack, preserves_flags)
        );
        if teb == 0 {
            return (0, 0);
        }
        let peb = *((teb as *const u8).add(TEB_PEB) as *const *const u8);
        if peb.is_null() {
            return (0, 0);
        }
        let ldr = *(peb.add(PEB_LDR) as *const *const u8);
        if ldr.is_null() {
            return (0, 0);
        }
        let head = ldr.add(LDR_IN_MEMORY_ORDER_LIST) as *const u8;
        let mut node = *(head as *const *const u8) as *const u8;
        let mut total = 0usize;
        let mut written = 0usize;
        // 链表坏了的时候不能无限转：加载器里的条目数是有限的，给一个远大于真实值的上限。
        while !node.is_null() && node != head && total < 4096 {
            let entry = node.sub(ENTRY_IN_MEMORY_ORDER_LINKS);
            let base = *(entry.add(ENTRY_DLL_BASE) as *const usize) as u64;
            let size = u64::from(*(entry.add(ENTRY_SIZE_OF_IMAGE) as *const u32));
            let name = &*entry.add(ENTRY_BASE_DLL_NAME).cast::<UnicodeString>();
            let units: &[u16] = if name.buffer.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(name.buffer, usize::from(name.length) / 2)
            };
            if written < out.len() {
                out[written] = ModuleRecord::from_wide(base, size, units);
                written += 1;
            }
            total += 1;
            node = *(node as *const *const u8) as *const u8;
        }
        (total, written)
    }

    #[cfg(not(target_arch = "x86_64"))]
    unsafe fn walk_loader_modules(_out: &mut [ModuleRecord]) -> (usize, usize) {
        (0, 0)
    }

    /// 退路：ToolHelp 快照。要内核替我们建一份模块列表，所以它自己会分配，
    /// 但在 PEB 结构不可信的时候它是唯一还能用的办法。
    pub(crate) unsafe fn toolhelp_modules(out: &mut [ModuleRecord]) -> (usize, usize) {
        let flags = CREATE_TOOLHELP_SNAPSHOT_FLAGS(TH32CS_SNAPMODULE.0 | TH32CS_SNAPMODULE32.0);
        let snapshot = match CreateToolhelp32Snapshot(flags, GetCurrentProcessId()) {
            Ok(handle) => handle,
            Err(_) => return (0, 0),
        };
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut total = 0usize;
        let mut written = 0usize;
        if Module32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if written < out.len() {
                    out[written] = ModuleRecord::from_wide(
                        entry.modBaseAddr as u64,
                        u64::from(entry.modBaseSize),
                        &entry.szModule,
                    );
                    written += 1;
                }
                total += 1;
                if total >= 4096 || Module32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        (total, written)
    }

    /// 报告按基址升序（人是按地址找模块的）；插入排序，条数有上限，最坏情况也是微秒级。
    pub(crate) fn sort_modules(modules: &mut [ModuleRecord]) {
        for index in 1..modules.len() {
            let mut position = index;
            while position > 0 && modules[position - 1].base() > modules[position].base() {
                modules.swap(position - 1, position);
                position -= 1;
            }
        }
    }

    /// 回读应用日志的最后 [`LOG_TAIL_BYTES`] 个字节，修成合法 UTF-8 返回。
    ///
    /// 打开方式是只读 + 全共享：日志文件正被日志插件打开着，轮换时还会被改名/删除，
    /// 全共享能让这些操作都不被我们挡住。
    unsafe fn read_log_tail(log_wide: &[u16]) -> &'static str {
        let bytes: *mut u8 = (*SCRATCH.log_bytes.get()).as_mut_ptr();
        let capacity = LOG_TAIL_BYTES;
        let share = FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);
        let handle = match CreateFileW(
            PCWSTR::from_raw(log_wide.as_ptr()),
            GENERIC_READ.0,
            share,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            Ok(handle) => handle,
            Err(_) => {
                return "（打不开日志文件：崩溃那一刻它可能刚被轮换掉）";
            }
        };
        let mut size = 0i64;
        let has_size = GetFileSizeEx(handle, &mut size).is_ok();
        // 比缓冲大就从「末尾往前 capacity 字节」开始，否则从头读。
        // （`0` 配 `FILE_END` 会停在文件末尾、一个字节也读不到 —— 这里必须分两种情况。）
        let positioned = if has_size && size > capacity as i64 {
            SetFilePointerEx(handle, -(capacity as i64), None, FILE_END).is_ok()
        } else {
            SetFilePointerEx(handle, 0, None, FILE_BEGIN).is_ok()
        };
        if !positioned {
            let _ = CloseHandle(handle);
            return "（日志文件读不到：定位失败）";
        }
        let mut read = 0u32;
        let buffer = std::slice::from_raw_parts_mut(bytes, capacity);
        let ok = ReadFile(handle, Some(buffer), Some(&mut read), None).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return "（日志文件读不到：读取失败）";
        }
        let length = (read as usize).min(capacity);
        // 暂存区是静态的，所以这段借用在进程生命周期内一直有效（写入者只有本函数，
        // 且已在 ReportGuard 之下）。
        let slice: &'static mut [u8] = std::slice::from_raw_parts_mut(bytes, length);
        scrub_utf8(slice)
    }

    /// 尽最大努力把「这一条异常」写成一份报告，返回是否真的写出了文件。
    unsafe fn emit_from_installed(
        origin: ReportOrigin,
        exception: Option<&ExceptionFacts<'_>>,
        panic: Option<&PanicFacts<'_>>,
        frames: &[u64],
        thread_id: u32,
    ) -> bool {
        match PATHS.get() {
            Some(paths) => {
                let environment = paths.environment();
                emit_with_dump(&environment, origin, exception, panic, frames, thread_id)
            }
            None => false,
        }
    }

    /// 报告落盘。全程只用静态缓冲与栈缓冲：不分配堆、不 panic、不取锁。
    ///
    /// 入口条件：调用方已经抢到「正在写报告」那把旗（见 [`enter`]）。**自己不再抢旗**——
    /// 那把旗是排他锁，重入会把自己挡在门外。公开入口是 [`emit_with_dump`] 与
    /// [`handle_native`]，两者都先抢旗再走到这里。
    pub(crate) unsafe fn emit(
        environment: &ReportEnvironment<'_>,
        origin: ReportOrigin,
        exception: Option<&ExceptionFacts<'_>>,
        panic: Option<&PanicFacts<'_>>,
        frames: &[u64],
        thread_id: u32,
        dump: Option<DumpFacts<'_>>,
    ) -> bool {
        let local_time = local_time();
        let utc_time = utc_time();

        // 文件名：crash-<YYYYMMDD-HHMMSS-mmm>.txt
        let mut stamp = [0u8; 32];
        let stamp_length = local_time.write_file_stamp(&mut stamp);
        let stamp = ascii_str(&stamp[..stamp_length.min(stamp.len())]);
        let mut name = [0u8; 64];
        let report_path: &mut [u16] = &mut *SCRATCH.report_path.get();
        let mut file_name_length = 0usize;
        let mut ordinal = 1u32;
        // 撞名就加序号：同一毫秒里的第二份报告多半来自另一条线程，不能让它盖掉第一份。
        // 探测上限是 16，而一个进程最多只写 8 份（见 MAX_REPORTS_PER_PROCESS），所以
        // 「探测完都没位置、只能盖掉」这条路实际上走不到。
        while ordinal <= 16 {
            file_name_length = write_report_file_name(&mut name, stamp, ordinal);
            let candidate = ascii_str(&name[..file_name_length.min(name.len())]);
            if join_path(environment.directory_wide, candidate, report_path) == 0 {
                return false;
            }
            if !file_exists(report_path) {
                break;
            }
            ordinal += 1;
        }
        let file_name = ascii_str(&name[..file_name_length.min(name.len())]);

        // 目录万一被用户删了：尽力重建一次。失败就放弃（比如权限问题）。
        let _ = CreateDirectoryW(PCWSTR::from_raw(environment.directory_wide.as_ptr()), None);

        let share = FILE_SHARE_MODE(FILE_SHARE_READ.0);
        let handle = match CreateFileW(
            PCWSTR::from_raw(report_path.as_ptr()),
            GENERIC_WRITE.0,
            share,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            Ok(handle) => handle,
            Err(_) => return false,
        };

        let modules: &mut [ModuleRecord] = &mut *SCRATCH.modules.get();
        let (module_total, listed) = collect_modules(modules);
        sort_modules(&mut modules[..listed]);
        let log_tail = read_log_tail(environment.log_wide);

        let mut os_version_buffer = [0u8; 32];
        let os_version_length = os_version_into(&mut os_version_buffer);
        let os_version = if os_version_length == 0 {
            "（未知）"
        } else {
            ascii_str(&os_version_buffer[..os_version_length.min(os_version_buffer.len())])
        };

        let facts = ReportFacts {
            product: environment.product,
            edition: environment.edition,
            app_version: environment.app_version,
            os_version,
            os_arch: std::env::consts::ARCH,
            process_id: GetCurrentProcessId(),
            process_exe: environment.process_exe,
            thread_id,
            origin,
            file_name,
            directory_display: environment.directory_display,
            dump,
            local_time,
            utc_time,
            exception: exception.copied(),
            panic: panic.copied(),
            frames,
            modules: &modules[..listed],
            module_total,
            log_display: environment.log_display,
            log_tail,
        };

        let mut sink = FileSink {
            handle,
            buffer: &mut *SCRATCH.out.get(),
            filled: 0,
            failed: false,
        };
        write_report(&mut sink, &facts);
        sink.flush();
        let _ = CloseHandle(handle);
        !sink.failed
    }

    /// 「先写全量转储、再写文本报告」的入口（panic hook 与两个原生处理器都走这里）。
    ///
    /// 顺序是有意的：转储要在现场最完整的时候写（早），而报告的正文里要**写清转储落在哪**
    /// （所以转储必须先落盘）。
    ///
    /// **入口条件：调用方已经抢到「正在写报告」那把旗（见 [`enter`]）**——旗是排他锁，
    /// 这里不再抢第二次：重入会被自己挡在门外，那会让所有原生崩溃与 panic 一个字节都写不出来。
    ///
    /// 返回值与 [`emit`] 一样：**报告**是否真的写出了文件。转储写没写成，看报告。
    pub(crate) unsafe fn emit_with_dump(
        environment: &ReportEnvironment<'_>,
        origin: ReportOrigin,
        exception: Option<&ExceptionFacts<'_>>,
        panic: Option<&PanicFacts<'_>>,
        frames: &[u64],
        thread_id: u32,
    ) -> bool {
        if !report_budget_left() {
            return false;
        }
        let local_time = local_time();
        let mut stamp = [0u8; 32];
        let stamp_length = local_time.write_file_stamp(&mut stamp);
        let stamp = ascii_str(&stamp[..stamp_length.min(stamp.len())]);

        let dump_write = write_dump(environment, exception, stamp, thread_id);

        // 报告里的这一段要区分两件事：转储写成了（给路径与大小），还是没写成（给原因）。
        let facts = match dump_write.error {
            Some(reason) => DumpFacts {
                directory: environment.dump_directory_display,
                file_name: "",
                path: "",
                size: None,
                pruned: 0,
                error: Some(reason),
                error_code: dump_write.error_code,
            },
            None => {
                // `write_dump` 已经把名字与路径写进静态暂存区；旗还在我们手里，没人能改。
                let name_buffer = dump_name_buffer();
                let length = name_buffer
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(name_buffer.len());
                let name = ascii_str(&name_buffer[..length.min(name_buffer.len())]);
                let path = dump_path_display(dump_path_buffer());
                DumpFacts {
                    directory: environment.dump_directory_display,
                    file_name: name,
                    path,
                    size: dump_write.size,
                    pruned: dump_write.pruned,
                    error: None,
                    error_code: 0,
                }
            }
        };

        if emit(environment, origin, exception, panic, frames, thread_id, Some(facts)) {
            spend_report_budget();
            true
        } else {
            false
        }
    }

    /// 全量转储的落盘与保留策略都在这里；文本报告只借用其中一小段（`emit_with_dump`）。
    // -----------------------------------------------------------------------

    /// 报告目录里那份转储的 **UTF-16** 路径暂存（文本报告里把它当 `dump.path` 用）。
    fn dump_path_buffer() -> &'static mut [u16] {
        unsafe { &mut *SCRATCH.dump_path.get() }
    }

    /// 报告里那份转储的 **UTF-8** 名字暂存（`emit` 拿它填 `DumpFacts`）。
    fn dump_name_buffer() -> &'static mut [u8] {
        unsafe { &mut *SCRATCH.dump_name.get() }
    }

    /// 写转储的实测结局，供文本报告用。
    ///
    /// 只带「成了没有（`error` 为空即成了）/ 为什么没成 / 多大 / 删了几份」；
    /// 文件名与路径直接留在静态暂存区里（`SCRATCH.dump_name` 与 `SCRATCH.dump_path`），
    /// 报告正文再从那里读一次 —— 这样这个结构里不带任何借用，出了 `write_dump` 也还能用。
    struct DumpWrite {
        /// 失败或没尝试时给报告看的说明（全静态串，不分配）；`None` 表示写出了转储。
        error: Option<&'static str>,
        /// 失败时的 `GetLastError()`（0 表示「不是系统调用失败，是我们自己没写」）。
        error_code: u32,
        /// 写出去了才有：字节数（取不到大小时是 `None`）。
        size: Option<u64>,
        /// 这次转储把几份更旧的删掉了（保留策略）。
        pruned: usize,
    }

    impl DumpWrite {
        /// 没写、也不打算写（非原生异常、被关掉、单测）。`error` 会进报告的 `dump.error`。
        fn skipped(error: &'static str) -> Self {
            Self {
                error: Some(error),
                error_code: 0,
                size: None,
                pruned: 0,
            }
        }
    }

    /// 试写一份全量转储。**不返回错误码**：报告只需要「写出去了没有 / 为什么没有」。
    ///
    /// 入口条件：调用方已经抢到「正在写报告」那把旗（见 [`enter`]），所以这里对
    /// `SCRATCH` 的写入是排他的，且这一次崩溃只会有一次转储尝试。
    unsafe fn write_dump(
        environment: &ReportEnvironment<'_>,
        exception: Option<&ExceptionFacts<'_>>,
        stamp: &str,
        thread_id: u32,
    ) -> DumpWrite {
        if !environment.dump_enabled {
            // 安装时就被关掉了（环境变量）。报告里说明原因，省得人去 crashes 目录白找。
            return DumpWrite::skipped(
                "按环境变量 DSH_WALLPAPER_NO_CRASH_DUMP 关闭：本次进程不写全量转储",
            );
        }
        let Some(exception) = exception else {
            // 非原生异常（Rust panic）：没有异常记录与出错线程上下文，写出来的转储
            // 只有「基础」部分，价值远低于成本，所以不写。
            return DumpWrite::skipped(
                "这不是原生异常（没有异常记录与出错线程上下文），跳过全量转储",
            );
        };
        if cfg!(test) {
            // 单测：一次全量转储几百 MB、还要锁定整个进程地址空间，跑几十遍测试不值得。
            // 名字、路径拼接与保留策略各有单独的测试钉住（这里只跳过真正落盘那一步）。
            return DumpWrite::skipped("单测环境：跳过全量转储（名字与保留策略另有单测）");
        }

        // 文件名与路径：与文本报告同一个时间戳，配上这次异常的代码。
        let mut name = [0u8; DUMP_NAME_CAP];
        let name_length = write_dump_file_name(&mut name, stamp, Some(exception.code));
        if name_length == 0 {
            return DumpWrite::skipped("转储文件名装不进定长缓冲，放弃写转储");
        }
        let name = ascii_str(&name[..name_length.min(name.len())]);
        // 路径写进静态暂存区（报告正文要用；报告写完之前没人能改它 —— 旗在我们手里）。
        let path = dump_path_buffer();
        if join_path(environment.dump_directory_wide, name, path) == 0 {
            return DumpWrite::skipped("转储目录路径装不进定长缓冲，放弃写转储");
        }

        // 目录万一被删了：尽力重建一次（失败就交给下面的 CreateFileW 报错）。
        let _ = CreateDirectoryW(
            PCWSTR::from_raw(environment.dump_directory_wide.as_ptr()),
            None,
        );

        // 全量转储动辄几百 MB，给足写缓存；全共享是为了「用户正拿着资源管理器看这个目录」
        // 这种无害的情况不挡路。CREATE_ALWAYS：文件名带毫秒时间戳，撞名几乎不可能。
        let share = FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0);
        let path_ptr = path.as_ptr();
        let handle = match CreateFileW(
            PCWSTR::from_raw(path_ptr),
            GENERIC_WRITE.0,
            share,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            Ok(handle) => handle,
            Err(_) => {
                return DumpWrite {
                    error: Some("打不开转储文件（crashes 目录权限或磁盘空间问题）"),
                    error_code: GetLastError().0,
                    size: None,
                    pruned: 0,
                };
            }
        };
        let exception_information = MiniDumpExceptionInformation {
            thread_id,
            // 调用方给的指针本来就是可写的（系统把异常记录放在栈上），这里只是去掉 const。
            exception_pointers: exception.thread_context as *mut ExceptionPointers,
            // 0：进程内写自己的转储，指针就是本进程的地址（跨进程才需要置 1）。
            client_pointers: 0,
        };
        let written = MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            handle,
            DUMP_TYPE_FLAGS,
            &exception_information,
            std::ptr::null(),
            std::ptr::null(),
        );
        let last_error = if written == 0 {
            GetLastError().0
        } else {
            0
        };
        // 文件还没关的时候问大小（此刻它已经写完，句柄还在我们手上）。
        let mut size = 0i64;
        let sized = written != 0 && GetFileSizeEx(handle, &mut size).is_ok() && size > 0;
        let _ = CloseHandle(handle);

        if written == 0 {
            // 失败只记一行错误码：绝不重试、绝不 panic（现在还在崩溃分发里）。
            let message = if last_error == E_INVALIDARG {
                "MiniDumpWriteDump 报 E_INVALIDARG：失败（参数不被接受）"
            } else {
                "MiniDumpWriteDump 失败（错误码见 dump.error.code）"
            };
            // 失败时把半截文件删掉：留着只会让人误以为拿到了转储。
            let _ = DeleteFileW(PCWSTR::from_raw(path_ptr));
            return DumpWrite {
                error: Some(message),
                error_code: last_error,
                size: None,
                pruned: 0,
            };
        }

        // 保留策略留给**下一次启动**（`prune_dumps`）：崩溃现场不删用户的文件。
        DumpWrite {
            error: None,
            error_code: 0,
            size: if sized { Some(size as u64) } else { None },
            pruned: 0,
        }
    }

    /// 转储的完整路径（UTF-16 定长缓冲）→ 报告里的显示串。
    ///
    /// 报告里要的是**看得懂的路径**，而路径缓冲是 UTF-16。这里不分配：直接借用静态缓冲
    /// （报告写完之后没人再用它），代价是报告里那句 `dump.path` 的生命周期与整个进程一样长。
    unsafe fn dump_path_display(path_wide: &[u16]) -> &'static str {
        // 结尾 NUL 之前的那一段才是路径。
        let length = path_wide
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(path_wide.len());
        // 路径在本机（%LOCALAPPDATA%）一律是 ASCII 可打印字符；万一有非 ASCII 码元，
        // 就地写成 `?` 会破坏 `SCRATCH` 的路径缓冲 —— 所以这里用一个纯 ASCII 的降级串。
        for unit in &path_wide[..length] {
            if *unit == 0 || *unit > 0x7e {
                return "（路径含非 ASCII 字符，见 dump.file：它在 crashes 目录里）";
            }
        }
        let bytes = std::slice::from_raw_parts(path_wide.as_ptr() as *const u8, length);
        ascii_str(bytes)
    }

    /// 按文件名与写入时间枚举 `crashes` 目录（只读，不删任何东西）。
    ///
    /// `FindFirstFileW` 而不是 `std::fs::read_dir`：后者要分配堆，而这条路的调用方
    /// 可能正处在堆已经坏掉的崩溃现场。这里只用栈上定长结构。
    unsafe fn enumerate_dumps(directory_wide: &[u16], out: &mut [DumpEntry; DUMP_LIST_CAP]) -> (usize, usize) {
        // 目录 + `\*`：`FindFirstFileW` 只认通配符形式的目录名。
        let mut pattern = [0u16; PATH_UNITS];
        let mut length = 0usize;
        for unit in directory_wide {
            if *unit == 0 {
                break;
            }
            if length + 3 >= pattern.len() {
                return (0, 0);
            }
            pattern[length] = *unit;
            length += 1;
        }
        if length > 0 && pattern[length - 1] != u16::from(b'\\') && pattern[length - 1] != u16::from(b'/')
        {
            pattern[length] = u16::from(b'\\');
            length += 1;
        }
        for unit in [u16::from(b'*'), 0] {
            if length + 1 >= pattern.len() {
                return (0, 0);
            }
            pattern[length] = unit;
            length += 1;
        }

        let mut find_data = WIN32_FIND_DATAW::default();
        let handle = match FindFirstFileW(PCWSTR::from_raw(pattern.as_ptr()), &mut find_data) {
            Ok(handle) => handle,
            Err(_) => return (0, 0),
        };
        let mut total = 0usize;
        let mut listed = 0usize;
        loop {
            // 目录本身（`.` / `..`）与子目录一律不算：`crash-*.dmp` 已经排除了它们，
            // 但枚举时顺手挡掉，免得日后有人改前缀时忘了这件事。
            let units = find_data.cFileName;
            let name_length = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
            let mut name_buffer = [0u8; DUMP_NAME_CAP];
            let mut name_len = 0usize;
            let mut ascii = true;
            for unit in &units[..name_length] {
                if *unit > 0x7f || name_len + 1 >= name_buffer.len() {
                    ascii = false;
                    break;
                }
                name_buffer[name_len] = *unit as u8;
                name_len += 1;
            }
            total += 1;
            if ascii && listed < out.len() {
                let name = ascii_str(&name_buffer[..name_len]);
                if !name.is_empty() {
                    let file_time = u64::from(find_data.ftLastWriteTime.dwLowDateTime)
                        | (u64::from(find_data.ftLastWriteTime.dwHighDateTime) << 32);
                    if let Some(entry) = DumpEntry::new(name, file_time) {
                        out[listed] = entry;
                        listed += 1;
                    }
                }
            }
            if total >= 4096 || FindNextFileW(handle, &mut find_data).is_err() {
                break;
            }
        }
        let _ = FindClose(handle);
        (total, listed)
    }

    /// 启动时的保留策略：只留最近 [`KEEP_DUMP_FILES`] 份全量转储，更旧的删掉。
    ///
    /// **这是唯一会删文件的地方，而且只在启动时调用**。它删的东西必须同时满足两条：
    /// 在崩溃转储目录里，且名字是 `crash-*.dmp`。其余文件（用户自己的、别的工具的）
    /// 一个都不碰；枚举或删除失败也只是「这次没清理」，绝不影响启动。
    pub(super) fn prune_dumps(environment: &ReportEnvironment<'_>) {
        // `out` 放在堆以外的栈上：这一步在启动时跑，堆是好的，但保持同一套纪律没有坏处。
        let mut entries = [DumpEntry::EMPTY; DUMP_LIST_CAP];
        let (total, listed) = unsafe { enumerate_dumps(environment.dump_directory_wide, &mut entries) };
        if total == 0 {
            return;
        }
        let deletable = partition_dumps_for_retention(&mut entries[..listed], KEEP_DUMP_FILES);
        if deletable == 0 {
            return;
        }
        // 前半段留下、后半段删掉（`partition_dumps_for_retention` 已经把顺序排好）。
        for entry in entries[listed - deletable..listed].iter() {
            let mut path = [0u16; PATH_UNITS];
            if join_path(environment.dump_directory_wide, entry.name(), &mut path) == 0 {
                continue;
            }
            unsafe {
                let _ = DeleteFileW(PCWSTR::from_raw(path.as_ptr()));
            }
        }
    }

    pub(crate) fn local_time() -> LocalTime {
        let mut raw = NativeSystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetLocalTime(&mut raw) };
        LocalTime {
            year: raw.year,
            month: raw.month,
            day: raw.day,
            hour: raw.hour,
            minute: raw.minute,
            second: raw.second,
            millis: raw.milliseconds,
        }
    }

    pub(crate) fn utc_time() -> LocalTime {
        let mut raw = NativeSystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe { GetSystemTime(&mut raw) };
        LocalTime {
            year: raw.year,
            month: raw.month,
            day: raw.day,
            hour: raw.hour,
            minute: raw.minute,
            second: raw.second,
            millis: raw.milliseconds,
        }
    }

    /// `10.0.26100`（`RtlGetVersion` 给的才是真话）。写不下或调用失败返回 0。
    pub(crate) fn os_version_into(out: &mut [u8]) -> usize {
        let mut info = OsVersionInfoW {
            size: size_of::<OsVersionInfoW>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
            csd_version: [0u16; 128],
        };
        let status = unsafe { RtlGetVersion(&mut info) };
        if status != 0 {
            return 0;
        }
        let mut cursor = 0usize;
        push_decimal(out, &mut cursor, info.major, 0);
        push_byte(out, &mut cursor, b'.');
        push_decimal(out, &mut cursor, info.minor, 0);
        push_byte(out, &mut cursor, b'.');
        push_decimal(out, &mut cursor, info.build, 0);
        cursor
    }

    /// 定长缓冲的输出端：满了就 `WriteFile` 一次。一次失败之后不再尝试（磁盘满、权限之类
    /// 的问题不该在崩溃现场反复折腾）。
    struct FileSink<'a> {
        handle: HANDLE,
        buffer: &'a mut [u8],
        filled: usize,
        failed: bool,
    }

    impl FileSink<'_> {
        fn flush(&mut self) {
            if self.filled == 0 || self.failed {
                self.filled = 0;
                return;
            }
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    Some(&self.buffer[..self.filled]),
                    Some(&mut written),
                    None,
                )
            };
            if ok.is_err() || written as usize != self.filled {
                self.failed = true;
            }
            self.filled = 0;
        }
    }

    impl ReportSink for FileSink<'_> {
        fn write_str(&mut self, text: &str) {
            if self.failed {
                return;
            }
            let mut remaining = text.as_bytes();
            while !remaining.is_empty() {
                if self.filled == self.buffer.len() {
                    self.flush();
                    if self.failed {
                        return;
                    }
                }
                let space = self.buffer.len() - self.filled;
                let take = space.min(remaining.len());
                self.buffer[self.filled..self.filled + take].copy_from_slice(&remaining[..take]);
                self.filled += take;
                remaining = &remaining[take..];
            }
        }
    }
}

/// 装上崩溃自证。进程里只调用一次，且要趁早（Tauri/WebView2 之前）。
pub(crate) fn install(lite: bool) {
    #[cfg(windows)]
    native::install(lite);
    #[cfg(not(windows))]
    {
        // 非 Windows 没有 SEH：panic hook 那条仍然有意义，但那套处理器不存在。
        let _ = lite;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_time_fixture() -> LocalTime {
        LocalTime {
            year: 2025,
            month: 10,
            day: 4,
            hour: 20,
            minute: 39,
            second: 12,
            millis: 473,
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn modules_fixture() -> Vec<ModuleRecord> {
        vec![
            ModuleRecord::from_wide(0x0000_7ff6_a000_0000, 0x00a2_f000, &wide("dsh-wallpaper.exe")),
            ModuleRecord::from_wide(0x0000_7ff8_c0c0_0000, 0x0020_0000, &wide("KERNEL32.DLL")),
        ]
    }

    fn modules_fixture_single() -> Vec<ModuleRecord> {
        vec![ModuleRecord::from_wide(
            0x1000,
            0x10000,
            &wide("dsh-wallpaper.exe"),
        )]
    }

    fn facts_fixture<'a>(
        modules: &'a [ModuleRecord],
        frames: &'a [u64],
        exception: Option<ExceptionFacts<'a>>,
        panic: Option<PanicFacts<'a>>,
        log_tail: &'a str,
    ) -> ReportFacts<'a> {
        facts_fixture_with_dump(modules, frames, exception, panic, log_tail, None)
    }

    /// `dump` 为 `None` 表示「根本没尝试写转储」；`Some(reason)` 表示试过、没写成。
    /// 写成了那种（带路径与大小）由 `expect_dump_written_facts` 单独造，好在正文里钉住字段。
    fn facts_fixture_with_dump<'a>(
        modules: &'a [ModuleRecord],
        frames: &'a [u64],
        exception: Option<ExceptionFacts<'a>>,
        panic: Option<PanicFacts<'a>>,
        log_tail: &'a str,
        dump: Option<&'a str>,
    ) -> ReportFacts<'a> {
        ReportFacts {
            product: "dsh-wallpaper",
            edition: "full",
            app_version: "0.4.7",
            os_version: "10.0.26100",
            os_arch: "x86_64",
            process_id: 26136,
            process_exe: r"C:\Program Files\dsh-wallpaper\dsh-wallpaper.exe",
            thread_id: 1234,
            origin: ReportOrigin::VectoredFirstChance,
            file_name: "crash-20251004-203912-473.txt",
            directory_display: r"C:\Users\me\AppData\Local\com.dsh.wallpaper\logs",
            dump: dump.map(|reason| DumpFacts {
                directory: DUMP_FIXTURE_DIRECTORY,
                file_name: "",
                path: "",
                size: None,
                pruned: 0,
                error: Some(reason),
                error_code: 0,
            }),
            local_time: local_time_fixture(),
            utc_time: LocalTime {
                hour: 12,
                ..local_time_fixture()
            },
            exception,
            panic,
            frames,
            modules,
            module_total: modules.len(),
            log_display: r"C:\Users\me\AppData\Local\com.dsh.wallpaper\logs\dsh-wallpaper.log",
            log_tail,
        }
    }

    #[test]
    fn the_report_directory_the_dump_directory_and_the_log_file_land_where_the_document_says() {
        // `%LOCALAPPDATA%\com.dsh.wallpaper\logs\crash-<时间戳>.txt`：这个落点是逐字写明的。
        let full = layout_for(std::path::Path::new(r"C:\Users\me\AppData\Local"), false);
        assert_eq!(
            full.directory,
            std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\logs")
        );
        assert_eq!(
            full.log_file,
            std::path::Path::new(
                r"C:\Users\me\AppData\Local\com.dsh.wallpaper\logs\dsh-wallpaper.log"
            )
        );
        assert_eq!(full.product, "dsh-wallpaper");
        assert_eq!(full.edition, "full");
        // 转储目录与报告目录平级（`logs` 与 `crashes` 同在一个标识符目录下），规则同出一个函数。
        assert_eq!(
            full.dump_directory,
            std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper\crashes")
        );

        // Lite 版是同一套规则、另一套标识符与产品名（与 tauri.lite.conf.json 一致）。
        let lite = layout_for(std::path::Path::new(r"C:\Users\me\AppData\Local"), true);
        assert_eq!(
            lite.directory,
            std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper.lite\logs")
        );
        assert_eq!(
            lite.log_file,
            std::path::Path::new(
                r"C:\Users\me\AppData\Local\com.dsh.wallpaper.lite\logs\dsh-wallpaper-lite.log"
            )
        );
        assert_eq!(
            lite.dump_directory,
            std::path::Path::new(r"C:\Users\me\AppData\Local\com.dsh.wallpaper.lite\crashes")
        );
        assert_eq!(lite.product, "dsh-wallpaper-lite");
        assert_eq!(lite.edition, "lite");
    }

    #[test]
    fn the_file_stamp_is_sortable_and_free_of_separator_surprises() {
        let mut buffer = [0u8; 32];
        let length = local_time_fixture().write_file_stamp(&mut buffer);
        assert_eq!(ascii_str(&buffer[..length]), "20251004-203912-473");
    }

    #[test]
    fn the_report_timestamp_is_zero_padded_and_carries_milliseconds() {
        let mut buffer = [0u8; 32];
        let length = local_time_fixture().write_report_timestamp(&mut buffer);
        assert_eq!(ascii_str(&buffer[..length]), "2025-10-04 20:39:12.473");
    }

    #[test]
    fn a_single_digit_year_still_gets_four_digits() {
        let time = LocalTime {
            year: 7,
            month: 1,
            day: 2,
            hour: 3,
            minute: 4,
            second: 5,
            millis: 6,
        };
        let mut buffer = [0u8; 32];
        let length = time.write_file_stamp(&mut buffer);
        assert_eq!(ascii_str(&buffer[..length]), "00070102-030405-006");
    }

    #[test]
    fn an_address_inside_a_module_becomes_module_plus_offset() {
        let modules = modules_fixture();
        // 事件日志里那条 0xc0000005 的偏移就是 0x353d17。
        let location = locate_address(0x0000_7ff6_a000_0000 + 0x35_3d17, &modules).unwrap();
        assert_eq!(location.index, 0);
        assert_eq!(location.offset, 0x35_3d17);
        assert!(location.inside);

        let mut out = String::new();
        write_address_location(&mut out, 0x0000_7ff6_a000_0000 + 0x35_3d17, &modules);
        assert_eq!(out, "dsh-wallpaper.exe+0x353d17");
    }

    #[test]
    fn the_module_base_itself_resolves_to_offset_zero() {
        let modules = modules_fixture();
        let location = locate_address(0x0000_7ff6_a000_0000, &modules).unwrap();
        assert_eq!(location.offset, 0);
        assert!(location.inside);
        let mut out = String::new();
        write_address_location(&mut out, 0x0000_7ff6_a000_0000, &modules);
        assert_eq!(out, "dsh-wallpaper.exe+0x0");
    }

    #[test]
    fn the_last_byte_of_a_module_is_still_inside_it() {
        let modules = modules_fixture();
        let base = 0x0000_7ff6_a000_0000;
        let size = 0x00a2_f000;
        assert!(locate_address(base + size - 1, &modules).unwrap().inside);
        // 越界一个字节就不再算它了：那更可能是即时生成的代码。
        assert!(!locate_address(base + size, &modules).unwrap().inside);
        let mut out = String::new();
        write_address_location(&mut out, base + size, &modules);
        assert!(
            out.starts_with("dsh-wallpaper.exe+0xa2f000（超出该模块大小 0xa2f000"),
            "{out}"
        );
    }

    #[test]
    fn the_greatest_base_below_the_address_wins() {
        let modules = vec![
            ModuleRecord::from_wide(0x1000, 0x100, &wide("first.dll")),
            ModuleRecord::from_wide(0x2000, 0x100, &wide("second.dll")),
        ];
        let location = locate_address(0x2050, &modules).unwrap();
        assert_eq!(location.index, 1);
        assert_eq!(location.offset, 0x50);
    }

    #[test]
    fn an_address_below_every_module_and_the_null_address_stay_unattributed() {
        let modules = modules_fixture();
        assert!(locate_address(0x10, &modules).is_none());
        assert!(locate_address(0, &modules).is_none());
        let mut out = String::new();
        write_address_location(&mut out, 0x10, &modules);
        assert_eq!(out, "0x0000000000000010（没有落在任何已枚举模块里）");
    }

    #[test]
    fn a_zero_sized_module_is_still_used_for_the_offset() {
        let modules = vec![ModuleRecord::from_wide(0x1000, 0, &wide("unknown.dll"))];
        let location = locate_address(0x1100, &modules).unwrap();
        assert_eq!(location.offset, 0x100);
        assert!(location.inside, "大小未知时只能认下，偏移仍然是有用的");
    }

    #[test]
    fn a_module_name_longer_than_the_cap_is_truncated_and_stays_valid_utf8() {
        let long = "a".repeat(200);
        let record = ModuleRecord::from_wide(0x1000, 0x10, &wide(&long));
        assert_eq!(record.name().len(), MODULE_NAME_CAP - 1);
        assert!(record.name().chars().all(|character| character == 'a'));
    }

    #[test]
    fn a_non_ascii_wide_name_degrades_instead_of_disappearing() {
        let record = ModuleRecord::from_wide(0x1000, 0x10, &[b'a' as u16, 0x4e2d, b'b' as u16]);
        assert_eq!(record.name(), "a?b");
    }

    #[test]
    fn the_fatal_codes_seen_on_the_real_machine_are_all_covered() {
        assert_eq!(exception_code_name(0xc000_0005), Some("ACCESS_VIOLATION"));
        assert_eq!(
            exception_code_name(0xc000_001d),
            Some("ILLEGAL_INSTRUCTION")
        );
        assert_eq!(exception_code_name(0xc000_0374), Some("HEAP_CORRUPTION"));
        for code in [0xc000_0005u32, 0xc000_001d, 0xc000_0374, 0xc000_0409] {
            assert!(is_fatal_exception_code(code), "{code:#x} 应当算致命");
        }
        // 用于控制流的异常绝不能被当成崩溃记下来。
        for code in [0xe06d_7363u32, 0xe043_4352, 0x8000_0003] {
            assert!(!is_fatal_exception_code(code), "{code:#x} 不算致命");
            assert_eq!(exception_code_name(code), None);
        }
    }

    #[test]
    fn an_access_violation_parameter_gets_its_meaning() {
        assert_eq!(access_violation_operation(0), Some("读取"));
        assert_eq!(access_violation_operation(1), Some("写入"));
        assert_eq!(access_violation_operation(8), Some("执行（DEP/NX）"));
        assert_eq!(access_violation_operation(2), None);
    }

    #[test]
    fn scrubbing_patches_invalid_bytes_in_place_without_changing_the_length() {
        let mut bytes = vec![b'a', 0xff, 0xfe, b'b'];
        assert_eq!(scrub_utf8(&mut bytes), "a??b");
        assert_eq!(bytes.len(), 4);

        // 被截断的多字节字符（日志尾部常见的形态）同样被补掉。
        let mut cut = "中文".as_bytes().to_vec();
        cut.truncate(4);
        let text = scrub_utf8(&mut cut);
        assert_eq!(text.len(), 4);
        assert!(text.starts_with('中'));
        assert!(text.ends_with('?'));
    }

    #[test]
    fn valid_utf8_survives_scrubbing_untouched() {
        let mut bytes = "输入岛点击已核验".as_bytes().to_vec();
        assert_eq!(scrub_utf8(&mut bytes), "输入岛点击已核验");
    }

    #[test]
    fn the_tail_keeps_the_last_lines_in_their_original_order() {
        let text = "1\n2\n3\n4\n5";
        let mut ring = [""; 3];
        assert_eq!(collect_tail(text, &mut ring), 3);
        assert_eq!(ring, ["3", "4", "5"]);

        let mut ring = [""; 10];
        assert_eq!(collect_tail(text, &mut ring), 5);
        assert_eq!(ring[..5], ["1", "2", "3", "4", "5"]);

        let mut ring = [""; 1];
        assert_eq!(collect_tail(text, &mut ring), 1);
        assert_eq!(ring, ["5"]);

        // 正好一轮的时候不能把顺序转错。
        let mut ring = [""; 2];
        assert_eq!(collect_tail("1\n2\n3\n4", &mut ring), 2);
        assert_eq!(ring, ["3", "4"]);

        let mut ring: [&str; 3] = [""; 3];
        assert_eq!(collect_tail("", &mut ring), 0);

        let mut empty: [&str; 0] = [];
        assert_eq!(collect_tail(text, &mut empty), 0);
    }

    #[test]
    fn report_paths_join_with_a_single_separator_and_end_in_nul() {
        let mut out = [0u16; 64];
        let length = join_path(&wide(r"C:\logs"), "crash-1.txt", &mut out);
        assert_eq!(
            String::from_utf16_lossy(&out[..length]),
            "C:\\logs\\crash-1.txt\0"
        );

        let mut out = [0u16; 64];
        let length = join_path(&wide(r"C:\logs\"), "crash-1.txt", &mut out);
        assert_eq!(
            String::from_utf16_lossy(&out[..length]),
            "C:\\logs\\crash-1.txt\0"
        );
    }

    #[test]
    fn a_path_that_does_not_fit_is_refused_rather_than_truncated() {
        let mut out = [0u16; 8];
        assert_eq!(join_path(&wide(r"C:\logs"), "crash-1.txt", &mut out), 0);
    }

    #[test]
    fn the_report_body_carries_every_field_the_diagnosis_needs() {
        let modules = modules_fixture();
        let frames = [0x0000_7ff6_a035_3d17u64, 0x0000_7ff8_c0d1_2345];
        let parameters = [0usize, 0x1234];
        let exception = ExceptionFacts {
            code: 0xc000_0005,
            address: 0x0000_7ff6_a035_3d17,
            flags: 0,
            thread_id: 1234,
            parameters: &parameters,
            thread_context: std::ptr::null_mut(),
        };
        // 这一份**带上全量转储那一段**：写成了的那种报告里有路径、有大小、有保留份数。
        let mut facts = facts_fixture(
            &modules,
            &frames,
            Some(exception),
            None,
            "first line\nsecond line",
        );
        facts.dump = Some(DumpFacts {
            directory: DUMP_FIXTURE_DIRECTORY,
            file_name: DUMP_FIXTURE_NAME,
            path: DUMP_FIXTURE_PATH,
            size: Some(DUMP_FIXTURE_SIZE),
            pruned: 1,
            error: None,
            error_code: 0,
        });
        assert_eq!(compose_report(&facts), EXPECTED_ACCESS_VIOLATION_REPORT);
        // 写成的那种：路径与大小必须在正文里（人照着路径就能去打开它）。
        assert!(EXPECTED_ACCESS_VIOLATION_REPORT.contains(DUMP_FIXTURE_PATH));
        assert!(EXPECTED_ACCESS_VIOLATION_REPORT.contains("dump.size: 512.0 MB"));
        assert!(EXPECTED_ACCESS_VIOLATION_REPORT.contains("dump.retention.pruned: 1"));
    }

    #[test]
    fn a_dump_that_was_not_written_says_why_instead_of_showing_an_empty_path() {
        let modules = modules_fixture_single();
        let facts = facts_fixture_with_dump(
            &modules,
            &[],
            None,
            None,
            "",
            Some("单测环境：跳过全量转储（名字与保留策略另有单测）"),
        );
        let report = compose_report(&facts);
        assert!(report.contains("[dump]"), "{report}");
        assert!(report.contains("dump.written: 否"), "{report}");
        assert!(
            report.contains("dump.error: 单测环境：跳过全量转储"),
            "{report}"
        );
        // 没写成就不该出现路径、大小与保留份数那种「好像拿到了」的字段。
        assert!(!report.contains("dump.path:"), "{report}");
        assert!(!report.contains("dump.size:"), "{report}");
        assert!(!report.contains("note.8"), "{report}");
    }

    #[test]
    fn a_panic_report_carries_the_message_the_location_and_the_backtrace() {
        let modules = modules_fixture_single();
        let panic = PanicFacts {
            message: "called `Option::unwrap()` on a `None` value",
            location: Some("src/lib.rs:100:5"),
            backtrace: "   0: dsh_wallpaper_lib::run\n   1: main",
        };
        let mut facts = facts_fixture(&modules, &[], None, Some(panic), "");
        facts.origin = ReportOrigin::RustPanic;
        facts.file_name = "crash-20251004-204500-001.txt";
        let report = compose_report(&facts);
        assert!(report.contains("report.kind: rust-panic"), "{report}");
        assert!(
            report.contains("panic.message:\n  called `Option::unwrap()` on a `None` value\n"),
            "{report}"
        );
        assert!(
            report.contains("panic.location: src/lib.rs:100:5"),
            "{report}"
        );
        assert!(
            report.contains("panic.backtrace:\n     0: dsh_wallpaper_lib::run\n     1: main\n"),
            "{report}"
        );
        assert!(report.contains("（没有帧：这不是原生异常"), "{report}");
        assert!(report.contains("panic 默认 unwind"), "{report}");
        assert!(!report.contains("[exception]"), "{report}");
    }

    #[test]
    fn a_missing_module_table_is_stated_instead_of_faked() {
        let frames = [0x0000_7ff8_c0d1_2345u64];
        let exception = ExceptionFacts {
            code: 0xc000_0374,
            address: 0x0000_7ff8_1234_5678,
            flags: 0,
            thread_id: 42,
            parameters: &[],
            thread_context: std::ptr::null_mut(),
        };
        let facts = facts_fixture(&[], &frames, Some(exception), None, "");
        let report = compose_report(&facts);
        assert!(report.contains("modules.loaded: 0"), "{report}");
        assert!(report.contains("（模块表不可用"), "{report}");
        assert!(
            report.contains("0x00007ff8c0d12345（没有落在任何已枚举模块里）"),
            "{report}"
        );
        assert!(report.contains("（空：崩溃那一刻日志里没有内容"), "{report}");
        assert!(report.contains("(HEAP_CORRUPTION)"), "{report}");
        assert!(report.contains("exception.parameter.count: 0"), "{report}");
    }

    #[test]
    fn a_truncated_module_table_says_so() {
        let modules = modules_fixture();
        let mut facts = facts_fixture(&modules, &[], None, None, "x");
        facts.module_total = 137;
        let report = compose_report(&facts);
        assert!(report.contains("modules.loaded: 137"), "{report}");
        assert!(report.contains("modules.listed: 2"), "{report}");
        assert!(report.contains("（截断：上面是基址最小的那些"), "{report}");
    }

    #[test]
    fn only_the_last_forty_log_lines_are_embedded() {
        let log_tail = (1..=60)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<String>>()
            .join("\n");
        let modules = modules_fixture_single();
        let facts = facts_fixture(&modules, &[], None, None, &log_tail);
        let report = compose_report(&facts);
        assert!(report.contains("  line-21\n"), "{report}");
        assert!(report.contains("  line-60\n"), "{report}");
        assert!(
            !report.contains("  line-20\n"),
            "第 41 行之前的不该出现：{report}"
        );
        assert!(report.contains("log.tail.max.lines: 40"), "{report}");
    }

    #[test]
    fn the_frame_list_stops_exactly_at_the_hard_coded_cap() {
        let modules = modules_fixture_single();
        let frames: Vec<u64> = (0..MAX_FRAMES).map(|index| 0x1000 + index as u64).collect();
        let facts = facts_fixture(&modules, &frames, None, None, "");
        let report = compose_report(&facts);
        assert!(report.contains("stack.frame.count: 64"), "{report}");
        assert!(report.contains("  #63 "), "{report}");
        assert!(!report.contains("  #64 "), "{report}");
    }

    /// 报告里那份转储的固定字段：全量转储段落用它们把「写没写成、写在哪、多大、留几份」
    /// 逐字钉住。
    const DUMP_FIXTURE_DIRECTORY: &str = r"C:\Users\me\AppData\Local\com.dsh.wallpaper\crashes";
    const DUMP_FIXTURE_NAME: &str = "crash-20251004-203912-473-c0000005.dmp";
    const DUMP_FIXTURE_PATH: &str =
        r"C:\Users\me\AppData\Local\com.dsh.wallpaper\crashes\crash-20251004-203912-473-c0000005.dmp";
    const DUMP_FIXTURE_SIZE: u64 = 512 * 1024 * 1024;

    /// 一个转储文件名（保留策略的测试用）：时间戳 + 异常码。
    fn dump_name_fixture(stamp: &str, code: u32) -> String {
        let mut buffer = [0u8; DUMP_NAME_CAP];
        let length = write_dump_file_name(&mut buffer, stamp, Some(code));
        ascii_str(&buffer[..length.min(buffer.len())]).to_string()
    }

    #[test]
    fn the_dump_file_name_carries_the_timestamp_and_the_exception_code() {
        let mut buffer = [0u8; DUMP_NAME_CAP];
        let length = write_dump_file_name(&mut buffer, "20251004-203912-473", Some(0xc000_0005));
        assert_eq!(
            ascii_str(&buffer[..length]),
            "crash-20251004-203912-473-c0000005.dmp"
        );

        // 没有异常码（还没走到异常分发）时也要拼得出名字。
        let length = write_dump_file_name(&mut buffer, "20251004-203912-473", None);
        assert_eq!(ascii_str(&buffer[..length]), "crash-20251004-203912-473.dmp");

        // 小码值也要补满 8 位：事件日志里写的就是 `0xc0000409` 这种固定宽度。
        let length = write_dump_file_name(&mut buffer, "20251004-203912-473", Some(0x409));
        assert_eq!(
            ascii_str(&buffer[..length]),
            "crash-20251004-203912-473-00000409.dmp"
        );
    }

    #[test]
    fn only_our_own_dump_names_are_candidates_for_deletion() {
        // 认得出来的：我们自己写的那种名字。
        assert!(is_dump_name("crash-20251004-203912-473-c0000005.dmp"));
        assert!(is_dump_name("crash-20251004-203912-473.dmp"));
        // 认不出来的：别的工具、用户自己放的文件、以及名字像但后缀不对的 —— 一律不删。
        for name in [
            "user-notes.dmp",
            "crash-20251004-203912-473-c0000005.txt",
            "not-a-dump.txt",
            "crash-.dmp",
            "",
            "data.dmp",
        ] {
            assert!(!is_dump_name(name), "{name} 不该被当成我们的转储");
        }
    }

    #[test]
    fn the_retention_keeps_the_newest_dumps_and_asks_to_delete_the_rest() {
        let mut entries = [
            DumpEntry::new(&dump_name_fixture("20251001-000000-001", 0xc000_0005), 100).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251002-000000-001", 0xc000_0005), 200).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251003-000000-001", 0xc000_0005), 300).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251004-000000-001", 0xc000_0005), 400).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251005-000000-001", 0xc000_0005), 500).unwrap(),
        ];
        let deletable = partition_dumps_for_retention(&mut entries, 3);
        assert_eq!(deletable, 2, "五份里应当有 2 份要删");

        // 前 3 份是留下的（最新的三份），顺序按写入时间从新到旧。
        let kept: Vec<&str> = entries[..3].iter().map(DumpEntry::name).collect();
        assert_eq!(
            kept,
            vec![
                dump_name_fixture("20251005-000000-001", 0xc000_0005),
                dump_name_fixture("20251004-000000-001", 0xc000_0005),
                dump_name_fixture("20251003-000000-001", 0xc000_0005),
            ]
        );
        // 后 2 份是要删的：正好是最旧的那两份。
        let doomed: Vec<&str> = entries[3..5].iter().map(DumpEntry::name).collect();
        assert_eq!(
            doomed,
            vec![
                dump_name_fixture("20251002-000000-001", 0xc000_0005),
                dump_name_fixture("20251001-000000-001", 0xc000_0005),
            ]
        );
    }

    #[test]
    fn the_retention_never_deletes_a_file_that_is_not_our_dump() {
        let mut entries = [
            DumpEntry::new("user-notes.dmp", 1).unwrap(),
            DumpEntry::new("report.txt", 2).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251004-000000-001", 0xc000_0005), 300).unwrap(),
            DumpEntry::new(&dump_name_fixture("20251005-000000-001", 0xc000_0409), 400).unwrap(),
        ];
        // 只留一份转储：两个「不是我们的」必须一个都不删，被删的只能是我们自己那份旧的。
        let deletable = partition_dumps_for_retention(&mut entries, 1);
        assert_eq!(deletable, 1);
        let deleted: Vec<&str> = entries[3..4].iter().map(DumpEntry::name).collect();
        assert_eq!(
            deleted,
            vec![dump_name_fixture("20251004-000000-001", 0xc000_0005)],
            "被删的必须是我们自己那份旧的（最新的留一份）"
        );
        let all: Vec<&str> = entries.iter().map(DumpEntry::name).collect();
        assert!(all.contains(&"user-notes.dmp"), "{all:?}");
        assert!(all.contains(&"report.txt"), "{all:?}");
    }

    #[test]
    fn a_dump_name_too_long_for_the_fixed_buffer_is_dropped_instead_of_truncated() {
        let long = format!("crash-{}.dmp", "9".repeat(200));
        assert!(DumpEntry::new(&long, 1).is_none());
        assert!(DumpEntry::new("", 1).is_none());
    }

    #[test]
    fn the_dump_switch_reads_the_usual_true_ish_values() {
        for text in ["1", "true", "TRUE", "Yes", "on", " on "] {
            assert!(dump_disabled_by_value(Some(text)), "{text} 应当算「关掉」");
        }
        for text in ["0", "false", "no", "off", "", "maybe"] {
            assert!(!dump_disabled_by_value(Some(text)), "{text} 不该算「关掉」");
        }
        assert!(!dump_disabled_by_value(None), "没设这个变量就是照常写");
    }

    #[test]
    fn dump_sizes_are_written_the_way_a_human_reads_them() {
        let mut buffer = [0u8; 32];

        let length = dump_size_into(&mut buffer, 1024 * 1024);
        assert_eq!(ascii_str(&buffer[..length]), "1.0 MB");

        let length = dump_size_into(&mut buffer, 512 * 1024 * 1024);
        assert_eq!(ascii_str(&buffer[..length]), "512.0 MB");

        // 不足 1 MB 就写字节（迷你转储那种小文件的真实样子）。
        let length = dump_size_into(&mut buffer, 1536);
        assert_eq!(ascii_str(&buffer[..length]), "1536 字节");

        // 带小数的：一位小数（向下取整到十分位，够读了）。
        let length = dump_size_into(&mut buffer, 1024 * 1024 + 512 * 1024);
        assert_eq!(ascii_str(&buffer[..length]), "1.5 MB");
    }

    /// 一次 0xc0000005 的报告正文，逐字钉住 —— 格式就是契约：字段一改，这个常量必须跟着改，
    /// 而且改的时候人得先看一眼新的正文还读不读得懂。
    const EXPECTED_ACCESS_VIOLATION_REPORT: &str = r#"dsh-wallpaper 崩溃报告（进程自己在崩溃现场写下的，不需要调试器）
=====================================================
report.file: crash-20251004-203912-473.txt
report.directory: C:\Users\me\AppData\Local\com.dsh.wallpaper\logs
report.kind: native-exception / first-chance（AddVectoredExceptionHandler：异常刚抛出时就抓到了；随后是否被别处处理掉未知）
time.local: 2025-10-04 20:39:12.473
time.utc: 2025-10-04 12:39:12.473（应用日志里的时间是 UTC）
app: dsh-wallpaper 0.4.7 (full)
os: 10.0.26100 x86_64
process.id: 26136
process.exe: C:\Program Files\dsh-wallpaper\dsh-wallpaper.exe
thread.id: 1234

[exception]
exception.code: 0xc0000005 (ACCESS_VIOLATION)
exception.address: 0x00007ff6a0353d17 dsh-wallpaper.exe+0x353d17
exception.flags: 0x00000000
exception.thread.id: 1234
exception.parameter.count: 2
exception.parameter.0: 0x0000000000000000 → 访问类型：读取
exception.parameter.1: 0x0000000000001234 → 访问地址：0x0000000000001234 0x0000000000001234（没有落在任何已枚举模块里）

[dump]
dump.directory: C:\Users\me\AppData\Local\com.dsh.wallpaper\crashes
dump.written: 是
dump.file: crash-20251004-203912-473-c0000005.dmp
dump.path: C:\Users\me\AppData\Local\com.dsh.wallpaper\crashes\crash-20251004-203912-473-c0000005.dmp
dump.size: 512.0 MB
dump.retention.kept: 3
dump.retention.pruned: 1

[stack]
stack.thread.id: 1234
stack.frame.count: 2
  #00 0x00007ff6a0353d17 dsh-wallpaper.exe+0x353d17
  #01 0x00007ff8c0d12345 KERNEL32.DLL+0x112345

[modules]
modules.loaded: 2
modules.listed: 2
  0x00007ff6a0000000 0x0000000000a2f000 dsh-wallpaper.exe
  0x00007ff8c0c00000 0x0000000000200000 KERNEL32.DLL

[log]
log.path: C:\Users\me\AppData\Local\com.dsh.wallpaper\logs\dsh-wallpaper.log
log.tail.max.lines: 40
log.tail:
  first line
  second line

[notes]
note.1: 本报告是进程自己在异常处理里写下的，和 %LOCALAPPDATA%\CrashDumps 里的 WER 转储互补：转储要有调试器 + PDB 才能读，本报告要的是模块基址与偏移，两样都在上面。
note.2: 帧是返回地址；“分发路径”的那几帧（KiUserExceptionDispatcher / UnhandledExceptionFilter / 本报告自己的处理器）在列表最前面，真正的出错指令看 exception.address。
note.3: Windows 事件日志里的“异常偏移”（例如 0x353d17）就是 exception.address 减去所属模块基址 —— 上面的“模块名+偏移”已经算好，可以直接对着事件日志核。
note.4: 崩溃处理器不分配堆内存、不递归（重入立刻返回），写完报告仍把异常交回系统（EXCEPTION_CONTINUE_SEARCH），所以 WER 转储和系统错误提示都不受影响。
note.5: 应用日志按 tauri-plugin-log 的默认值轮换（KeepOne，40 000 字节阈值）：一超阈值就把旧文件删掉重开 —— 日志变成 0 字节就是这么来的。所以本报告自带尾部，但崩溃前刚轮换过时它可能是空的。
note.6: 模块名截断到 63 字节，最多列 384 条；每行是“基址 大小 模块名”，按基址升序。
note.7: 采集路径（SEH 处理器）在开发机上无法用真机崩溃验证，格式由 cargo test --lib 的单测钉住；各字段的含义见上面的前缀。
note.8: 同一目录（dump.path 所在的 crashes 目录）里最多保留最近三份全量转储，写这一份之前已经删掉了更旧的；不想要转储就设 DSH_WALLPAPER_NO_CRASH_DUMP=1。读它要用 cdb.exe，符号路径里必须有一次构建出的同名 PDB（见 docs/diagnostics/crash-dumps.md）。

（报告结束：以上内容由进程在 native-exception / first-chance（AddVectoredExceptionHandler：异常刚抛出时就抓到了；随后是否被别处处理掉未知） 中写出）
"#;

    #[cfg(windows)]
    mod windows_only {
        use super::*;
        use crate::crash_report::native as platform;

        /// 崩溃暂存区（`SCRATCH`）按设计只有一个写入者：生产路径由「正在写报告」那把旗保证，
        /// 而测试是并行跑的。所以这里用一把测试专用的锁把 `emit` 串起来 —— 否则两个测试会
        /// 互相把对方的暂存区写花（第一版就是栽在这上面：报告文件里出现了半个 UTF-8 字符）。
        /// 顺手把报告配额清零，好让每个用到它的测试都从同一个起点出发。
        fn serial_emit() -> std::sync::MutexGuard<'static, ()> {
            static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let guard = LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            REPORT_COUNT.store(0, Ordering::Release);
            guard
        }

        fn wide_terminated(text: &str) -> Vec<u16> {
            let mut units: Vec<u16> = text.encode_utf16().collect();
            units.push(0);
            units
        }

        /// 测试用的安装环境：日志目录用调用方给的临时目录，转储目录就用它下面的 `crashes`
        /// （与真实布局一致：`%LOCALAPPDATA%\<标识符>\logs` 与 `...\crashes` 平级）。
        fn environment_for<'a>(
            directory: &'a [u16],
            directory_display: &'a str,
            log: &'a [u16],
            log_display: &'a str,
        ) -> ReportEnvironment<'a> {
            let dump_directory = dump_directory_for(directory_display);
            let dump_directory_wide = Box::leak(wide_terminated(&dump_directory).into_boxed_slice());
            ReportEnvironment {
                product: "dsh-wallpaper",
                edition: "test",
                app_version: "0.4.7",
                process_exe: r"C:\test\dsh-wallpaper.exe",
                directory_wide: directory,
                directory_display: directory_display,
                log_wide: log,
                log_display: log_display,
                dump_directory_wide,
                dump_directory_display: Box::leak(
                    dump_directory.into_boxed_str(),
                ),
                dump_enabled: true,
            }
        }

        /// 与日志目录平级的 `crashes` 目录（真实布局就是这样）。
        fn dump_directory_for(log_directory_display: &str) -> String {
            let parent = std::path::Path::new(log_directory_display)
                .parent()
                .map(|path| path.to_path_buf())
                .unwrap_or_default();
            parent
                .join(CRASH_DUMP_DIRECTORY)
                .display()
                .to_string()
        }

        /// 把一份转储文件真的放到 crashes 目录里（保留策略的测试要用它）。
        fn place_dump_file(environment: &ReportEnvironment<'_>, name: &str) -> std::path::PathBuf {
            let directory = std::path::PathBuf::from(environment.dump_directory_display);
            std::fs::create_dir_all(&directory).expect("create crashes dir");
            let path = directory.join(name);
            std::fs::write(&path, b"not a real dump, just a file with the right name")
                .expect("write dump stand-in");
            path
        }

        /// crashes 目录里现在有哪些 `crash-*.dmp`（按名字排序）。
        fn dumps_in(environment: &ReportEnvironment<'_>) -> Vec<String> {            let directory = std::path::PathBuf::from(environment.dump_directory_display);
            let mut found: Vec<String> = std::fs::read_dir(&directory)
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.file_name().to_string_lossy().to_string())
                        .filter(|name| is_dump_name(name))
                        .collect()
                })
                .unwrap_or_default();
            found.sort();
            found
        }

        fn crash_reports_in(directory: &std::path::Path) -> Vec<std::path::PathBuf> {
            let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(directory)
                .expect("read dir")
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().starts_with("crash-"))
                        .unwrap_or(false)
                })
                .collect();
            found.sort();
            found
        }

        #[test]
        fn the_loader_walk_finds_this_process_own_modules() {
            let mut buffer = [ModuleRecord::EMPTY; MODULE_CAP];
            let (total, listed) = platform::collect_modules(&mut buffer);
            assert!(listed > 10, "只枚举到 {listed} 个模块");
            assert_eq!(total, listed, "上限之内不该发生截断");

            let names: Vec<String> = buffer[..listed]
                .iter()
                .map(|module| module.name().to_ascii_lowercase())
                .collect();
            for expected in ["ntdll.dll", "kernel32.dll", "kernelbase.dll"] {
                assert!(
                    names.iter().any(|name| name == expected),
                    "模块表里没有 {expected}：{names:?}"
                );
            }
            // 可执行模块必须能找到：它的基名就是当前测试二进制的名字。
            let exe = std::env::current_exe().expect("current_exe");
            let exe_name = exe
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            assert!(
                names.iter().any(|name| name == &exe_name),
                "模块表里没有 {exe_name}：{names:?}"
            );
            for module in &buffer[..listed] {
                assert!(module.base() != 0, "模块基址不该是 0");
                assert!(module.size() > 0, "模块大小不该是 0");
            }
        }

        #[test]
        fn the_loader_walk_and_the_toolhelp_snapshot_agree_on_the_basics() {
            let mut from_loader = [ModuleRecord::EMPTY; MODULE_CAP];
            let (_, loaded_listed) = platform::collect_modules(&mut from_loader);
            let mut from_toolhelp = [ModuleRecord::EMPTY; MODULE_CAP];
            let (toolhelp_total, toolhelp_listed) =
                unsafe { platform::toolhelp_modules(&mut from_toolhelp) };
            assert!(loaded_listed > 10 && toolhelp_listed > 10);
            assert!(
                toolhelp_total.abs_diff(loaded_listed) <= 2,
                "两条路数出来的模块数差太多：{toolhelp_total} vs {loaded_listed}"
            );
            // ntdll / kernel32 的基址与大小两条路必须一模一样 —— 这同时校验了 PEB 那套手写偏移。
            for expected in ["ntdll.dll", "kernel32.dll"] {
                let loader = from_loader[..loaded_listed]
                    .iter()
                    .find(|module| module.name().eq_ignore_ascii_case(expected))
                    .unwrap_or_else(|| panic!("loader 没有 {expected}"));
                let toolhelp = from_toolhelp[..toolhelp_listed]
                    .iter()
                    .find(|module| module.name().eq_ignore_ascii_case(expected))
                    .unwrap_or_else(|| panic!("toolhelp 没有 {expected}"));
                assert_eq!(loader.base(), toolhelp.base(), "{expected} 的基址不一致");
                assert_eq!(loader.size(), toolhelp.size(), "{expected} 的大小不一致");
            }
        }

        #[test]
        fn sorting_puts_the_module_bases_in_ascending_order() {
            let mut modules = [
                ModuleRecord::from_wide(0x3000, 0x10, &wide("c.dll")),
                ModuleRecord::from_wide(0x1000, 0x10, &wide("a.dll")),
                ModuleRecord::from_wide(0x2000, 0x10, &wide("b.dll")),
            ];
            platform::sort_modules(&mut modules);
            let bases: Vec<u64> = modules.iter().map(ModuleRecord::base).collect();
            assert_eq!(bases, vec![0x1000, 0x2000, 0x3000]);
        }

        #[test]
        fn captured_frames_are_return_addresses_that_resolve_into_real_modules() {
            let mut frames = [0u64; MAX_FRAMES];
            let mut modules = [ModuleRecord::EMPTY; MODULE_CAP];
            let (_, listed) = platform::collect_modules(&mut modules);
            assert!(listed > 0);
            // `captured` 借的是 `frames`，所以这里先拷出来再交给下一步的借用检查。
            let captured: Vec<u64> = platform::capture_frames(&mut frames).to_vec();
            assert!(!captured.is_empty(), "一帧都没抓到");
            assert!(captured.len() <= MAX_FRAMES);
            for frame in captured {
                assert!(frame != 0, "帧不该是 0");
                let location = locate_address(frame, &modules[..listed])
                    .unwrap_or_else(|| panic!("帧 {frame:#x} 认不出模块"));
                assert!(location.inside, "帧 {frame:#x} 落在模块之外");
            }
        }

        #[test]
        fn the_os_version_comes_out_as_a_dotted_number() {
            let mut buffer = [0u8; 32];
            let length = platform::os_version_into(&mut buffer);
            assert!(length > 0, "RtlGetVersion 没给出东西");
            let text = ascii_str(&buffer[..length]);
            let parts: Vec<&str> = text.split('.').collect();
            assert_eq!(parts.len(), 3, "版本号应当是 major.minor.build：{text}");
            assert!(
                parts.iter().all(|part| !part.is_empty()),
                "版本号里有空段：{text}"
            );
        }

        #[test]
        fn emitting_a_report_writes_a_real_file_with_the_tail_of_a_real_log() {
            let directory = tempfile::tempdir().expect("temp dir");
            let log_path = directory.path().join("dsh-wallpaper.log");
            std::fs::write(
                &log_path,
                "older line\n输入岛点击已核验：光标=(2152, 306)\n交接结果：附接=false\n",
            )
            .expect("write log");

            let directory_display = directory.path().display().to_string();
            let log_display = log_path.display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );

            let parameters = [0usize, 0x2152];
            let exception = ExceptionFacts {
                code: 0xc000_0005,
                address: 0x0000_7ff6_a035_3d17,
                flags: 0,
                thread_id: 4242,
                parameters: &parameters,
                thread_context: std::ptr::null_mut(),
            };
            let wrote = {
                let _serial = serial_emit();
                unsafe {
                    platform::emit(
                        &environment,
                        ReportOrigin::UnhandledFilter,
                        Some(&exception),
                        None,
                        &[0x0000_7ff6_a035_3d17],
                        4242,
                        // 这一条测的是文本报告本身。转储这一段按 `DumpFacts` 的语义给「没写成、
                        // 原因见 dump.error」那种事实（单测里转储本来就被跳过），于是 `[dump]`
                        // 段照常出现、`dump.written: 否` + 原因写出来；转储真的落盘那种（带路径
                        // 与大小、还有保留策略）由 `the_report_body_carries…` 的正文常量钉住。
                        Some(DumpFacts {
                            directory: environment.dump_directory_display,
                            file_name: "",
                            path: "",
                            size: None,
                            pruned: 0,
                            error: Some("单测环境：跳过全量转储（名字与保留策略另有单测）"),
                            error_code: 0,
                        }),
                    )
                }
            };
            assert!(wrote, "报告没写出来");

            let written = crash_reports_in(directory.path());
            assert_eq!(written.len(), 1, "报告文件应当正好有一个：{written:?}");
            let name = written[0]
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            assert!(name.starts_with("crash-"), "{name}");
            assert!(name.ends_with(".txt"), "{name}");
            assert_eq!(name.len(), "crash-20251004-203912-473.txt".len(), "{name}");

            let report = std::fs::read_to_string(&written[0]).expect("read report");
            assert!(
                report.contains("report.kind: native-exception / unhandled"),
                "{report}"
            );
            assert!(report.contains(&format!("report.file: {name}")), "{report}");
            assert!(
                report.contains("exception.code: 0xc0000005 (ACCESS_VIOLATION)"),
                "{report}"
            );
            assert!(report.contains("exception.parameter.0: 0x0000000000000000 → 访问类型：读取"), "{report}");
            assert!(report.contains("thread.id: 4242"), "{report}");
            // 模块表必须来自这个进程，且描出来的帧要能对上里面的某个模块。
            assert!(report.contains("modules.loaded: "), "{report}");
            assert!(
                report.to_ascii_lowercase().contains("kernel32.dll"),
                "模块表看起来是空的：{report}"
            );
            // 自带日志尾部：报告里必须有日志正文，而不只是路径。
            assert!(report.contains("  older line"), "{report}");
            assert!(report.contains("  输入岛点击已核验：光标=(2152, 306)"), "{report}");
            assert!(report.contains("  交接结果：附接=false"), "{report}");
            // 文本报告与实际落盘的转储由 `emit_with_dump` 串起来（这里走的是 `emit`，
            // 所以转储那一段只有原因）；写成的那种在 `the_report_body_carries...` 里逐字钉住。
            assert!(report.contains("[dump]"), "{report}");
            assert!(report.contains("dump.written: 否"), "{report}");
            // 没写成也要写出转储目录：这份报告自己就得说清「去哪儿找 / 不必去别处找」。
            assert!(
                report.contains(&format!("dump.directory: {}", environment.dump_directory_display)),
                "{report}"
            );
            assert!(report.contains("（报告结束"), "{report}");
        }

        #[test]
        fn a_missing_log_file_is_stated_in_the_report_instead_of_aborting_it() {
            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("nothing.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let wrote = {
                let _serial = serial_emit();
                unsafe { platform::emit(&environment, ReportOrigin::RustPanic, None, None, &[], 7, None) }
            };
            assert!(wrote, "日志缺失不该让报告写不出来");

            let written = crash_reports_in(directory.path());
            assert_eq!(written.len(), 1);
            let report = std::fs::read_to_string(&written[0]).expect("read report");
            assert!(report.contains("（打不开日志文件"), "{report}");
            assert!(report.contains("report.kind: rust-panic"), "{report}");
        }

        #[test]
        fn a_second_report_in_the_same_millisecond_gets_its_own_file() {
            // 同一毫秒里的第二份报告多半来自另一条线程：它必须有自己的文件，
            // 而不是把第一份盖掉（那正是最不该丢的现场）。
            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("none.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let _serial = serial_emit();
            for _ in 0..2 {
                assert!(unsafe {
                    platform::emit(&environment, ReportOrigin::RustPanic, None, None, &[], 7, None)
                });
            }
            let written = crash_reports_in(directory.path());
            assert_eq!(written.len(), 2, "{written:?}");
            let names: Vec<String> = written
                .iter()
                .map(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .collect();
            assert!(names.iter().all(|name| name.starts_with("crash-")), "{names:?}");
            assert!(names.iter().all(|name| name.ends_with(".txt")), "{names:?}");
            assert_ne!(names[0], names[1], "两份报告不能同名");
        }

        #[test]
        fn a_log_bigger_than_the_read_window_still_yields_its_last_lines() {
            // 回读是定长的：日志大的时候要**从末尾往前**读，否则报告里会是空白的尾部。
            let directory = tempfile::tempdir().expect("temp dir");
            let log_path = directory.path().join("dsh-wallpaper.log");
            let mut contents = String::from("[first] 这一行不该出现在报告里\n");
            for index in 0..600 {
                contents.push_str(&format!("[fill] line-{index}\n"));
            }
            contents.push_str("[last] 最后一行\n");
            assert!(contents.len() > LOG_TAIL_BYTES + 1024);
            std::fs::write(&log_path, &contents).expect("write log");

            let directory_display = directory.path().display().to_string();
            let log_display = log_path.display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let wrote = {
                let _serial = serial_emit();
                unsafe { platform::emit(&environment, ReportOrigin::RustPanic, None, None, &[], 7, None) }
            };
            assert!(wrote);

            let written = crash_reports_in(directory.path());
            let report = std::fs::read_to_string(&written[0]).expect("read report");
            assert!(report.contains("  [last] 最后一行"), "{report}");
            assert!(
                !report.contains("  [first] 这一行不该出现在报告里"),
                "报告只该带尾部：{report}"
            );
            // 尾部截断点落在多字节字符中间时也要修成合法 UTF-8。
            assert!(String::from_utf8(report.into_bytes()).is_ok());
        }

        #[test]
        fn the_handlers_always_hand_the_exception_back_to_the_system() {
            // 这是整个模块最要紧的一条：处理器**绝不吞异常**。
            // EXCEPTION_CONTINUE_SEARCH = 0（交回系统）；EXCEPTION_EXECUTE_HANDLER = 1 才是吞。
            assert_eq!(platform::EXCEPTION_CONTINUE_SEARCH, 0);

            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("none.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let _serial = serial_emit();

            // 致命码：第一机会就落盘，返回码仍然是「交回系统」。
            let mut record = platform::exception_record(0xc000_0005, 0x1000, &[0, 0x1234]);
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::VectoredFirstChance,
                    &mut record,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0, "必须返回 EXCEPTION_CONTINUE_SEARCH");
            assert_eq!(crash_reports_in(directory.path()).len(), 1);

            // 同一条异常随后走到顶层过滤器：不再写第二份（同一个现场的副本没有价值）。
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::UnhandledFilter,
                    &mut record,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0);
            assert_eq!(
                crash_reports_in(directory.path()).len(),
                1,
                "第一机会写过的那条不该在顶层过滤器里重复落盘"
            );

            // 控制流异常（C++ EH）在第一机会里记都不记，但照样交回系统。
            let mut control_flow = platform::exception_record(0xe06d_7363, 0x1000, &[]);
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::VectoredFirstChance,
                    &mut control_flow,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0);
            assert_eq!(
                crash_reports_in(directory.path()).len(),
                1,
                "0xe06d7363 不该被当成崩溃记下来"
            );

            // 同一个控制流异常真的一路没人处理（顶层过滤器）：那时它就是要命的了，照样留证据。
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::UnhandledFilter,
                    &mut control_flow,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0);
            assert_eq!(crash_reports_in(directory.path()).len(), 2);
        }

        #[test]
        fn a_handler_that_is_already_running_writes_nothing_and_returns_at_once() {
            // 重入保护：旗被占住时立刻返回 —— 崩溃处理器里再崩一次是最坏的结果。
            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("none.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let _serial = serial_emit();

            let held = crate::crash_report::enter().expect("旗此刻应当是空的");
            let mut record = platform::exception_record(0xc000_0005, 0x1000, &[0, 0]);
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::VectoredFirstChance,
                    &mut record,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0, "重入也必须交回系统");
            assert!(
                crash_reports_in(directory.path()).is_empty(),
                "重入时一个字节都不该写"
            );
            drop(held);

            // 旗放开之后同一条异常才真正落盘。
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::VectoredFirstChance,
                    &mut record,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0);
            assert_eq!(crash_reports_in(directory.path()).len(), 1);
        }

        #[test]
        fn a_handler_report_names_the_module_the_faulting_address_belongs_to() {
            // 用**真的**地址（测试二进制里的一段代码）走一遍：报告里必须出现“模块+偏移”。
            let mut modules = [ModuleRecord::EMPTY; MODULE_CAP];
            let (_, listed) = platform::collect_modules(&mut modules);
            let exe = std::env::current_exe().expect("current_exe");
            let exe_name = exe
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let module = modules[..listed]
                .iter()
                .find(|module| module.name().eq_ignore_ascii_case(&exe_name))
                .expect("测试二进制自己的模块");
            let address = (module.base() + 0x1234) as usize;

            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("none.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );
            let _serial = serial_emit();

            let mut record = platform::exception_record(0xc000_0005, address, &[0, 0x1234]);
            let code = unsafe {
                platform::handle_native(
                    &environment,
                    ReportOrigin::UnhandledFilter,
                    &mut record,
                    std::ptr::null_mut(),
                    99,
                )
            };
            assert_eq!(code, 0);

            let written = crash_reports_in(directory.path());
            assert_eq!(written.len(), 1);
            let report = std::fs::read_to_string(&written[0]).expect("read report");
            assert!(
                report.contains(&format!("{exe_name}+0x1234")),
                "报告里没有“模块+偏移”：{report}"
            );
            assert!(
                report.contains("exception.address.module: ") || report.contains(&format!("exception.address: 0x{:016x} {exe_name}+0x1234", address)),
                "{report}"
            );
        }

        #[test]
        fn a_name_collision_within_the_same_millisecond_gets_a_sequence_number() {
            // 撞名避让那一步是纯函数，可以单独钉住。
            let mut buffer = [0u8; 64];
            let length = write_report_file_name(&mut buffer, "20251004-203912-473", 1);
            assert_eq!(ascii_str(&buffer[..length]), "crash-20251004-203912-473.txt");
            let length = write_report_file_name(&mut buffer, "20251004-203912-473", 2);
            assert_eq!(ascii_str(&buffer[..length]), "crash-20251004-203912-473-2.txt");
            let length = write_report_file_name(&mut buffer, "20251004-203912-473", 12);
            assert_eq!(
                ascii_str(&buffer[..length]),
                "crash-20251004-203912-473-12.txt"
            );
        }

        #[test]
        fn pruning_the_dump_directory_keeps_the_newest_three_and_spares_foreign_files() {
            // 真的在一个真目录上走一遍：枚举（FindFirstFileW）→ 排序 → 删除。
            // 这是唯一会删文件的那段代码，所以它必须在真实文件系统上被验一次。
            let directory = tempfile::tempdir().expect("temp dir");
            let directory_display = directory.path().display().to_string();
            let log_display = directory.path().join("none.log").display().to_string();
            let directory_wide = wide_terminated(&directory_display);
            let log_wide = wide_terminated(&log_display);
            let environment = environment_for(
                &directory_wide,
                &directory_display,
                &log_wide,
                &log_display,
            );

            // 五份「我们的」转储 + 两个不该被碰的文件。
            let mut placed: Vec<String> = Vec::new();
            for stamp in [
                "20251001-000000-001",
                "20251002-000000-001",
                "20251003-000000-001",
                "20251004-000000-001",
                "20251005-000000-001",
            ] {
                let name = dump_name_fixture(stamp, 0xc000_0005);
                place_dump_file(&environment, &name);
                placed.push(name);
            }
            place_dump_file(&environment, "user-notes.dmp");
            place_dump_file(&environment, "report.txt");
            // 顺序按名字写下来，让「删的是哪两份」一眼可见。
            placed.sort();

            platform::prune_dumps(&environment);

            let left = dumps_in(&environment);
            assert_eq!(
                left.len(),
                KEEP_DUMP_FILES,
                "应当只剩 {KEEP_DUMP_FILES} 份转储：{left:?}"
            );
            // 留下的必须是名字最大的那三份（时间戳在名字里，所以名字顺序就是时间顺序）。
            let expected: Vec<String> = placed[placed.len() - KEEP_DUMP_FILES..].to_vec();
            assert_eq!(left, expected);

            // 不是我们写的文件一个都不能少。
            let crashes_directory = std::path::PathBuf::from(environment.dump_directory_display);
            assert!(crashes_directory.join("user-notes.dmp").exists());
            assert!(crashes_directory.join("report.txt").exists());

            // 再跑一次：没什么可删的，也不该出错（幂等）。
            platform::prune_dumps(&environment);
            assert_eq!(dumps_in(&environment), expected);
        }
    }
}
