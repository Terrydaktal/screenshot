use chrono::Local;
use eframe::egui;
use image::{DynamicImage, Rgba, RgbaImage};
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
            let texture =
                cc.egui_ctx
                    .load_texture("screen_capture", color_image, Default::default());

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
                pen_mode: false,
                rect_mode: false,
                draw_color: egui::Color32::from_rgb(255, 203, 5),
                draw_size: 3.0,
                annotations: Vec::new(),
                current_pen_stroke: None,
                current_rect_shape: None,
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

#[derive(Clone)]
struct PenStroke {
    points: Vec<egui::Pos2>,
    color: egui::Color32,
    width: f32,
}

#[derive(Clone)]
struct RectShape {
    start: egui::Pos2,
    end: egui::Pos2,
    color: egui::Color32,
    width: f32,
}

#[derive(Clone)]
enum Annotation {
    Pen(PenStroke),
    Rect(RectShape),
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
    pen_mode: bool,
    rect_mode: bool,
    draw_color: egui::Color32,
    draw_size: f32,
    annotations: Vec<Annotation>,
    current_pen_stroke: Option<PenStroke>,
    current_rect_shape: Option<RectShape>,
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
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if *key == egui::Key::C && (modifiers.ctrl || modifiers.command) {
                            trigger_copy = true;
                        }
                        if *key == egui::Key::X && (modifiers.ctrl || modifiers.command) {
                            trigger_copy = true;
                        }
                        if *key == egui::Key::S && (modifiers.ctrl || modifiers.command) {
                            trigger_save = true;
                        }
                        if *key == egui::Key::Enter {
                            trigger_copy = true;
                        }
                        if *key == egui::Key::Escape {
                            trigger_exit = true;
                        }
                    }
                    _ => {}
                }
            }

            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
                i.pointer.interact_pos(),
                i.pointer.primary_down(),
                i.pointer.primary_pressed(),
            )
        });

        // Use action flags
        if trigger_exit || esc {
            std::process::exit(0);
        }
        if self.selection.is_some() {
            if trigger_copy || enter {
                self.copy_requested = true;
            }
            if trigger_save {
                self.save_requested = true;
            }
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
            if self.pen_mode || self.rect_mode {
                if let Some(rect) = self.selection {
                    let clamped_pos = Self::clamp_pos_to_rect(pos, rect);
                    if primary_pressed && !ctx.wants_pointer_input() && rect.contains(pos) {
                        if self.pen_mode {
                            self.current_rect_shape = None;
                            self.current_pen_stroke = Some(PenStroke {
                                points: vec![clamped_pos],
                                color: self.draw_color,
                                width: self.draw_size,
                            });
                        } else if self.rect_mode {
                            self.current_pen_stroke = None;
                            self.current_rect_shape = Some(RectShape {
                                start: clamped_pos,
                                end: clamped_pos,
                                color: self.draw_color,
                                width: self.draw_size,
                            });
                        }
                    }
                    if primary_down {
                        if self.pen_mode {
                            if let Some(stroke) = self.current_pen_stroke.as_mut() {
                                let should_push = stroke
                                    .points
                                    .last()
                                    .map(|last| last.distance(clamped_pos) >= 0.8)
                                    .unwrap_or(true);
                                if should_push {
                                    stroke.points.push(clamped_pos);
                                }
                            }
                        } else if self.rect_mode {
                            if let Some(shape) = self.current_rect_shape.as_mut() {
                                shape.end = clamped_pos;
                            }
                        }
                    } else {
                        self.finalize_active_annotation();
                    }
                }
            } else {
                if primary_pressed && !ctx.wants_pointer_input() {
                    self.drag_mode = DragMode::None;
                    if let Some(rect) = self.selection {
                        let handle_size = 20.0;
                        let on_top = (pos.y - rect.min.y).abs() < handle_size;
                        let on_bottom = (pos.y - rect.max.y).abs() < handle_size;
                        let on_left = (pos.x - rect.min.x).abs() < handle_size;
                        let on_right = (pos.x - rect.max.x).abs() < handle_size;
                        let edge_check = pos.x >= rect.min.x - handle_size
                            && pos.x <= rect.max.x + handle_size
                            && pos.y >= rect.min.y - handle_size
                            && pos.y <= rect.max.y + handle_size;

                        if (on_top || on_bottom || on_left || on_right) && edge_check {
                            self.drag_mode = DragMode::Resizing(ResizeEdge {
                                top: on_top,
                                bottom: on_bottom,
                                left: on_left,
                                right: on_right,
                            });
                        } else if rect.contains(pos) {
                            self.drag_mode = DragMode::Moving;
                        } else {
                            self.clear_all_annotations();
                            self.anchor_point = Some(pos);
                            self.selection = Some(egui::Rect::from_min_size(pos, egui::Vec2::ZERO));
                            self.drag_mode = DragMode::Creating;
                        }
                    } else {
                        self.clear_all_annotations();
                        self.anchor_point = Some(pos);
                        self.selection = Some(egui::Rect::from_min_size(pos, egui::Vec2::ZERO));
                        self.drag_mode = DragMode::Creating;
                    }
                }

                if primary_down {
                    match self.drag_mode {
                        DragMode::Creating => {
                            if let (Some(anchor), Some(rect)) =
                                (self.anchor_point, self.selection.as_mut())
                            {
                                *rect = egui::Rect::from_two_pos(anchor, pos);
                            }
                        }
                        DragMode::Moving => {
                            if let Some(ref mut rect) = self.selection {
                                let delta = ctx.input(|i| i.pointer.delta());
                                *rect = rect.translate(delta);
                                self.translate_annotations(delta);
                            }
                        }
                        DragMode::Resizing(edge) => {
                            if let Some(ref mut rect) = self.selection {
                                if edge.top {
                                    rect.min.y = pos.y.min(rect.max.y - 1.0);
                                }
                                if edge.bottom {
                                    rect.max.y = pos.y.max(rect.min.y + 1.0);
                                }
                                if edge.left {
                                    rect.min.x = pos.x.min(rect.max.x - 1.0);
                                }
                                if edge.right {
                                    rect.max.x = pos.x.max(rect.min.x + 1.0);
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    self.drag_mode = DragMode::None;
                }
            }
        }
        if !primary_down {
            self.finalize_active_annotation();
        }

        // 5. Draw Selection
        if let Some(rect) = self.selection {
            let rect = rect.intersect(screen_rect);
            let mut mesh = egui::Mesh::with_texture(self.texture.id());
            mesh.add_rect_with_uv(
                rect,
                egui::Rect::from_min_max(
                    egui::pos2(
                        rect.min.x / self.screen_width,
                        rect.min.y / self.screen_height,
                    ),
                    egui::pos2(
                        rect.max.x / self.screen_width,
                        rect.max.y / self.screen_height,
                    ),
                ),
                egui::Color32::WHITE,
            );
            painter.add(mesh);
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
                egui::StrokeKind::Outside,
            );
            let annotation_painter = painter.with_clip_rect(rect);
            self.draw_annotations_on_screen(&annotation_painter);

            // Controls
            if self.drag_mode == DragMode::None && rect.width() > 10.0 {
                // Keep controls above the desktop panel/taskbar area.
                const BOTTOM_SAFE_INSET: f32 = 56.0;
                let button_size = 36.0;
                let spacing = 8.0;
                let action_size = egui::vec2(button_size * 4.0 + spacing * 3.0, button_size);
                let visible_bottom = (screen_rect.max.y - BOTTOM_SAFE_INSET)
                    .max(screen_rect.min.y + action_size.y + 4.0);
                let action_y_outside = rect.max.y + 8.0;
                let action_y_inside = rect.max.y - action_size.y - 8.0;
                let action_y = if action_y_outside + action_size.y <= visible_bottom {
                    action_y_outside
                } else {
                    // If controls would go off-screen at the bottom, move them inside the selection.
                    action_y_inside.max(rect.min.y + 4.0)
                };
                let action_desired_pos = egui::pos2(rect.max.x - action_size.x, action_y);
                let action_pos = egui::pos2(
                    action_desired_pos
                        .x
                        .clamp(0.0, screen_rect.max.x - action_size.x),
                    action_desired_pos
                        .y
                        .clamp(0.0, visible_bottom - action_size.y),
                );

                egui::Area::new(egui::Id::new("selection_action_controls"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(action_pos)
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(spacing, 0.0);
                        ui.horizontal(|ui| {
                            if Self::draw_icon_button(
                                ui,
                                IconKind::Pen,
                                button_size,
                                egui::Color32::from_rgb(230, 164, 38),
                                self.pen_mode,
                            )
                            .clicked()
                            {
                                self.pen_mode = !self.pen_mode;
                                if self.pen_mode {
                                    self.rect_mode = false;
                                }
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if Self::draw_icon_button(
                                ui,
                                IconKind::Rect,
                                button_size,
                                egui::Color32::from_rgb(223, 126, 78),
                                self.rect_mode,
                            )
                            .clicked()
                            {
                                self.rect_mode = !self.rect_mode;
                                if self.rect_mode {
                                    self.pen_mode = false;
                                }
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if Self::draw_icon_button(
                                ui,
                                IconKind::Copy,
                                button_size,
                                egui::Color32::from_rgb(34, 139, 230),
                                false,
                            )
                            .clicked()
                            {
                                self.copy_requested = true;
                            }
                            if Self::draw_icon_button(
                                ui,
                                IconKind::Save,
                                button_size,
                                egui::Color32::from_rgb(46, 184, 92),
                                false,
                            )
                            .clicked()
                            {
                                self.save_requested = true;
                            }
                        });
                    });

                let draw_controls_active = self.pen_mode || self.rect_mode;
                if draw_controls_active {
                    let slider_width = 150.0;
                    let color_width = 40.0;
                    let draw_tools_gap = 20.0;
                    let draw_tools_size =
                        egui::vec2(color_width + spacing + slider_width + spacing + button_size, button_size);
                    let left_aligned_x = action_pos.x - draw_tools_gap - draw_tools_size.x;
                    let draw_tools_pos = if left_aligned_x >= 0.0 {
                        egui::pos2(
                            left_aligned_x,
                            action_pos.y.clamp(0.0, visible_bottom - draw_tools_size.y),
                        )
                    } else {
                        // If there's not enough space to the left, place draw controls above the action strip.
                        egui::pos2(
                            action_pos.x,
                            (action_pos.y - draw_tools_size.y - spacing)
                                .max(rect.min.y + 4.0)
                                .max(0.0),
                        )
                    };

                    egui::Area::new(egui::Id::new("selection_draw_controls"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(draw_tools_pos)
                        .show(ctx, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(spacing, 0.0);
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(color_width, button_size),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.color_edit_button_srgba(&mut self.draw_color);
                                    },
                                );
                                ui.add_sized(
                                    [slider_width, button_size],
                                    egui::Slider::new(&mut self.draw_size, 1.0..=16.0),
                                );
                                if Self::draw_icon_button(
                                    ui,
                                    IconKind::Undo,
                                    button_size,
                                    egui::Color32::from_rgb(120, 120, 120),
                                    false,
                                )
                                .clicked()
                                {
                                    self.undo_annotation();
                                }
                            });
                        });
                }
            }
        }

        // 6. Action Execution
        if self.save_requested {
            if let Some(rect) = self.selection {
                self.save_image(rect);
            }
            std::process::exit(0);
        }
        if self.copy_requested {
            if let Some(rect) = self.selection {
                self.copy_image(rect);
            }
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
        active: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter();

        let active_fill = fill.gamma_multiply(1.22);
        let hovered_fill = fill.gamma_multiply(1.15);
        let bg = if active {
            active_fill
        } else if response.hovered() {
            hovered_fill
        } else {
            fill
        };
        let border = egui::Color32::from_white_alpha(220);
        let icon = egui::Color32::WHITE;
        let stroke = egui::Stroke::new(1.8, icon);

        painter.circle_filled(rect.center(), rect.width() * 0.5, bg);
        let border_width = if active { 1.8 } else { 1.0 };
        painter.circle_stroke(
            rect.center(),
            rect.width() * 0.5,
            egui::Stroke::new(border_width, border),
        );

        match kind {
            IconKind::Pen => {
                let left = rect.left() + size * 0.28;
                let right = rect.right() - size * 0.26;
                let top = rect.top() + size * 0.30;
                let bottom = rect.bottom() - size * 0.28;
                painter.line_segment([egui::pos2(left, bottom), egui::pos2(right, top)], stroke);
                painter.line_segment(
                    [
                        egui::pos2(right, top),
                        egui::pos2(right - size * 0.12, top + size * 0.10),
                    ],
                    stroke,
                );
            }
            IconKind::Rect => {
                let shape = rect.shrink(size * 0.30);
                Self::stroke_rect(painter, shape, stroke);
            }
            IconKind::Undo => {
                let center = rect.center() + egui::vec2(size * 0.05, 0.0);
                let radius = size * 0.22;
                let mut arc_points = Vec::with_capacity(14);
                for i in 0..=13 {
                    let t = i as f32 / 13.0;
                    let angle = (200.0_f32 - 180.0 * t).to_radians();
                    arc_points.push(egui::pos2(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    ));
                }
                painter.add(egui::Shape::line(
                    arc_points.clone(),
                    egui::Stroke::new(2.1, icon),
                ));

                let tip = arc_points[0];
                let head = size * 0.11;
                painter.line_segment(
                    [tip, egui::pos2(tip.x + head, tip.y - head * 0.75)],
                    egui::Stroke::new(2.1, icon),
                );
                painter.line_segment(
                    [tip, egui::pos2(tip.x + head, tip.y + head * 0.75)],
                    egui::Stroke::new(2.1, icon),
                );
            }
            IconKind::Copy => {
                let back = rect
                    .shrink(size * 0.34)
                    .translate(egui::vec2(-size * 0.07, size * 0.07));
                let front = rect
                    .shrink(size * 0.34)
                    .translate(egui::vec2(size * 0.07, -size * 0.07));
                Self::stroke_rect(painter, back, stroke);
                Self::stroke_rect(painter, front, stroke);
            }
            IconKind::Save => {
                let body = rect.shrink(size * 0.30);
                Self::stroke_rect(painter, body, stroke);
                let slot_y = body.top() + body.height() * 0.32;
                painter.line_segment(
                    [
                        egui::pos2(body.left(), slot_y),
                        egui::pos2(body.right(), slot_y),
                    ],
                    stroke,
                );
                let notch = egui::Rect::from_min_max(
                    egui::pos2(
                        body.left() + body.width() * 0.60,
                        body.top() + body.height() * 0.12,
                    ),
                    egui::pos2(
                        body.right() - body.width() * 0.12,
                        body.top() + body.height() * 0.32,
                    ),
                );
                painter.rect_filled(notch, 0.0, icon);
            }
        }

        response
    }

    fn clear_active_annotation_preview(&mut self) {
        self.current_pen_stroke = None;
        self.current_rect_shape = None;
    }

    fn clear_all_annotations(&mut self) {
        self.annotations.clear();
        self.clear_active_annotation_preview();
    }

    fn undo_annotation(&mut self) {
        if self.current_pen_stroke.is_some() {
            self.current_pen_stroke = None;
            return;
        }
        if self.current_rect_shape.is_some() {
            self.current_rect_shape = None;
            return;
        }
        let _ = self.annotations.pop();
    }

    fn finalize_active_annotation(&mut self) {
        if let Some(stroke) = self.current_pen_stroke.take() {
            if stroke.points.len() > 1 {
                self.annotations.push(Annotation::Pen(stroke));
            }
        }
        if let Some(shape) = self.current_rect_shape.take() {
            let rect = egui::Rect::from_two_pos(shape.start, shape.end);
            if rect.width() >= 1.0 && rect.height() >= 1.0 {
                self.annotations.push(Annotation::Rect(shape));
            }
        }
    }

    fn translate_annotations(&mut self, delta: egui::Vec2) {
        if delta == egui::Vec2::ZERO {
            return;
        }
        for annotation in &mut self.annotations {
            match annotation {
                Annotation::Pen(stroke) => {
                    for point in &mut stroke.points {
                        *point += delta;
                    }
                }
                Annotation::Rect(shape) => {
                    shape.start += delta;
                    shape.end += delta;
                }
            }
        }
        if let Some(stroke) = &mut self.current_pen_stroke {
            for point in &mut stroke.points {
                *point += delta;
            }
        }
        if let Some(shape) = &mut self.current_rect_shape {
            shape.start += delta;
            shape.end += delta;
        }
    }

    fn draw_annotations_on_screen(&self, painter: &egui::Painter) {
        for annotation in &self.annotations {
            Self::draw_annotation_on_screen(painter, annotation);
        }
        if let Some(stroke) = &self.current_pen_stroke {
            Self::draw_pen_stroke_path(painter, stroke);
        }
        if let Some(shape) = &self.current_rect_shape {
            Self::draw_rect_shape_path(painter, shape);
        }
    }

    fn draw_annotation_on_screen(painter: &egui::Painter, annotation: &Annotation) {
        match annotation {
            Annotation::Pen(stroke) => Self::draw_pen_stroke_path(painter, stroke),
            Annotation::Rect(shape) => Self::draw_rect_shape_path(painter, shape),
        }
    }

    fn draw_pen_stroke_path(painter: &egui::Painter, stroke: &PenStroke) {
        if stroke.points.len() < 2 {
            return;
        }
        let line = egui::Stroke::new(stroke.width, stroke.color);
        for segment in stroke.points.windows(2) {
            painter.line_segment([segment[0], segment[1]], line);
        }
    }

    fn draw_rect_shape_path(painter: &egui::Painter, shape: &RectShape) {
        let rect = egui::Rect::from_two_pos(shape.start, shape.end);
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(shape.width, shape.color),
            egui::StrokeKind::Outside,
        );
    }

    fn clamp_pos_to_rect(pos: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        egui::pos2(
            pos.x.clamp(rect.min.x, rect.max.x),
            pos.y.clamp(rect.min.y, rect.max.y),
        )
    }

    fn crop_with_annotations(&self, rect: egui::Rect) -> Option<RgbaImage> {
        let (x, y, w, h) = (
            rect.min.x as u32,
            rect.min.y as u32,
            rect.width() as u32,
            rect.height() as u32,
        );
        if w == 0 || h == 0 {
            return None;
        }

        let mut cropped = self.full_image.crop_imm(x, y, w, h).to_rgba8();
        self.draw_annotations_on_image(&mut cropped, rect);
        Some(cropped)
    }

    fn draw_annotations_on_image(&self, image: &mut RgbaImage, rect: egui::Rect) {
        for annotation in &self.annotations {
            Self::draw_annotation_on_image(image, rect, annotation);
        }
        if let Some(stroke) = &self.current_pen_stroke {
            Self::draw_pen_stroke_on_image(image, rect, stroke);
        }
        if let Some(shape) = &self.current_rect_shape {
            Self::draw_rect_shape_on_image(image, rect, shape);
        }
    }

    fn draw_annotation_on_image(image: &mut RgbaImage, rect: egui::Rect, annotation: &Annotation) {
        match annotation {
            Annotation::Pen(stroke) => Self::draw_pen_stroke_on_image(image, rect, stroke),
            Annotation::Rect(shape) => Self::draw_rect_shape_on_image(image, rect, shape),
        }
    }

    fn draw_pen_stroke_on_image(image: &mut RgbaImage, rect: egui::Rect, stroke: &PenStroke) {
        if stroke.points.len() < 2 {
            return;
        }

        let rgba = Rgba([
            stroke.color.r(),
            stroke.color.g(),
            stroke.color.b(),
            stroke.color.a(),
        ]);

        for segment in stroke.points.windows(2) {
            let p0 = segment[0];
            let p1 = segment[1];
            let x0 = p0.x - rect.min.x;
            let y0 = p0.y - rect.min.y;
            let x1 = p1.x - rect.min.x;
            let y1 = p1.y - rect.min.y;
            Self::draw_thick_line(image, x0, y0, x1, y1, stroke.width, rgba);
        }
    }

    fn draw_rect_shape_on_image(image: &mut RgbaImage, rect: egui::Rect, shape: &RectShape) {
        let draw_rect = egui::Rect::from_two_pos(shape.start, shape.end);
        if draw_rect.width() < 1.0 || draw_rect.height() < 1.0 {
            return;
        }

        let rgba = Rgba([
            shape.color.r(),
            shape.color.g(),
            shape.color.b(),
            shape.color.a(),
        ]);

        let left = draw_rect.min.x - rect.min.x;
        let right = draw_rect.max.x - rect.min.x;
        let top = draw_rect.min.y - rect.min.y;
        let bottom = draw_rect.max.y - rect.min.y;

        Self::draw_thick_line(image, left, top, right, top, shape.width, rgba);
        Self::draw_thick_line(image, right, top, right, bottom, shape.width, rgba);
        Self::draw_thick_line(image, right, bottom, left, bottom, shape.width, rgba);
        Self::draw_thick_line(image, left, bottom, left, top, shape.width, rgba);
    }

    fn draw_thick_line(
        image: &mut RgbaImage,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        color: Rgba<u8>,
    ) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = dx.abs().max(dy.abs()).ceil() as i32;
        if steps <= 0 {
            Self::draw_disc(image, x0, y0, width * 0.5, color);
            return;
        }

        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let x = x0 + dx * t;
            let y = y0 + dy * t;
            Self::draw_disc(image, x, y, width * 0.5, color);
        }
    }

    fn draw_disc(image: &mut RgbaImage, cx: f32, cy: f32, radius: f32, color: Rgba<u8>) {
        let r = radius.max(1.0).ceil() as i32;
        let cx = cx.round() as i32;
        let cy = cy.round() as i32;
        let w = image.width() as i32;
        let h = image.height() as i32;
        let rr = r * r;

        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy > rr {
                    continue;
                }
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && px < w && py >= 0 && py < h {
                    image.put_pixel(px as u32, py as u32, color);
                }
            }
        }
    }

    fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
        painter.line_segment([rect.left_top(), rect.right_top()], stroke);
        painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
        painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
        painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
    }

    fn save_image(&self, rect: egui::Rect) {
        let Some(cropped) = self.crop_with_annotations(rect) else {
            return;
        };
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = format!("screenshot_{}.png", timestamp);
        let mut path = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("Pictures");
        path.push("Screenshots");
        let _ = std::fs::create_dir_all(&path);
        path.push(filename);
        let _ = DynamicImage::ImageRgba8(cropped).save(&path);
    }

    fn copy_image(&self, rect: egui::Rect) {
        let Some(cropped) = self.crop_with_annotations(rect) else {
            return;
        };
        let mut buffer: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);
        let _ = DynamicImage::ImageRgba8(cropped).write_to(&mut cursor, image::ImageFormat::Png);
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Prefer xclip so clipboard managers (e.g. CopyQ) observe a normal clipboard
        // ownership change and store the screenshot in history.
        let mut copied_with_xclip = false;
        if let Ok(mut child) = Command::new("xclip")
            .args(&["-selection", "clipboard", "-t", "image/png"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            copied_with_xclip = child.wait().map(|status| status.success()).unwrap_or(false);
        }

        if copied_with_xclip {
            return;
        }

        // Fallback if xclip is unavailable or failed.
        if let Ok(mut child) = Command::new("copyq")
            .args(&["copy", "image/png", "-"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Copy)]
enum IconKind {
    Pen,
    Rect,
    Undo,
    Copy,
    Save,
}
