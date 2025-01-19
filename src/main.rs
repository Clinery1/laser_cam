use dxf::{
    entities::EntityType,
    Drawing,
};
use geo::{
    Coord,
    LineString,
    Polygon,
    Contains,
};
use iced::{
    widget::canvas::{
        event::{
            Event,
            Status,
        },
        path::{
            Builder as PathBuilder,
            Path,
        },
        Cache,
        Program as CanvasProgram,
        Canvas,
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
use anyhow::{
    Result,
    bail,
};
use std::{
    fmt::{
        Display,
        Formatter,
        Result as FmtResult,
    },
    result::Result as StdResult,
};


pub type Point = ultraviolet::DVec2;
pub type Vector = ultraviolet::DVec2;
pub type Rotation = ultraviolet::DRotor2;
pub type Transform = ultraviolet::DSimilarity2;
pub type Translation = ultraviolet::DVec2;


/// Which axis is "up" in the model so we can rotate it.
pub enum ModelMode {
    ZUp,
    XUp,
    YUp,
}

#[derive(Debug)]
pub enum ModelLoadError {
    /// The model is not in an axis-aligned plane. We only accept models that are in either the XY,
    /// XZ, or YZ planes.
    ModelNotInPlane,
}
impl std::error::Error for ModelLoadError {}
impl Display for ModelLoadError {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        use ModelLoadError::*;
        match self {
            ModelNotInPlane=>write!(f,"The model is not in one of the XY, XZ, or YZ planes."),
        }
    }
}

/// State changes that can occur to entities
#[derive(Debug)]
pub enum ModelCanvasMessage {
    /// Recalculate the paths. We might have deselected something or changed the selection or
    /// something else.
    RecalcPaths,
    /// An amount to pan relative to the previous position.
    Pan(Translation),
    /// An amount to move an entity and its index.
    Move(usize, Translation),
    /// Contains the the cursor position.
    ZoomIn(Point),
    /// Contains the the cursor position.
    ZoomOut(Point),
}

/// Main program state changes
#[derive(Debug)]
pub enum Message {
    ModelCanvas(ModelCanvasMessage),
}

/// A line consisting of a list of points.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// The list of points is assumed to be closed, and the last item IS NOT the same as the first.
    /// There is an implied line from the last point to the first when drawing.
    Closed(Vec<Point>),
    /// A list of points creating a line. This is assumed open.
    Open(Vec<Point>),
}
impl Line {
    /// Get a point with the given index.
    pub fn point(&self, idx: usize)->Point {
        match self {
            Self::Closed(pts)|Self::Open(pts)=>pts[idx],
        }
    }

