//! 启动时间线的统一起点与统一写法（**只测量，不改行为**）。
//!
//! 背景：现有日志里带的时间戳都从一个模块内部的起点算起 ——
//! `native_bootstrap` 的 `elapsed_ms` 以 `prepare()` 为原点，
//! `tauri_plugin_log` 的文件前缀只有**整秒**精度。两者都看不出
//! 「Windows 从登录到真正拉起这个进程用了多久」，也看不出启动链上各段各花了几毫秒。
//!
//! 这一个模块只做两件事，而且都不碰任何启动顺序、开关、返回值：
//!
//! 1. 锚点：进程入口处（`main()` 第一行）打一个戳，同时记下
//!    * 单调时钟（`Instant`，用来算毫秒差）；
//!    * 墙上时钟（Unix 纪元毫秒，用来与 `Win32_Process.CreationDate` 对齐）。
//!    这个锚点只写一次，第一次调用的人就是原点，之后谁调用都读同一份。
//! 2. 记一条时间点：打印中文文案 + 绝对时间（本机时区，毫秒）+ 相对进程入口的毫秒，
//!    并往 `%LOCALAPPDATA%\DSHWallpaper\startup-diagnostic.log` 追加一行
//!    `elapsed_ms=<毫秒> absolute=<绝对时间> epoch_ms=<纪元毫秒> event=<英文标识> 中文文案`。
//!    文件那一行是 `scripts/measure-startup-gap.ps1` 的数据来源。
//!
//! 与既有 `elapsed_ms` 的关系：`since_prepare_ms` 就是原来那套
//! 「相对 `native_bootstrap::prepare()`」的读数换算到同一时刻的值。原来那套
//! 读数与原点一律不动 —— 新老数字可以放在一起看，谁都不必改口径。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 进程入口的时间锚点：单调时钟 + 墙上时钟，成对取一次。
static PROCESS_ANCHOR: OnceLock<(Instant, u128)> = OnceLock::new();

/// `native_bootstrap::prepare()` 开始的时刻（相对 [`PROCESS_ANCHOR`] 的毫秒）。
///
/// 用 `u64::MAX` 当「还没开始」：`prepare()` 在进程入口几毫秒之内就会调用，
/// 真到了读数的时候总已经写过一次。
static PREPARE_START_MS: AtomicU64 = AtomicU64::new(u64::MAX);

/// 现在距 Unix 纪元多少毫秒。
///
/// 时钟被往前调过（返回 `None`）时取 0：这是诊断用的时间戳，
/// 宁可写一个明显不对的 0，也不要让启动链上多一个 `unwrap`。
pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

/// 进程入口（`main()` 第一行）打的戳；只生效一次，后到的调用读同一份。
pub fn anchor_process_entry() {
    let _ = PROCESS_ANCHOR.get_or_init(|| (Instant::now(), unix_millis()));
}

/// 记下 `native_bootstrap::prepare()` 开始的时刻；只生效一次。
pub fn mark_prepare_start() {
    anchor_process_entry();
    let anchor_ms = PROCESS_ANCHOR
        .get()
        .map(|anchor| anchor.0.elapsed().as_millis())
        .unwrap_or_default();
    let _ = PREPARE_START_MS.compare_exchange(
        u64::MAX,
        u64::try_from(anchor_ms).unwrap_or(u64::MAX),
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

/// 相对 `native_bootstrap::prepare()` 的毫秒数：单位与既有 `elapsed_ms` 完全相同。
///
/// `prepare()` 还没开始时返回 0（那说明量的是进程入口本身，差不出一个可说的数）。
fn since_prepare_ms(anchor_ms: u128) -> u128 {
    let prepare_ms = PREPARE_START_MS.load(Ordering::Acquire);
    if prepare_ms == u64::MAX {
        return 0;
    }
    anchor_ms.saturating_sub(u128::from(prepare_ms))
}

fn report() -> (u128, u128, u128) {
    anchor_process_entry();
    let (started, epoch_started) = *PROCESS_ANCHOR
        .get()
        .expect("进程入口锚点在 anchor_process_entry() 之后必定存在");
    let anchor_ms = started.elapsed().as_millis();
    (
        anchor_ms,
        epoch_started + anchor_ms,
        since_prepare_ms(anchor_ms),
    )
}

/// 本机时区、毫秒精度的绝对时间，形如 `2026-10-06 01:52:37.412`。
///
/// 只用了极小的 Windows 调用（`GetLocalTime`），不引第三方时间库 —— 这一个模块
/// 不值得为一行时间戳往依赖表里加东西。非 Windows 目标退化成 Unix 纪元毫秒。
fn absolute_local_time() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let now = unsafe { GetLocalTime() };
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
        );
    }
    #[cfg(not(windows))]
    {
        format!("unix_ms={}", unix_millis())
    }
}

/// 一条时间点的全部读数：相对进程入口的毫秒、绝对时间、纪元毫秒、相对 `prepare()` 的毫秒。
pub struct Report {
    pub anchor_ms: u128,
    pub wall_clock: String,
    pub epoch_ms: u128,
    pub since_prepare_ms: u128,
}

/// 取一条时间点的读数。这不是**记录**，只是取数，方便调用方把它拼进既有日志。
pub fn sample() -> Report {
    let (anchor_ms, epoch_ms, since_prepare_ms) = report();
    Report {
        anchor_ms,
        wall_clock: absolute_local_time(),
        epoch_ms,
        since_prepare_ms,
    }
}

/// `startup-diagnostic.log` 里一行的前缀：`elapsed_ms=<毫秒> absolute=<绝对时间> epoch_ms=<纪元毫秒>`。
///
/// 既有字段 `elapsed_ms` 保持原样在前、口径不变，新字段一律追加在后面 ——
/// 早期版本写下的行照样能读，新行也能被脚本按 `epoch_ms` 精确对齐。
pub fn diagnostic_prefix() -> String {
    let report = sample();
    format!(
        "elapsed_ms={} absolute={} epoch_ms={}",
        report.anchor_ms, report.wall_clock, report.epoch_ms
    )
}

/// 打印一条中文启动时间点（进应用日志），单位写清是毫秒。
pub fn log_point(label: &str) {
    let report = sample();
    log::info!(
        "启动时间点：{label} absolute={} elapsed_ms={} since_prepare_ms={}",
        report.wall_clock,
        report.anchor_ms,
        report.since_prepare_ms
    );
}

/// 记录一条启动时间点：应用日志一条（中文），诊断文件一行（可被脚本解析）。
pub fn mark(event: &str, label: &str) {
    log_point(label);
    #[cfg(windows)]
    crate::native_bootstrap::record_startup_diagnostic(&format!("event={event} {label}"));
}
