use smallvec::SmallVec;
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
    Write,
};
use GcodeInstruction as Ins;


#[derive(Debug, Clone, PartialEq)]
pub enum GcodeInstruction {
    G(u16),
    S(u16),
    M(u16),
    F(u16),
    X(f64),
    Y(f64),
    Custom(String),
}
impl Display for GcodeInstruction {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        use GcodeInstruction::*;
        match self {
            G(n)=>write!(f,"G{n}"),
            S(n)=>write!(f,"S{n}"),
            M(n)=>write!(f,"M{n}"),
            F(n)=>write!(f,"F{n}"),
            X(flt)=>write!(f,"X{flt:.6}"),
            Y(flt)=>write!(f,"Y{flt:.6}"),
            Custom(s)=>s.fmt(f),
        }
    }
}


#[derive(Default)]
pub struct GcodeBuilder {
    inner: Vec<GcodeBlock>,
    current_block: GcodeBlock,
}
impl GcodeBuilder {
    /// This inserts a header with G54, G17, G21, G90, G94
    pub fn default_header(&mut self) {
        let mut block = GcodeBlock::default();
        block.push(Ins::G(54));
        block.push(Ins::G(17));
        block.push(Ins::G(21));
        block.push(Ins::G(90));
        block.push(Ins::G(94));
        self.inner.push(block);
    }

    pub fn coolant(&mut self, on: bool)->&mut Self {
        if on {
            self.current_block.push(Ins::M(8));
        } else {
            self.current_block.push(Ins::M(9));
        }
        return self;
    }

    /// NOTE: Laser power is from 0..=1000 for GRBL software
    pub fn laser_power(&mut self, power: u16)->&mut Self {
        self.current_block.push(Ins::S(power));
        return self;
    }

    pub fn x(&mut self, x: f64)->&mut Self {
        self.current_block.push(Ins::X(x));
        return self;
    }

    pub fn y(&mut self, y: f64)->&mut Self {
        self.current_block.push(Ins::Y(y));
        return self;
    }

    /// NOTE: Feedrates are in mm/min for GRBL
    pub fn feed(&mut self, feed: u16)->&mut Self {
        self.current_block.push(Ins::F(feed));
        return self;
    }

    pub fn laser_on_const(&mut self)->&mut Self {
        self.current_block.push(Ins::M(3));
        return self;
    }

    pub fn laser_on_dyn(&mut self)->&mut Self {
        self.current_block.push(Ins::M(4));
        return self;
    }

    pub fn laser_off(&mut self)->&mut Self {
        self.current_block.push(Ins::M(5));
        return self;
    }

    pub fn rapid_motion(&mut self)->&mut Self {
        self.current_block.push(Ins::G(0));
        return self;
    }

    pub fn cutting_motion(&mut self)->&mut Self {
        self.current_block.push(Ins::G(1));
        return self;
    }

    pub fn custom(&mut self, s: String)->&mut Self {
        self.current_block.push(Ins::Custom(s));
        return self;
    }

    /// Add a comment to the current block. If there is already a comment, it adds a `;` and
    /// appends it to the end.
    pub fn comment(&mut self, text: impl Display)->&mut Self {
        self.current_block.add_comment(text);
        return self;
    }

    /// Adds a block with the given comment
    pub fn comment_block(&mut self, text: impl Display)->&mut Self {
        let mut block = GcodeBlock::default();
        block.add_comment(text);
        self.inner.push(block);
        return self;
    }

    pub fn eob(&mut self) {
        let block = std::mem::take(&mut self.current_block);

        self.inner.push(block);
    }

    pub fn finish(mut self)->String {
        if self.current_block.len() > 0 {
            self.inner.push(self.current_block);
        }

        // add end-of-program gcode
        let mut last_block = GcodeBlock::default();
        last_block.push(Ins::M(30));
        self.inner.push(last_block);

        let mut out = String::new();
        for block in self.inner {
            write!(&mut out, "{block}\n").unwrap();
        }

        return out;
    }
}

/// A block of gcode instructions. We don't support need many instructions, so we store them in a
/// [`SmallVec`] so we don't make as many allocations.
#[derive(Default)]
pub struct GcodeBlock(SmallVec<[GcodeInstruction;6]>, Option<String>);
impl GcodeBlock {
    pub fn len(&self)->usize {self.0.len()}

    pub fn push(&mut self, code: GcodeInstruction) {
        self.0.push(code);
    }

    pub fn add_comment(&mut self, text: impl Display) {
        if self.1.is_none() {
            self.1 = Some(text.to_string());
        } else {
            let s = self.1.as_mut().unwrap();
            write!(s, "; {text}").unwrap();
        }
    }
}
impl Display for GcodeBlock {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        if self.0.len() > 0 {
            write!(f, "{}", self.0[0])?;
            for code in self.0.iter().skip(1) {
                write!(f, " {code}")?;
            }

            // we add a space before the comment to separate it from the actual gcode
            if let Some(comment) = &self.1 {
                write!(f, " ({comment})")?;
            }
        } else {
            if let Some(comment) = &self.1 {
                write!(f, "({comment})")?;
            }
        }


        return Ok(());
    }
}
