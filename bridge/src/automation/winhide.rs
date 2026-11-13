//! 전용 Chrome 창을 Windows에서 완전히 숨기거나(ShowWindow(SW_HIDE))
//! 다시 화면에 꺼낼 때(ShowWindow(SW_SHOW) + SetWindowPos) 사용한다.
//!
//! 화면 밖 좌표(--window-position=-32000,-32000)만으로는 Chrome이 좌표를 클램프해서
//! 창이 보일 수 있고, 작업표시줄 버튼도 남는다. SW_HIDE를 쓰면 창이 보이지 않고
//! 작업표시줄에서도 사라진다. CDP로만 조작하므로 렌더링/입력에는 영향이 없다.

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicU32, Ordering};

    const SW_HIDE: i32 = 0;
    const SW_SHOW: i32 = 5;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const HWND_TOP: isize = 0;
    const SWP_NOSIZE: u32 = 0x0001;
    const STILL_ACTIVE: u32 = 259;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const TRUE: i32 = 1;

    type HWND = *mut core::ffi::c_void;
    type HANDLE = *mut core::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(lp_enum_func: extern "system" fn(HWND, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(h_wnd: HWND) -> i32;
        fn GetWindowThreadProcessId(h_wnd: HWND, lp_dw_process_id: *mut u32) -> u32;
        fn ShowWindow(h_wnd: HWND, n_cmd_show: i32) -> i32;
        fn SetWindowPos(h_wnd: HWND, h_wnd_insert_after: isize, x: i32, y: i32, cx: i32, cy: i32, u_flags: u32) -> i32;
        fn GetClassNameW(h_wnd: HWND, lp_class_name: *mut u16, n_max_count: i32) -> i32;
    }

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static ACTION: AtomicU32 = AtomicU32::new(0); // 0=hide 1=show
    static DONE_COUNT: AtomicU32 = AtomicU32::new(0);
    // hide/show 호출이 겹쳐도 EnumWindows 사이에 전역 상태가 바뀌지 않도록 직렬화한다
    static RUN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const CLASS_MAIN_WINDOW: &str = "Chrome_WidgetWin_1";

    fn class_matches(hwnd: HWND, target: &str) -> bool {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if len <= 0 {
            return false;
        }
        buf[..len as usize] == target.encode_utf16().collect::<Vec<u16>>()[..]
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> HANDLE;
        fn GetExitCodeProcess(h_process: HANDLE, lp_exit_code: *mut u32) -> i32;
        fn CloseHandle(h_object: HANDLE) -> i32;
    }

    extern "system" fn enum_proc(hwnd: HWND, _lparam: isize) -> i32 {
        let mut pid: u32 = 0;
        let tid = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if tid == 0 || pid != TARGET_PID.load(Ordering::Relaxed) {
            return TRUE;
        }
        match ACTION.load(Ordering::Relaxed) {
            0 => {
                // 숨길 때는 해당 Chrome 프로세스의 모든 보이는 창을 숨긴다 (팝업 포함)
                if unsafe { IsWindowVisible(hwnd) } != 0 {
                    unsafe {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                    DONE_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {
                // 보일 때는 메인 창만 — WidgetWin_0(팝업)은 그냥 둔다 (PoC: 이상한 창 방지)
                if !class_matches(hwnd, CLASS_MAIN_WINDOW) {
                    return TRUE;
                }
                unsafe {
                    ShowWindow(hwnd, SW_SHOW);
                    SetWindowPos(hwnd, HWND_TOP, 48, 48, 1100, 800, SWP_SHOWWINDOW | SWP_NOSIZE);
                }
                DONE_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
        TRUE
    }

    fn run(pid: u32, action: u32) -> u32 {
        let _guard = RUN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        TARGET_PID.store(pid, Ordering::Relaxed);
        ACTION.store(action, Ordering::Relaxed);
        DONE_COUNT.store(0, Ordering::Relaxed);
        unsafe {
            EnumWindows(enum_proc, 0);
        }
        DONE_COUNT.load(Ordering::Relaxed)
    }

    pub fn hide_for_pid(pid: u32) -> u32 {
        run(pid, 0)
    }

    pub fn show_for_pid(pid: u32) -> u32 {
        run(pid, 1)
    }

    pub fn process_alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            ok != 0 && code == STILL_ACTIVE
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn hide_for_pid(_pid: u32) -> u32 {
        0
    }
    pub fn show_for_pid(_pid: u32) -> u32 {
        0
    }
    pub fn process_alive(_pid: u32) -> bool {
        false
    }
}

/// 지정한 프로세스 ID에 속한 보이는 창을 숨기고, 숨긴 창 수를 돌려준다.
pub fn hide_chrome_windows_for_pid(pid: u32) -> u32 {
    imp::hide_for_pid(pid)
}

/// 지정한 프로세스 ID에 속한 창을 표시하고 화면 안으로 옮기며, 표시한 창 수를 돌려준다.
pub fn show_chrome_windows_for_pid(pid: u32) -> u32 {
    imp::show_for_pid(pid)
}

/// 프로세스가 살아 있는지 확인한다.
pub fn process_alive(pid: u32) -> bool {
    imp::process_alive(pid)
}
