use dxf::{
    entities::EntityType,
    Drawing,
};
use geo::{
    MultiPolygon,
    Coord,
    LineString,
    Polygon,
    Contains,
    Area,
    ConvexHull,
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
    cmp::PartialOrd,
    ops::Deref,
    rc::Rc,
    sync::Arc,
    path::Path as StdPath,
    result::Result as StdResult,
};
use crate::{
    laser::{
        Condition,
        SequenceItem as Seq,
    },
    sheet::EntityState,
    utils::*,
    gcode::*,
    Point,
    Rotation,
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


/// A closed shape with one polygon or more polygons that may have holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    parts: MultiPolygon,
    hull: Polygon,
    pub min: Point,
    pub max: Point,
}
impl Shape {
    /// Creates a clockwise circle
    #[allow(unused)]
    pub fn circle(r: f64, min_points: usize, max_dist: f64)->Self {
        let mut line = LineString::from(
            ArcToPoints::new_circle(r, min_points, max_dist, true)
                .map(|p|p.to_geo())
                .collect::<Vec<_>>()
        );
        line.close();

        let outline = Polygon::new(line, Vec::new());

        return Self {
            parts: outline.clone().into(),
            hull: outline,
            min: Point::new(-r, -r),
            max: Point::new(r, r),
        };
    }

    /// NOTE: We sort the lines by area, so holes are more likely to be put into an outline instead
    /// of by themselves. We also assume the outline has a larger area than its holes, which makes
    /// sense.
    pub fn from_lines(lines: Vec<LineString>)->Self {
        let mut min = Point::new(f64::MAX, f64::MAX);
        let mut max = Point::new(f64::MIN, f64::MIN);

        let mut polys = lines.into_iter()
            .map(|l|{
                let min_x = l.coords()
                    .map(|c|c.x)
                    .min_by(|a,b|a.partial_cmp(b).unwrap())
                    .unwrap();
                let min_y = l.coords()
                    .map(|c|c.y)
                    .min_by(|a,b|a.partial_cmp(b).unwrap())
                    .unwrap();

                let max_x = l.coords()
                    .map(|c|c.x)
                    .max_by(|a,b|a.partial_cmp(b).unwrap())
                    .unwrap();
                let max_y = l.coords()
                    .map(|c|c.y)
                    .max_by(|a,b|a.partial_cmp(b).unwrap())
                    .unwrap();

                min.x = min.x.min(min_x);
                min.y = min.y.min(min_y);

                max.x = max.x.max(max_x);
                max.y = max.y.max(max_y);

                let p = Polygon::new(l, Vec::new());
                let a = p.unsigned_area();
                (p, a)
            })
            .collect::<Vec<_>>();

        polys.sort_by(|(_, a1), (_, a2)|a1.partial_cmp(a2).unwrap());

        let (largest_idx, _) = polys.iter()
            .map(|(_, area)|*area)
            .enumerate()
            .min_by(|(_, a1), (_, a2)|a1.partial_cmp(a2).unwrap())
            .unwrap();

        let mut top_level = vec![polys.remove(largest_idx).0];

        'poly_iter:for (poly, _) in polys {
            for outline in top_level.iter_mut() {
                if outline.contains(&poly) {
                    let line = poly.into_inner().0;
                    outline.interiors_push(line);

                    continue 'poly_iter;
                }
            }

            top_level.push(poly);
        }

        let parts = MultiPolygon::new(top_level);

        let hull = parts.convex_hull();

        return Shape {
            parts,
            hull,
            min,
            max,
        };
    }

    #[allow(unused)]
    pub fn aabb(&self)->Polygon {
        Polygon::new(LineString::new(vec![
            Coord {
                x: self.min.x,
                y: self.min.y,
            },
            Coord {
                x: self.min.x,
                y: self.min.y,
            },
            Coord {
                x: self.min.x,
                y: self.min.y,
            },
            Coord {
                x: self.min.x,
                y: self.min.y,
            },
        ]), Vec::new())
    }
}

