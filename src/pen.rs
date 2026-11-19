use std::{
    cell::{Cell, RefCell},
    mem::MaybeUninit,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::Pointer::*,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Clone, Copy)]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
    MouseCaptureLost,
}

#[derive(Clone, Copy)]
pub struct PenSample {
    pub position: [f32; 2],
    pub pressure: f32,
    pub timestamp_ms: u32,
    pub phase: Phase,
}

type SampleKey = (u64, u32, i32, i32, u32);

#[derive(Default)]
struct Input {
    surfaces: Cell<[Option<egui::Rect>; 3]>,
    overlay: Cell<Option<egui::Rect>>,
    active: Cell<Option<u32>>,
    last_sample: Cell<Option<SampleKey>>,
    samples: RefCell<Vec<PenSample>>,
}

pub struct PenInput {
    hwnd: HWND,
    input: Box<Input>,
}

impl PenInput {
    pub fn attach(window: &winit::window::Window) -> Result<Self, String> {
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("The painting window does not expose a Win32 handle.".into());
        };
        let hwnd = handle.hwnd.get() as HWND;
        let input = Box::<Input>::default();
        // The window and the boxed state are owned on the UI thread. Drop removes
        // the callback before it frees the pointer passed to comctl32.
        let installed = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(window_proc),
                1,
                (&*input as *const Input) as usize,
            )
        };
        if installed == 0 {
            return Err("Could not connect Windows pen input. Restart the app.".into());
        }
        Ok(Self { hwnd, input })
    }

    pub fn surfaces(&self, surfaces: [Option<egui::Rect>; 3], overlay: Option<egui::Rect>) {
        self.input.surfaces.set(surfaces);
        self.input.overlay.set(overlay);
    }

    pub fn drain(&self) -> Vec<PenSample> {
        self.input.samples.take()
    }
}

impl Drop for PenInput {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.hwnd, Some(window_proc), 1);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let input = unsafe { &*(data as *const Input) };
    let pointer_id = (wparam & 0xffff) as u32;

    match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            if unsafe { read_pointer(input, hwnd, pointer_id, message) } {
                return 0;
            }
        }
        WM_POINTERCAPTURECHANGED if input.active.get() == Some(pointer_id) => cancel(input),
        WM_KILLFOCUS | WM_CANCELMODE => cancel(input),
        WM_CAPTURECHANGED => input.samples.borrow_mut().push(PenSample {
            position: [0.; 2],
            pressure: 0.,
            timestamp_ms: 0,
            phase: Phase::MouseCaptureLost,
        }),
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_MOUSEMOVE => {
            // Windows tags mouse messages promoted from a pen. Preserve ordinary
            // mouse messages and let the native pen stream own canvas presses.
            let extra = unsafe { GetMessageExtraInfo() } as u32;
            let promoted_pen = (extra & 0xffff_ff80) == 0xff51_5700;
            let position = egui::pos2(lparam as i16 as f32, (lparam >> 16) as i16 as f32);
            if promoted_pen && (input.active.get().is_some() || hit_surface(input, position)) {
                return 0;
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn hit_surface(input: &Input, point: egui::Pos2) -> bool {
    !input.overlay.get().is_some_and(|rect| rect.contains(point))
        && input
            .surfaces
            .get()
            .iter()
            .flatten()
            .any(|rect| rect.contains(point))
}

fn cancel(input: &Input) {
    if input.active.take().is_some() {
        input.last_sample.set(None);
        input.samples.borrow_mut().push(PenSample {
            position: [0.; 2],
            pressure: 0.,
            timestamp_ms: 0,
            phase: Phase::Cancel,
        });
    }
}

unsafe fn read_pointer(input: &Input, hwnd: HWND, id: u32, message: u32) -> bool {
    let active = input.active.get() == Some(id);
    if message != WM_POINTERDOWN && !active {
        return false;
    }

    let mut current = MaybeUninit::uninit();
    if unsafe { GetPointerPenInfo(id, current.as_mut_ptr()) } == 0 {
        if active && message == WM_POINTERUP {
            cancel(input);
        }
        return active;
    }
    let current = unsafe { current.assume_init() };
    if current.pointerInfo.pointerFlags & POINTER_FLAG_CANCELED != 0 {
        if active {
            cancel(input);
        }
        return active;
    }
    if message == WM_POINTERDOWN {
        let Some(pos) = client_position(hwnd, current.pointerInfo.ptPixelLocation) else {
            return false;
        };
        if input.active.get().is_some() || !hit_surface(input, pos) {
            return false;
        }
        input.active.set(Some(id));
        input.last_sample.set(None);
        push(input, hwnd, &current, Phase::Down);
        return true;
    }
    if message == WM_POINTERUP {
        push(input, hwnd, &current, Phase::Up);
        input.active.set(None);
        input.last_sample.set(None);
        return true;
    }

    // The current record already supplies the required capacity. Win32 writes
    // newest-first and may report fewer entries than requested.
    let mut count = current.pointerInfo.historyCount.clamp(1, 8192);
    let mut history = vec![current; count as usize];
    if count == 1 || unsafe { GetPointerPenInfoHistory(id, &mut count, history.as_mut_ptr()) } == 0
    {
        push(input, hwnd, &current, Phase::Move);
    } else {
        let available = history.len().min(count as usize);
        for record in history[..available].iter().rev() {
            if record.pointerInfo.pointerFlags & (POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE)
                == (POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE)
            {
                push(input, hwnd, record, Phase::Move);
            }
        }
    }
    true
}

fn client_position(hwnd: HWND, mut point: POINT) -> Option<egui::Pos2> {
    (unsafe { ScreenToClient(hwnd, &mut point) } != 0)
        .then(|| egui::pos2(point.x as f32, point.y as f32))
}

fn push(input: &Input, hwnd: HWND, record: &POINTER_PEN_INFO, phase: Phase) {
    let pointer = &record.pointerInfo;
    let position = pointer.ptPixelLocation;
    let key = (
        pointer.PerformanceCount,
        pointer.dwTime,
        position.x,
        position.y,
        record.pressure,
    );
    if matches!(phase, Phase::Move) {
        if let Some(last) = input.last_sample.get() {
            let older = if key.0 != 0 && last.0 != 0 {
                key.0 < last.0
            } else {
                (key.1.wrapping_sub(last.1) as i32) < 0
            };
            if older || key == last {
                return;
            }
        }
    }
    let Some(position) = client_position(hwnd, position) else {
        return;
    };
    input.last_sample.set(Some(key));
    let pressure = if record.penMask & PEN_MASK_PRESSURE != 0 {
        record.pressure.min(1024) as f32 / 1024.
    } else {
        0.65
    };
    input.samples.borrow_mut().push(PenSample {
        position: [position.x, position.y],
        pressure,
        timestamp_ms: pointer.dwTime,
        phase,
    });
}
