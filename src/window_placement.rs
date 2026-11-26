use serde::{Deserialize, Serialize};
use std::{io, io::Write, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    UI::WindowsAndMessaging::IsZoomed,
};
use winit::{
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub const MIN_SIZE: LogicalSize<u32> = LogicalSize::new(520, 400);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WindowPlacement {
    x: i32,
    y: i32,
    width: f64,
    height: f64,
}

impl Default for WindowPlacement {
    fn default() -> Self {
        Self {
            x: 300,
            y: 65,
            width: 1261.0,
            height: 863.0,
        }
    }
}

impl WindowPlacement {
    pub fn load() -> Self {
        preferences_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|placement| {
                placement.width.is_finite()
                    && placement.height.is_finite()
                    && placement.width > 0.0
                    && placement.height > 0.0
            })
            .unwrap_or_default()
    }

    pub fn save(self) -> io::Result<()> {
        let Some(path) = preferences_path() else {
            return Ok(());
        };
        let directory = path.parent().expect("preferences directory");
        std::fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(file.as_file_mut(), &self)?;
        file.as_file_mut().flush()?;
        file.persist(path)?;
        Ok(())
    }

    pub fn capture(window: &Window) -> Option<Self> {
        let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // Winit's requested maximize flag can lag a native title-bar action.
        let maximized = unsafe { IsZoomed(handle.hwnd.get() as _) } != 0;
        if maximized || window.is_minimized() == Some(true) || window.fullscreen().is_some() {
            return None;
        }
        let position = window.outer_position().ok()?;
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        (size.width > 0.0 && size.height > 0.0).then_some(Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        })
    }

    pub fn initial_position(self) -> PhysicalPosition<i32> {
        if let Some(area) = self.work_area() {
            // Place the small hidden window on the target monitor before measuring its frame.
            PhysicalPosition::new(area.left, area.top)
        } else {
            PhysicalPosition::new(self.x, self.y)
        }
    }

    pub fn apply(self, window: &Window) {
        let scale = window.scale_factor();
        if let Some(area) = self.work_area() {
            let inner = window.inner_size();
            let outer = window.outer_size();
            let frame = PhysicalSize::new(
                outer.width.saturating_sub(inner.width),
                outer.height.saturating_sub(inner.height),
            );
            let (position, size, minimum) = self.fit(area, scale, frame);
            window.set_min_inner_size(Some(minimum.to_logical::<f64>(scale)));
            let _ = window.request_inner_size(size);
            window.set_outer_position(position);
        } else {
            let _ = window.request_inner_size(LogicalSize::new(self.width, self.height));
        }
    }

    fn work_area(self) -> Option<RECT> {
        let point = POINT {
            x: self.x,
            y: self.y,
        };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }
        let area = info.rcWork;
        (area.right > area.left && area.bottom > area.top).then_some(area)
    }

    fn fit(
        self,
        area: RECT,
        scale: f64,
        frame: PhysicalSize<u32>,
    ) -> (PhysicalPosition<i32>, PhysicalSize<u32>, PhysicalSize<u32>) {
        let available = PhysicalSize::new(
            ((area.right - area.left) as u32)
                .saturating_sub(frame.width)
                .max(1),
            ((area.bottom - area.top) as u32)
                .saturating_sub(frame.height)
                .max(1),
        );
        let minimum = MIN_SIZE.to_physical::<u32>(scale);
        let minimum = PhysicalSize::new(
            minimum.width.min(available.width),
            minimum.height.min(available.height),
        );
        let requested = LogicalSize::new(self.width, self.height).to_physical::<u32>(scale);
        let size = PhysicalSize::new(
            requested.width.clamp(minimum.width, available.width),
            requested.height.clamp(minimum.height, available.height),
        );
        let position = PhysicalPosition::new(
            self.x.clamp(
                area.left,
                (area.right - (size.width + frame.width) as i32).max(area.left),
            ),
            self.y.clamp(
                area.top,
                (area.bottom - (size.height + frame.height) as i32).max(area.top),
            ),
        );
        (position, size, minimum)
    }
}

fn preferences_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|root| {
        PathBuf::from(root)
            .join("Pigment Studio")
            .join("window.json")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_launch_matches_the_captured_window_at_100_percent() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1032,
        };
        let (position, size, _) =
            WindowPlacement::default().fit(area, 1.0, PhysicalSize::new(16, 39));
        assert_eq!(position, PhysicalPosition::new(300, 65));
        assert_eq!(size, PhysicalSize::new(1261, 863));
    }

    #[test]
    fn restore_fits_a_smaller_work_area_after_a_display_change() {
        let saved = WindowPlacement {
            x: 2250,
            y: 200,
            ..WindowPlacement::default()
        };
        let area = RECT {
            left: -1280,
            top: 40,
            right: 0,
            bottom: 800,
        };
        let (position, size, _) = saved.fit(area, 1.5, PhysicalSize::new(24, 58));
        assert_eq!(position, PhysicalPosition::new(-1280, 40));
        assert_eq!(size, PhysicalSize::new(1256, 702));
    }
}
