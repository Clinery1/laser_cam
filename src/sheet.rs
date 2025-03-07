use iced::{
    widget::canvas::{
        event::{
            Event,
            Status,
        },
        path::{
            Path,
            Builder as PathBuilder,
        },
        Text as CanvasText,
        Cache,
        Program as CanvasProgram,
        Canvas,
        Frame,
    },
    alignment::{
        Vertical as VerticalAlign,
        Horizontal as HorizontalAlign,
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
    Color,
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
use indexmap::IndexSet;
use time::OffsetDateTime;
use anyhow::Result;
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    cell::{
        RefCell,
        Cell,
    },
    rc::Rc,
};
use crate::{
    laser::{
        ConditionId,
        ConditionStore,
    },
    model::*,
    gcode::*,
    utils::*,
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
    /// Recalculate the paths for an Entity
    RecalcPathsId(EntityId),
    /// Select an entity.
    Select(EntityId),
    /// Deselect and entity.
    Deselect(EntityId),
    /// An amount to pan relative to the previous position.
    Pan(Translation, Translation),
    /// An amount to move an entity and its index.
    Move(EntityId, Translation),
    /// An amount to move an entity and its index. Also selects the entity.
    SelectMove(EntityId, Translation),
    /// Contains the the cursor position.
    ZoomIn(Point, Point),
    /// Contains the the cursor position.
    ZoomOut(Point, Point),

    Delete(EntityId),

    StartOrder,
    SetShowOrder(bool),
    AddToOrder(EntityId),
    FinishOrder(EntityId),
}

/// What the current action is for the sheet.
#[derive(Debug, PartialEq)]
pub enum SheetState {
    /// Delay the selection of an entity.
    /// `DelaySelect(current_id, next_id, prev_cursor_pos)`
    DelaySelect(EntityId, EntityId, Point),
    /// Where the previous mouse position is located and the index of the model
    Select(EntityId, Point),
    /// An amount to move a model and its index.
    Move(EntityId, Point),

    /// Pan with an entity selected.
    PanSelected(EntityId, Point, Point),

    /// Pan the screen.
    Pan(Point, Point),

    OrderEdit,
    OrderEditSelect(EntityId),
    OrderEditPan(Point, Point),
    OrderEditPanSelect(EntityId, Point, Point),

    /// Do nothing
    None(Point),
}
impl Default for SheetState {
    fn default()->Self {
        Self::None(Point::zero())
    }
}


/// An entity's transform and if it is flipped. This only flips it in the Y axis.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EntityState {
    pub transform: Transform,
    pub flip: bool,
    pub laser_condition: ConditionId,
}
impl EntityState {
    pub fn transform(&self, mut point: Point)->Point {
        if self.flip {
            point.y *= -1.0;
        }

        self.transform.transform_vec(point)
    }
}

/// A sheet to nest the models in. Has a sheet size to display an outline and handles displaying
/// all instances of a model.
pub struct Sheet {
    pub active_models: HashMap<ModelHandle, HashSet<EntityId>>,
    pub entities: HashMap<EntityId, (ModelHandle, EntityState)>,
    pub sheet_size: Vector,

    pub laser_conditions: Rc<RefCell<ConditionStore>>,

    models: ModelStore,
    paths: HashMap<EntityId, (Color, ModelPaths)>,
    cached_models: HashMap<EntityId, Cache>,
    view: Transform,
    world: Transform,
    sheet_cache: Cache,
    window_height: Cell<f64>,
    height_change: Cell<bool>,

    recent_clicks: RefCell<HashSet<EntityId>>,

    order: IndexSet<EntityId>,

    pub show_order: bool,
    pub reorder: bool,
    pub grbl_comments: bool,
}
impl Sheet {
    pub fn new(models: ModelStore, laser_conditions: Rc<RefCell<ConditionStore>>)->Self {
        Sheet {
            models,
            active_models: HashMap::new(),
            entities: HashMap::new(),
            paths: HashMap::new(),
            cached_models: HashMap::new(),
            view: Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0),
            world: Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0),
            sheet_size: Vector::new(300.0, 300.0),
            sheet_cache: Cache::new(),
            laser_conditions,
            window_height: Cell::new(1000.0),
            height_change: Cell::new(false),

            recent_clicks: RefCell::new(HashSet::new()),

            order: IndexSet::new(),

