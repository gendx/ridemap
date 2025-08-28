//! Window backed by piston.

use crate::config::FONT_PATH;
use crate::map::tile_channel::TileRequestSender;
use crate::ui::camera::Camera;
use crate::ui::tiles::TileState;
use crate::ui::tracks::TrackState;
use crate::ui::util::{warn_on_error, RenderStats};
use crate::ui::UiMessage;
use anyhow::bail;
use anyhow::Context as AnyhowContext;
use futures::channel::oneshot;
use graphics::character::CharacterCache;
use graphics::image::Image;
use graphics::line::{Line, Shape};
use graphics::types::FontSize;
use graphics::Graphics;
use log::{debug, error, info, trace};
use piston_window::ellipse::circle;
use piston_window::{
    Button, ButtonArgs, ButtonState, Context, Event, Filter, G2dTexture, GenericEvent, Glyphs,
    Input, Key, Loop, Motion, MouseButton, PistonWindow, ResizeArgs, Texture, TextureContext,
    TextureSettings, Transformed, WindowSettings,
};
use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

/// Window state on the GUI.
pub struct Window {
    ui_rx: Receiver<UiMessage>,
    cancel_tx: oneshot::Sender<()>,
    camera: Camera,
    tile_state: TileState<(Image, G2dTexture)>,
    track_state: TrackState,
    lazy_ui_refresh: bool,
    thick: bool,
    click: bool,
    need_refresh: bool,
    iteration: Rc<Cell<usize>>,
}

impl Window {
    /// Initial window width.
    const INITIAL_WIDTH: u32 = 640;
    /// Initial window height.
    const INITIAL_HEIGHT: u32 = 480;
    /// Radius of track endpoints.
    const CIRCLE_RADIUS: f64 = 5.0;
    /// Thickness of tracks in thick mode.
    const THICKNESS: f64 = 4.0;
    /// Font size.
    const FONT_SIZE: FontSize = 12;

    /// Runs the UI loop, in the UI thread.
    pub fn ui_loop(
        ui_rx: Receiver<UiMessage>,
        cancel_tx: oneshot::Sender<()>,
        tiles_tx: TileRequestSender,
        lazy_ui_refresh: bool,
        speculative_tile_load: bool,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
    ) -> anyhow::Result<()> {
        let mut piston_window =
            match WindowSettings::new("Ridemap", (Self::INITIAL_WIDTH, Self::INITIAL_HEIGHT))
                .exit_on_esc(true)
                .build()
            {
                Ok(window) => window,
                Err(e) => bail!("Failed to build PistonWindow: {e:?}"),
            };

        let window = Window::new(
            ui_rx,
            cancel_tx,
            tiles_tx,
            lazy_ui_refresh,
            speculative_tile_load,
            max_pixels_per_tile,
            max_tile_level,
        );
        window.do_loop(&mut piston_window)
    }

    /// Runs the UI loop using the given window state.
    fn do_loop(mut self, piston_window: &mut PistonWindow) -> anyhow::Result<()> {
        self.tile_state.start();
        let mut glyphs = Glyphs::new(
            FONT_PATH,
            TextureContext {
                factory: piston_window.factory.clone(),
                encoder: piston_window.factory.create_command_buffer().into(),
            },
            // Supposedly this avoids blurry text.
            TextureSettings::new().min(Filter::Nearest),
        )
        .context(format!("Failed to load font from {FONT_PATH}"))?;

        while let Some(event) = piston_window.next() {
            self.iteration.set(self.iteration.get() + 1);

            match event {
                Event::Input(input, _) => {
                    self.process_input(input);
                }
                Event::Loop(Loop::Update(_)) => {
                    self.process_update(piston_window);
                }
                Event::Loop(Loop::Render(_)) => {
                    self.process_render(&event, piston_window, &mut glyphs);
                }
                Event::Loop(Loop::AfterRender(_)) => {
                    trace!("[{i}] AfterRender", i = self.iteration.get());
                }
                Event::Loop(Loop::Idle(_)) => {
                    trace!("[{i}] Idle", i = self.iteration.get());
                }
                _ => {}
            }
        }

        info!("End of window loop");
        self.tile_state.stop();
        warn_on_error(self.cancel_tx.send(()), "message on one-shot channel");

        Ok(())
    }

    /// Creates a new window state.
    fn new(
        ui_rx: Receiver<UiMessage>,
        cancel_tx: oneshot::Sender<()>,
        tiles_tx: TileRequestSender,
        lazy_ui_refresh: bool,
        speculative_tile_load: bool,
        max_pixels_per_tile: usize,
        max_tile_level: i32,
    ) -> Self {
        let iteration = Rc::new(Cell::new(0));
        Self {
            ui_rx,
            cancel_tx,
            camera: Camera::new(Self::INITIAL_WIDTH, Self::INITIAL_HEIGHT),
            tile_state: TileState::new(
                tiles_tx,
                speculative_tile_load,
                max_pixels_per_tile,
                max_tile_level,
                iteration.clone(),
            ),
            track_state: TrackState::new(),
            lazy_ui_refresh,
            thick: false,
            click: false,
            need_refresh: true,
            iteration,
        }
    }

