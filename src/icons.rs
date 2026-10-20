//! App icons for the window's app list, read from each program's exe.

use windows::core::HSTRING;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::UI::Shell::SHDefExtractIconW;
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// Square RGBA pixels, `size` on a side.
pub struct Icon {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// The exe's own icon at `size` pixels, or None when it has none Blurman can read.
pub fn exe_icon(path: &str, size: u32) -> Option<Icon> {
    let mut icon = HICON::default();
    let found =
        unsafe { SHDefExtractIconW(&HSTRING::from(path), 0, 0, Some(&mut icon), None, size) };
    if found.is_err() || icon.is_invalid() {
        return None;
    }
    let pixels = unsafe { icon_pixels(icon) };
    unsafe {
        let _ = DestroyIcon(icon);
    }
    pixels
}

unsafe fn icon_pixels(icon: HICON) -> Option<Icon> {
    let mut info = ICONINFO::default();
    GetIconInfo(icon, &mut info).ok()?;
    let dc = CreateCompatibleDC(None);
    let color = read_bitmap(dc, info.hbmColor);
    let mask = read_bitmap(dc, info.hbmMask);
    let _ = DeleteDC(dc);
    let _ = DeleteObject(info.hbmColor.into());
    let _ = DeleteObject(info.hbmMask.into());
    let (width, height, mut bgra) = color?;
    if width != height || width == 0 {
        return None;
    }
    // Old icons carry no alpha and mark transparency in the mask instead: white is clear.
    let (pixels, _) = bgra.as_chunks_mut::<4>();
    if pixels.iter().all(|px| px[3] == 0) {
        let (_, _, mask) = mask?;
        let (mask, _) = mask.as_chunks::<4>();
        for (px, m) in pixels.iter_mut().zip(mask) {
            px[3] = if m[0] == 0 { 255 } else { 0 };
        }
    }
    for px in pixels {
        px.swap(0, 2);
    }
    Some(Icon {
        size: width,
        rgba: bgra,
    })
}

/// A bitmap as top-down 32-bit BGRA rows.
unsafe fn read_bitmap(dc: HDC, bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    if bitmap.is_invalid() {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    // With no buffer, GetDIBits only fills in the size.
    if GetDIBits(dc, bitmap, 0, 0, None, &mut info, DIB_RGB_COLORS) == 0 {
        return None;
    }
    let width = info.bmiHeader.biWidth.unsigned_abs();
    let height = info.bmiHeader.biHeight.unsigned_abs();
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    info.bmiHeader.biHeight = -(height as i32);
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;
    info.bmiHeader.biSizeImage = 0;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = GetDIBits(
        dc,
        bitmap,
        0,
        height,
        Some(pixels.as_mut_ptr().cast()),
        &mut info,
        DIB_RGB_COLORS,
    );
    (lines == height as i32).then_some((width, height, pixels))
}