/// A model loaded from a DXF. We take in a list of lines from the DXF and process it to extract
/// the outline and AABB. Once created, nothing can change. Transforms are stored externally.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    shape: Shape,
    pub name: String,
}
impl Model {
    /// Load a new model from a file path. See [`Model::new`] and [`load_model`] for more information.
    pub fn load<P: AsRef<StdPath>>(path: P)->Result<Self> {
        load_model(path)
    }

    /// Create a new model from a list of lines. The largest one is assumed to be the outline. Each
    /// other line is tested to see if it contains the other line, then they are inserted as holes.
    fn new(lines: Vec<LineString>, name: String)->Self {
        let shape = Shape::from_lines(lines);

        Model {
            shape,
            name,
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

        for (i, seq) in laser_condition.sequence.iter().enumerate() {
            let passes_str = if seq.passes() > 1 {"passes"} else {"pass"};
            match seq {
                Seq::GrblConst{passes, feed, power}|Seq::GrblDyn{passes, feed, power}=>{
                    builder.comment_block(format!(
                        "- Begin GRBL sequence {} with {} {passes_str} at {}mm/min and {}% power",
                        i + 1,
                        passes,
                        feed,
                        (*power as f32) / 10.0,
                    ));
                },
                Seq::Custom{passes, ..}=>{
                    builder.comment_block(format!(
                        "- Begin Custom sequence {} with {} {passes_str}",
                        i + 1,
                        passes,
                    ));
                },
            }

            for pass in 0..seq.passes() {
                builder.comment_block(format!("-- Begin pass {}", pass + 1));

                self.generate_gcode_lines(builder, mt, &seq);
            }
        }

        builder.comment_block(format!("End model `{}`", self.name));
    }

    fn lines_iter(&self)->impl Iterator<Item = &LineString> {
        self.shape.parts.iter()
            .map(|p|{
                let ext = p.exterior();
                let int_iter = p.interiors()
                    .iter();
                std::iter::once(ext)
                    .chain(int_iter)
            })
            .flatten()
    }

    /// For each line we move to the start, turn on the laser, set the power and feedrate, perform
    /// the cutting motion, turn off the laser, and repeat.
    fn generate_gcode_lines(&self, builder: &mut GcodeBuilder, mt: &EntityState, seq: &Seq) {
        let iter = self.lines_iter().enumerate();

        for (i, line) in iter {
            builder.comment_block(format!("--- Start line {i}"));

            // create an iterator of the points and transform them
            let mut points_iter = line.coords()
                .map(|p|mt.transform(p.to_uv()));

            let start = points_iter.next().unwrap();
            builder.rapid_motion()
                .x(start.x)
                .y(start.y)
                .eob();

            match seq {
                Seq::GrblConst{power, feed, ..}=>{
                    builder.cutting_motion()
                        .laser_power(*power)
                        .feed(*feed)
                        .laser_on_const()
                        .eob();
                },
                Seq::GrblDyn{power, feed, ..}=>{
                    builder.cutting_motion()
                        .laser_power(*power)
                        .feed(*feed)
                        .laser_on_dyn()
                        .eob();
                },
                Seq::Custom{laser_on, feed, power, ..}=>{
                    builder
                        .custom(power.clone())
                        .custom(feed.clone())
                        .eob();

                    builder
                        .custom(laser_on.clone())
                        .eob();
                },
            }

            for point in points_iter {
                builder.cutting_motion()
                    .x(point.x)
                    .y(point.y)
                    .eob();
            }

            match seq {
                Seq::GrblConst{..}|Seq::GrblDyn{..}=>{
                    builder.cutting_motion()
                        .laser_power(0)
                        .laser_off()
                        .eob();
                },
                Seq::Custom{laser_off, ..}=>{
                    builder.custom(laser_off.clone())
                        .eob();
                },
            }
        }
    }

    /// Check if a point is within the outline of this model.
    /// We assume the given point is in model space and any transforms are performed prior to
    /// receiving it.
    pub fn point_within(&self, point: Point)->bool {
        let x_bb = point.x >= self.shape.min.x && point.x <= self.shape.max.x;
        let y_bb = point.y >= self.shape.min.y && point.y <= self.shape.max.y;
        if !(x_bb && y_bb) {
            return false;
        }

        return self.shape.hull.contains(&Coord{x:point.x,y:point.y});
    }

    /// Build the [`iced::Path`]s from this model and a transform.
    /// TODO(optimization): Reuse built paths and transform them instead of creating new ones every
    /// time.
    pub fn paths(&self, mt: EntityState, height: f64)->ModelPaths {
        let mut paths = Vec::new();

        let mut min = Point::new(f64::MAX, f64::MAX);
        let mut max = Point::new(-f64::MAX, -f64::MAX);

        for line in self.lines_iter() {
            // build the line based on the points
            let mut builder = PathBuilder::new();
            let mut points_iter = line.coords()
                .copied()
                .map(|p|{
                    let p = mt.transform(p.to_uv());
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);

                    p.to_ydown(height).to_iced()
                });

            let start = points_iter.next().unwrap();
            builder.move_to(start);

            for point in points_iter {
                builder.line_to(point);
            }

            builder.close();

            paths.push(builder.build());
        }

        // Build the outline as a rectangle based on the AABB
        let mut builder = PathBuilder::new();
        builder.move_to(Point::new(min.x, min.y).to_ydown(height).to_iced());
        builder.line_to(Point::new(max.x, min.y).to_ydown(height).to_iced());
        builder.line_to(Point::new(max.x, max.y).to_ydown(height).to_iced());
        builder.line_to(Point::new(min.x, max.y).to_ydown(height).to_iced());
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
struct LineBuilder(Vec<Coord>);
impl LineBuilder {
    /// Try to add a segment to the line. If the first point in the segment is the same as the last
    /// point in the line, then add it. If not then return it in a `Result::Err`. This signals the
    /// caller to finish this line and start a new one.
    pub fn try_add(&mut self, seg: Segment)->StdResult<(), Segment> {
        let seg2 = (seg.0.to_geo(), seg.1.to_geo());
        if self.0.is_empty() {
            self.0.push(seg2.0);
            self.0.push(seg2.1);
        } else {
            let last = self.0.last().unwrap();
            if *last == seg2.0 {
                self.0.push(seg2.1);
            } else {
                return Err(seg);
            }
        }

        return Ok(());
    }

    /// Is it empty?
    pub fn is_empty(&self)->bool {self.0.is_empty()}

    /// Finish the line and determine if it is supposed to be open or closed.
    #[inline]
    pub fn finish(self)->LineString {
        LineString::new(self.0)
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

/// An iterator returning points along an arc. Might be a circle.
///
/// The points are returned in either clockwise or counter-clockwise order. The arc always starts
/// on Y=0, X=r and goes "up" or "down" depending on {counter,}-clockwise
///
/// Attempts to create an iterator of points with an equal spacing of about `max_dist`. If the
/// count is lower than `min_points`, then it will use that number of points.
///
/// NOTE: The `max_dist` uses the arc distance NOT the point-to-point distance to calculate the
/// point count.
pub struct ArcToPoints {
    start: Point,
    i: usize,
    points: usize,
    step: f64,
}
impl ArcToPoints {
    /// Minor optimization to make things slightly more accurate
    #[inline]
    pub fn new_circle(r: f64, min_points: usize, max_dist: f64, clockwise: bool)->Self {
        use std::f64::consts::TAU;

        Self::new_arc(r, min_points, max_dist, clockwise, TAU)
    }

    /// Returns if the arc is clockwise or not.
    #[inline]
    #[allow(unused)]
    pub fn is_clockwise(&self)->bool {
        self.step > 0.0
    }

    /// NOTE: Angle is in Radians
    pub fn new_arc(r: f64, min_points: usize, max_dist: f64, clockwise: bool, angle: f64)->Self {
        let clockwise = if clockwise {1.0} else {-1.0};

        let length = angle * r;
        let points = ((length / max_dist).ceil() as usize)
            .max(min_points);
        let step = (angle / (points as f64)) * clockwise;

        let start = Point{x: r, y: 0.0};

        return ArcToPoints {
            start,
            step,
            i: 0,
            points,
        };
    }
}
impl Iterator for ArcToPoints {
    type Item = Point;
    fn next(&mut self)->Option<Point> {
        if self.i == self.points {
            return None;
        }

        let angle = self.step * (self.i as f64);
        self.i += 1;
        let point = self.start.rotated(Rotation::from_angle(angle));

        return Some(point);
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
