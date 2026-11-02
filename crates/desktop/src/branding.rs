//! Static Sidecar assets, decoded once before the dashboard starts.
use eframe::egui;

pub const NAME: &str = "Sidecar";
// Matches the retained desktop filename, so Plasma resolves the Wayland icon.
pub const APP_ID: &str = "igpu-man";

pub fn load() -> eframe::Result<(egui::IconData, Vec<ksni::Icon>)> {
    let decode = |bytes: &[u8]| {
        eframe::icon_data::from_png_bytes(bytes)
            .map_err(|error| eframe::Error::AppCreation(Box::new(error)))
    };
    let window = decode(include_bytes!("../../../data/igpu-man-128.png"))?;
    let mut tray = Vec::with_capacity(3);
    for bytes in [
        include_bytes!("../../../data/igpu-man-24.png").as_slice(),
        include_bytes!("../../../data/igpu-man-32.png").as_slice(),
        include_bytes!("../../../data/igpu-man-48.png").as_slice(),
    ] {
        let mut image = decode(bytes)?;
        for pixel in image.rgba.as_chunks_mut::<4>().0 {
            pixel.rotate_right(1); // RGBA → network-byte-order ARGB, as SNI requires.
        }
        tray.push(ksni::Icon {
            width: image.width as i32,
            height: image.height as i32,
            data: image.rgba,
        });
    }
    Ok((window, tray))
}