    /// Processes user input.
    fn process_input(&mut self, input: Input) {
        trace!("[{i}] Input", i = self.iteration.get());

        let mut need_zoom_refresh = false;
        let mut need_offset_refresh = false;

        // Direction of movement along all coordinate axes. Used to speculatively load
        // tiles in the respective direction.
        let mut x_dir = Ordering::Equal;
        let mut y_dir = Ordering::Equal;
        let mut z_dir = Ordering::Equal;

        self.need_refresh |= match input {
            Input::Resize(ResizeArgs {
                window_size,
                draw_size: _,
            }) => {
                self.camera.resize(
                    window_size[0],
                    window_size[1],
                    &mut need_zoom_refresh,
                    &mut need_offset_refresh,
                );
                true
            }
            Input::Button(ButtonArgs {
                state,
                button: Button::Mouse(MouseButton::Left),
                scancode: _,
            }) => {
                self.click = match state {
                    ButtonState::Press => true,
                    ButtonState::Release => false,
                };
                false
            }
            Input::Button(ButtonArgs {
                state: ButtonState::Press,
                button: Button::Keyboard(key),
                scancode: _,
            }) => match key {
                Key::Space => {
                    self.thick = !self.thick;
                    true
                }
                Key::T => {
                    self.track_state.toggle_color_by_type();
                    true
                }
                Key::R => {
                    self.track_state.randomize_colors();
                    true
                }
                _ => false,
            },
            Input::Move(Motion::MouseScroll(scroll)) => {
                self.camera
                    .scroll(scroll[1], &mut need_zoom_refresh, &mut z_dir);
                true
            }
            Input::Move(Motion::MouseRelative(coord)) => {
                if self.click {
                    self.camera.drag_relative(
                        coord[0],
                        coord[1],
                        &mut need_offset_refresh,
                        &mut x_dir,
                        &mut y_dir,
                    );
                    true
                } else {
                    false
                }
            }
            _ => false,
        };

        if need_zoom_refresh || need_offset_refresh {
            self.tile_state
                .update(&mut self.camera, x_dir, y_dir, z_dir);
        }

        if need_zoom_refresh {
            self.track_state.refresh_zoom(&self.camera);
        }
    }

