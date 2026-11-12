//! 전용 Chrome 창을 Windows에서 완전히 숨긴다 (ShowWindow(SW_HIDE)).
//!
//! 화면 밖 좌표(--window-position=-32000,-32000)만으로는 Chrome이 좌표를 클램프해서
//! 창이 보일 수 있고, 작업표시줄 버튼도 남는다. SW_HIDE를 쓰면 창이 보이지 않고
//! 작업표시줄에서도 사라진다. CDP로만 조작하므로 렌더링/입력에는 영향이 없다.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicU32, Ordering};

    const SW_HIDE: i32 = 0;
    const TRUE: i32 = 1;

    type HWND = *mut core::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(lp_enum_func: extern "system" fn(HWND, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(h_wnd: HWND) -> i32;
        fn GetWindowThreadProcessId(h_wnd: HWND, lp_dw_process_id: *mut u32) -> u32;
        fn ShowWindow(h_wnd: HWND, n_cmd_show: i32) -> i32;
    }

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static HIDDEN_COUNT: AtomicU32 = AtomicU32::new(0);

    extern "system" fn enum_proc(hwnd: HWND, _lparam: isize) -> i32 {
        let mut pid: u32 = 0;
        let tid = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if tid == 0 || pid != TARGET_PID.load(Ordering::Relaxed) {
            return TRUE;
        }
        // 전용 Chrome 브라우저 프로세스가 소유한 보이는 최상위 창은 그 창 하나뿐이다.
        // (자식/유틸리티 프로세스의 창은 pid가 다르다)
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
            HIDDEN_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        TRUE
    }

    pub fn hide_for_pid(pid: u32) -> u32 {
        TARGET_PID.store(pid, Ordering::Relaxed);
        HIDDEN_COUNT.store(0, Ordering::Relaxed);
        unsafe {
            EnumWindows(enum_proc, 0);
        }
        HIDDEN_COUNT.load(Ordering::Relaxed)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn hide_for_pid(_pid: u32) -> u32 {
        0
    }
}

/// 지정한 프로세스 ID에 속한 보이는 창을 숨기고, 숨긴 창 수를 돌려준다.
pub fn hide_chrome_windows_for_pid(pid: u32) -> u32 {
    imp::hide_for_pid(pid)
}
