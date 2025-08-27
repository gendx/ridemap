//! Window backed by gtk.

use crate::config::FONT_PATH;
use crate::map::tile_channel::TileRequestSender;
use crate::tracks::polyline::Point;
use crate::ui::camera::Camera;
use crate::ui::tiles::TileState;
use crate::ui::tracks::TrackState;
use crate::ui::util::{warn_on_error, RenderStats};
use crate::ui::UiMessage;
use anyhow::Context as AnyhowContext;
use futures::channel::oneshot;
use gtk4::cairo::{Context, FontFace, LineJoin};
use gtk4::gdk::prelude::GdkCairoContextExt;
use gtk4::gdk::Key;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::glib::signal::Propagation;
use gtk4::glib::source::timeout_add_local;
use gtk4::glib::{Bytes, ControlFlow};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, DrawingArea, EventControllerKey, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, GestureClick,
};
use log::{debug, info, trace, warn};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Window state on the GUI.
pub struct Window {
    camera: Camera,
    tile_state: TileState<(Pixbuf, u32)>,
    track_state: TrackState,
    thick: bool,
    click: bool,
    last_pos: Option<Point<f64>>,
    iteration: Rc<Cell<usize>>,
    area: Option<DrawingArea>,
    font_face: FontFace,
}

impl Window {
    /// GTK application ID.
    const APP_ID: &'static str = "com.github.gendx.ridemap";
    /// Initial window width.
    const INITIAL_WIDTH: u32 = 1280;
    /// Initial window height.
    const INITIAL_HEIGHT: u32 = 960;
    /// Radius of track endpoints.
    const CIRCLE_RADIUS: f64 = 5.0;
    /// Thickness of tracks in thick mode.
    const THICKNESS: f64 = 4.0;
    /// Font size.
    const FONT_SIZE: f64 = 20.0;
    /// How often to fetch messages from the background thread.
    const REFRESH_RATE: Duration = Duration::from_millis(50);

    /// Runs the UI loop, in the UI thread.
    pub fn ui_loop(
        ui_rx: Receiver<UiMessage>,
        cancel_tx: oneshot::Sender<()>,
        tiles_tx: TileRequestSender,
        _lazy_ui_refresh: bool,
        speculative_tile_load: bool,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
    ) -> anyhow::Result<()> {
        let freetype =
            freetype::Library::init().context("Failed to initialize FreeType library")?;
        let font = freetype
            .new_face(FONT_PATH, 0)
            .context("Failed to load font from path: {FONT_PATH}")?;
        let font_face = FontFace::create_from_ft(&font).context("Failed to create font face")?;

        let app = Application::builder().application_id(Self::APP_ID).build();

        let window = Rc::new(RefCell::new(Window::new(
            tiles_tx,
            speculative_tile_load,
            max_pixels_per_tile,
            max_tile_level,
            font_face,
        )));
        window.borrow_mut().tile_state.start();

        let window_init = window.clone();
        app.connect_activate(move |app| Self::build_ui(&window_init, app));

        let window_updates = window.clone();
        timeout_add_local(Self::REFRESH_RATE, move || {
            window_updates.borrow_mut().increment_iteration();
            for msg in ui_rx.try_iter() {
                window_updates.borrow_mut().process_update(msg);
            }
            ControlFlow::Continue
        });

        // Run the application, using empty arguments rather than the CLI.
        let exit_code = app.run_with_args::<&str>(&[]);

        info!("GTK app finished with code: {exit_code:?}");
        window.borrow_mut().tile_state.stop();
        warn_on_error(cancel_tx.send(()), "message on one-shot channel");

        Ok(())
    }

    /// Creates a new window state.
    fn new(
        tiles_tx: TileRequestSender,
        speculative_tile_load: bool,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
        font_face: FontFace,
    ) -> Self {
        let iteration = Rc::new(Cell::new(0));
        Self {
            camera: Camera::new(Self::INITIAL_WIDTH, Self::INITIAL_HEIGHT),
            tile_state: TileState::new(
                tiles_tx,
                speculative_tile_load,
                max_pixels_per_tile,
                max_tile_level,
                iteration.clone(),
            ),
            track_state: TrackState::new(),
            thick: false,
            click: false,
            last_pos: None,
            iteration,
            area: None,
            font_face,
        }
    }

