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
use iced::widget::canvas::path::{
    Builder as PathBuilder,
    Path,
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
    hash::{
        Hash,
        Hasher,
    },
    cell::{
        RefCell,
        Ref,
    },
    ops::Deref,
    rc::Rc,
    sync::Arc,
    path::Path as StdPath,
    result::Result as StdResult,
};
use crate::{
    sheet::EntityState,
    laser::Condition,
    gcode::*,
    p_conv,
    Point,
};


/// Which axis is "up" in the model so we can rotate it.
enum ModelMode {
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

/// A line consisting of a list of points.
#[derive(Debug, Clone, PartialEq)]
enum Line {
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


/// A model loaded from a DXF. We take in a list of lines from the DXF and process it to extract
/// the outline and AABB. Once created, nothing can change. Transforms are stored externally.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    lines: Vec<Line>,
    outline: Polygon,
    pub name: String,
    pub segments: usize,
    pub min: Point,
    pub max: Point,
}
impl Model {
    /// Load a new model from a file path. See [`Model::new`] and [`load_model`] for more information.
    pub fn load<P: AsRef<StdPath>>(path: P)->Result<Self> {
        load_model(path)
    }

    /// Create a new model from a list of lines. The largest one is assumed to be the outline and
    /// all others are assumed to be inside of it.
    fn new(mut lines: Vec<Line>, name: String)->Self {
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
            name,
            lines,
            outline,
            segments,
            min,
            max,
        }
    }

    /// Generate the gcode for this model with the given transform, laser power, and feedrate.
    ///
    /// The generated code includes laser on const, laser off, and proper feeds and speeds for
    /// safety. After each line we set laser power to 0 and rapid move to the next line. After all
    /// lines are done, we turn the laser off.
    pub fn generate_gcode(&self, mt: &EntityState, builder: &mut GcodeBuilder, laser_condition: &Condition) {
        builder.comment_block(format!(
            "Start model `{}` with laser condition `{}` and {} sequence items",
            self.name,
            laser_condition.name,
            laser_condition.sequence.len(),
        ));

        builder.laser_on_const().eob();

        for (i, seq) in laser_condition.sequence.iter().enumerate() {
            let passes = if seq.passes > 1 {"passes"} else {"pass"};
            builder.comment_block(format!(
                "- Begin sequence {} with {} {passes} at {}mm/min and {}% power",
                i + 1,
                seq.passes,
                seq.feed,
                (seq.power as f32) / 10.0,
            ));

            for pass in 0..seq.passes {
                builder.comment_block(format!("-- Begin pass {}", pass + 1));

                self.generate_gcode_lines(builder, mt, seq.power, seq.feed);
            }
        }

        builder.laser_off().eob();

        builder.comment_block(format!("End model `{}`", self.name));
    }

    fn generate_gcode_lines(&self, builder: &mut GcodeBuilder, mt: &EntityState, laser_power: u16, feedrate: u16) {
        for (i, line) in self.lines.iter().enumerate() {
            if i == 0 {
                builder.cutting_motion()
                    .feed(feedrate)
                    .laser_power(0)
                    .eob();
            }

            builder.comment_block(format!("--- Start line {i}"));

            let (Line::Closed(points)|Line::Open(points)) = line;

            // create an iterator of the points and transform them
            let mut points_iter = points.iter()
                .map(|p|transform_point(*p, mt));

            let start = points_iter.next().unwrap();
            builder.rapid_motion()
                .x(start.x)
                .y(start.y)
                .eob();

            for point in points_iter {
                builder.cutting_motion()
                    .x(point.x)
                    .y(point.y)
                    .laser_power(laser_power)
                    .eob();
            }

            // close the line if it needs to be
            match line {
                Line::Closed(_)=>{
                    builder.cutting_motion()
                        .x(start.x)
                        .y(start.y)
                        .laser_power(laser_power)
                        .eob();
                },
                _=>{},
            }

            builder.cutting_motion()
                .laser_power(0)
                .eob();
        }
    }

