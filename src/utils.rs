#![allow(unused)]


//! We have some projection and conversion methods here.
//!
//! # Assumptions
//! Iced: Y down (+x->right, +y->down)
//! Ultraviolet: Y up (+x->right, +y->up)
//! Geo: Y up (+x->right, +y->up)


use geo::Coord;
use iced::Point as IcedPoint;
use ultraviolet::{
    DVec2,
    DRotor2,
    DSimilarity2,
};


/// Projects a point to yup or ydown and returns it as an `ultraviolet::DVec2`
pub trait Project2D {
    /// Height is relative to the destination coordinate system
    fn to_yup<F: Into<f64>>(self, height: F)->DVec2;
    /// Height is relative to the destination coordinate system
    fn to_ydown<F: Into<f64>>(self, height: F)->DVec2;
}
impl Project2D for IcedPoint {
    fn to_yup<F: Into<f64>>(self, height: F)->DVec2 {
        DVec2::new(
            self.x as f64,
            height.into() - (self.y as f64),
        )
    }

    // Already ydown
    fn to_ydown<F: Into<f64>>(self, height: F)->DVec2 {
        DVec2::new(
            self.x as f64,
            self.y as f64,
        )
    }
}
impl Project2D for DVec2 {
    // Already yup
    fn to_yup<F: Into<f64>>(self, height: F)->DVec2 {
        return self;
    }

    fn to_ydown<F: Into<f64>>(self, height: F)->DVec2 {
        DVec2::new(
            self.x as f64,
            height.into() - (self.y as f64),
        )
    }
}

impl Project2D for Coord {
    // Already yup
    fn to_yup<F: Into<f64>>(self, height: F)->DVec2 {
        return self.to_uv();
    }

    fn to_ydown<F: Into<f64>>(self, height: F)->DVec2 {
        DVec2::new(
            self.x as f64,
            height.into() - (self.y as f64),
        )
    }
}


pub trait UvCompat {
    fn rotated(self, rotor: DRotor2)->Self;
    fn transformed(self, t: DSimilarity2)->Self;
    fn to_uv(self)->DVec2;
    fn to_iced(self)->IcedPoint;
}
impl UvCompat for Coord<f64> {
    fn rotated(self, rotor: DRotor2)->Self {
        let v = rotor * DVec2 {
            x: self.x,
            y: self.y,
        };

        return Coord {
            x: v.x,
            y: v.y,
        };
    }

    fn transformed(self, t: DSimilarity2)->Self {
        let v = t * DVec2 {
            x: self.x,
            y: self.y,
        };

        return Coord {
            x: v.x,
            y: v.y,
        };
    }

    fn to_uv(self)->DVec2 {
        DVec2 {
            x: self.x,
            y: self.y,
        }
    }

    fn to_iced(self)->IcedPoint {
        IcedPoint {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

pub trait UvCompat2 {
    fn rotated(self, rotor: DRotor2)->Self;
    fn transformed(self, t: DSimilarity2)->Self;
    fn to_geo(self)->Coord<f64>;
    fn to_iced(self)->IcedPoint;
}
impl UvCompat2 for DVec2 {
    fn rotated(self, rotor: DRotor2)->Self {
        return rotor * self;
    }

    fn transformed(self, t: DSimilarity2)->Self {
        return t * self;
    }

    fn to_geo(self)->Coord<f64> {
        Coord {
            x: self.x,
            y: self.y,
        }
    }

    fn to_iced(self)->IcedPoint {
        IcedPoint {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

pub trait UvCompat3 {
    fn rotated(self, rotor: DRotor2)->Self;
    fn transformed(self, t: DSimilarity2)->Self;
    fn to_geo(self)->Coord<f64>;
    fn to_uv(self)->DVec2;
}
impl UvCompat3 for IcedPoint {
    fn rotated(self, rotor: DRotor2)->Self {
        self.to_uv().rotated(rotor).to_iced()
    }

    fn transformed(self, t: DSimilarity2)->Self {
        self.to_uv().transformed(t).to_iced()
    }

    fn to_geo(self)->Coord<f64> {
        Coord {
            x: self.x as f64,
            y: self.y as f64,
        }
    }

    fn to_uv(self)->DVec2 {
        DVec2 {
            x: self.x as f64,
            y: self.y as f64,
        }
    }
}