    /// Increments the iteration value.
    fn increment_iteration(&mut self) {
        self.iteration.set(self.iteration.get() + 1);
    }

    /// Processes the given message from the background thread.
    fn process_update(&mut self, msg: UiMessage) {
        let need_refresh = match msg {
            UiMessage::Activity { id, r#type, points } => {
                debug!("[{}] Received activity #{}", self.iteration.get(), id);
                self.track_state
                    .process_activity(r#type, points, &self.camera);
                true
            }
            UiMessage::Tile {
                index,
                png_image,
                rgba_image,
            } => {
                self.tile_state
                    .process_tile(index, &png_image, rgba_image, |rgba_image| {
                        let width = rgba_image.width();
                        let height = rgba_image.height();
                        let stride = width * 4;

                        let pixbuf = Pixbuf::from_bytes(
                            &Bytes::from_owned(rgba_image.into_raw()),
                            Colorspace::Rgb,
                            /* has_alpha = */ true,
                            /* bits_per_sample = */ 8,
                            width as i32,
                            height as i32,
                            stride as i32,
                        );
                        Some((pixbuf, width))
                    })
            }
        };

        if need_refresh {
            self.queue_draw();
        }
    }

    /// Builds the UI, when triggered by GTK's activate signal.
    fn build_ui(shared_window: &Rc<RefCell<Window>>, app: &Application) {
        let area = DrawingArea::new();
        let window_draw = shared_window.clone();
        area.set_draw_func(move |_area, context, _width, _height| {
            if let Err(e) = window_draw.borrow().render(context) {
                warn!("Failed to render: {e:?}");
            }
        });
        let window_resize = shared_window.clone();
        area.connect_resize(move |_area, width, height| {
            window_resize.borrow_mut().handle_resize(width, height);
        });

        let app_window = ApplicationWindow::builder()
            .application(app)
            .title("Ridemap")
            .default_width(Self::INITIAL_WIDTH as i32)
            .default_height(Self::INITIAL_HEIGHT as i32)
            .child(&area)
            .build();

        shared_window.borrow_mut().area = Some(area);

        let keyboard = EventControllerKey::new();
        let window_keyboard = shared_window.clone();
        keyboard.connect_key_pressed(move |_controller, keyval, _keycode, _state| {
            let accepted = window_keyboard.borrow_mut().handle_key_press(keyval);
            Propagation::from(accepted)
        });
        app_window.add_controller(keyboard);

        let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        let window_scroll = shared_window.clone();
        scroll.connect_scroll(move |_controller, _dx, dy| {
            window_scroll.borrow_mut().handle_scroll(-dy);
            Propagation::Stop
        });
        app_window.add_controller(scroll);

        let motion = EventControllerMotion::new();
        let window_scroll = shared_window.clone();
        motion.connect_motion(move |_controller, x, y| {
            let mut window = window_scroll.borrow_mut();
            window.handle_motion(x, y);
        });
        app_window.add_controller(motion);

        let click = GestureClick::new();
        let window_pressed = shared_window.clone();
        click.connect_pressed(move |_gesture, _count, x, y| {
            window_pressed.borrow_mut().handle_pressed(x, y);
        });
        let window_released = shared_window.clone();
        click.connect_released(move |_gesture, _count, _x, _y| {
            window_released.borrow_mut().handle_released();
        });
        app_window.add_controller(click);

        app_window.present();
    }

    /// Handles a resize event of the drawing area.
    fn handle_resize(&mut self, width: i32, height: i32) {
        debug!("[{}] Resize({width}, {height})", self.iteration.get());

        let mut need_zoom_refresh = false;
        let mut need_offset_refresh = false;
        let x_dir = Ordering::Equal;
        let y_dir = Ordering::Equal;
        let z_dir = Ordering::Equal;

        self.camera.resize(
            width.into(),
            height.into(),
            &mut need_zoom_refresh,
            &mut need_offset_refresh,
        );

        if need_zoom_refresh || need_offset_refresh {
            self.tile_state
                .update(&mut self.camera, x_dir, y_dir, z_dir);
        }

        if need_zoom_refresh {
            self.track_state.refresh_zoom(&self.camera);
        }

        self.queue_draw();
    }

    /// Handles a key press on the keyboard.
    fn handle_key_press(&mut self, keyval: Key) -> bool {
        debug!("[{}] Key press", self.iteration.get());

        let accepted = match keyval {
            Key::space => {
                self.thick = !self.thick;
                true
            }
            Key::t => {
                self.track_state.toggle_color_by_type();
                true
            }
            Key::r => {
                self.track_state.randomize_colors();
                true
            }
            _ => false,
        };
        if accepted {
            self.queue_draw();
        }
        accepted
    }

    /// Handles a mouse scroll event.
    fn handle_scroll(&mut self, scroll: f64) {
        debug!("[{}] Scroll({scroll})", self.iteration.get());

        let mut need_zoom_refresh = false;
        let x_dir = Ordering::Equal;
        let y_dir = Ordering::Equal;
        let mut z_dir = Ordering::Equal;

        self.camera
            .scroll(scroll, &mut need_zoom_refresh, &mut z_dir);

        if need_zoom_refresh {
            self.tile_state
                .update(&mut self.camera, x_dir, y_dir, z_dir);
            self.track_state.refresh_zoom(&self.camera);
        }

        self.queue_draw();
    }

    /// Handles a mouse press event.
    fn handle_pressed(&mut self, x: f64, y: f64) {
        debug!("[{}] Pressed({x}, {y})", self.iteration.get());

        self.click = true;
        self.last_pos = Some(Point { x, y })
    }

    /// Handles a mouse release event.
    fn handle_released(&mut self) {
        debug!("[{}] Released", self.iteration.get());

        self.click = false;
    }

    /// Handles a mouse motion event.
    fn handle_motion(&mut self, x: f64, y: f64) {
        if !self.click {
            return;
        }

        debug!("[{}] Drag({x}, {y})", self.iteration.get());

        let mut need_offset_refresh = false;
        let mut x_dir = Ordering::Equal;
        let mut y_dir = Ordering::Equal;
        let z_dir = Ordering::Equal;

        let last = self.last_pos.unwrap();
        self.last_pos = Some(Point { x, y });

        let dx = x - last.x;
        let dy = y - last.y;
        self.camera
            .drag_relative(dx, dy, &mut need_offset_refresh, &mut x_dir, &mut y_dir);

        if need_offset_refresh {
            self.tile_state
                .update(&mut self.camera, x_dir, y_dir, z_dir);
        }

        self.queue_draw();
    }

    /// Appends a drawing request to the queue.
    fn queue_draw(&self) {
        self.area.as_ref().unwrap().queue_draw();
    }

    /// Renders the map on the given Cairo context.
    fn render(&self, context: &Context) -> anyhow::Result<()> {
        debug!("[{}] Render", self.iteration.get());

        let track_stats = self.track_state.debug_statistics(&self.camera);

        context.set_source_rgb(1.0, 1.0, 0.7);
        context.paint().context("Failed to draw background")?;

        let ioffset = self.camera.ioffset();
        let zoom = self.camera.zoom();

        let tiles_to_draw = self.tile_state.tiles_to_draw();
        for (i, (tile_index, tile)) in tiles_to_draw.iter().enumerate() {
            trace!("Drawing tile {}/{}", i, tiles_to_draw.len());

            let pixbuf: &Pixbuf = &tile.image.0;
            let pixbuf_width = tile.image.1 - 1;

            let rect = tile_index.rect();
            let target_width: f64 = zoom * rect[2];

            let scale_factor: f64 = target_width / (pixbuf_width as f64);
            let offset = Point {
                x: ioffset.x as f64 + zoom * rect[0],
                y: ioffset.y as f64 + zoom * rect[1],
            };

            context.translate(offset.x, offset.y);
            context.scale(scale_factor, scale_factor);

            context.set_source_pixbuf(pixbuf, 0.0, 0.0);
            context.paint().context("Failed to draw tile")?;
            context.identity_matrix();
        }
        debug!("Drawn tiles");

        context.set_line_join(LineJoin::Bevel);

        let mut segment_count = 0;
        let mut drawn_segment_count = 0;
        for (i, poly) in self.track_state.visible_polylines(&self.camera).enumerate() {
            trace!("Drawing polyline {}", i);
            let color = poly.color.0;
            context.set_source_rgb(color[0].into(), color[1].into(), color[2].into());
            if self.thick {
                context.set_line_width(Self::THICKNESS);
            } else {
                context.set_line_width(1.0);
            };

            segment_count += poly.segments_count();
            let mut last_index = None;
            for (index, p1, p2) in poly.segments() {
                drawn_segment_count += 1;
                if last_index.is_none_or(|last| last + 1 < index) {
                    context.move_to(p1.x as f64, p1.y as f64);
                }
                context.line_to(p2.x as f64, p2.y as f64);
                last_index = Some(index);
            }

            context.stroke().context("Failed to draw polyline")?;
        }
        debug!("Drawn {} / {} segments", drawn_segment_count, segment_count);

        let endpoint_count = 2 * self.track_state.polylines_count();
        let mut drawn_endpoint_count = 0;
        for (i, poly) in self.track_state.visible_polylines(&self.camera).enumerate() {
            trace!("Drawing polyline {}'s endpoints", i);
            if let Some(point) = poly.first_point() {
                context.set_source_rgb(0.0, 1.0, 0.0);
                context.arc(
                    point.x as f64,
                    point.y as f64,
                    Self::CIRCLE_RADIUS,
                    0.0,
                    2.0 * std::f64::consts::PI,
                );
                context.fill().context("Failed to draw circle")?;

                drawn_endpoint_count += 1;
            }
            if let Some(point) = poly.last_point() {
                context.set_source_rgb(1.0, 0.2, 0.2);
                context.arc(
                    point.x as f64,
                    point.y as f64,
                    Self::CIRCLE_RADIUS,
                    0.0,
                    2.0 * std::f64::consts::PI,
                );
                context.fill().context("Failed to draw circle")?;

                drawn_endpoint_count += 1;
            }
        }
        debug!(
            "Drawn {} / {} endpoints",
            drawn_endpoint_count, endpoint_count
        );

        let render_stats = RenderStats {
            drawn_tiles_count: tiles_to_draw.len(),
            track_stats,
            segment_count,
            drawn_segment_count,
        };

        self.render_text(context, render_stats)?;

        Ok(())
    }

    /// Renders the debugging statistics at the bottom of the UI.
    fn render_text(&self, context: &Context, render_stats: RenderStats) -> anyhow::Result<()> {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.5);
        context.rectangle(
            0.0,
            self.camera.height() - 3.5 * Self::FONT_SIZE,
            self.camera.width(),
            3.5 * Self::FONT_SIZE,
        );
        context.fill().context("Failed to draw rectangle")?;

        context.set_font_face(&self.font_face);
        context.set_font_size(Self::FONT_SIZE);
        context.set_source_rgb(0.0, 0.0, 0.0);

        context.move_to(0.0, self.camera.height() - 2.5 * Self::FONT_SIZE);
        context
            .show_text(&format!("Drawn {} tiles", render_stats.drawn_tiles_count))
            .context("Failed to draw text")?;

        let track_stats = &render_stats.track_stats;
        context.move_to(0.0, self.camera.height() - 1.5 * Self::FONT_SIZE);
        context
            .show_text(&format!(
                "Deduped {} / {} / {} points",
                track_stats.visible_points, track_stats.deduped_points, track_stats.total_points
            ))
            .context("Failed to draw text")?;

        context.move_to(0.0, self.camera.height() - 0.5 * Self::FONT_SIZE);
        context
            .show_text(&format!(
                "Drawn {} / {} segments",
                render_stats.drawn_segment_count, render_stats.segment_count
            ))
            .context("Failed to draw text")?;

        Ok(())
    }
}