    /// Get the list of points.
    pub fn points(&self)->&[Point] {
        match self {
            Self::Closed(pts)|Self::Open(pts)=>&pts,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub enum ModelCanvasState {
    /// Where the previous mouse position is located and the index of the model
    Select(usize),
    /// An amount to move a model and its index.
    Move(usize, Point),

    /// Pan with an entity selected.
    PanSelected(usize, Point),

    /// Pan the screen.
    Pan(Point),

    /// Do nothing
    #[default]
    None,
}


/// A model loaded from a DXF. We take in a list of lines from the DXF and process it to extract
/// the outline and AABB. Once created, nothing can change. Transforms are stored externally.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub lines: Vec<Line>,
    pub outline: Polygon,
    pub segments: usize,
    pub min: Point,
    pub max: Point,
}
impl Model {
    /// Create a new model from a list of lines. The largest one is assumed to be the outline and
    /// all others are assumed to be inside of it.
    pub fn new(mut lines: Vec<Line>)->Self {
        let mut segments = 0;
        let mut max = lines[0].point(0);
        let mut min = lines[0].point(0);

        let mut outline_idx = 0;

        for (i, line) in lines.iter().enumerate() {
            segments += line.points().len();
            for point in line.points().iter() {
                if max.x < point.x {
                    outline_idx = i;
                }

                max.x = max.x.max(point.x);
                max.y = max.y.max(point.y);

                min.x = min.x.min(point.x);
                min.y = min.y.min(point.y);
            }
        }

        // Ensure the outline is the first item
        if outline_idx != 0 {
            lines.swap(0, outline_idx);
        }

        let outline = lines[0].points()
            .into_iter()
            .rev()
            .map(|p|Coord{x:p.x,y:p.y})
            .collect::<Vec<_>>();
        let outline = Polygon::new(LineString::new(outline), Vec::new());

        Model {
            lines,
            outline,
            segments,
            min,
            max,
        }
    }

    /// Get the center of the model based on extents.
    /// NOTE: This IS NOT center-of-mass.
    pub fn center(&self)->Point {
        let extents = self.max - self.min;

        return self.min + (extents / 2.0);
    }

    /// Check if a point is within the outline of this model.
    /// We assume the given point is in model space and any transforms are performed prior to
    /// receiving it.
    pub fn point_within(&self, point: Point)->bool {
        let x_bb = point.x >= self.min.x && point.x <= self.max.x;
        let y_bb = point.y >= self.min.y && point.y <= self.max.y;
        if !(x_bb && y_bb) {
            return false;
        }

        return self.outline.contains(&Coord{x:point.x,y:point.y});
    }

    /// Build the [`iced::Path`]s from this model and a transform.
    /// TODO(optimization): Reuse built paths and transform them instead of creating new ones every
    /// time.
    pub fn paths(&self, mt: EntityTransform)->ModelPaths {
        let mut paths = Vec::with_capacity(self.lines.len());

        for line in self.lines.iter() {
            let (Line::Closed(points)|Line::Open(points)) = line;

            // build the line based on the points
            let mut builder = PathBuilder::new();
            let mut points_iter = points.iter()
                .copied()
                .map(|mut p|{
                    if mt.flip {p.y *= -1.0}
                    p_conv(mt.transform.transform_vec(p))
                });

            builder.move_to(points_iter.next().unwrap());

            for point in points_iter {
                builder.line_to(point);
            }

            // If its a closed line, then ensure its closed
            match line {
                Line::Closed(_)=>builder.close(),
                _=>{},
            }

            paths.push(builder.build());
        }

        // build the outline
        let mut outline_min = self.min;
        let mut outline_max = self.max;

        if mt.flip {
            outline_max.y *= -1.0;
            outline_min.y *= -1.0;
        }

        let outline_min = mt.transform.transform_vec(outline_min);
        let outline_max = mt.transform.transform_vec(outline_max);

        // Build the outline as a rectangle based on the AABB
        let mut builder = PathBuilder::new();
        builder.move_to(p_conv(Point::new(outline_min.x, outline_min.y)));
        builder.line_to(p_conv(Point::new(outline_max.x, outline_min.y)));
        builder.line_to(p_conv(Point::new(outline_max.x, outline_max.y)));
        builder.line_to(p_conv(Point::new(outline_min.x, outline_max.y)));
        builder.close();

        let ret = ModelPaths {
            outline: builder.build(),
            lines: paths,
        };

        return ret;
    }
}

/// An easy way to build lines and make sure the internal state is correct.
#[derive(Debug, Default)]
pub struct LineBuilder(Vec<Point>);
impl LineBuilder {
    /// Try to add a segment to the line. If the first point in the segment is the same as the last
    /// point in the line, then add it. If not then return it in a `Result::Err`. This signals the
    /// caller to finish this line and start a new one.
    pub fn try_add(&mut self, seg: Segment)->StdResult<(), Segment> {
        if self.0.is_empty() {
            self.0.push(seg.0);
            self.0.push(seg.1);
        } else {
            let last = self.0.last().unwrap();
            if *last == seg.0 {
                self.0.push(seg.1);
            } else {
                return Err(seg);
            }
        }

        return Ok(());
    }

    /// Is it empty?
    pub fn is_empty(&self)->bool {self.0.is_empty()}

    /// Finish the line and determine if it is supposed to be open or closed.
    pub fn finish(mut self)->Line {
        if self.0.is_empty() {
            return Line::Open(self.0);
        } else {
            let first = self.0.first().unwrap();
            let last = self.0.last().unwrap();

            if first == last {
                self.0.pop();
                return Line::Closed(self.0);
            } else {
                return Line::Open(self.0);
            }
        }
    }
}

/// A line segment made of two points.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Segment(pub Point, pub Point);

/// An entity's transform and if it is flipped. This only flips it in the Y axis.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EntityTransform {
    pub transform: Transform,
    pub flip: bool,
}

/// The [`iced::Path`]s created from a [`Model`].
pub struct ModelPaths {
    pub outline: Path,
    pub lines: Vec<Path>,
}

/// The ID of a [`Model`] stored in a [`ModelStore`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ModelId(pub usize);

/// Encapsulate immutable state models in a struct that disallows mutation, but does allow adding
/// more models when required.
pub struct ModelStore(Vec<Model>);
impl ModelStore {
    pub fn new()->Self {
        ModelStore(Vec::new())
    }

