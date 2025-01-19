use iced::{
    widget::canvas::{
        event::{
            Event,
            Status,
        },
        path::Path,
        Cache,
        Program as CanvasProgram,
        Canvas,
    },
    keyboard::{
        key::Named as NamedKey,
        Event as KeyboardEvent,
        Key,
    },
    mouse::{
        Cursor,
        Event as MouseEvent,
        Button as MouseButton,
        ScrollDelta,
    },
    Point as IcedPoint,
    Element,
    Length,
    Theme,
    Renderer,
    Rectangle,
    Size,
    Task,
};
use iced_graphics::geometry::{
    Renderer as GeometryRenderer,
    Stroke,
    Style,
    // Fill,
    LineCap,
    LineJoin,
};
use anyhow::Result;
use std::collections::{
    HashMap,
    HashSet,
};
use crate::{
    model::*,
    Point,
    Transform,
    Translation,
    Rotation,
    Vector,
};


/// State changes that can occur to entities
#[derive(Debug, Clone)]
pub enum SheetMessage {
    /// Recalculate the paths. We might have deselected something or changed the selection or
    /// something else.
    RecalcPaths,
    /// Select an entity.
    Select(EntityId),
    /// Deselect and entity.
    Deselect(EntityId),
    /// An amount to pan relative to the previous position.
    Pan(Translation),
    /// An amount to move an entity and its index.
    Move(EntityId, Translation),
    /// Contains the the cursor position.
    ZoomIn(Point),
    /// Contains the the cursor position.
    ZoomOut(Point),
}

/// What the current action is for the sheet.
#[derive(Debug, Default, PartialEq)]
pub enum SheetState {
    /// Where the previous mouse position is located and the index of the model
    Select(EntityId),
    /// An amount to move a model and its index.
    Move(EntityId, Point),

    /// Pan with an entity selected.
    PanSelected(EntityId, Point),

    /// Pan the screen.
    Pan(Point),

    /// Do nothing
    #[default]
    None,
}


/// An entity's transform and if it is flipped. This only flips it in the Y axis.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EntityTransform {
    pub transform: Transform,
    pub flip: bool,
}

/// A sheet to nest the models in. Has a sheet size to display an outline and handles displaying
/// all instances of a model.
pub struct Sheet {
    pub active_models: HashMap<ModelHandle, HashSet<usize>>,
    pub entities: HashMap<EntityId, (ModelHandle, EntityTransform)>,
    pub sheet_size: Vector,

    models: ModelStore,
    paths: HashMap<EntityId, ModelPaths>,
    cached_models: HashMap<EntityId, Cache>,
    view: Transform,
    sheet_cache: Cache,
}
impl Sheet {
    pub fn new(models: ModelStore)->Self {
        Sheet {
            models,
            active_models: HashMap::new(),
            entities: HashMap::new(),
            paths: HashMap::new(),
            cached_models: HashMap::new(),
            view: Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0),
            sheet_size: Vector::new(300.0, 300.0),
            sheet_cache: Cache::new(),
        }
    }

    /// Add a model with a quantity.
    #[inline]
    #[allow(unused)]
    pub fn add_model(&mut self, path: &str, qty: usize)->Result<()> {
        let transform = Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0);

        self.add_model_with_transform(path, EntityTransform {transform, flip:false}, qty)
    }

    /// Add a model with a transform and quantity.
    pub fn add_model_with_transform(&mut self, path: &str, transform: EntityTransform, qty: usize)->Result<()> {
        let model = Model::load(path)?;

        let handle = self.models.add(model);

        self.add_model_from_handle_with_transform(handle, transform, qty);
        
        return Ok(());
    }

    /// Add a model from the given ID
    pub fn add_model_from_handle(&mut self, handle: ModelHandle, qty: usize) {
        let transform = Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0);

        self.add_model_from_handle_with_transform(handle, EntityTransform {transform, flip:false}, qty)
    }

    /// Add a model from the given ID and transform
    pub fn add_model_from_handle_with_transform(&mut self, handle: ModelHandle, mut transform: EntityTransform, qty: usize) {
        let model_entity_list = self.active_models
            .entry(handle.clone())
            .or_default();

        for _ in 0..qty {
            let entity_idx = self.entities.len();
            model_entity_list.insert(entity_idx);

            let id = next_entity_id();
            self.entities.insert(id, (handle.clone(), transform));
            self.paths.insert(id, handle.paths(transform));
            self.cached_models.insert(id, Cache::new());
            transform.transform.translation += Point::new(5.0, 5.0);
        }

        self.recalc_paths();
    }

    pub fn main_view(&self)->Element<SheetMessage> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn main_update(&mut self, msg: SheetMessage)->Task<SheetMessage> {
        match msg {
            SheetMessage::RecalcPaths|
                SheetMessage::Select(_)|
                SheetMessage::Deselect(_)=>self.recalc_paths(),
            SheetMessage::Move(id, delta)=>{
                self.entities
                    .get_mut(&id)
                    .unwrap()
                    .1.transform
                    .translation += delta / self.view.scale;
                self.recalc_paths();
            },
            SheetMessage::Pan(delta)=>{
                self.view.translation += delta;
                self.recalc_paths();
            },
            SheetMessage::ZoomIn(mouse_pos)=>{
                const ZOOM: f64 = 1.1;

                let mouse_offset = self.view.translation - mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;
                self.view.translation += offset;

                self.view.scale *= ZOOM;

                self.recalc_paths();
            },
            SheetMessage::ZoomOut(mouse_pos)=>{
                const ZOOM: f64 = 0.9;

                let mouse_offset = self.view.translation - mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;
                self.view.translation += offset;

                self.view.scale *= ZOOM;

                self.recalc_paths();
            },
        }

        Task::none()
    }

    /// Recalculate the paths and clear the geometry caches.
    pub fn recalc_paths(&mut self) {
        self.cached_models.values().for_each(Cache::clear);
        self.sheet_cache.clear();
        for (id, (handle, mt)) in self.entities.iter() {
            let mut mt = *mt;
            mt.transform.append_similarity(self.view);
            self.paths.insert(*id, handle.paths(mt));
        }
    }

    pub fn delete_entity(&mut self, id: EntityId) {
        eprintln!("Delete entity: {id:?}");
        self.entities.remove(&id);
        self.paths.remove(&id);
        self.cached_models.remove(&id);
    }
}
impl CanvasProgram<SheetMessage> for Sheet {
    type State = SheetState;

