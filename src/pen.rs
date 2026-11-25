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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pressed: Cell<bool>,
    native_press: Cell<bool>,
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

    pub fn reset_contact(&self) {
        self.input.active.set(None);
        self.input.pressed.set(false);
        self.input.last_sample.set(None);
        self.input.samples.borrow_mut().clear();
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
            let queued = !input.samples.borrow().is_empty();
            let message_contact = (wparam >> 16) as u32 & POINTER_MESSAGE_FLAG_INCONTACT != 0;
            if unsafe { read_pointer(input, hwnd, pointer_id, message, message_contact) } {
                if !queued && !input.samples.borrow().is_empty() {
                    unsafe { PostMessageW(hwnd, WM_NULL, 0, 0) };
                }
                return 0;
            }
        }
        WM_POINTERLEAVE | WM_POINTERCAPTURECHANGED if input.active.get() == Some(pointer_id) => {
            end_contact(input, None);
            unsafe {
                PostMessageW(hwnd, WM_NULL, 0, 0);
            }
        }
        WM_KILLFOCUS | WM_CANCELMODE => {
            end_contact(input, None);
            unsafe {
                PostMessageW(hwnd, WM_NULL, 0, 0);
            }
        }
        WM_CAPTURECHANGED => input.samples.borrow_mut().push(PenSample {
            position: [0.; 2],
            pressure: 0.,
            timestamp_ms: 0,
            phase: Phase::MouseCaptureLost,
        }),
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_MOUSEMOVE => {
            // Route the promoted press and release together, even if the pen
            // crosses from a widget to the canvas before lifting.
            let extra = unsafe { GetMessageExtraInfo() } as u32;
            let promoted_pen = (extra & 0xffff_ff80) == 0xff51_5700;
            if promoted_pen && suppress_promoted(input, message) {
                if message == WM_LBUTTONUP && input.active.get().is_some() {
                    end_contact(input, None);
                    unsafe { PostMessageW(hwnd, WM_NULL, 0, 0) };
                }
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

fn suppress_promoted(input: &Input, message: u32) -> bool {
    match message {
        WM_LBUTTONUP => input.native_press.replace(false),
        WM_MOUSEMOVE => input.active.get().is_some(),
        _ => input.native_press.get(),
    }
}

fn contact_phase(input: &Input, pressure: f32) -> Option<Phase> {
    let pressed = pressure > 0.;
    match (input.pressed.replace(pressed), pressed) {
        (false, true) => Some(Phase::Down),
        (true, true) => Some(Phase::Move),
        (true, false) => Some(Phase::Cancel),
        (false, false) => None,
    }
}

fn end_contact(input: &Input, release: Option<(egui::Pos2, u32)>) {
    input.active.set(None);
    input.last_sample.set(None);
    if input.pressed.replace(false) {
        let (position, timestamp_ms, phase) = match release {
            Some((point, time)) => ([point.x, point.y], time, Phase::Up),
            None => ([0.; 2], 0, Phase::Cancel),
        };
        input.samples.borrow_mut().push(PenSample {
            position,
            pressure: 0.,
            timestamp_ms,
            phase,
        });
    }
}

unsafe fn read_pointer(
    input: &Input,
    hwnd: HWND,
    id: u32,
    message: u32,
    message_contact: bool,
) -> bool {
    let active = input.active.get() == Some(id);
    if message != WM_POINTERDOWN && !active {
        return false;
    }

    let mut current = MaybeUninit::uninit();
    if unsafe { GetPointerPenInfo(id, current.as_mut_ptr()) } == 0 {
        if active && (message == WM_POINTERUP || !message_contact) {
            end_contact(input, None);
        }
        return active;
    }
    let current = unsafe { current.assume_init() };
    let flags = current.pointerInfo.pointerFlags;
    if flags & POINTER_FLAG_CANCELED != 0 {
        if active {
            end_contact(input, None);
        }
        return active;
    }
    if message == WM_POINTERDOWN {
        if active {
            end_contact(input, None);
        }
        if input.active.get().is_some() {
            return false;
        }
        input.native_press.set(false);
        let Some(pos) = client_position(hwnd, current.pointerInfo.ptPixelLocation) else {
            return false;
        };
        if !hit_surface(input, pos) || !in_contact(flags) {
            return false;
        }
        input.active.set(Some(id));
        input.native_press.set(true);
        push_contact(input, hwnd, &current);
        return true;
    }
    if message == WM_POINTERUP || flags & POINTER_FLAG_UP != 0 {
        let release = client_position(hwnd, current.pointerInfo.ptPixelLocation)
            .map(|point| (point, current.pointerInfo.dwTime));
        end_contact(input, release);
        return true;
    }
    if !message_contact || !in_contact(flags) {
        end_contact(input, None);
        return true;
    }

    // The current record already supplies the required capacity. Win32 writes
    // newest-first and may report fewer entries than requested.
    let mut count = current.pointerInfo.historyCount.clamp(1, 8192);
    if count == 1 {
        push_contact(input, hwnd, &current);
        return true;
    }
    let mut history = vec![current; count as usize];
    if unsafe { GetPointerPenInfoHistory(id, &mut count, history.as_mut_ptr()) } == 0 {
        push_contact(input, hwnd, &current);
    } else {
        let available = history.len().min(count as usize);
        for record in history[..available].iter().rev() {
            let record_flags = record.pointerInfo.pointerFlags;
            if in_contact(record_flags) && record_flags & POINTER_FLAG_UPDATE != 0 {
                push_contact(input, hwnd, record);
            }
        }
    }
    true
}

fn in_contact(flags: u32) -> bool {
    flags & (POINTER_FLAG_INCONTACT | POINTER_FLAG_UP | POINTER_FLAG_CANCELED)
        == POINTER_FLAG_INCONTACT
}

fn client_position(hwnd: HWND, mut point: POINT) -> Option<egui::Pos2> {
    (unsafe { ScreenToClient(hwnd, &mut point) } != 0)
        .then(|| egui::pos2(point.x as f32, point.y as f32))
}

fn push_contact(input: &Input, hwnd: HWND, record: &POINTER_PEN_INFO) {
    let pointer = &record.pointerInfo;
    let position = pointer.ptPixelLocation;
    let key = (
        pointer.PerformanceCount,
        pointer.dwTime,
        position.x,
        position.y,
        record.pressure,
    );
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
    let Some(position) = client_position(hwnd, position) else {
        return;
    };
    input.last_sample.set(Some(key));
    let pressure = if record.penMask & PEN_MASK_PRESSURE != 0 {
        record.pressure.min(1024) as f32 / 1024.
    } else {
        0.65
    };
    let Some(phase) = contact_phase(input, pressure) else {
        return;
    };
    input.samples.borrow_mut().push(PenSample {
        position: [position.x, position.y],
        pressure,
        timestamp_ms: pointer.dwTime,
        phase,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_contact_waits_for_pressure_and_recovers_after_zero() {
        let input = Input::default();
        input.active.set(Some(1));
        assert_eq!(contact_phase(&input, 0.), None);
        assert_eq!(contact_phase(&input, 0.001), Some(Phase::Down));
        assert_eq!(contact_phase(&input, 0.2), Some(Phase::Move));
        assert_eq!(contact_phase(&input, 0.), Some(Phase::Cancel));
        assert_eq!(input.active.get(), Some(1));
        assert_eq!(contact_phase(&input, 0.), None);
        assert_eq!(contact_phase(&input, 0.16), Some(Phase::Down));
    }

    #[test]
    fn lift_closes_contact_and_suppresses_its_promoted_release() {
        let input = Input::default();
        assert!(!suppress_promoted(&input, WM_LBUTTONDOWN));
        assert!(!suppress_promoted(&input, WM_LBUTTONUP));
        input.active.set(Some(1));
        input.native_press.set(true);
        assert!(suppress_promoted(&input, WM_LBUTTONDOWN));
        assert_eq!(contact_phase(&input, 0.3), Some(Phase::Down));

        end_contact(&input, Some((egui::pos2(160., 20.), 10)));
        assert_eq!(input.active.get(), None);
        let samples = input.samples.take();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].phase, Phase::Up);
        assert_eq!(samples[0].pressure, 0.);
        assert!(suppress_promoted(&input, WM_LBUTTONUP));
        assert!(!suppress_promoted(&input, WM_LBUTTONDOWN));
    }

    #[test]
    fn hover_and_cancelled_history_cannot_feed_paint() {
        assert!(in_contact(POINTER_FLAG_UPDATE | POINTER_FLAG_INCONTACT));
        assert!(!in_contact(POINTER_FLAG_UPDATE));
        assert!(!in_contact(POINTER_FLAG_INCONTACT | POINTER_FLAG_UP));
        assert!(!in_contact(POINTER_FLAG_INCONTACT | POINTER_FLAG_CANCELED));
    }
}