    /// Add a model to the store and return its ID.
    pub fn add(&mut self, model: Model)->ModelId {
        let id = ModelId(self.0.len());
        self.0.push(model);
        return id;
    }

    /// Get the model with the given ID.
    pub fn get(&self, id: ModelId)->&Model {
        &self.0[id.0]
    }

    /// How many models do we have stored?
    pub fn count(&self)->usize {self.0.len()}
}

/// A sheet to nest the models in. Has a sheet size to display an outline and handles displaying
/// all instances of a model.
pub struct Sheet {
    models: ModelStore,
    entities: Vec<(ModelId, EntityTransform)>,
    paths: Vec<ModelPaths>,
    cached_models: Vec<Cache>,
    view: Transform,
    sheet_size: Vector,
    sheet_cache: Cache,
}
impl Sheet {
    pub fn new()->Self {
        Sheet {
            models: ModelStore::new(),
            entities: Vec::new(),
            paths: Vec::new(),
            cached_models: Vec::new(),
            view: Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0),
            sheet_size: Vector::new(300.0, 300.0),
            sheet_cache: Cache::new(),
        }
    }

    #[inline]
    pub fn add_model(&mut self, path: &str, qty: usize)->Result<()> {
        let transform = Transform::new(Translation::zero(), Rotation::from_angle(0.0), 1.0);

        self.add_model_with_transform(path, transform, false, qty)
    }

    pub fn add_model_with_transform(&mut self, path: &str, transform: Transform, flip: bool, qty: usize)->Result<()> {
        let model = load_model(path)?;

        let id = self.models.add(model);
        let model = self.models.get(id);

        let mut mt = EntityTransform {
            transform,
            flip,
        };

        for _ in 0..qty {
            self.entities.push((id, mt));
            self.paths.push(model.paths(mt));
            self.cached_models.push(Cache::new());
            mt.transform.translation += Point::new(5.0, 5.0);
        }
        
        return Ok(());
    }

