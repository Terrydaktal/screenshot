mod capture;

use chrono::Local;
use eframe::egui;
use image::{DynamicImage, Rgba, RgbaImage};
use resvg::tiny_skia;
use resvg::usvg;
use std::io::Read;
use std::path::PathBuf;
use zbus::blocking::Connection;

const APP_ID: &str = "io.github.terrydaktal.screenshot";

fn main() -> eframe::Result {
    let frame = if let Some((width, height, scale)) = parse_stdin_rgba_args() {
        capture::CapturedFrame {
            width,
            height,
            scale,
            rgba: read_rgba_frame_from_stdin(width, height),
        }
    } else {
        let connection = Connection::session().unwrap_or_else(|err| {
            eprintln!("ERROR: failed to connect to the session D-Bus: {err:#}");
            std::process::exit(1);
        });
        capture::capture_workspace(&connection).unwrap_or_else(|err| {
            eprintln!("ERROR: failed to capture the Plasma workspace: {err:#}");
            eprintln!(
                "HINT: run wayland/scripts/install-user-service.sh to authorize KWin capture."
            );
            std::process::exit(1);
        })
    };

    let width = frame.width;
    let height = frame.height;
    let scale = frame.scale as f32;
    let rgba_image = RgbaImage::from_raw(width, height, frame.rgba)
        .expect("captured RGBA frame has an invalid byte count");
    let dynamic_image = DynamicImage::ImageRgba8(rgba_image);

    let screen_size = [width as f32 / scale, height as f32 / scale];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_position([0.0, 0.0])
            .with_inner_size(screen_size)
            .with_resizable(true)
            .with_always_on_top()
            .with_decorations(false)
            .with_active(true)
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "screenshot",
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
            let icon_textures = load_toolbar_icon_textures(cc);

            Ok(Box::new(ScreenshotApp {
                texture,
                icon_textures,
                full_image: dynamic_image,
                selection: None,
                anchor_point: None,
                drag_mode: DragMode::None,
                save_requested: false,
                copy_requested: false,
                focus_requested: false,
                pen_mode: false,
                rect_mode: false,
                line_mode: false,
                arrow_mode: false,
                pick_color_mode: false,
                draw_color: egui::Color32::from_rgb(255, 203, 5),
                draw_size: 3.0,
                annotations: Vec::new(),
                current_pen_stroke: None,
                current_rect_shape: None,
                current_line_shape: None,
            }))
        }),
    )
}

fn parse_stdin_rgba_args() -> Option<(u32, u32, f64)> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--stdin-rgba") {
        return None;
    }

    let width = args
        .next()
        .expect("Missing WIDTH for --stdin-rgba")
        .parse::<u32>()
        .expect("Invalid WIDTH for --stdin-rgba");
    let height = args
        .next()
        .expect("Missing HEIGHT for --stdin-rgba")
        .parse::<u32>()
        .expect("Invalid HEIGHT for --stdin-rgba");
    let scale = args
        .next()
        .expect("Missing SCALE for --stdin-rgba")
        .parse::<f64>()
        .expect("Invalid SCALE for --stdin-rgba");

    assert!(
        width > 0 && height > 0 && scale.is_finite() && scale > 0.0,
        "WIDTH, HEIGHT, and SCALE must be greater than zero",
    );
    Some((width, height, scale))
}

