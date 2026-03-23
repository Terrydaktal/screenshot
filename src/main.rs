use eframe::egui;
use image::{DynamicImage, RgbaImage};
use chrono::Local;
use std::path::PathBuf;
use x11_dl::xlib;

fn main() -> eframe::Result {
    let mut capturer = captrs::Capturer::new(0).expect("Failed to create capturer");
    let (width, height) = capturer.geometry();
    
    // Prefer a few short retries over a single long fallback delay.
    let image_data = capture_frame_with_retries(&mut capturer);
    
    let mut rgba_pixels = Vec::with_capacity((width * height * 4) as usize);
    for pixel in image_data {
        rgba_pixels.push(pixel.r);
        rgba_pixels.push(pixel.g);
        rgba_pixels.push(pixel.b);
        rgba_pixels.push(255);
    }
    
    let rgba_image = RgbaImage::from_raw(width as u32, height as u32, rgba_pixels).unwrap();
    let dynamic_image = DynamicImage::ImageRgba8(rgba_image);

    // Context menus can keep pointer/keyboard grabs active; release grabs after capture
    // so the selection overlay remains interactive without losing the captured menu frame.
    release_x11_grabs();

    let screen_size = [width as f32, height as f32];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Use fixed origin + explicit size to avoid WM fullscreen transition animations.
            .with_position([0.0, 0.0])
            .with_inner_size(screen_size)
            .with_resizable(false)
            .with_always_on_top()
            .with_decorations(false)
            .with_active(true)
            .with_transparent(true),
        ..Default::default()
    };
    
    eframe::run_native(
        "Screencap Rust",
        options,
        Box::new(move |cc| {
            let size = [width as usize, height as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                size,
                dynamic_image.as_rgba8().unwrap().as_raw(),
            );
            let texture = cc.egui_ctx.load_texture("screen_capture", color_image, Default::default());

            Ok(Box::new(ScreencapApp {
                texture,
                full_image: dynamic_image,
                selection: None,
                anchor_point: None,
                drag_mode: DragMode::None,
                save_requested: false,
                copy_requested: false,
                screen_width: width as f32,
                screen_height: height as f32,
                focus_requested: false,
            }))
        }),
    )
}

fn capture_frame_with_retries(capturer: &mut captrs::Capturer) -> Vec<captrs::Bgr8> {
    const MAX_RETRIES: usize = 6;
    const RETRY_SLEEP_MS: u64 = 8;

    let mut last_error: Option<captrs::CaptureError> = None;
    for attempt in 0..=MAX_RETRIES {
        match capturer.capture_frame() {
            Ok(data) => return data,
            Err(err) => {
                last_error = Some(err);
                if attempt < MAX_RETRIES {
                    std::thread::sleep(std::time::Duration::from_millis(RETRY_SLEEP_MS));
                }
            }
        }
    }

    panic!("Capture failed after retries: {:?}", last_error);
}

