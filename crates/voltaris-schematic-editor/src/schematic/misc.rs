use crate::error::{Error, Result};
use crate::sexp::{atom, qstr, tagged, SexpNode};
use crate::types::{fmt_f64, At, Effects};

// ---- Junction ---------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Junction {
    pub x: f64,
    pub y: f64,
    pub diameter: f64,
    pub uuid: String,
    pub raw_color: Option<SexpNode>,
}

impl Junction {
    pub fn new(x: f64, y: f64) -> Self {
        Junction {
            x,
            y,
            diameter: 0.0,
            uuid: uuid::Uuid::new_v4().to_string(),
            raw_color: None,
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let at = node.find("at").ok_or(Error::MissingField("at"))?;
        let s = at.scalar_args();
        let x: f64 = s.first().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let y: f64 = s.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let diameter = node.get_float("diameter").unwrap_or(0.0);
        let uuid = node.get_value("uuid").unwrap_or("").to_owned();
        let raw_color = node.find("color").cloned();
        Ok(Junction {
            x,
            y,
            diameter,
            uuid,
            raw_color,
        })
    }

    pub fn to_sexp(&self) -> SexpNode {
        let mut c = vec![
            atom("junction"),
            tagged("at", vec![atom(fmt_f64(self.x)), atom(fmt_f64(self.y))]),
            tagged("diameter", vec![atom(fmt_f64(self.diameter))]),
        ];
        if let Some(col) = &self.raw_color {
            c.push(col.clone());
        }
        c.push(tagged("uuid", vec![qstr(self.uuid.clone())]));
        SexpNode::List(c)
    }

    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }
}

// ---- Text -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Text {
    pub text: String,
    pub at: At,
    pub uuid: String,
    pub effects: Option<Effects>,
}

impl Text {
    pub fn new(text: impl Into<String>, x: f64, y: f64) -> Self {
        Text {
            text: text.into(),
            at: At::new(x, y),
            uuid: uuid::Uuid::new_v4().to_string(),
            effects: None,
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let text = node
            .value()
            .ok_or(Error::MissingField("text content"))?
            .to_owned();
        let at = node
            .find("at")
            .and_then(At::from_sexp)
            .ok_or(Error::MissingField("at"))?;
        let uuid = node.get_value("uuid").unwrap_or("").to_owned();
        let effects = node.find("effects").and_then(Effects::from_sexp);
        Ok(Text {
            text,
            at,
            uuid,
            effects,
        })
    }

    pub fn to_sexp(&self) -> SexpNode {
        let mut c = vec![atom("text"), qstr(self.text.clone()), self.at.to_sexp()];
        if let Some(e) = &self.effects {
            c.push(e.to_sexp());
        }
        c.push(tagged("uuid", vec![qstr(self.uuid.clone())]));
        SexpNode::List(c)
    }

    pub fn position(&self) -> (f64, f64) {
        (self.at.x, self.at.y)
    }
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.at.translate(dx, dy);
    }
}

// ---- NoConnect --------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NoConnect {
    pub x: f64,
    pub y: f64,
    pub uuid: String,
}

impl NoConnect {
    pub fn new(x: f64, y: f64) -> Self {
        NoConnect {
            x,
            y,
            uuid: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let at = node.find("at").ok_or(Error::MissingField("at"))?;
        let s = at.scalar_args();
        let x: f64 = s.first().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let y: f64 = s.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let uuid = node.get_value("uuid").unwrap_or("").to_owned();
        Ok(NoConnect { x, y, uuid })
    }

    pub fn to_sexp(&self) -> SexpNode {
        SexpNode::List(vec![
            atom("no_connect"),
            tagged("at", vec![atom(fmt_f64(self.x)), atom(fmt_f64(self.y))]),
            tagged("uuid", vec![qstr(self.uuid.clone())]),
        ])
    }

    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }
}