fn read_rgba_frame_from_stdin(width: u32, height: u32) -> Vec<u8> {
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .expect("Prefetched RGBA frame byte count overflow");
    let mut rgba_pixels = vec![0_u8; expected_len];
    std::io::stdin()
        .read_exact(&mut rgba_pixels)
        .expect("Failed to read prefetched RGBA frame from stdin");
    rgba_pixels
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
struct LineShape {
    start: egui::Pos2,
    end: egui::Pos2,
    color: egui::Color32,
    width: f32,
    arrow: bool,
}

#[derive(Clone)]
enum Annotation {
    Pen(PenStroke),
    Rect(RectShape),
    Line(LineShape),
}

struct ScreenshotApp {
    texture: egui::TextureHandle,
    icon_textures: IconTextures,
    full_image: DynamicImage,
    selection: Option<egui::Rect>,
    anchor_point: Option<egui::Pos2>,
    drag_mode: DragMode,
    save_requested: bool,
    copy_requested: bool,
    focus_requested: bool,
    pen_mode: bool,
    rect_mode: bool,
    line_mode: bool,
    arrow_mode: bool,
    pick_color_mode: bool,
    draw_color: egui::Color32,
    draw_size: f32,
    annotations: Vec<Annotation>,
    current_pen_stroke: Option<PenStroke>,
    current_rect_shape: Option<RectShape>,
    current_line_shape: Option<LineShape>,
}

struct IconTextures {
    pen: Option<egui::TextureHandle>,
    rect: Option<egui::TextureHandle>,
    line: Option<egui::TextureHandle>,
    arrow: Option<egui::TextureHandle>,
    eyedropper: Option<egui::TextureHandle>,
    undo: Option<egui::TextureHandle>,
    copy: Option<egui::TextureHandle>,
    save: Option<egui::TextureHandle>,
}

impl eframe::App for ScreenshotApp {
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

        let screen_rect = ctx.content_rect();
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
            let mut consumed_primary_press = false;
            if self.pick_color_mode && primary_pressed && !ctx.wants_pointer_input() {
                if let Some(color) = self.screen_color_at_pos(pos, screen_rect) {
                    self.draw_color = color;
                    self.pick_color_mode = false;
                }
                consumed_primary_press = true;
            }

            if self.any_draw_tool_active() || self.pick_color_mode {
                if let Some(rect) = self.selection {
                    let clamped_pos = Self::clamp_pos_to_rect(pos, rect);
                    if primary_pressed
                        && !consumed_primary_press
                        && !ctx.wants_pointer_input()
                        && rect.contains(pos)
                    {
                        if self.pen_mode {
                            self.current_rect_shape = None;
                            self.current_line_shape = None;
                            self.current_pen_stroke = Some(PenStroke {
                                points: vec![clamped_pos],
                                color: self.draw_color,
                                width: self.draw_size,
                            });
                        } else if self.rect_mode {
                            self.current_pen_stroke = None;
                            self.current_line_shape = None;
                            self.current_rect_shape = Some(RectShape {
                                start: clamped_pos,
                                end: clamped_pos,
                                color: self.draw_color,
                                width: self.draw_size,
                            });
                        } else if self.line_mode || self.arrow_mode {
                            self.current_pen_stroke = None;
                            self.current_rect_shape = None;
                            self.current_line_shape = Some(LineShape {
                                start: clamped_pos,
                                end: clamped_pos,
                                color: self.draw_color,
                                width: self.draw_size,
                                arrow: self.arrow_mode,
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
                        } else if self.line_mode || self.arrow_mode {
                            if let Some(shape) = self.current_line_shape.as_mut() {
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
                        if let Some(edge) = Self::resize_edge_at(rect, pos, handle_size) {
                            self.drag_mode = DragMode::Resizing(edge);
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

        // 5. Cursor hints for move/resize interactions on the selection border.
        if !self.any_draw_tool_active() && !self.pick_color_mode && !ctx.wants_pointer_input() {
            if let Some(pos) = pointer_pos {
                if let Some(rect) = self.selection {
                    let handle_size = 20.0;
                    let hover_cursor = if primary_down {
                        match self.drag_mode {
                            DragMode::Resizing(edge) => Some(Self::cursor_for_resize_edge(edge)),
                            DragMode::Moving => Some(egui::CursorIcon::Grabbing),
                            DragMode::Creating => Some(egui::CursorIcon::Crosshair),
                            DragMode::None => None,
                        }
                    } else if let Some(edge) = Self::resize_edge_at(rect, pos, handle_size) {
                        Some(Self::cursor_for_resize_edge(edge))
                    } else if rect.contains(pos) {
                        Some(egui::CursorIcon::Grab)
                    } else {
                        Some(egui::CursorIcon::Crosshair)
                    };

                    if let Some(cursor) = hover_cursor {
                        ctx.set_cursor_icon(cursor);
                    }
                } else {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
            }
        }
        if self.pick_color_mode && !ctx.wants_pointer_input() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // 6. Draw Selection
        if let Some(rect) = self.selection {
            let rect = rect.intersect(screen_rect);
            let mut mesh = egui::Mesh::with_texture(self.texture.id());
            mesh.add_rect_with_uv(
                rect,
                Self::selection_uv_rect(rect, screen_rect),
                egui::Color32::WHITE,
            );
            painter.add(mesh);
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.4, egui::Color32::WHITE),
                egui::StrokeKind::Outside,
            );
            let annotation_painter = painter.with_clip_rect(rect);
            self.draw_annotations_on_screen(&annotation_painter);

            // Controls
            if self.drag_mode == DragMode::None && rect.width() > 10.0 {
                let ui_scale = self.ui_points_per_pixel(screen_rect).clamp(0.6, 1.4);
                // Keep controls above the desktop panel/taskbar area.
                let bottom_safe_inset = 56.0 * ui_scale;
                let button_size = 36.0 * ui_scale;
                let spacing = 8.0 * ui_scale;
                const ACTION_BUTTON_COUNT: f32 = 6.0;
                let action_size = egui::vec2(
                    button_size * ACTION_BUTTON_COUNT + spacing * (ACTION_BUTTON_COUNT - 1.0),
                    button_size,
                );
                let visible_bottom = (screen_rect.max.y - bottom_safe_inset)
                    .max(screen_rect.min.y + action_size.y + 4.0);
                let action_y_outside = rect.max.y + 8.0 * ui_scale;
                let action_y_inside = rect.max.y - action_size.y - 8.0 * ui_scale;
                let action_y = if action_y_outside + action_size.y <= visible_bottom {
                    action_y_outside
                } else {
                    // If controls would go off-screen at the bottom, move them inside the selection.
                    action_y_inside.max(rect.min.y + 4.0 * ui_scale)
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
                            if self
                                .draw_icon_button(
                                    ui,
                                    IconKind::Pen,
                                    button_size,
                                    egui::Color32::from_rgb(230, 164, 38),
                                    self.pen_mode,
                                )
                                .clicked()
                            {
                                let next = !self.pen_mode;
                                self.set_active_draw_tool(next, false, false, false);
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if self
                                .draw_icon_button(
                                    ui,
                                    IconKind::Rect,
                                    button_size,
                                    egui::Color32::from_rgb(223, 126, 78),
                                    self.rect_mode,
                                )
                                .clicked()
                            {
                                let next = !self.rect_mode;
                                self.set_active_draw_tool(false, next, false, false);
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if self
                                .draw_icon_button(
                                    ui,
                                    IconKind::Line,
                                    button_size,
                                    egui::Color32::from_rgb(76, 133, 245),
                                    self.line_mode,
                                )
                                .clicked()
                            {
                                let next = !self.line_mode;
                                self.set_active_draw_tool(false, false, next, false);
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if self
                                .draw_icon_button(
                                    ui,
                                    IconKind::Arrow,
                                    button_size,
                                    egui::Color32::from_rgb(122, 94, 223),
                                    self.arrow_mode,
                                )
                                .clicked()
                            {
                                let next = !self.arrow_mode;
                                self.set_active_draw_tool(false, false, false, next);
                                self.clear_active_annotation_preview();
                                self.drag_mode = DragMode::None;
                            }
                            if self
                                .draw_icon_button(
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
                            if self
                                .draw_icon_button(
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

                let draw_controls_active = self.any_draw_tool_active();
                if draw_controls_active {
                    let slider_width = 150.0 * ui_scale;
                    let color_width = 40.0 * ui_scale;
                    let draw_tools_gap = 20.0 * ui_scale;
                    // Reserve extra width so placement math remains correct even if any control
                    // renders slightly wider on a given DPI/theme.
                    let layout_guard = 40.0 * ui_scale;
                    let draw_tools_size = egui::vec2(
                        color_width
                            + spacing
                            + button_size
                            + spacing
                            + slider_width
                            + spacing
                            + button_size
                            + layout_guard,
                        button_size,
                    );
                    let action_rect = egui::Rect::from_min_size(action_pos, action_size);
                    // Include safety margin so visual spacing survives theme/scale variation.
                    let action_hit_rect = action_rect.expand(spacing * 0.6);
                    let min_x = screen_rect.min.x.max(0.0);
                    let max_x = (screen_rect.max.x - draw_tools_size.x).max(min_x);
                    let min_y = screen_rect.min.y.max(0.0);
                    let max_y = (visible_bottom - draw_tools_size.y).max(min_y);

                    let clamp_pos = |p: egui::Pos2| -> egui::Pos2 {
                        egui::pos2(p.x.clamp(min_x, max_x), p.y.clamp(min_y, max_y))
                    };
                    let candidate_positions = [
                        // preferred: left of action strip
                        egui::pos2(
                            action_hit_rect.min.x - draw_tools_gap - draw_tools_size.x,
                            action_rect.min.y,
                        ),
                        // then right
                        egui::pos2(action_hit_rect.max.x + draw_tools_gap, action_rect.min.y),
                        // then above
                        egui::pos2(
                            action_rect.min.x,
                            action_hit_rect.min.y - draw_tools_size.y - spacing,
                        ),
                        // then below
                        egui::pos2(action_rect.min.x, action_hit_rect.max.y + spacing),
                    ];

                    let mut draw_tools_pos = None;
                    for candidate in candidate_positions {
                        let pos = clamp_pos(candidate);
                        let draw_rect = egui::Rect::from_min_size(pos, draw_tools_size);
                        if !draw_rect.intersects(action_hit_rect) {
                            draw_tools_pos = Some(pos);
                            break;
                        }
                    }

                    // Hard fallback: force vertical separation from action strip even if clamped choices collide.
                    let draw_tools_pos = draw_tools_pos.unwrap_or_else(|| {
                        let above_y =
                            (action_hit_rect.min.y - draw_tools_size.y - spacing).max(min_y);
                        let below_y = (action_hit_rect.max.y + spacing).min(max_y);
                        let y = if above_y + draw_tools_size.y <= action_hit_rect.min.y {
                            above_y
                        } else {
                            below_y
                        };
                        let mut pos = egui::pos2(action_rect.min.x.clamp(min_x, max_x), y);
                        let mut draw_rect = egui::Rect::from_min_size(pos, draw_tools_size);
                        if draw_rect.intersects(action_hit_rect) {
                            pos.y = (action_hit_rect.max.y + spacing).min(max_y);
                            draw_rect = egui::Rect::from_min_size(pos, draw_tools_size);
                            if draw_rect.intersects(action_hit_rect) {
                                pos.y = (action_hit_rect.min.y - draw_tools_size.y - spacing)
                                    .max(min_y);
                            }
                        }
                        pos
                    });

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
                                if self
                                    .draw_icon_button(
                                        ui,
                                        IconKind::Eyedropper,
                                        button_size,
                                        egui::Color32::from_rgb(84, 84, 84),
                                        self.pick_color_mode,
                                    )
                                    .clicked()
                                {
                                    self.pick_color_mode = !self.pick_color_mode;
                                }
                                ui.add_sized(
                                    [slider_width, button_size],
                                    egui::Slider::new(&mut self.draw_size, 1.0..=16.0)
                                        .show_value(false),
                                );
                                if self
                                    .draw_icon_button(
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

        // 7. Action Execution
        if self.save_requested {
            if let Some(rect) = self.selection {
                self.save_image(rect, screen_rect);
            }
            std::process::exit(0);
        }
        if self.copy_requested {
            if let Some(rect) = self.selection {
                self.copy_image(rect, screen_rect);
            }
            std::process::exit(0);
        }
    }
}

impl ScreenshotApp {
    fn any_draw_tool_active(&self) -> bool {
        self.pen_mode || self.rect_mode || self.line_mode || self.arrow_mode
    }

    fn set_active_draw_tool(&mut self, pen: bool, rect: bool, line: bool, arrow: bool) {
        self.pen_mode = pen;
        self.rect_mode = rect;
        self.line_mode = line;
        self.arrow_mode = arrow;
        self.pick_color_mode = false;
    }

    fn ui_points_per_pixel(&self, screen_rect: egui::Rect) -> f32 {
        let image_w = self.full_image.width().max(1) as f32;
        let image_h = self.full_image.height().max(1) as f32;
        let x = screen_rect.width() / image_w;
        let y = screen_rect.height() / image_h;
        x.min(y)
    }

    fn resize_edge_at(rect: egui::Rect, pos: egui::Pos2, handle_size: f32) -> Option<ResizeEdge> {
        let on_top = (pos.y - rect.min.y).abs() < handle_size;
        let on_bottom = (pos.y - rect.max.y).abs() < handle_size;
        let on_left = (pos.x - rect.min.x).abs() < handle_size;
        let on_right = (pos.x - rect.max.x).abs() < handle_size;
        let edge_check = pos.x >= rect.min.x - handle_size
            && pos.x <= rect.max.x + handle_size
            && pos.y >= rect.min.y - handle_size
            && pos.y <= rect.max.y + handle_size;

        if (on_top || on_bottom || on_left || on_right) && edge_check {
            Some(ResizeEdge {
                top: on_top,
                bottom: on_bottom,
                left: on_left,
                right: on_right,
            })
        } else {
            None
        }
    }

    fn cursor_for_resize_edge(edge: ResizeEdge) -> egui::CursorIcon {
        if (edge.top && edge.left) || (edge.bottom && edge.right) {
            egui::CursorIcon::ResizeNwSe
        } else if (edge.top && edge.right) || (edge.bottom && edge.left) {
            egui::CursorIcon::ResizeNeSw
        } else if edge.left || edge.right {
            egui::CursorIcon::ResizeHorizontal
        } else if edge.top || edge.bottom {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::Default
        }
    }

    fn icon_texture(&self, kind: IconKind) -> Option<&egui::TextureHandle> {
        match kind {
            IconKind::Pen => self.icon_textures.pen.as_ref(),
            IconKind::Rect => self.icon_textures.rect.as_ref(),
            IconKind::Line => self.icon_textures.line.as_ref(),
            IconKind::Arrow => self.icon_textures.arrow.as_ref(),
            IconKind::Eyedropper => self.icon_textures.eyedropper.as_ref(),
            IconKind::Undo => self.icon_textures.undo.as_ref(),
            IconKind::Copy => self.icon_textures.copy.as_ref(),
            IconKind::Save => self.icon_textures.save.as_ref(),
        }
    }

    fn icon_padding_ratio(kind: IconKind) -> f32 {
        match kind {
            // Slightly larger to keep the pen glyph readable.
            IconKind::Pen => 0.14,
            // Make undo a touch larger for clarity at high DPI.
            IconKind::Undo => 0.12,
            _ => 0.16,
        }
    }

    fn draw_icon_button(
        &self,
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

        if let Some(texture) = self.icon_texture(kind) {
            let icon_rect = rect.shrink(size * Self::icon_padding_ratio(kind));
            painter.image(
                texture.id(),
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            return response;
        }

        match kind {
            IconKind::Pen => {
                // Pencil body + nib for clearer recognition.
                let tip = egui::pos2(rect.left() + size * 0.28, rect.bottom() - size * 0.28);
                let tail = egui::pos2(rect.right() - size * 0.24, rect.top() + size * 0.24);
                let dir = tail - tip;
                let len = dir.length().max(1.0);
                let unit = dir / len;
                let perp = egui::vec2(-unit.y, unit.x);
                let body_half = size * 0.07;
                let body_start = tip + unit * (size * 0.12);
                let body_end = tail - unit * (size * 0.14);
                painter.line_segment(
                    [body_start + perp * body_half, body_end + perp * body_half],
                    stroke,
                );
                painter.line_segment(
                    [body_start - perp * body_half, body_end - perp * body_half],
                    stroke,
                );
                let eraser_center = tail - unit * (size * 0.05);
                let cap_half = size * 0.09;
                painter.line_segment(
                    [
                        eraser_center + perp * cap_half,
                        eraser_center - perp * cap_half,
                    ],
                    stroke,
                );

                let nib_base = tip + unit * (size * 0.12);
                let nib_left = nib_base + perp * (size * 0.09);
                let nib_right = nib_base - perp * (size * 0.09);
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, nib_left, nib_right],
                    icon,
                    egui::Stroke::NONE,
                ));
            }
            IconKind::Rect => {
                let shape = rect.shrink(size * 0.30);
                Self::stroke_rect(painter, shape, stroke);
            }
            IconKind::Line => {
                let left = rect.left() + size * 0.24;
                let right = rect.right() - size * 0.24;
                let y = rect.center().y;
                painter.line_segment([egui::pos2(left, y), egui::pos2(right, y)], stroke);
            }
            IconKind::Arrow => {
                let left = rect.left() + size * 0.22;
                let right = rect.right() - size * 0.24;
                let y = rect.center().y;
                let start = egui::pos2(left, y);
                let end = egui::pos2(right, y);
                painter.line_segment([start, end], stroke);
                Self::draw_arrow_head_on_screen(painter, start, end, stroke);
            }
            IconKind::Eyedropper => {
                // Pipette shape: bulb, tube, and tip.
                let tip = egui::pos2(rect.left() + size * 0.30, rect.bottom() - size * 0.30);
                let bulb = egui::pos2(rect.right() - size * 0.26, rect.top() + size * 0.26);
                let dir = bulb - tip;
                let len = dir.length().max(1.0);
                let unit = dir / len;
                let perp = egui::vec2(-unit.y, unit.x);
                let tube_half = size * 0.05;
                let tube_start = tip + unit * (size * 0.10);
                let tube_end = bulb - unit * (size * 0.12);
                painter.line_segment(
                    [tube_start + perp * tube_half, tube_end + perp * tube_half],
                    stroke,
                );
                painter.line_segment(
                    [tube_start - perp * tube_half, tube_end - perp * tube_half],
                    stroke,
                );
                painter.circle_stroke(bulb, size * 0.12, stroke);

                let tip_base = tip + unit * (size * 0.10);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        tip,
                        tip_base + perp * (size * 0.06),
                        tip_base - perp * (size * 0.06),
                    ],
                    icon,
                    egui::Stroke::NONE,
                ));
                let drop = tip + egui::vec2(0.0, size * 0.08);
                painter.circle_filled(drop, size * 0.04, icon);
            }
            IconKind::Undo => {
                // Fallback icon if SVG fails to load.
                let s = egui::Stroke::new(2.5, icon);
                let center = rect.center() + egui::vec2(size * 0.05, size * 0.03);
                let rx = size * 0.29;
                let ry = size * 0.22;
                let start_angle = 205.0_f32.to_radians();
                let end_angle = 8.0_f32.to_radians();
                let mut arc = Vec::with_capacity(24);
                for i in 0..=23 {
                    let t = i as f32 / 23.0;
                    let a = start_angle + (end_angle - start_angle) * t;
                    arc.push(egui::pos2(center.x + rx * a.cos(), center.y + ry * a.sin()));
                }
                painter.add(egui::Shape::line(arc.clone(), s));

                if let Some(tail_start) = arc.last().copied() {
                    painter.line_segment(
                        [
                            tail_start,
                            tail_start + egui::vec2(size * 0.13, size * 0.01),
                        ],
                        s,
                    );
                }

                let tip = *arc.first().unwrap_or(&rect.left_center());
                let head = size * 0.16;
                painter.line_segment([tip, egui::pos2(tip.x + head, tip.y - head * 0.58)], s);
                painter.line_segment([tip, egui::pos2(tip.x + head, tip.y + head * 0.62)], s);
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
        self.current_line_shape = None;
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
        if self.current_line_shape.is_some() {
            self.current_line_shape = None;
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
        if let Some(shape) = self.current_line_shape.take() {
            let len = shape.start.distance(shape.end);
            if len >= 1.0 {
                self.annotations.push(Annotation::Line(shape));
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
                Annotation::Line(shape) => {
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
        if let Some(shape) = &mut self.current_line_shape {
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
        if let Some(shape) = &self.current_line_shape {
            Self::draw_line_shape_path(painter, shape);
        }
    }

    fn draw_annotation_on_screen(painter: &egui::Painter, annotation: &Annotation) {
        match annotation {
            Annotation::Pen(stroke) => Self::draw_pen_stroke_path(painter, stroke),
            Annotation::Rect(shape) => Self::draw_rect_shape_path(painter, shape),
            Annotation::Line(shape) => Self::draw_line_shape_path(painter, shape),
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

    fn draw_line_shape_path(painter: &egui::Painter, shape: &LineShape) {
        let stroke = egui::Stroke::new(shape.width, shape.color);
        painter.line_segment([shape.start, shape.end], stroke);
        if shape.arrow {
            Self::draw_arrow_head_on_screen(painter, shape.start, shape.end, stroke);
        }
    }

    fn draw_arrow_head_on_screen(
        painter: &egui::Painter,
        start: egui::Pos2,
        end: egui::Pos2,
        stroke: egui::Stroke,
    ) {
        let dir = end - start;
        let len = dir.length();
        if len < 2.0 {
            return;
        }
        let unit = dir / len;
        let head_len = (stroke.width * 4.5).max(10.0);
        let head_half_width = head_len * 0.5;
        let base = end - unit * head_len;
        let perp = egui::vec2(-unit.y, unit.x);
        let left = base + perp * head_half_width;
        let right = base - perp * head_half_width;
        painter.line_segment([end, left], stroke);
        painter.line_segment([end, right], stroke);
    }

    fn clamp_pos_to_rect(pos: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        egui::pos2(
            pos.x.clamp(rect.min.x, rect.max.x),
            pos.y.clamp(rect.min.y, rect.max.y),
        )
    }

    fn selection_uv_rect(selection_rect: egui::Rect, screen_rect: egui::Rect) -> egui::Rect {
        let width = screen_rect.width().max(1.0);
        let height = screen_rect.height().max(1.0);
        let u_min = ((selection_rect.min.x - screen_rect.min.x) / width).clamp(0.0, 1.0);
        let v_min = ((selection_rect.min.y - screen_rect.min.y) / height).clamp(0.0, 1.0);
        let u_max = ((selection_rect.max.x - screen_rect.min.x) / width).clamp(0.0, 1.0);
        let v_max = ((selection_rect.max.y - screen_rect.min.y) / height).clamp(0.0, 1.0);
        egui::Rect::from_min_max(egui::pos2(u_min, v_min), egui::pos2(u_max, v_max))
    }

    fn selection_to_image_pixels(
        &self,
        selection_rect: egui::Rect,
        screen_rect: egui::Rect,
    ) -> Option<(u32, u32, u32, u32)> {
        let image_width = self.full_image.width();
        let image_height = self.full_image.height();

        let width = screen_rect.width().max(1.0);
        let height = screen_rect.height().max(1.0);

        let u_min = ((selection_rect.min.x - screen_rect.min.x) / width).clamp(0.0, 1.0);
        let v_min = ((selection_rect.min.y - screen_rect.min.y) / height).clamp(0.0, 1.0);
        let u_max = ((selection_rect.max.x - screen_rect.min.x) / width).clamp(0.0, 1.0);
        let v_max = ((selection_rect.max.y - screen_rect.min.y) / height).clamp(0.0, 1.0);

        let x = (u_min * image_width as f32).floor() as u32;
        let y = (v_min * image_height as f32).floor() as u32;
        let right = (u_max * image_width as f32).ceil() as u32;
        let bottom = (v_max * image_height as f32).ceil() as u32;

        let right = right.clamp(0, image_width);
        let bottom = bottom.clamp(0, image_height);
        let x = x.min(right);
        let y = y.min(bottom);
        let w = right.saturating_sub(x);
        let h = bottom.saturating_sub(y);

        if w == 0 || h == 0 {
            return None;
        }

        Some((x, y, w, h))
    }

    fn image_scale_for_selection(image: &RgbaImage, selection_rect: egui::Rect) -> (f32, f32) {
        let sx = image.width() as f32 / selection_rect.width().max(1.0);
        let sy = image.height() as f32 / selection_rect.height().max(1.0);
        (sx, sy)
    }

    fn selection_pos_to_image_pos(
        pos: egui::Pos2,
        selection_rect: egui::Rect,
        selection_to_image_scale: (f32, f32),
    ) -> (f32, f32) {
        let local_x = (pos.x - selection_rect.min.x).clamp(0.0, selection_rect.width());
        let local_y = (pos.y - selection_rect.min.y).clamp(0.0, selection_rect.height());
        (
            local_x * selection_to_image_scale.0,
            local_y * selection_to_image_scale.1,
        )
    }

    fn screen_color_at_pos(
        &self,
        pos: egui::Pos2,
        screen_rect: egui::Rect,
    ) -> Option<egui::Color32> {
        let image = self.full_image.as_rgba8()?;
        let width = screen_rect.width().max(1.0);
        let height = screen_rect.height().max(1.0);
        let u = ((pos.x - screen_rect.min.x) / width).clamp(0.0, 1.0);
        let v = ((pos.y - screen_rect.min.y) / height).clamp(0.0, 1.0);
        let max_x = image.width().saturating_sub(1);
        let max_y = image.height().saturating_sub(1);
        let x = (u * max_x as f32).round() as u32;
        let y = (v * max_y as f32).round() as u32;
        let px = image.get_pixel(x, y);
        Some(egui::Color32::from_rgba_unmultiplied(
            px[0], px[1], px[2], px[3],
        ))
    }

    fn crop_with_annotations(
        &self,
        selection_rect: egui::Rect,
        screen_rect: egui::Rect,
    ) -> Option<RgbaImage> {
        let (x, y, w, h) = self.selection_to_image_pixels(selection_rect, screen_rect)?;

        let mut cropped = self.full_image.crop_imm(x, y, w, h).to_rgba8();
        self.draw_annotations_on_image(&mut cropped, selection_rect);
        Some(cropped)
    }

    fn draw_annotations_on_image(&self, image: &mut RgbaImage, selection_rect: egui::Rect) {
        for annotation in &self.annotations {
            Self::draw_annotation_on_image(image, selection_rect, annotation);
        }
        if let Some(stroke) = &self.current_pen_stroke {
            Self::draw_pen_stroke_on_image(image, selection_rect, stroke);
        }
        if let Some(shape) = &self.current_rect_shape {
            Self::draw_rect_shape_on_image(image, selection_rect, shape);
        }
        if let Some(shape) = &self.current_line_shape {
            Self::draw_line_shape_on_image(image, selection_rect, shape);
        }
    }

    fn draw_annotation_on_image(
        image: &mut RgbaImage,
        selection_rect: egui::Rect,
        annotation: &Annotation,
    ) {
        match annotation {
            Annotation::Pen(stroke) => {
                Self::draw_pen_stroke_on_image(image, selection_rect, stroke)
            }
            Annotation::Rect(shape) => Self::draw_rect_shape_on_image(image, selection_rect, shape),
            Annotation::Line(shape) => Self::draw_line_shape_on_image(image, selection_rect, shape),
        }
    }

    fn draw_pen_stroke_on_image(
        image: &mut RgbaImage,
        selection_rect: egui::Rect,
        stroke: &PenStroke,
    ) {
        if stroke.points.len() < 2 {
            return;
        }

        let scale = Self::image_scale_for_selection(image, selection_rect);
        let stroke_width = stroke.width * scale.0.max(scale.1);

        let rgba = Rgba([
            stroke.color.r(),
            stroke.color.g(),
            stroke.color.b(),
            stroke.color.a(),
        ]);

        for segment in stroke.points.windows(2) {
            let p0 = segment[0];
            let p1 = segment[1];
            let (x0, y0) = Self::selection_pos_to_image_pos(p0, selection_rect, scale);
            let (x1, y1) = Self::selection_pos_to_image_pos(p1, selection_rect, scale);
            Self::draw_thick_line(image, x0, y0, x1, y1, stroke_width, rgba);
        }
    }

    fn draw_rect_shape_on_image(
        image: &mut RgbaImage,
        selection_rect: egui::Rect,
        shape: &RectShape,
    ) {
        let draw_rect = egui::Rect::from_two_pos(shape.start, shape.end);
        if draw_rect.width() < 1.0 || draw_rect.height() < 1.0 {
            return;
        }

        let scale = Self::image_scale_for_selection(image, selection_rect);
        let stroke_width = shape.width * scale.0.max(scale.1);

        let rgba = Rgba([
            shape.color.r(),
            shape.color.g(),
            shape.color.b(),
            shape.color.a(),
        ]);

        let (left, top) = Self::selection_pos_to_image_pos(draw_rect.min, selection_rect, scale);
        let (right, bottom) =
            Self::selection_pos_to_image_pos(draw_rect.max, selection_rect, scale);

        Self::draw_thick_line(image, left, top, right, top, stroke_width, rgba);
        Self::draw_thick_line(image, right, top, right, bottom, stroke_width, rgba);
        Self::draw_thick_line(image, right, bottom, left, bottom, stroke_width, rgba);
        Self::draw_thick_line(image, left, bottom, left, top, stroke_width, rgba);
    }

    fn draw_line_shape_on_image(
        image: &mut RgbaImage,
        selection_rect: egui::Rect,
        shape: &LineShape,
    ) {
        let scale = Self::image_scale_for_selection(image, selection_rect);
        let stroke_width = shape.width * scale.0.max(scale.1);
        let rgba = Rgba([
            shape.color.r(),
            shape.color.g(),
            shape.color.b(),
            shape.color.a(),
        ]);
        let (x0, y0) = Self::selection_pos_to_image_pos(shape.start, selection_rect, scale);
        let (x1, y1) = Self::selection_pos_to_image_pos(shape.end, selection_rect, scale);
        Self::draw_thick_line(image, x0, y0, x1, y1, stroke_width, rgba);
        if shape.arrow {
            Self::draw_arrow_head_on_image(image, x0, y0, x1, y1, stroke_width, rgba);
        }
    }

    fn draw_arrow_head_on_image(
        image: &mut RgbaImage,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        color: Rgba<u8>,
    ) {
        let dir = egui::vec2(x1 - x0, y1 - y0);
        let len = dir.length();
        if len < 2.0 {
            return;
        }
        let unit = dir / len;
        let head_len = (width * 4.5).max(10.0);
        let head_half_width = head_len * 0.5;
        let base_x = x1 - unit.x * head_len;
        let base_y = y1 - unit.y * head_len;
        let perp_x = -unit.y;
        let perp_y = unit.x;
        let left_x = base_x + perp_x * head_half_width;
        let left_y = base_y + perp_y * head_half_width;
        let right_x = base_x - perp_x * head_half_width;
        let right_y = base_y - perp_y * head_half_width;
        Self::draw_thick_line(image, x1, y1, left_x, left_y, width, color);
        Self::draw_thick_line(image, x1, y1, right_x, right_y, width, color);
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

    fn save_image(&self, rect: egui::Rect, screen_rect: egui::Rect) {
        let Some(cropped) = self.crop_with_annotations(rect, screen_rect) else {
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

    fn copy_image(&self, rect: egui::Rect, screen_rect: egui::Rect) {
        let Some(cropped) = self.crop_with_annotations(rect, screen_rect) else {
            return;
        };
        let mut buffer: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);
        let _ = DynamicImage::ImageRgba8(cropped).write_to(&mut cursor, image::ImageFormat::Png);
        use std::io::Write;
        use std::process::{Command, Stdio};

        // wl-copy owns the native Wayland clipboard after this short-lived UI exits.
        let mut copied_with_wayland = false;
        if let Ok(mut child) = Command::new("wl-copy")
            .args(["--type", "image/png"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            copied_with_wayland = child.wait().map(|status| status.success()).unwrap_or(false);
        }

        if copied_with_wayland {
            return;
        }

        // CopyQ can write its own history directly if wl-clipboard is unavailable.
        if let Ok(mut child) = Command::new("copyq")
            .args(["copy", "image/png", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&buffer);
            }
            let _ = child.wait();
        }
    }
}

fn load_toolbar_icon_textures(cc: &eframe::CreationContext<'_>) -> IconTextures {
    IconTextures {
        pen: render_svg_texture(cc, "icon_pen", include_bytes!("../assets/icons/pen.svg")),
        rect: render_svg_texture(cc, "icon_rect", include_bytes!("../assets/icons/rect.svg")),
        line: render_svg_texture(cc, "icon_line", include_bytes!("../assets/icons/line.svg")),
        arrow: render_svg_texture(
            cc,
            "icon_arrow",
            include_bytes!("../assets/icons/arrow.svg"),
        ),
        eyedropper: render_svg_texture(
            cc,
            "icon_eyedropper",
            include_bytes!("../assets/icons/eyedropper.svg"),
        ),
        undo: render_svg_texture(cc, "icon_undo", include_bytes!("../assets/icons/undo.svg")),
        copy: render_svg_texture(cc, "icon_copy", include_bytes!("../assets/icons/copy.svg")),
        save: render_svg_texture(cc, "icon_save", include_bytes!("../assets/icons/save.svg")),
    }
}

fn render_svg_texture(
    cc: &eframe::CreationContext<'_>,
    texture_name: &str,
    svg_bytes: &[u8],
) -> Option<egui::TextureHandle> {
    let tree = usvg::Tree::from_data(svg_bytes, &usvg::Options::default()).ok()?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())?;
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap_mut);

    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [size.width() as usize, size.height() as usize],
        pixmap.data(),
    );
    Some(
        cc.egui_ctx
            .load_texture(texture_name, color_image, egui::TextureOptions::LINEAR),
    )
}

#[derive(Clone, Copy)]
enum IconKind {
    Pen,
    Rect,
    Line,
    Arrow,
    Eyedropper,
    Undo,
    Copy,
    Save,
}