fn release_x11_grabs() {
    unsafe {
        let xl = match xlib::Xlib::open() {
            Ok(lib) => lib,
            Err(_) => return,
        };

        let display = (xl.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return;
        }

        (xl.XUngrabPointer)(display, xlib::CurrentTime);
        (xl.XUngrabKeyboard)(display, xlib::CurrentTime);
        (xl.XSync)(display, 0);
        (xl.XCloseDisplay)(display);
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum DragMode {
    None,
    Creating,
    Moving,
    Resizing(ResizeEdge),
}

#[derive(PartialEq, Clone, Copy, Debug)]
struct ResizeEdge {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

struct ScreencapApp {
    texture: egui::TextureHandle,
    full_image: DynamicImage,
    selection: Option<egui::Rect>,
    anchor_point: Option<egui::Pos2>,
    drag_mode: DragMode,
    save_requested: bool,
    copy_requested: bool,
    screen_width: f32,
    screen_height: f32,
    focus_requested: bool,
}

impl eframe::App for ScreencapApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Request focus once; forcing it every frame can cause flicker.
        if !self.focus_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus_requested = true;
        }

        // 2. Gather All Input and Events
        let mut trigger_copy = false;
        let mut trigger_save = false;
        let mut trigger_exit = false;

        let (enter, esc, pointer_pos, primary_down, primary_pressed) = ctx.input(|i| {
            // Check events for high-level Copy/Save signals
            for event in &i.events {
                match event {
                    egui::Event::Copy => trigger_copy = true,
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if *key == egui::Key::C && (modifiers.ctrl || modifiers.command) { trigger_copy = true; }
                        if *key == egui::Key::X && (modifiers.ctrl || modifiers.command) { trigger_copy = true; }
                        if *key == egui::Key::S && (modifiers.ctrl || modifiers.command) { trigger_save = true; }
                        if *key == egui::Key::Enter { trigger_copy = true; }
                        if *key == egui::Key::Escape { trigger_exit = true; }
                    }
                    _ => {}
                }
            }

            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.pointer.interact_pos(),
                i.pointer.primary_down(),
                i.pointer.primary_pressed()
            )
        });

        // Use action flags
        if trigger_exit || esc { std::process::exit(0); }
        if self.selection.is_some() {
            if trigger_copy || enter { self.copy_requested = true; }
            if trigger_save { self.save_requested = true; }
        }

        let screen_rect = ctx.input(|i| i.screen_rect());
        let painter = ctx.layer_painter(egui::LayerId::background());

        // 3. Draw Background
        painter.image(
            self.texture.id(),
            screen_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(120));

        // 4. Interaction Logic
        if let Some(pos) = pointer_pos {
            if primary_pressed && !ctx.wants_pointer_input() {
                self.drag_mode = DragMode::None;
                if let Some(rect) = self.selection {
                    let handle_size = 20.0;
                    let on_top = (pos.y - rect.min.y).abs() < handle_size;
                    let on_bottom = (pos.y - rect.max.y).abs() < handle_size;
                    let on_left = (pos.x - rect.min.x).abs() < handle_size;
                    let on_right = (pos.x - rect.max.x).abs() < handle_size;
                    let edge_check = pos.x >= rect.min.x - handle_size && pos.x <= rect.max.x + handle_size && 
                                     pos.y >= rect.min.y - handle_size && pos.y <= rect.max.y + handle_size;

                    if (on_top || on_bottom || on_left || on_right) && edge_check {
                        self.drag_mode = DragMode::Resizing(ResizeEdge { top: on_top, bottom: on_bottom, left: on_left, right: on_right });
                    } else if rect.contains(pos) {
                        self.drag_mode = DragMode::Moving;
                    } else {
                        self.anchor_point = Some(pos);
                        self.selection = Some(egui::Rect::from_min_size(pos, egui::Vec2::ZERO));
                        self.drag_mode = DragMode::Creating;
                    }
                } else {
                    self.anchor_point = Some(pos);
                    self.selection = Some(egui::Rect::from_min_size(pos, egui::Vec2::ZERO));
                    self.drag_mode = DragMode::Creating;
                }
            }

            if primary_down {
                match self.drag_mode {
                    DragMode::Creating => {
                        if let (Some(anchor), Some(rect)) = (self.anchor_point, self.selection.as_mut()) {
                            *rect = egui::Rect::from_two_pos(anchor, pos);
                        }
                    }
                    DragMode::Moving => {
                        if let Some(ref mut rect) = self.selection {
                            let delta = ctx.input(|i| i.pointer.delta());
                            *rect = rect.translate(delta);
                        }
                    }
                    DragMode::Resizing(edge) => {
                        if let Some(ref mut rect) = self.selection {
                            if edge.top { rect.min.y = pos.y.min(rect.max.y - 1.0); }
                            if edge.bottom { rect.max.y = pos.y.max(rect.min.y + 1.0); }
                            if edge.left { rect.min.x = pos.x.min(rect.max.x - 1.0); }
                            if edge.right { rect.max.x = pos.x.max(rect.min.x + 1.0); }
                        }
                    }
                    _ => {}
                }
            } else {
                self.drag_mode = DragMode::None;
            }
        }

        // 5. Draw Selection
        if let Some(rect) = self.selection {
            let rect = rect.intersect(screen_rect);
            let mut mesh = egui::Mesh::with_texture(self.texture.id());
            mesh.add_rect_with_uv(
                rect,
                egui::Rect::from_min_max(
                    egui::pos2(rect.min.x / self.screen_width, rect.min.y / self.screen_height),
                    egui::pos2(rect.max.x / self.screen_width, rect.max.y / self.screen_height),
                ),
                egui::Color32::WHITE,
            );
            painter.add(mesh);
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::WHITE), egui::StrokeKind::Outside);

            // Controls
            if self.drag_mode == DragMode::None && rect.width() > 10.0 {
                let button_size = 36.0;
                let spacing = 8.0;
                let controls_size = egui::vec2(button_size * 2.0 + spacing, button_size);
                let desired_pos = egui::pos2(rect.max.x - controls_size.x, rect.max.y + 8.0);
                let controls_pos = egui::pos2(
                    desired_pos.x.clamp(0.0, screen_rect.max.x - controls_size.x),
                    desired_pos.y.clamp(0.0, screen_rect.max.y - controls_size.y),
                );

                egui::Area::new(egui::Id::new("selection_controls"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(controls_pos)
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(spacing, 0.0);
                        ui.horizontal(|ui| {
                            if Self::draw_icon_button(ui, IconKind::Copy, button_size, egui::Color32::from_rgb(34, 139, 230)).clicked() {
                                self.copy_requested = true;
                            }
                            if Self::draw_icon_button(ui, IconKind::Save, button_size, egui::Color32::from_rgb(46, 184, 92)).clicked() {
                                self.save_requested = true;
                            }
                        });
                    });
            }
        }

        // 6. Action Execution
        if self.save_requested {
            if let Some(rect) = self.selection { self.save_image(rect); }
            std::process::exit(0);
        }
        if self.copy_requested {
            if let Some(rect) = self.selection { self.copy_image(rect); }
            std::process::exit(0);
        }
    }
}