    /// TODO: Document this thing better
    fn draw(&self,
        state: &SheetState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<<Renderer as GeometryRenderer>::Geometry> {
        let color = theme.palette().text;
        let outline_color = theme.palette().primary;
        let sheet_fg_color = theme.palette().primary;
        let mut ret = Vec::new();

        assert!(self.entities.len() == self.paths.len());
        assert!(self.entities.len() == self.cached_models.len());

        // draw the sheet first
        ret.push(self.sheet_cache.draw(
            renderer,
            Size {
                width: bounds.width,
                height: bounds.height,
            },
            |frame|{
                let sheet_size = self.sheet_size * self.view.scale;
                let size = Size {
                    width: sheet_size.x as f32,
                    height: sheet_size.y as f32,
                };
                let point = self.view.transform_vec(Point::zero());
                let point = IcedPoint::new(point.x as f32, point.y as f32);

                // do the background of the sheet
                // frame.fill(
                //     &Path::rectangle(
                //         point,
                //         size,
                //     ),
                //     Fill {
                //         style: Style::Solid(sheet_bg_color),
                //         ..Fill::default()
                //     },
                // );

                // do the outline of the sheet
                frame.stroke(
                    &Path::rectangle(
                        point,
                        size,
                    ),
                    Stroke {
                        style: Style::Solid(sheet_fg_color),
                        width: 2.0,
                        line_join: LineJoin::Miter,
                        line_cap: LineCap::Square,
                        ..Stroke::default()
                    },
                );
            },
        ));

        // then the models
        for (id, cache) in self.cached_models.iter() {
            let paths = self.paths.get(id).unwrap();
            ret.push(cache.draw(
                renderer,
                Size {
                    width: bounds.width,
                    height: bounds.height,
                },
                |frame|{
                    use SheetState as State;

                    let stroke = Stroke {
                        style: Style::Solid(color),
                        width: 1.0,
                        line_join: LineJoin::Miter,
                        line_cap: LineCap::Square,
                        ..Stroke::default()
                    };

                    // Do the main path before the outline so the outline shows over the paths
                    for path in paths.lines.iter() {
                        frame.stroke(
                            path,
                            stroke,
                        );
                    }

                    // do the outline
                    match state {
                        State::Move(idx, _)|State::Select(idx)|State::PanSelected(idx, _)=>{
                            if id == idx {
                                frame.stroke(
                                    &paths.outline,
                                    Stroke {
                                        style: Style::Solid(outline_color),
                                        ..stroke
                                    },
                                );
                            }
                        },
                        _=>{},
                    }
                },
            ));
        }

        return ret;
    }

    /// TODO: Document this thing
    fn update(
        &self,
        state: &mut Self::State,
        event: Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> (Status, Option<SheetMessage>) {
        use SheetState as State;
        if cursor.is_over(bounds) {
            let cursor_pos = cursor.position_in(bounds).unwrap();
            let cursor_pos = Point::new(cursor_pos.x as f64, cursor_pos.y as f64);
            match event {
                Event::Keyboard(e)=>{
                    // let movement = (1.0 / self.view.scale.sqrt()).min(5.0);
                    let movement = 1.0;
                    let id = match state {
                        State::Select(id)=>*id,
                        _=>return (Status::Ignored, None),
                    };
                    match e {
                        KeyboardEvent::KeyPressed{key:Key::Named(key),..}=>match key {
                            NamedKey::ArrowLeft=>return (
                                Status::Captured,
                                Some(SheetMessage::Move(id, Vector::new(-movement, 0.0))),
                            ),
                            NamedKey::ArrowRight=>return (
                                Status::Captured,
                                Some(SheetMessage::Move(id, Vector::new(movement, 0.0))),
                            ),
                            NamedKey::ArrowUp=>return (
                                Status::Captured,
                                Some(SheetMessage::Move(id, Vector::new(0.0, -movement))),
                            ),
                            NamedKey::ArrowDown=>return (
                                Status::Captured,
                                Some(SheetMessage::Move(id, Vector::new(0.0, movement))),
                            ),
                            _=>{},
                        },
                        _=>{},
                    }
                },
                Event::Mouse(e)=>{
                    match e {
                        MouseEvent::ButtonPressed(MouseButton::Left)=>{
                            let mouse_offset = cursor_pos;
                            for (id, (model, mt)) in self.entities.iter() {
                                let mut model_tr = mt.transform;
                                model_tr.append_similarity(self.view);
                                let inv_model_view = model_tr.inversed();
                                let mut model_point = inv_model_view.transform_vec(mouse_offset);

                                if mt.flip {
                                    model_point.y *= -1.0;
                                }

                                if model.point_within(model_point) {
                                    match state {
                                        State::Select(idx)=>if idx == id {
                                            eprintln!("Start move {id:?}");
                                        } else {
                                            eprintln!("Select and start move {id:?}");
                                        },
                                        _=>eprintln!("Select and start move {id:?}"),
                                    }
                                    *state = State::Move(*id, cursor_pos);
                                    return (Status::Captured, Some(SheetMessage::Select(*id)));
                                }
                            }

                            match state {
                                State::Select(id)=>{
                                    let id = *id;
                                    eprintln!("Deselect {id:?}");
                                    *state = State::None;
                                    return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                },
                                _=>{},
                            }

                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonReleased(MouseButton::Left)=>{
                            match state {
                                State::Move(id,_ )=>{
                                    eprintln!("Stop move {id:?}");
                                    *state = State::Select(*id);
                                    return (Status::Captured, None);
                                },
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonPressed(MouseButton::Right)=>{
                            match state {
                                State::Select(id)=>{
                                    eprintln!("Start pan with selection {id:?}");
                                    *state = State::PanSelected(*id, cursor_pos);
                                },
                                State::None=>{
                                    *state = State::Pan(cursor_pos);
                                    eprintln!("Start pan");
                                },
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonReleased(MouseButton::Right)=>{
                            match state {
                                State::Pan(_)=>{
                                    *state = State::None;
                                    eprintln!("Stop pan");
                                },
                                State::PanSelected(id, _)=>{
                                    eprintln!("Stop pan with selection {id:?}");
                                    *state = State::Select(*id);
                                },
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::CursorMoved{..}=>{
                            match state {
                                State::Pan(prev)|State::PanSelected(_, prev)=>{
                                    let delta = cursor_pos - *prev;
                                    *prev = cursor_pos;

                                    return (
                                        Status::Captured,
                                        Some(SheetMessage::Pan(delta)),
                                    );
                                },
                                State::Move(idx, prev)=>{
                                    let idx = *idx;
                                    let delta = cursor_pos - *prev;
                                    *state = State::Move(idx, cursor_pos);

                                    return (
                                        Status::Captured,
                                        Some(SheetMessage::Move(idx, delta)),
                                    );
                                },
                                _=>{},
                            }
                        },
                        MouseEvent::WheelScrolled{delta:ScrollDelta::Lines{y,..}}=>{
                            let msg = if y > 0.0 {
                                eprintln!("Zoom in");
                                SheetMessage::ZoomIn(cursor_pos)
                            } else {
                                eprintln!("Zoom out");
                                SheetMessage::ZoomOut(cursor_pos)
                            };
                            return (Status::Captured, Some(msg));
                        },
                        _=>{},
                    }
                },
                _=>{},
            }
        }

        (Status::Ignored, None)
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[repr(transparent)]
pub struct EntityId(usize);

fn next_entity_id()->EntityId {
    use std::sync::atomic::{
        Ordering,
        AtomicUsize,
    };
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    EntityId(COUNT.fetch_add(1, Ordering::SeqCst))
}
