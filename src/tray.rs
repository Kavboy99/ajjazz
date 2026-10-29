//! Native notification icon. Its message loop is independent of the hidden UI.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Open,
    Exit,
}

#[cfg(windows)]
mod windows {
    use super::Action;
    use eframe::egui;
    use std::{ptr::null_mut, sync::mpsc, thread};
    use windows_sys::Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    };
    const CALLBACK: u32 = WM_APP + 71;
    const OPEN: u32 = WM_APP + 72;
    const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
    const CLASS: &[u16] = &[
        65, 74, 65, 90, 90, 83, 116, 117, 100, 105, 111, 84, 114, 97, 121, 0,
    ];
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    struct State {
        tx: mpsc::Sender<Action>,
        ctx: egui::Context,
        icon: HICON,
        taskbar_created: u32,
        main_window: usize,
    }
    impl State {
        fn send(&self, action: Action) {
            let _ = self.tx.send(action);
            // eframe 0.33 doesn't run App::update while its HWND is hidden.
            // Wake the actual owned window before asking egui to repaint.
            unsafe {
                ShowWindowAsync(
                    self.main_window as HWND,
                    match action {
                        Action::Open => SW_RESTORE,
                        Action::Exit => SW_SHOWNOACTIVATE,
                    },
                );
                if matches!(action, Action::Open) {
                    SetForegroundWindow(self.main_window as HWND);
                }
            }
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.ctx.request_repaint();
        }
        unsafe fn add(&self, hwnd: HWND) -> bool {
            let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = 1;
            data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
            data.uCallbackMessage = CALLBACK;
            data.hIcon = self.icon;
            let tip = wide("AJAZZ Studio · открыть настройки");
            data.szTip[..tip.len()].copy_from_slice(&tip);
            if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
                return false;
            }
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            unsafe {
                Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
            true
        }
    }
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_NCCREATE {
            let create = unsafe { &*(l as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
        }
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
        if !pointer.is_null() {
            let state = unsafe { &*pointer };
            if msg == state.taskbar_created {
                unsafe {
                    state.add(hwnd);
                }
                return 0;
            }
            if msg == OPEN {
                state.send(Action::Open);
                return 0;
            }
            if msg == CALLBACK {
                match (l as u32) & 0xffff {
                    NIN_SELECT | NIN_KEYSELECT => state.send(Action::Open),
                    WM_CONTEXTMENU => unsafe {
                        let menu = CreatePopupMenu();
                        if !menu.is_null() {
                            AppendMenuW(menu, MF_STRING, 1, wide("Открыть AJAZZ Studio").as_ptr());
                            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                            AppendMenuW(menu, MF_STRING, 2, wide("Завершить Studio").as_ptr());
                            SetMenuDefaultItem(menu, 1, 0);
                            let mut point: POINT = std::mem::zeroed();
                            GetCursorPos(&mut point);
                            SetForegroundWindow(hwnd);
                            let command = TrackPopupMenu(
                                menu,
                                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                                point.x,
                                point.y,
                                0,
                                hwnd,
                                std::ptr::null(),
                            );
                            DestroyMenu(menu);
                            PostMessageW(hwnd, WM_NULL, 0, 0);
                            match command {
                                1 => state.send(Action::Open),
                                2 => state.send(Action::Exit),
                                _ => {}
                            }
                        }
                    },
                    _ => {}
                }
                return 0;
            }
            if msg == WM_DESTROY {
                let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
                data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = hwnd;
                data.uID = 1;
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &data);
                    PostQuitMessage(0);
                }
                return 0;
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }
    pub struct Tray {
        hwnd: usize,
        rx: mpsc::Receiver<Action>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl Tray {
        pub fn new(
            ctx: egui::Context,
            creation: &eframe::CreationContext<'_>,
        ) -> anyhow::Result<Self> {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let RawWindowHandle::Win32(handle) = creation.window_handle()?.as_raw() else {
                anyhow::bail!("Not a Windows window");
            };
            let main_window = handle.hwnd.get() as usize;
            let (tx, rx) = mpsc::channel();
            let (ready, startup) = mpsc::sync_channel(1);
            let thread = thread::spawn(move || unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance,
                    lpszClassName: CLASS.as_ptr(),
                    ..std::mem::zeroed()
                };
                if RegisterClassW(&wc) == 0 {
                    let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                // A compact blue tile with a white A; owned icon, no external files.
                let mut pixels = [0u8; 32 * 32 * 4];
                for y in 0..32 {
                    for x in 0..32 {
                        let k = (y * 32 + x) * 4;
                        let white = (7..25).contains(&y)
                            && ((x as i32 - 16).abs() == (y as i32 - 6) / 2
                                || y == 18 && (10..23).contains(&x));
                        pixels[k..k + 4].copy_from_slice(if white {
                            &[255, 255, 255, 255]
                        } else {
                            &[245, 157, 88, 255]
                        });
                    }
                }
                let icon = CreateIcon(
                    instance,
                    32,
                    32,
                    1,
                    32,
                    [0u8; 128].as_ptr(),
                    pixels.as_ptr(),
                );
                let state = Box::new(State {
                    tx,
                    ctx,
                    icon,
                    taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                    main_window,
                });
                // A hidden top-level window receives Explorer's TaskbarCreated broadcast.
                let hwnd = CreateWindowExW(
                    0,
                    CLASS.as_ptr(),
                    CLASS.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    instance,
                    (&*state as *const State).cast(),
                );
                if hwnd.is_null() || icon.is_null() || !state.add(hwnd) {
                    let _ = ready.send(Err("Не удалось создать значок в трее".into()));
                    if !hwnd.is_null() {
                        DestroyWindow(hwnd);
                    }
                } else {
                    // Open is the only parameter-free message accepted from a
                    // normal second launch while this instance is elevated.
                    ChangeWindowMessageFilterEx(hwnd, OPEN, MSGFLT_ALLOW, std::ptr::null_mut());
                    let _ = ready.send(Ok(hwnd as usize));
                    let mut message: MSG = std::mem::zeroed();
                    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                UnregisterClassW(CLASS.as_ptr(), instance);
            });
            let hwnd = startup
                .recv()
                .map_err(|e| anyhow::anyhow!(e))?
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok(Self {
                hwnd,
                rx,
                thread: Some(thread),
            })
        }
        pub fn action(&self) -> Option<Action> {
            self.rx.try_recv().ok()
        }
    }
    impl Drop for Tray {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
    pub fn wait_for_previous_exit() -> bool {
        for _ in 0..600 {
            if unsafe { FindWindowW(CLASS.as_ptr(), CLASS.as_ptr()) }.is_null() {
                return true;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }
    pub fn activate_existing() -> bool {
        unsafe {
            let hwnd = FindWindowW(CLASS.as_ptr(), CLASS.as_ptr());
            if hwnd.is_null() {
                return false;
            }
            let mut process = 0;
            GetWindowThreadProcessId(hwnd, &mut process);
            AllowSetForegroundWindow(process);
            // Also wake a hidden window owned by an older Studio version: its
            // receiver can queue Open, but cannot repaint until the HWND wakes.
            let title = wide("AJAZZ Studio");
            let mut window =
                FindWindowExW(null_mut(), null_mut(), std::ptr::null(), title.as_ptr());
            while !window.is_null() {
                let mut owner = 0;
                GetWindowThreadProcessId(window, &mut owner);
                if owner == process {
                    ShowWindowAsync(window, SW_RESTORE);
                    SetForegroundWindow(window);
                    break;
                }
                window = FindWindowExW(null_mut(), window, std::ptr::null(), title.as_ptr());
            }
            PostMessageW(hwnd, OPEN, 0, 0);
            true
        }
    }
}
#[cfg(windows)]
pub use windows::{Tray, activate_existing, wait_for_previous_exit};
#[cfg(not(windows))]
pub struct Tray;
#[cfg(not(windows))]
impl Tray {
    pub fn new(_: eframe::egui::Context, _: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        anyhow::bail!("Трей доступен в Windows")
    }
    pub fn action(&self) -> Option<Action> {
        None
    }
}
#[cfg(not(windows))]
pub fn activate_existing() -> bool {
    false
}
#[cfg(not(windows))]
pub fn wait_for_previous_exit() -> bool {
    true
}
