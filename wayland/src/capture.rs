use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::io::Read;
use std::os::fd::OwnedFd as StdOwnedFd;
use std::os::unix::net::UnixStream;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedFd, OwnedValue, Value};

const KWIN_SCREENSHOT_SERVICE: &str = "org.kde.KWin.ScreenShot2";
const KWIN_SCREENSHOT_PATH: &str = "/org/kde/KWin/ScreenShot2";
const KWIN_SCREENSHOT_INTERFACE: &str = "org.kde.KWin.ScreenShot2";

const QIMAGE_FORMAT_RGB32: u32 = 4;
const QIMAGE_FORMAT_ARGB32: u32 = 5;
const QIMAGE_FORMAT_ARGB32_PREMULTIPLIED: u32 = 6;
const QIMAGE_FORMAT_RGB888: u32 = 13;
const QIMAGE_FORMAT_RGBX8888: u32 = 16;
const QIMAGE_FORMAT_RGBA8888: u32 = 17;
const QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED: u32 = 18;
const QIMAGE_FORMAT_BGR888: u32 = 29;

pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub rgba: Vec<u8>,
}

pub fn capture_workspace(connection: &Connection) -> Result<CapturedFrame> {
    let (mut reader, writer) = UnixStream::pair().context("failed to create capture pipe")?;
    let writer_fd: StdOwnedFd = writer.into();
    let writer_fd = OwnedFd::from(writer_fd);

    let proxy = Proxy::new(
        connection,
        KWIN_SCREENSHOT_SERVICE,
        KWIN_SCREENSHOT_PATH,
        KWIN_SCREENSHOT_INTERFACE,
    )
    .context("failed to create KWin screenshot proxy")?;

    let options = HashMap::from([
        ("include-cursor", Value::from(false)),
        ("native-resolution", Value::from(true)),
        ("hide-caller-windows", Value::from(false)),
    ]);
    let metadata: HashMap<String, OwnedValue> = proxy
        .call("CaptureWorkspace", &(options, writer_fd))
        .context("KWin CaptureWorkspace failed")?;

    let width = metadata_u32(&metadata, "width")?;
    let height = metadata_u32(&metadata, "height")?;
    if width == 0 || height == 0 {
        bail!("KWin returned an empty {width}x{height} screenshot");
    }
    let stride = metadata_u32(&metadata, "stride")?;
    let format = metadata_u32(&metadata, "format")?;
    let scale = metadata_f64(&metadata, "scale").unwrap_or(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        bail!("KWin returned invalid screenshot scale {scale}");
    }

    let expected_len = (stride as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("KWin screenshot byte count overflow"))?;
    let mut raw = Vec::with_capacity(expected_len);
    reader
        .read_to_end(&mut raw)
        .context("failed to read KWin screenshot pixels")?;
    if raw.len() != expected_len {
        bail!(
            "KWin screenshot was truncated: expected {expected_len} bytes, received {}",
            raw.len()
        );
    }

    let rgba = qimage_to_rgba(raw, width, height, stride, format)?;
    Ok(CapturedFrame {
        width,
        height,
        scale,
        rgba,
    })
}

fn metadata_u32(metadata: &HashMap<String, OwnedValue>, key: &str) -> Result<u32> {
    let value = metadata
        .get(key)
        .ok_or_else(|| anyhow!("KWin screenshot metadata is missing '{key}'"))?;
    u32::try_from(value).with_context(|| format!("KWin screenshot metadata '{key}' is not u32"))
}

fn metadata_f64(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<f64> {
    metadata
        .get(key)
        .and_then(|value| f64::try_from(value).ok())
}

fn qimage_to_rgba(
    mut raw: Vec<u8>,
    width: u32,
    height: u32,
    stride: u32,
    format: u32,
) -> Result<Vec<u8>> {
    let bytes_per_pixel = match format {
        QIMAGE_FORMAT_RGB32
        | QIMAGE_FORMAT_ARGB32
        | QIMAGE_FORMAT_ARGB32_PREMULTIPLIED
        | QIMAGE_FORMAT_RGBX8888
        | QIMAGE_FORMAT_RGBA8888
        | QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED => 4,
        QIMAGE_FORMAT_RGB888 | QIMAGE_FORMAT_BGR888 => 3,
        _ => bail!("unsupported KWin QImage format {format}"),
    };

    let row_bytes = (width as usize)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| anyhow!("KWin screenshot row byte count overflow"))?;
    if (stride as usize) < row_bytes {
        bail!("KWin screenshot stride {stride} is smaller than row width {row_bytes}");
    }

    if stride as usize == row_bytes && bytes_per_pixel == 4 {
        match format {
            QIMAGE_FORMAT_RGB32 | QIMAGE_FORMAT_ARGB32 | QIMAGE_FORMAT_ARGB32_PREMULTIPLIED => {
                for pixel in raw.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
            }
            QIMAGE_FORMAT_RGBX8888 => {
                for pixel in raw.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
            }
            QIMAGE_FORMAT_RGBA8888 | QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED => {}
            _ => unreachable!(),
        }
        return Ok(raw);
    }

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("KWin screenshot pixel count overflow"))?;
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    for y in 0..height as usize {
        let row_start = y * stride as usize;
        let row = &raw[row_start..row_start + row_bytes];
        match format {
            QIMAGE_FORMAT_RGB32 | QIMAGE_FORMAT_ARGB32 | QIMAGE_FORMAT_ARGB32_PREMULTIPLIED => {
                for pixel in row.chunks_exact(4) {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
            }
            QIMAGE_FORMAT_RGBX8888 => {
                for pixel in row.chunks_exact(4) {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            QIMAGE_FORMAT_RGBA8888 | QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED => {
                rgba.extend_from_slice(row);
            }
            QIMAGE_FORMAT_RGB888 => {
                for pixel in row.chunks_exact(3) {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
            QIMAGE_FORMAT_BGR888 => {
                for pixel in row.chunks_exact(3) {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_argb32_with_stride_padding() {
        let raw = [
            3, 2, 1, 255, 6, 5, 4, 255, 0, 0, 0, 0, 9, 8, 7, 255, 12, 11, 10, 255, 0, 0, 0, 0,
        ];
        let rgba = qimage_to_rgba(raw.to_vec(), 2, 2, 12, QIMAGE_FORMAT_ARGB32_PREMULTIPLIED)
            .expect("ARGB32 conversion should succeed");
        assert_eq!(
            rgba,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255,]
        );
    }

    #[test]
    fn converts_rgba8888_without_swizzling() {
        let raw = [1, 2, 3, 4, 5, 6, 7, 8];
        let rgba = qimage_to_rgba(raw.to_vec(), 2, 1, 8, QIMAGE_FORMAT_RGBA8888)
            .expect("RGBA conversion should succeed");
        assert_eq!(rgba, raw);
    }

    #[test]
    fn rejects_short_stride() {
        let error = qimage_to_rgba(vec![0; 8], 3, 1, 8, QIMAGE_FORMAT_RGB32)
            .expect_err("short stride must fail");
        assert!(error.to_string().contains("stride"));
    }
}