impl ScreencapApp {
    fn draw_icon_button(
        ui: &mut egui::Ui,
        kind: IconKind,
        size: f32,
        fill: egui::Color32,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter();

        let hovered_fill = fill.gamma_multiply(1.15);
        let bg = if response.hovered() { hovered_fill } else { fill };
        let border = egui::Color32::from_white_alpha(220);
        let icon = egui::Color32::WHITE;
        let stroke = egui::Stroke::new(1.8, icon);

        painter.circle_filled(rect.center(), rect.width() * 0.5, bg);
        painter.circle_stroke(rect.center(), rect.width() * 0.5, egui::Stroke::new(1.0, border));

        match kind {
            IconKind::Copy => {
                let back = rect.shrink(size * 0.34).translate(egui::vec2(-size * 0.07, size * 0.07));
                let front = rect.shrink(size * 0.34).translate(egui::vec2(size * 0.07, -size * 0.07));
                Self::stroke_rect(painter, back, stroke);
                Self::stroke_rect(painter, front, stroke);
            }
            IconKind::Save => {
                let body = rect.shrink(size * 0.30);
                Self::stroke_rect(painter, body, stroke);
                let slot_y = body.top() + body.height() * 0.32;
                painter.line_segment(
                    [egui::pos2(body.left(), slot_y), egui::pos2(body.right(), slot_y)],
                    stroke,
                );
                let notch = egui::Rect::from_min_max(
                    egui::pos2(body.left() + body.width() * 0.60, body.top() + body.height() * 0.12),
                    egui::pos2(body.right() - body.width() * 0.12, body.top() + body.height() * 0.32),
                );
                painter.rect_filled(notch, 0.0, icon);
            }
        }

        response
    }

    fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
        painter.line_segment([rect.left_top(), rect.right_top()], stroke);
        painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
        painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
        painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
    }

    fn save_image(&self, rect: egui::Rect) {
        let (x, y, w, h) = (rect.min.x as u32, rect.min.y as u32, rect.width() as u32, rect.height() as u32);
        if w == 0 || h == 0 { return; }
        let cropped = self.full_image.crop_imm(x, y, w, h);
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = format!("screenshot_{}.png", timestamp);
        let mut path = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("Pictures"); path.push("Screenshots");
        let _ = std::fs::create_dir_all(&path);
        path.push(filename);
        let _ = cropped.save(&path);
    }

    fn copy_image(&self, rect: egui::Rect) {
        let (x, y, w, h) = (rect.min.x as u32, rect.min.y as u32, rect.width() as u32, rect.height() as u32);
        if w == 0 || h == 0 { return; }
        let cropped = self.full_image.crop_imm(x, y, w, h);
        let mut buffer: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);
        let _ = cropped.write_to(&mut cursor, image::ImageFormat::Png);
        use std::io::Write;
        use std::process::{Command, Stdio};
        
        if let Ok(mut child) = Command::new("copyq").args(&["copy", "image/png", "-"]).stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            let _ = child.wait();
        } else if let Ok(mut child) = Command::new("xclip").args(&["-selection", "clipboard", "-t", "image/png"]).stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Copy)]
enum IconKind {
    Copy,
    Save,
}