    /// Processes the update event from Piston.
    fn process_update(&mut self, piston_window: &mut PistonWindow) {
        trace!("[{i}] Update", i = self.iteration.get());

        for msg in self.ui_rx.try_iter() {
            match msg {
                UiMessage::Activity { id, r#type, points } => {
                    debug!("[{i}] Received activity #{id}", i = self.iteration.get());
                    self.track_state
                        .process_activity(r#type, points, &self.camera);

                    self.need_refresh = true;
                }
                UiMessage::Tile {
                    index,
                    png_image,
                    rgba_image,
                } => {
                    self.need_refresh |=
                        self.tile_state
                            .process_tile(index, &png_image, rgba_image, |rgba_image| {
                                match Texture::from_image(
                                    &mut piston_window.create_texture_context(),
                                    &rgba_image,
                                    &TextureSettings::new(),
                                ) {
                                    Ok(texture) => Some((Image::new().rect(index.rect()), texture)),
                                    Err(e) => {
                                        error!("Error creating texture: {e}");
                                        None
                                    }
                                }
                            });
                }
            }
        }
    }

    /// Processes the render event from Piston.
    fn process_render<E: GenericEvent>(
        &mut self,
        event: &E,
        piston_window: &mut PistonWindow,
        glyphs: &mut Glyphs,
    ) {
        trace!("[{i}] Render", i = self.iteration.get());

        if self.need_refresh || !self.lazy_ui_refresh {
            if self.lazy_ui_refresh {
                debug!("[{i}] Rendering", i = self.iteration.get());
            }

            piston_window.draw_2d(event, |context, graphics, device| {
                let render_stats = self.render(context, graphics);
                if let Err(e) = self.render_text(context, graphics, glyphs, render_stats) {
                    error!("Failed to render text: {e:?}");
                }
                // Update glyphs before rendering.
                glyphs.factory.encoder.flush(device);
            });
        }

        self.need_refresh = false;
    }

    /// Renders the UI on the given graphical context.
    fn render<G>(&self, context: Context, graphics: &mut G) -> RenderStats
    where
        G: Graphics<Texture = G2dTexture>,
    {
        let track_stats = self.track_state.debug_statistics(&self.camera);

        graphics::clear([1.0, 1.0, 0.7, 1.0], graphics);

        let ioffset = self.camera.ioffset();
        let zoom = self.camera.zoom();
        let tile_transform = context
            .transform
            .trans(ioffset.x as f64, ioffset.y as f64)
            .scale(zoom, zoom);

        let tiles_to_draw = self.tile_state.tiles_to_draw();
        for (i, (_, tile)) in tiles_to_draw.iter().enumerate() {
            trace!("Drawing tile {i}/{}", tiles_to_draw.len());
            let image: &Image = &tile.image.0;
            let texture: &G2dTexture = &tile.image.1;
            image.draw(texture, &context.draw_state, tile_transform, graphics);
        }
        debug!("Drawn tiles");

        let mut segment_count = 0;
        let mut drawn_segment_count = 0;
        for (i, poly) in self.track_state.visible_polylines(&self.camera).enumerate() {
            trace!("Drawing polyline {i}");
            let color = poly.color.0;
            let line = if self.thick {
                Line::new(color, Self::THICKNESS)
                    .width(Self::THICKNESS)
                    .shape(Shape::Bevel)
            } else {
                Line::new(color, 1.0)
            };

            segment_count += poly.segments_count();
            for (_index, p1, p2) in poly.segments() {
                drawn_segment_count += 1;
                line.draw(
                    [p1.x as f64, p1.y as f64, p2.x as f64, p2.y as f64],
                    &context.draw_state,
                    context.transform,
                    graphics,
                );
            }
        }
        debug!("Drawn {drawn_segment_count} / {segment_count} segments");

        let endpoint_count = 2 * self.track_state.polylines_count();
        let mut drawn_endpoint_count = 0;
        for (i, poly) in self.track_state.visible_polylines(&self.camera).enumerate() {
            trace!("Drawing polyline {i}'s endpoints");
            if let Some(point) = poly.first_point() {
                graphics::ellipse(
                    [0.0, 1.0, 0.0, 1.0],
                    circle(point.x as f64, point.y as f64, Self::CIRCLE_RADIUS),
                    context.transform,
                    graphics,
                );
                drawn_endpoint_count += 1;
            }
            if let Some(point) = poly.last_point() {
                graphics::ellipse(
                    [1.0, 0.2, 0.2, 1.0],
                    circle(point.x as f64, point.y as f64, Self::CIRCLE_RADIUS),
                    context.transform,
                    graphics,
                );
                drawn_endpoint_count += 1;
            }
        }
        debug!("Drawn {drawn_endpoint_count} / {endpoint_count} endpoints");

        RenderStats {
            drawn_tiles_count: tiles_to_draw.len(),
            track_stats,
            segment_count,
            drawn_segment_count,
        }
    }

    /// Renders the debugging statistics at the bottom of the UI.
    fn render_text<C, G>(
        &self,
        context: Context,
        graphics: &mut G,
        character_cache: &mut C,
        render_stats: RenderStats,
    ) -> anyhow::Result<()>
    where
        G: Graphics<Texture = G2dTexture>,
        C: CharacterCache<Texture = G2dTexture>,
        C::Error: Debug,
    {
        let font_size = Self::FONT_SIZE as f64;

        graphics::rectangle(
            [1.0, 1.0, 1.0, 0.5],
            [
                0.0,
                self.camera.height() - 3.5 * font_size,
                self.camera.width(),
                3.5 * font_size,
            ],
            context.transform,
            graphics,
        );

        // Render at twice the font size but with 0.5 zoom for Retina displays. See https://github.com/PistonDevelopers/piston/issues/1240#issuecomment-569318143.
        if let Err(e) = graphics::text(
            [0.0, 0.0, 0.0, 1.0],
            Self::FONT_SIZE * 2,
            &format!("Drawn {} tiles", render_stats.drawn_tiles_count),
            character_cache,
            context
                .transform
                .trans(0.0, self.camera.height() - 2.5 * font_size)
                .zoom(0.5),
            graphics,
        ) {
            bail!("Failed to draw text: {e:?}");
        }

        let track_stats = &render_stats.track_stats;
        if let Err(e) = graphics::text(
            [0.0, 0.0, 0.0, 1.0],
            Self::FONT_SIZE * 2,
            &format!(
                "Deduped {} / {} / {} points",
                track_stats.visible_points, track_stats.deduped_points, track_stats.total_points
            ),
            character_cache,
            context
                .transform
                .trans(0.0, self.camera.height() - 1.5 * font_size)
                .zoom(0.5),
            graphics,
        ) {
            bail!("Failed to draw text: {e:?}");
        }

        if let Err(e) = graphics::text(
            [0.0, 0.0, 0.0, 1.0],
            Self::FONT_SIZE * 2,
            &format!(
                "Drawn {} / {} segments",
                render_stats.drawn_segment_count, render_stats.segment_count
            ),
            character_cache,
            context
                .transform
                .trans(0.0, self.camera.height() - 0.5 * font_size)
                .zoom(0.5),
            graphics,
        ) {
            bail!("Failed to draw text: {e:?}");
        }

        Ok(())
    }
}
