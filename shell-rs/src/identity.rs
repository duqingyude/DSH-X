// Windows 上「这条通知是谁发的」只由 AppUserModelID 决定，两件事都得做：
//
// 1) 进程要 SetCurrentProcessExplicitAppUserModelID(appId)。不设的话，WinRT 通知库会退回
//    它自己的兜底 AUMID（`Microsoft.Windows.PowerShell`），用户看到的就是一条署名
//    「Windows PowerShell」、配着 PowerShell 图标的通知——真机上报过。
// 2) 通知中心显示的名字与图标来自 HKCU\Software\Classes\AppUserModelId\<appId> 的
//    DisplayName / IconUri。只设进程 AUMID 不注册这一份，通知头上就只剩 exe 文件名。
//
// 两件事都幂等，每次启动重写一遍：用户改了安装目录、换了版本，注册表也跟着更新。
// 注册失败不拦启动（通知最坏退回 exe 名），只往日志写一行。
#![cfg(windows)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ,
};
use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

/// UTF-16 + NUL：Windows 的注册表与 Shell API 都要这种字符串。
fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect()
}

/// 声明本进程的应用身份；exe 同时当通知图的来源（IconUri 收 exe 路径，Windows 会取它的资源段）。
pub fn claim(app_id: &str, display_name: &str, exe: &str) {
    let app_id_wide = wide(app_id);
    // SAFETY: 传进去的宽字符串在调用期间一直活着；这个 API 只读它。
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(app_id_wide.as_ptr());
    }
    if let Err(error) = register(app_id, display_name, exe) {
        eprintln!("注册应用身份失败，通知会退回系统默认名字：{error}");
    }
}

fn register(app_id: &str, display_name: &str, exe: &str) -> std::io::Result<()> {
    let subkey = wide(&format!(r"Software\Classes\AppUserModelId\{app_id}"));
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: 参数都是本函数里活着的宽字符串或空指针；成功后 key 由我们关闭。
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_WRITE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    let result = set_string(key, "DisplayName", display_name).and_then(|()| set_string(key, "IconUri", exe));
    // SAFETY: key 来自上面成功的 RegCreateKeyExW，且只关闭一次。
    unsafe {
        RegCloseKey(key);
    }
    result
}

fn set_string(key: HKEY, name: &str, value: &str) -> std::io::Result<()> {
    let name = wide(name);
    let data = wide(value);
    // SAFETY: data 是活的 UTF-16 缓冲，长度按字节给（REG_SZ）；key 由调用方保证有效。
    let status = unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * std::mem::size_of::<u16>()) as u32,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}