    /// Get the center of the model based on extents.
    /// NOTE: This IS NOT center-of-mass.
    #[allow(unused)]
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
    pub fn paths(&self, mt: EntityState)->ModelPaths {
        let mut paths = Vec::with_capacity(self.lines.len());

        let mut min = Point::new(999999999.0,999999999.0);
        let mut max = Point::new(-99999999.0,-99999999.0);

        for line in self.lines.iter() {
            let (Line::Closed(points)|Line::Open(points)) = line;

            // build the line based on the points
            let mut builder = PathBuilder::new();
            let mut points_iter = points.iter()
                .copied()
                .map(|p|{
                    let p = transform_point(p, &mt);
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);

                    p_conv(p)
                });

            let start = points_iter.next().unwrap();
            builder.move_to(start);

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
        let outline_min = min;
        let outline_max = max;

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
struct LineBuilder(Vec<Point>);
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

/// The [`iced::Path`]s created from a [`Model`].
pub struct ModelPaths {
    pub outline: Path,
    pub lines: Vec<Path>,
}

/// The ID of a [`Model`] stored in a [`ModelStore`].
#[derive(Debug, Clone)]
pub struct ModelHandle(pub usize, Arc<Model>);
impl ModelHandle {
    pub fn name(&self)->&str {
        self.1.name.as_str()
    }
}
impl Deref for ModelHandle {
    type Target = Model;
    fn deref(&self)->&Model {
        &self.1
    }
}
impl Eq for ModelHandle {}
impl PartialEq for ModelHandle {
    fn eq(&self, other: &Self)->bool {
        self.0 == other.0
    }
}
impl Hash for ModelHandle {
    fn hash<H: Hasher>(&self, h: &mut H) {
        h.write_usize(self.0);
    }
}
impl Display for ModelHandle {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        self.1.name.fmt(f)
    }
}

/// Encapsulate immutable state models in a struct that disallows mutation, but does allow adding
/// more models when required.
///
/// When cloned, this refers to the same model store. It is cheap to clone being just an
/// `Rc<RefCell>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelStore(Rc<RefCell<Vec<Arc<Model>>>>);
impl ModelStore {
    pub fn new()->Self {
        ModelStore(Rc::new(RefCell::new(Vec::new())))
    }

    /// Add a model to the store and return its ID.
    pub fn add(&self, model: Model)->ModelHandle {
        let mut models = self.0.borrow_mut();
        let model = Arc::new(model);
        let id = ModelHandle(models.len(), model.clone());
        models.push(model);
        return id;
    }

    /// How many models do we have stored?
    #[allow(unused)]
    pub fn count(&self)->usize {self.0.borrow().len()}

    /// Create an iterator over all the models
    pub fn iter<'a>(&'a self)->ModelIter<'a> {
        ModelIter(0, self.0.borrow())
    }
}
pub struct ModelIter<'a>(usize, Ref<'a, Vec<Arc<Model>>>);
impl<'a> ExactSizeIterator for ModelIter<'a> {}
impl<'a> Iterator for ModelIter<'a> {
    type Item = ModelHandle;

    fn size_hint(&self)->(usize, Option<usize>) {
        let len = self.1.len() - self.0;

        (len, Some(len))
    }
    fn next(&mut self)->Option<ModelHandle> {
        if self.0 == self.1.len() {
            return None;
        }

        let idx = self.0;
        self.0 += 1;

        let model = self.1[idx].clone();

        Some(ModelHandle(idx, model))
    }
}


fn load_model<P: AsRef<StdPath>>(path: P)->Result<Model> {
    let path = path.as_ref();
    let name = path.file_stem()
        .expect("File does not have a name")
        .to_str()
        .expect("File name is not valid UTF-8");
    let drawing = Drawing::load_file(path)?;

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

    return Ok(Model::new(lines, name.into()));
}

fn transform_point(mut point: Point, mt: &EntityState)->Point {
    if mt.flip {
        point.y *= -1.0;
    }

    mt.transform.transform_vec(point)
}
