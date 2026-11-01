// 「保持唤醒」开关（settings.json 的 keepAwake）：只防系统睡眠，不动屏幕。
//
// Windows 上用 SetThreadExecutionState，它是**线程级**的：每个调用它的线程各自
// 记一份要求，所以必须在外壳主线程（eframe 的 logic/ui 就跑在主线程）上调，
// 且要反复调——中间被别的线程覆盖掉就丢了。进程退出时系统自己清。
//
// macOS / Linux 上没做：mac 要 IOPMAssertion、Linux 要 systemd-inhibit，
// 两者都得起一个常驻子进程，为这一个开关不值得。那边保持系统默认睡眠行为。

/// 把当前线程的唤醒要求设成 keep_awake：开着就请求系统保持运行，关掉就还原。
pub fn apply(keep_awake: bool) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
        };
        // 不带 ES_DISPLAY_REQUIRED：用户关屏是用户的事，这里只拦睡眠。
        let flags = if keep_awake {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        SetThreadExecutionState(flags);
    }
    #[cfg(not(windows))]
    let _ = keep_awake;
}