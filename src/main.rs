use dxf::{
    entities::EntityType,
    Drawing,
};
use anyhow::{
    Result,
    Error,
    anyhow,
    bail,
};
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
};


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


#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub lines: Vec<Line>,
    pub segments: usize,
    pub min: Point2,
    pub max: Point2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line(pub Vec<Segment>);

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Segment(pub Point2, pub Point2);

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}
impl Point2 {
    pub const fn zero()->Self {
        Point2 {
            x: 0.0,
            y: 0.0,
        }
    }

    pub const fn one()->Self {
        Point2 {
            x: 1.0,
            y: 1.0,
        }
    }
}


fn main() {
    let model = load_model("sample.dxf").unwrap();

    dbg!(model.lines.len(), model.segments, model.min, model.max);
    // load_model("sample_with_text.dxf");
}


fn load_model(name: &str)->Result<Model> {
    let drawing = Drawing::load_file(name)?;

    let mut model = Model {
        lines: Vec::new(),
        segments: 0,
        min: Point2::zero(),
        max: Point2::zero(),
    };

    let mut line_warning = false;
    let mut mode = ModelMode::ZUp;

    let mut current_line = Vec::new();
    let mut last_point = None;

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
                p1 = Point2 {
                    x: line.p1.x,
                    y: line.p1.y,
                };
                p2 = Point2 {
                    x: line.p2.x,
                    y: line.p2.y,
                };
            },
            XUp=>{
                p1 = Point2 {
                    x: line.p1.y,
                    y: line.p1.z,
                };
                p2 = Point2 {
                    x: line.p2.y,
                    y: line.p2.z,
                };
            },
            YUp=>{
                p1 = Point2 {
                    x: line.p1.x,
                    y: line.p1.z,
                };
                p2 = Point2 {
                    x: line.p2.x,
                    y: line.p2.z,
                };
            },
        }

        model.segments += 1;

        // Adjust the max value in the model with the values in p1 and p2
        model.max.x = model.max.x.max(p1.x.max(p2.x));
        model.max.y = model.max.y.max(p1.y.max(p2.y));

        // Do the same for the min values
        model.min.x = model.min.x.min(p1.x.min(p2.x));
        model.min.y = model.min.y.min(p1.y.min(p2.y));

        // Logic determining when we start a new line
        if let Some(last_point) = last_point {
            if p1 == last_point {
                current_line.push(Segment(p1, p2));
            } else {
                model.lines.push(Line(current_line));
                current_line = Vec::new();
            }
        }
        last_point = Some(p2);
    }
    
    if !current_line.is_empty() {
        model.lines.push(Line(current_line));
    }

    if line_warning {
        eprintln!("WARNING: We only support lines in DXF files. Anything else is IGNORED!");
    }

    return Ok(model);
}