            show_order: false,
            reorder: false,
            grbl_comments: false,
        }
    }

    pub fn generate_gcode(&self, name: &str)->String {
        let mut builder = GcodeBuilder::default();
        if self.grbl_comments {
            builder.set_grbl_mode();
        }
        let now = OffsetDateTime::now_local()
            .unwrap_or(OffsetDateTime::now_utc());

        builder.comment_block(concat!("Gcode generated by LaserCAM ", env!("CARGO_PKG_VERSION")));
        builder.comment_block(env!("CARGO_PKG_REPOSITORY"));

        // builder.comment_block("NOTE: 0,0 is the \"top left\" of the sheet");

        builder.comment_block(format!("Sheet \"{}\" width: {}; height: {}", name, self.sheet_size.x, self.sheet_size.y));
        builder.comment_block(format!(
            "Generated on {} {}, {} at {}:{}",
            now.month(),
            now.day(),
            now.year(),
            now.hour(),
            now.minute(),
        ));
        builder.default_header();

        let store = self.laser_conditions.borrow();
        for (model, mt) in self.entities.values() {
            let condition = store.get(mt.laser_condition);
            model.generate_gcode(mt, &mut builder, condition);
        }
        drop(store);

        builder.rapid_motion()
            .x(0.0)
            .y(0.0)
            .eob();

        return builder.finish();
    }

    /// Add a model with a quantity.
    #[inline]
    #[allow(unused)]
    pub fn add_model(&mut self, path: &str, qty: usize, laser_condition: ConditionId)->Result<()> {
        let transform = Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0);

        self.add_model_with_transform(path, EntityState {transform, flip: false, laser_condition}, qty)
    }

    /// Add a model with a transform and quantity.
    pub fn add_model_with_transform(&mut self, path: &str, transform: EntityState, qty: usize)->Result<()> {
        let model = Model::load(path)?;

        let handle = self.models.add(model);

        self.add_model_from_handle_with_transform(handle, transform, qty);

        return Ok(());
    }

    /// Add a model from the given ID
    pub fn add_model_from_handle(&mut self, handle: ModelHandle, qty: usize, laser_condition: ConditionId) {
        let transform = Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0);

        self.add_model_from_handle_with_transform(handle, EntityState {transform, flip:false, laser_condition}, qty)
    }

    /// Add a model from the given ID and transform
    pub fn add_model_from_handle_with_transform(&mut self, handle: ModelHandle, mut transform: EntityState, qty: usize) {
        let model_entity_list = self.active_models
            .entry(handle.clone())
            .or_default();

        let store = self.laser_conditions.borrow();
        let color = store.get(transform.laser_condition).color;
        drop(store);

        for _ in 0..qty {
            let id = next_entity_id();
            model_entity_list.insert(id);
            self.entities.insert(id, (handle.clone(), transform));
            self.order.insert(id);
            self.paths.insert(id, (color.into(), handle.paths(transform, self.window_height.get())));
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
        // If the height has changed, then recalc the paths.
        if self.height_change.take() {
            self.recalc_paths();
        }

        match msg {
            SheetMessage::RecalcPaths=>self.recalc_paths(),
            SheetMessage::RecalcPathsId(id)=>self.recalc_paths_id(id),
            SheetMessage::Select(id)=>self.clear_cache_id(id),
            SheetMessage::Delete(id)=>self.delete_entity(id),
            SheetMessage::StartOrder=>{
                if self.entities.len() > 0 {
                    self.order.clear();
                    eprintln!("Start order");
                    self.reorder = true;
                } else {
                    eprintln!("No entities. Not starting order");
                }
            },
            SheetMessage::SetShowOrder(b)=>{
                self.show_order = b;
                if self.show_order {
                    eprintln!("Showing entities");
                } else {
                    eprintln!("Hiding entities");
                }
            },
            SheetMessage::Deselect(id)=>{
                self.recent_clicks.borrow_mut().clear();

                self.clear_cache_id(id);
            },
            SheetMessage::Move(id, delta)=>{
                self.recent_clicks.borrow_mut().clear();

                self.entities
                    .get_mut(&id)
                    .unwrap()
                    .1.transform
                    .translation += delta / self.world.scale;

                self.recalc_paths_id(id);
            },
            SheetMessage::SelectMove(id, delta)=>{
                self.clear_cache_id(id);
                self.recent_clicks.borrow_mut().clear();

                self.entities
                    .get_mut(&id)
                    .unwrap()
                    .1.transform
                    .translation += delta / self.world.scale;

                self.recalc_paths_id(id);
            },
            SheetMessage::Pan(delta, w_delta)=>{
                self.recent_clicks.borrow_mut().clear();

                self.view.translation += delta;
                self.world.translation += w_delta;

                self.clear_cache();
            },
            SheetMessage::ZoomIn(mouse_pos, w_mouse_pos)=>{
                const ZOOM: f64 = 1.1;

                self.recent_clicks.borrow_mut().clear();

                let mouse_offset = self.view.translation - mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;

                self.view.translation.x += offset.x;
                self.view.translation.y += offset.y;

                self.view.scale *= ZOOM;

                let mouse_offset = self.world.translation - w_mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;

                self.world.translation.x += offset.x;
                self.world.translation.y += offset.y;

                self.world.scale *= ZOOM;

                self.clear_cache();
            },
            SheetMessage::ZoomOut(mouse_pos, w_mouse_pos)=>{
                const ZOOM: f64 = 0.9;

                self.recent_clicks.borrow_mut().clear();

                let mouse_offset = self.view.translation - mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;

                self.view.translation.x += offset.x;
                self.view.translation.y += offset.y;

                self.view.scale *= ZOOM;

                let mouse_offset = self.world.translation - w_mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;

                self.world.translation.x += offset.x;
                self.world.translation.y += offset.y;

                self.world.scale *= ZOOM;

                self.clear_cache();
            },
            SheetMessage::AddToOrder(id)=>{
                if self.order.contains(&id) {
                    self.order.shift_remove(&id);
                }
                self.order.insert(id);
                eprintln!("Add entity to order: {id:?}");
            },
            SheetMessage::FinishOrder(id)=>{
                if self.order.contains(&id) {
                    self.order.shift_remove(&id);
                }
                self.order.insert(id);
                self.reorder = false;
                eprintln!("Finish order with entity: {id:?}");
            },
        }

        Task::none()
    }

    fn clear_cache(&self) {
        self.cached_models.values().for_each(Cache::clear);
        self.sheet_cache.clear();
    }

    fn clear_cache_id(&self, id: EntityId) {
        if let Some(cache) = self.cached_models.get(&id) {
            cache.clear();
        }
    }

    /// Recalculate the paths and clear the geometry caches.
    pub fn recalc_paths(&mut self) {
        self.clear_cache();

        let store = self.laser_conditions.borrow();
        for (id, (handle, mt)) in self.entities.iter() {
            let condition = store.get(mt.laser_condition);
            self.paths.insert(*id, (condition.color.into(), handle.paths(*mt, self.window_height.get())));
        }
    }

    /// Recalculate a specific Entity's paths and clear its geometry cache.
    pub fn recalc_paths_id(&mut self, id: EntityId) {
        self.clear_cache_id(id);

        let store = self.laser_conditions.borrow();
        if let Some((handle, mt)) = self.entities.get(&id) {
            let condition = store.get(mt.laser_condition);
            self.paths.insert(id, (condition.color.into(), handle.paths(*mt, self.window_height.get())));
        }
    }

    pub fn delete_entity(&mut self, id: EntityId) {
        eprintln!("Delete entity: {id:?}");
        let (model, _) = self.entities.remove(&id).unwrap();
        self.order.shift_remove(&id);
        self.paths.remove(&id);
        self.cached_models.remove(&id);

        if let Some(entities) = self.active_models.get_mut(&model) {
            entities.remove(&id);
            if entities.len() == 0 {
                self.active_models.remove(&model);
            }
        }

        if self.show_order {
            self.clear_cache();
        }
    }

    pub fn change_width(&mut self, width: f64) {
        self.sheet_size.x = width;
        self.sheet_cache.clear();
    }

    pub fn change_height(&mut self, height: f64) {
        self.sheet_size.y = height;
        self.sheet_cache.clear();
    }

    fn draw_line(&self, f: &mut Frame, line: &Path, color: Color, width: f32) {
        let stroke = Stroke {
            style: Style::Solid(color),
            width,
            line_join: LineJoin::Miter,
            line_cap: LineCap::Square,
            ..Stroke::default()
        };

        f.stroke(line, stroke);
    }

    fn transform_frame(&self, frame: &mut Frame, _bounds: Size) {
        frame.translate(iced::Vector {
            x: self.view.translation.x as f32,
            // y: (bounds.height - self.view.translation.y as f32),
            y: self.view.translation.y as f32,
        });
        frame.scale(self.view.scale as f32);
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
        let text_color = theme.palette().text;
        let outline_color = theme.palette().primary;
        let sheet_fg_color = theme.palette().primary;
        let mut ret = Vec::new();

        let height = bounds.height as f64;

        assert!(self.entities.len() == self.paths.len());
        assert!(self.entities.len() == self.cached_models.len());

        let size = Size {
            width: bounds.width,
            height: bounds.height,
        };

        // draw the sheet first
        ret.push(self.sheet_cache.draw(
            renderer,
            size,
            |frame|{
                self.transform_frame(frame, size);

                let sheet_size = self.sheet_size;

                let mut builder = PathBuilder::new();
                builder.move_to(Point::new(
                    0.0,
                    0.0,
                ).to_ydown(height).to_iced());
                builder.line_to(Point::new(
                    sheet_size.x,
                    0.0,
                ).to_ydown(height).to_iced());
                builder.line_to(Point::new(
                    sheet_size.x,
                    sheet_size.y,
                ).to_ydown(height).to_iced());
                builder.line_to(Point::new(
                    0.0,
                    sheet_size.y,
                ).to_ydown(height).to_iced());
                builder.close();

                let path = builder.build();

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
                self.draw_line(frame, &path, sheet_fg_color, 2.0);
            },
        ));

        // then the models
        for (id, cache) in self.cached_models.iter() {
            let (color, paths) = self.paths.get(id).unwrap();
            let index = self.order.get_index_of(id)
                .map(|i|format!("#{}", i + 1))
                .unwrap_or(String::from("??"));
            ret.push(cache.draw(
                renderer,
                Size {
                    width: bounds.width,
                    height: bounds.height,
                },
                |frame|{
                    use SheetState as State;

                    self.transform_frame(frame, size);

                    if self.show_order || self.reorder {
                        let mut text = CanvasText::from(index);
                        text.position = paths.display_center;
                        text.size = (16.0 / self.view.scale as f32).into();
                        text.color = text_color;
                        text.horizontal_alignment = HorizontalAlign::Center;
                        text.vertical_alignment = VerticalAlign::Center;

                        frame.fill_text(text);
                    }

                    // Do the main path before the outline so the outline shows over the paths
                    for path in paths.lines.iter() {
                        self.draw_line(frame, &path, *color, 1.0);
                    }

                    // do the outline
                    match state {
                        State::Move(idx, _)|
                            State::Select(idx, _)|
                            State::PanSelected(idx, ..)|
                            State::DelaySelect(idx, ..)|
                            State::OrderEditSelect(idx)|
                            State::OrderEditPanSelect(idx, ..)=>{
                                if id == idx {
                                    self.draw_line(frame, &paths.outline, outline_color, 1.0);
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

        let height = bounds.height as f64;
        let old_height = self.window_height.get();

        self.window_height.set(height);
        self.height_change.set(old_height == height);

        if self.reorder {
            match state {
                State::OrderEdit|State::OrderEditSelect(_)=>{},
                State::Select(id, ..)|State::DelaySelect(id, ..)=>*state = State::OrderEditSelect(*id),
                _=>*state = State::OrderEdit,
            }
        }

        if cursor.is_over(bounds) {
            let cursor_pos = cursor.position_in(bounds)
                .unwrap()
                .to_uv();
            let move_pos = cursor.position_in(bounds)
                .unwrap()
                .to_yup(height);

            match event {
                Event::Keyboard(e)=>{
                    // let movement = (1.0 / self.view.scale.sqrt()).min(5.0);
                    let movement = 1.0;
                    let id = match state {
                        State::Select(id, _)=>*id,
                        State::OrderEditSelect(id)=>match e {
                            KeyboardEvent::KeyPressed{key:Key::Named(NamedKey::Enter|NamedKey::Space),..}=>{
                                eprintln!("Add {id:?} as index {}", self.order.len());

                                let id = *id;
                                *state = State::OrderEdit;
                                if self.order.len() == self.entities.len() - 1 {
                                    *state = State::Select(id, move_pos);
                                    return (Status::Captured, Some(SheetMessage::FinishOrder(id)));
                                } else {
                                    return (Status::Captured, Some(SheetMessage::AddToOrder(id)));
                                }
                            },
                            _=>return (Status::Ignored, None),
                        },
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
                                Some(SheetMessage::Move(id, Vector::new(0.0, movement))),
                            ),
                            NamedKey::ArrowDown=>return (
                                Status::Captured,
                                Some(SheetMessage::Move(id, Vector::new(0.0, -movement))),
                            ),
                            NamedKey::Delete=>{
                                *state = State::None(move_pos);
                                return (
                                    Status::Captured,
                                    Some(SheetMessage::Delete(id)),
                                );
                            },
                            NamedKey::Escape=>{
                                *state = State::None(move_pos);
                                return (
                                    Status::Captured,
                                    Some(SheetMessage::Deselect(id)),
                                );
                            },
                            _=>{},
                        },
                        _=>{},
                    }
                },
                Event::Mouse(e)=>{
                    match e {
                        MouseEvent::ButtonPressed(MouseButton::Left)=>{
                            let mut fallback_id = None;
                            let mut found_id = None;

                            let mut rc = self.recent_clicks.borrow_mut();

                            let mut cleared = None;

                            for (id, (model, mt)) in self.entities.iter() {
                                // let mut model_tr = mt.transform;
                                // model_tr.append_similarity(self.view);
                                // let inv_model_view = model_tr.inversed();
                                // let mut model_point = inv_model_view
                                //     .transform_vec(cursor_pos)
                                //     .to_ydown(height);

                                // let view_point = inv_view.transform_vec(move_pos);
                                let mut view_point = move_pos;
                                let t = self.world.translation;

                                view_point.x = view_point.x - t.x;
                                view_point.y = view_point.y - t.y;

                                view_point /= self.world.scale;

                                let inv_model = mt.transform.inversed();
                                let mut model_point = inv_model.transform_vec(view_point);

                                // dbg!(
                                //     self.world.translation,
                                //     self.view.translation,
                                //     self.world.scale,
                                //     move_pos,
                                //     cursor_pos,
                                //     view_point,
                                //     model_point,
                                // );
                                // eprintln!();

                                if mt.flip {
                                    model_point.y *= -1.0;
                                }

                                if model.point_within(model_point) {
                                    match state {
                                        State::Select(id2, _)|State::DelaySelect(id2, ..)|State::OrderEditSelect(id2)=>{
                                            if id == id2 || rc.contains(id) {
                                                eprintln!("Click fallback {id:?}");
                                                fallback_id = Some(*id);
                                            } else {
                                                if found_id.is_none() {
                                                    found_id = Some(*id);
                                                }
                                            }
                                        },
                                        _=>{
                                            if found_id.is_none() {
                                                found_id = Some(*id);
                                            }
                                        },
                                    }
                                } else {
                                    match state {
                                        State::Select(id2, _)|State::DelaySelect(id2, ..)=>{
                                            eprintln!("Missed selected entity {id2:?}");
                                            if id == id2 {
                                                eprintln!("Cleared {id2:?}");
                                                cleared = Some(*id2);
                                                *state = State::None(move_pos);
                                            }
                                        },
                                        State::OrderEditSelect(id2)=>{
                                            if id == id2 {
                                                eprintln!("Cleared {id2:?}");
                                                cleared = Some(*id);
                                                *state = State::OrderEdit;
                                            }
                                        },
                                        _=>{},
                                    }
                                }
                            }

                            if fallback_id.is_some() && found_id.is_none() {
                                eprintln!("Cycled all entities under cursor. Restarting.");
                                rc.clear();
                            }

                            if let Some(id) = found_id.or(fallback_id) {
                                eprintln!("Select and start move {id:?}");
                                rc.insert(id);
                                match state {
                                    State::Select(current_id, ..) if fallback_id.is_some()=>{
                                        eprintln!("Delay selection incase of move");
                                        *state = State::DelaySelect(*current_id, id, move_pos);
                                        return (Status::Captured, None);
                                    },
                                    State::OrderEdit|State::OrderEditSelect(_)=>{
                                        eprintln!("Order Edit Select");
                                        *state = State::OrderEditSelect(id);
                                        return (Status::Captured, Some(SheetMessage::Select(id)));
                                    },
                                    _=>{
                                        *state = State::Move(id, move_pos);
                                        return (Status::Captured, Some(SheetMessage::Select(id)));
                                    },
                                }
                            }

                            if let Some(id) = cleared {
                                match state {
                                    State::OrderEdit|State::OrderEditSelect(_)=>{
                                        eprintln!("Deselect {id:?}");
                                        *state = State::OrderEdit;
                                        return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                    },
                                    State::OrderEditPan(..)|State::OrderEditPanSelect(..)=>{
                                        eprintln!("Deselect {id:?}");
                                        *state = State::OrderEditPan(cursor_pos, move_pos);
                                        return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                    },
                                    _=>{
                                        eprintln!("Deselect {id:?}");
                                        *state = State::None(move_pos);
                                        return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                    },
                                }
                            }
                            match state {
                                State::OrderEditSelect(id)=>{
                                    let id = *id;
                                    eprintln!("Deselect {id:?}");
                                    *state = State::OrderEdit;
                                    return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                },
                                State::OrderEditPanSelect(id, ..)=>{
                                    let id = *id;
                                    eprintln!("Deselect {id:?}");
                                    *state = State::OrderEditPan(cursor_pos, move_pos);
                                    return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                },
                                State::Select(id, _)|State::DelaySelect(id, ..)=>{
                                    let id = *id;
                                    eprintln!("Deselect {id:?}");
                                    *state = State::None(move_pos);
                                    return (Status::Captured, Some(SheetMessage::Deselect(id)));
                                },
                                _=>{},
                            }

                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonReleased(MouseButton::Left)=>{
                            match state {
                                State::Move(id, _)=>{
                                    eprintln!("Stop move {id:?}");
                                    *state = State::Select(*id, move_pos);
                                    return (Status::Captured, None);
                                },
                                State::DelaySelect(_, id, _)=>{
                                    eprintln!("Stop delayed select {id:?}");
                                    let id = *id;
                                    *state = State::Select(id, move_pos);
                                    return (Status::Captured, Some(SheetMessage::Select(id)));
                                },
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonPressed(MouseButton::Right)=>{
                            match state {
                                State::Select(id, _)=>{
                                    eprintln!("Start pan with selection {id:?}");
                                    *state = State::PanSelected(*id, cursor_pos, move_pos);
                                },
                                State::None(_)=>{
                                    *state = State::Pan(cursor_pos, move_pos);
                                    eprintln!("Start pan");
                                },
                                State::OrderEdit=>*state = State::OrderEditPan(cursor_pos, move_pos),
                                State::OrderEditSelect(id)=>*state = State::OrderEditPanSelect(*id, cursor_pos, move_pos),
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonReleased(MouseButton::Right)=>{
                            match state {
                                State::Pan(_, _)=>{
                                    *state = State::None(move_pos);
                                    eprintln!("Stop pan");
                                },
                                State::PanSelected(id, _, _)=>{
                                    eprintln!("Stop pan with selection {id:?}");
                                    *state = State::Select(*id, move_pos);
                                },
                                State::OrderEditPan(..)=>*state = State::OrderEdit,
                                State::OrderEditPanSelect(id, ..)=>*state = State::OrderEditSelect(*id),
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::CursorMoved{..}=>{
                            match state {
                                State::Pan(prev, w_prev)|
                                    State::PanSelected(_, prev, w_prev)|
                                    State::OrderEditPan(prev, w_prev)|
                                    State::OrderEditPanSelect(_, prev, w_prev)=>{
                                    let delta = cursor_pos - *prev;
                                    *prev = cursor_pos;

                                    let w_delta = move_pos - *w_prev;
                                    *w_prev = move_pos;

                                    if delta.mag_sq() >= 8.0 {
                                        self.recent_clicks.borrow_mut().clear();
                                    }

                                    return (
                                        Status::Captured,
                                        Some(SheetMessage::Pan(delta, w_delta)),
                                    );
                                },
                                State::Move(id, prev)|State::DelaySelect(id, _, prev)=>{
                                    let id = *id;
                                    let delta = move_pos - *prev;

                                    if delta.mag_sq() >= 8.0 {
                                        self.recent_clicks.borrow_mut().clear();
                                    }

                                    match state {
                                        State::DelaySelect(..)=>{
                                            *state = State::Move(id, move_pos);
                                            return (
                                                Status::Captured,
                                                Some(SheetMessage::SelectMove(id, delta)),
                                            );
                                        },
                                        _=>{
                                            *state = State::Move(id, move_pos);
                                            return (
                                                Status::Captured,
                                                Some(SheetMessage::Move(id, delta)),
                                            );
                                        },
                                    }
                                },
                                State::Select(_, prev)|State::None(prev)=>{
                                    let delta = move_pos - *prev;
                                    *prev = move_pos;
                                    if delta.mag_sq() >= 8.0 {
                                        self.recent_clicks.borrow_mut().clear();
                                    }
                                },
                                State::OrderEdit|State::OrderEditSelect(_)=>{},
                            }
                        },
                        MouseEvent::WheelScrolled{delta:ScrollDelta::Lines{y,..}}=>{
                            let msg = if y > 0.0 {
                                SheetMessage::ZoomIn(cursor_pos, move_pos)
                            } else {
                                SheetMessage::ZoomOut(cursor_pos, move_pos)
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