    pub fn main_view(&self)->Element<ModelCanvasMessage> {
        Canvas::new(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn main_update(&mut self, msg: ModelCanvasMessage)->Task<ModelCanvasMessage> {
        match msg {
            ModelCanvasMessage::RecalcPaths=>self.recalc_paths(),
            ModelCanvasMessage::Move(idx, delta)=>{
                self.entities[idx].1.transform.translation += delta / self.view.scale;
                self.recalc_paths();
            },
            ModelCanvasMessage::Pan(delta)=>{
                self.view.translation += delta;
                self.recalc_paths();
            },
            ModelCanvasMessage::ZoomIn(mouse_pos)=>{
                const ZOOM: f64 = 1.1;

                let mouse_offset = self.view.translation - mouse_pos;
                let offset = (mouse_offset * ZOOM) - mouse_offset;
                self.view.translation += offset;

                self.view.scale *= ZOOM;

                self.recalc_paths();
            },
            ModelCanvasMessage::ZoomOut(mouse_pos)=>{
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

    pub fn recalc_paths(&mut self) {
        self.paths.clear();
        self.cached_models.iter().for_each(Cache::clear);
        self.sheet_cache.clear();
        for (model_id, mt) in self.entities.iter() {
            let model = self.models.get(*model_id);
            let mut mt = *mt;
            mt.transform.append_similarity(self.view);
            self.paths.push(model.paths(mt));
        }
    }
}
impl CanvasProgram<ModelCanvasMessage> for Sheet {
    type State = ModelCanvasState;

    fn draw(&self,
        state: &ModelCanvasState,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<<Renderer as GeometryRenderer>::Geometry> {
        let color = theme.palette().text;
        let outline_color = theme.palette().danger;
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
        for (i, (cache, paths)) in self.cached_models.iter().zip(self.paths.iter()).enumerate() {
            ret.push(cache.draw(
                renderer,
                Size {
                    width: bounds.width,
                    height: bounds.height,
                },
                |frame|{
                    use ModelCanvasState as State;

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
                            if i == *idx {
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

    fn update(
        &self,
        state: &mut Self::State,
        event: Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> (Status, Option<ModelCanvasMessage>) {
        use ModelCanvasState as State;
        if cursor.is_over(bounds) {
            let cursor_pos = cursor.position_in(bounds).unwrap();
            let cursor_pos = Point::new(cursor_pos.x as f64, cursor_pos.y as f64);
            match event {
                Event::Mouse(e)=>{
                    match e {
                        MouseEvent::ButtonPressed(MouseButton::Left)=>{
                            let mouse_offset = cursor_pos;
                            for (i, (model_id, mt)) in self.entities.iter().enumerate() {
                                let model = self.models.get(*model_id);
                                let mut model_tr = mt.transform;
                                model_tr.append_similarity(self.view);
                                let inv_model_view = model_tr.inversed();
                                let mut model_point = inv_model_view.transform_vec(mouse_offset);

                                if mt.flip {
                                    model_point.y *= -1.0;
                                }

                                if model.point_within(model_point) {
                                    match state {
                                        State::Select(idx)=>if *idx == i {
                                            eprintln!("Start move {i}");
                                        } else {
                                            eprintln!("Select and start move {i}");
                                        },
                                        _=>eprintln!("Select and start move {i}"),
                                    }
                                    *state = State::Move(i, cursor_pos);
                                    return (Status::Captured, Some(ModelCanvasMessage::RecalcPaths));
                                }
                            }

                            match state {
                                State::Select(idx)=>{
                                    eprintln!("Deselect {idx}");
                                    *state = State::None;
                                    return (Status::Captured, Some(ModelCanvasMessage::RecalcPaths));
                                },
                                _=>{},
                            }

                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonReleased(MouseButton::Left)=>{
                            match state {
                                State::Move(idx,_ )=>{
                                    eprintln!("Stop move {idx}");
                                    *state = State::Select(*idx);
                                    return (Status::Captured, None);
                                },
                                _=>{},
                            }
                            return (Status::Captured, None);
                        },
                        MouseEvent::ButtonPressed(MouseButton::Right)=>{
                            match state {
                                State::Select(idx)=>{
                                    eprintln!("Start pan with selection {idx}");
                                    *state = State::PanSelected(*idx, cursor_pos);
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
                                State::PanSelected(idx, _)=>{
                                    eprintln!("Stop pan with selection {idx}");
                                    *state = State::Select(*idx);
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
                                        Some(ModelCanvasMessage::Pan(delta)),
                                    );
                                },
                                State::Move(idx, prev)=>{
                                    let idx = *idx;
                                    let delta = cursor_pos - *prev;
                                    *state = State::Move(idx, cursor_pos);

                                    return (
                                        Status::Captured,
                                        Some(ModelCanvasMessage::Move(idx, delta)),
                                    );
                                },
                                _=>{},
                            }
                        },
                        MouseEvent::WheelScrolled{delta:ScrollDelta::Lines{y,..}}=>{
                            let msg = if y > 0.0 {
                                eprintln!("Zoom in");
                                ModelCanvasMessage::ZoomIn(cursor_pos)
                            } else {
                                eprintln!("Zoom out");
                                ModelCanvasMessage::ZoomOut(cursor_pos)
                            };
                            return (Status::Captured, Some(msg));
                        },
                        _=>{},
                    }
                },
                _=>{},
            }
        } else {
            *state = State::None;
        }

        (Status::Ignored, None)
    }
}

pub struct MainProgram {
    canvas: Sheet,
}
impl MainProgram {
    pub fn view(&self)->Element<Message> {
        self.canvas.main_view()
            .map(|m|Message::ModelCanvas(m))
    }

    pub fn update(&mut self, msg: Message)->Task<Message> {
        match msg {
            Message::ModelCanvas(msg)=>self.canvas.main_update(msg)
                .map(|m|Message::ModelCanvas(m)),
        }
    }
}
impl Default for MainProgram {
    fn default()->Self {
        let mut canvas = Sheet::new();
        let transform = Transform::new(
            Translation::new(100.0, 100.0),
            Rotation::from_angle(0.0),
            1.0,
        );
        canvas.add_model_with_transform("sample_with_text.dxf", transform, true, 3).unwrap();

        MainProgram {
            canvas,
        }
    }
}


fn main()->iced::Result {
    iced::application(
        "LaserCAM",
        MainProgram::update,
        MainProgram::view,
    )
        .centered()
        .theme(|_|Theme::Dark)
        .run()
}

fn p_conv(uv: Point)->iced::Point {
    iced::Point {
        x: uv.x as f32,
        y: uv.y as f32,
    }
}

fn load_model(name: &str)->Result<Model> {
    let drawing = Drawing::load_file(name)?;

    let mut lines = Vec::new();

    let mut line_warning = false;
    let mut mode = ModelMode::ZUp;

    let mut line_builder = LineBuilder::default();

    for (i, entity) in drawing.entities().enumerate() {
        use ModelMode::*;

        let EntityType::Line(line)=&entity.specific else {line_warning=true;continue};

        if i==0 {
            let up = &line.extrusion_direction;
            if up.x == 1.0 {
                mode = XUp;
            } else if up.y == 1.0 {
                mode = YUp;
            } else if up.z == 1.0 {
                mode = ZUp;
            } else {
                bail!(ModelLoadError::ModelNotInPlane);
            }
        }

        let p1;
        let p2;

        match mode {
            ZUp=>{
                p1 = Point {
                    x: line.p1.x,
                    y: line.p1.y,
                };
                p2 = Point {
                    x: line.p2.x,
                    y: line.p2.y,
                };
            },
            XUp=>{
                p1 = Point {
                    x: line.p1.y,
                    y: line.p1.z,
                };
                p2 = Point {
                    x: line.p2.y,
                    y: line.p2.z,
                };
            },
            YUp=>{
                p1 = Point {
                    x: line.p1.x,
                    y: line.p1.z,
                };
                p2 = Point {
                    x: line.p2.x,
                    y: line.p2.z,
                };
            },
        }

        // Logic determining when we start a new line
        match line_builder.try_add(Segment(p1, p2)) {
            Err(seg)=>{
                lines.push(line_builder.finish());
                line_builder = LineBuilder::default();
                line_builder.try_add(seg).unwrap();
            },
            Ok(())=>{},
        }
    }
    
    if !line_builder.is_empty() {
        lines.push(line_builder.finish());
    }

    if line_warning {
        eprintln!("WARNING: We only support lines in DXF files. Anything else is IGNORED!");
    }

    return Ok(Model::new(lines));
}
