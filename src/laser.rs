use iced::{
    widget::{
        text::Wrapping,
        column,
        row,
        text,
        self,
    },
    alignment::{
        Vertical as VerticalAlign,
        Horizontal as HorizontalAlign,
    },
    Color as IcedColor,
    Background,
    Element,
    Task,
    Length,
};
use serde::{Serialize, Deserialize};
use indexmap::IndexMap;
use std::{
    sync::atomic::{
        Ordering,
        AtomicUsize,
    },
    fmt::{
        Display,
        Formatter,
        Result as FmtResult,
    },
    rc::Rc,
    cell::RefCell,
};
use SequenceItem as Seq;


#[derive(Debug, Clone)]
pub enum Message {
    CloseEditor,
    RecalcSheet,

    SelectCondition(ConditionId),
    DefaultCondition(ConditionId),

    NewCondition,
    DeleteCondition,
    ChangeName(String),
    ChangeColorR(f32),
    ChangeColorG(f32),
    ChangeColorB(f32),

    NewSequence,
    DeleteSequence(usize),
    ChangeFeed(usize, String),
    ChangePower(usize, String),
    ChangePasses(usize, String),

    // For custom sequence items
    ChangeLaserOn(usize, String),
    ChangeLaserOff(usize, String),

    ChangeSeqItemType(usize, SeqItemType),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SequenceItem {
    GrblConst {
        passes: u16,
        power: u16,
        feed: u16,
    },
    GrblDyn {
        passes: u16,
        power: u16,
        feed: u16,
    },
    Custom {
        passes: u16,
        laser_on: String,
        laser_off: String,
        power: String,
        feed: String,
    },
}
impl SequenceItem {
    pub fn item_type(&self)->SeqItemType {
        match self {
            Self::GrblConst{..}=>SeqItemType::GrblConst,
            Self::GrblDyn{..}=>SeqItemType::GrblDyn,
            Self::Custom{..}=>SeqItemType::Custom,
        }
    }

    pub fn passes(&self)->u16 {
        match self {
            Self::GrblConst{passes, ..}|Self::GrblDyn{passes, ..}|Self::Custom{passes, ..}=>*passes,
        }
    }

    pub fn feed_string(&self)->String {
        match self {
            Self::GrblConst{feed, ..}|Self::GrblDyn{feed, ..}=>feed.to_string(),
            Self::Custom{feed, ..}=>feed.clone(),
        }
    }

    pub fn power_string(&self)->String {
        match self {
            Self::GrblConst{power, ..}|Self::GrblDyn{power, ..}=>power.to_string(),
            Self::Custom{power, ..}=>power.clone(),
        }
    }

    pub fn power_pretty_string(&self)->String {
        match self {
            Self::GrblConst{power, ..}|Self::GrblDyn{power, ..}=>format!("{}%", (*power as f32) / 10.0),
            Self::Custom{power, ..}=>power.clone(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SeqItemType {
    GrblConst,
    GrblDyn,
    Custom,
}
impl SeqItemType {
    const LIST: &[Self] = &[
        Self::GrblConst,
        Self::GrblDyn,
        Self::Custom,
    ];
}
impl Display for SeqItemType {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        match self {
            Self::GrblConst=>write!(f, "GRBL Const (M3)"),
            Self::GrblDyn=>write!(f, "GRBL Dyn (M4)"),
            Self::Custom=>write!(f, "Custom"),
        }
    }
}


pub struct ConditionEditor {
    store: Rc<RefCell<ConditionStore>>,
    condition: Option<ConditionId>,
    feed_val: Vec<String>,
    power_val: Vec<String>,
    passes_val: Vec<String>,
    changed: bool,
}
impl Default for ConditionEditor {
    fn default()->Self {
        ConditionEditor {
            store: Rc::new(RefCell::new(ConditionStore {
                default: None,
                conditions: IndexMap::new(),
            })),
            feed_val: Vec::new(),
            power_val: Vec::new(),
            passes_val: Vec::new(),
            condition: None,
            changed: false,
        }
    }
}
impl ConditionEditor {
    pub fn get_store(&self)->Rc<RefCell<ConditionStore>> {
        self.store.clone()
    }

    pub fn load()->Self {
        let config_path = directories::BaseDirs::new()
            .unwrap()
            .config_dir()
            .to_path_buf()
            .join("laser_cam")
            .join("laser_conditions.ron");

        if config_path.exists() {
            let s = std::fs::read_to_string(config_path).expect("Could not read the config file");

            let store = match ron::from_str::<ConditionStore>(&s) {
                Ok(s)=>s,
                Err(e)=>{
                    eprintln!("Error loading condition store: {e}");
                    return Self::default();
                },
            };
            eprintln!("Loaded laser conditions");

            // update the condition count
            let mut max = 0;
            for id in store.conditions.keys() {
                max = max.max(id.0);
            }
            eprintln!("DEBUG: Next ConditionId = {}", max + 1);
            CONDITION_COUNT.store(max + 1, Ordering::Relaxed);

            let mut ret = ConditionEditor {
                condition: store.default,
                store: Rc::new(RefCell::new(store)),
                ..Default::default()
            };
            ret.update_sequence_values();

            return ret;
        }

        return Self::default();
    }

    pub fn save(&self) {
        if self.changed {
            use ron::{
                ser::PrettyConfig,
                extensions::Extensions,
            };
            let config_path = directories::BaseDirs::new()
                .unwrap()
                .config_dir()
                .to_path_buf()
                .join("laser_cam");
            std::fs::create_dir_all(&config_path).unwrap();
            let config_path = config_path.join("laser_conditions.ron");

            let mut pc = PrettyConfig::default();
            pc.extensions = Extensions::UNWRAP_NEWTYPES|Extensions::IMPLICIT_SOME;
            pc.depth_limit = 8;
            pc.struct_names = false;

            let s = ron::ser::to_string_pretty(
                &*self.store.borrow(),
                pc,
            )
                .unwrap();
            std::fs::write(config_path, s).expect("Could not write config file");

            eprintln!("Saved laser conditions");
        } else {
            eprintln!("Laser conditions not changed");
        }
    }

    pub fn default_condition(&mut self)->ConditionId {
        let store = self.store.borrow();
        if store.conditions.len() == 0 {
            drop(store);
            self.new_condition();
        } else {
            drop(store);
        }

        let mut store = self.store.borrow_mut();

        if store.default.is_none() {
            let id = store.conditions.keys().next().unwrap();
            store.default = Some(*id);
        }

        return store.default.unwrap();
    }

    pub fn view(&self)->Element<Message> {
        let mut column = Vec::new();
        let store = self.store.borrow();

        let condition_list = store.conditions.values().map(Condition::display).collect::<Vec<_>>();
        let condition = self.condition
            .as_ref()
            .map(|c|store.conditions[c].display());
        let default_condition = store.default
            .as_ref()
            .map(|c|store.conditions[c].display());
        column.push(
            row![
                widget::pick_list(
                    condition_list.clone(),
                    condition,
                    |c|Message::SelectCondition(c.id),
                )
                    .width(Length::FillPortion(6)),
                widget::Space::with_width(5.0),

                text!("Default condition: "),
                widget::pick_list(
                    condition_list,
                    default_condition,
                    |c|Message::DefaultCondition(c.id),
                )
                    .width(Length::FillPortion(6)),
                widget::Space::with_width(5.0),
                widget::button(text!("New condition").center())
                    .width(Length::FillPortion(3))
                    .height(Length::Fill)
                    .on_press(Message::NewCondition),
                widget::button(text!("Close editor").center())
                    .width(Length::FillPortion(2))
                    .height(Length::Fill)
                    .on_press(Message::CloseEditor),
            ]
                .spacing(5.0)
                .height(Length::Shrink)
                .align_y(VerticalAlign::Center)
                .into()
        );

        if let Some(id) = self.condition {
            let condition = &store.conditions[&id];
            let color = condition.color.into();
            column.push(
                widget::center(widget::horizontal_rule(1.0))
                    .height(Length::Shrink)
                    .into()
            );
            column.push(
                row![
                    column![
                        row![
                            text!("R: "),
                            widget::slider(
                                0.0..=1.0f32,
                                condition.color.r,
                                Message::ChangeColorR,
                            )
                                .step(1.0 / 512.0),
                        ]
                            .align_y(VerticalAlign::Center),

                        row![
                            text!("G: "),
                            widget::slider(
                                0.0..=1.0f32,
                                condition.color.g,
                                Message::ChangeColorG,
                            )
                                .step(1.0 / 512.0),
                        ]
                            .align_y(VerticalAlign::Center),

                        row![
                            text!("B: "),
                            widget::slider(
                                0.0..=1.0f32,
                                condition.color.b,
                                Message::ChangeColorB,
                            )
                                .step(1.0 / 512.0),
                        ]
                            .align_y(VerticalAlign::Center),
                    ]
                        .align_x(HorizontalAlign::Center)
                        .height(Length::Shrink)
                        .width(Length::FillPortion(2)),

                    widget::center(widget::Space::with_width(10.0))
                        .style(move|_|widget::container::Style {
                            background: Some(Background::Color(color)),
                            ..Default::default()
                        })
                        .height(Length::Fill)
                        .width(Length::FillPortion(1)),

                    column![
                        widget::text_input(
                            "Condition name",
                            &condition.name.as_str(),
                        )
                            .on_input(Message::ChangeName),

                        widget::button(text!("New sequence item").center().width(Length::Fill))
                            .on_press(Message::NewSequence)
                            .width(Length::Fill),
                    ]
                        .width(Length::FillPortion(2)),

                    widget::center(
                        widget::button("Delete condition")
                            .style(crate::danger_button)
                            .width(Length::Shrink)
                            .height(Length::Shrink)
                            .on_press(Message::DeleteCondition),
                    ).width(Length::FillPortion(1)),
                ]
                    .align_y(VerticalAlign::Center)
                    .height(Length::Shrink)
                    .spacing(5.0)
                    .into()
            );

            let mut seq_column = Vec::new();

            seq_column.push(widget::horizontal_rule(1.0).into());

            for (i, seq) in condition.sequence.iter().enumerate() {
                let mut row_items = ElementList::new();

                row_items.push(
                    widget::pick_list(
                        SeqItemType::LIST,
                        Some(seq.item_type()),
                        move|ty|Message::ChangeSeqItemType(i, ty),
                    )
                        .width(Length::Shrink)
                );

                row_items.push(column![
                    widget::center(text!("Passes: ")).height(Length::Shrink),
                    widget::text_input(
                        "Passes",
                        self.passes_val[i].as_str(),
                    )
                        .on_input(move|s|Message::ChangePasses(i, s))
                ].width(Length::FillPortion(1)));

                row_items.push(column![
                    widget::center(text!("Feed: ")).height(Length::Shrink),
                    widget::text_input(
                        "Feed",
                        self.feed_val[i].as_str(),
                    )
                        .on_input(move|s|Message::ChangeFeed(i, s))
                ].width(Length::FillPortion(1)));

                row_items.push(column![
                    widget::center(text!("Power: ")).height(Length::Shrink),
                    widget::text_input(
                        "Power",
                        self.power_val[i].as_str(),
                    )
                        .on_input(move|s|Message::ChangePower(i, s))
                ].width(Length::FillPortion(1)));

                match seq {
                    Seq::Custom{laser_on, laser_off, ..}=>{
                        row_items.push(column![
                            widget::center(
                                text!("Laser on GCODE: ").wrapping(Wrapping::None)
                            ).height(Length::Shrink).width(Length::Fill),
                            widget::text_input(
                                "GCODE",
                                laser_on.as_str(),
                            )
                                .width(Length::Fill)
                                .on_input(move|s|Message::ChangeLaserOn(i, s))
                        ].width(Length::FillPortion(2)));

                        row_items.push(column![
                            widget::center(
                                text!("Laser off GCODE: ").wrapping(Wrapping::None)
                            ).height(Length::Shrink).width(Length::Fill),
                            widget::text_input(
                                "GCODE",
                                laser_off.as_str(),
                            )
                                .width(Length::Fill)
                                .on_input(move|s|Message::ChangeLaserOff(i, s))
                        ].width(Length::FillPortion(2)));
                    },
                    _=>{},
                }

                row_items.push(widget::Space::with_width(20.0));

                row_items.push(
                    widget::button("Delete")
                        .style(crate::danger_button)
                        .width(Length::Shrink)
                        .on_press(Message::DeleteSequence(i))
                );


                seq_column.push(widget::row(row_items.0)
                    .align_y(VerticalAlign::Bottom)
                    .spacing(10.0)
                    .padding(5.0)
                    .height(Length::Fixed(70.0))
                    .into()
                );

                seq_column.push(widget::horizontal_rule(1.0).into());
            }

            column.push(widget::scrollable(
                widget::column(seq_column)
                    .spacing(5.0)
                    .align_x(HorizontalAlign::Center)
            ).into());
        }


        widget::column(column)
            .align_x(HorizontalAlign::Center)
            .spacing(5.0)
            .padding(10.0)
            .into()
    }

    fn new_condition(&mut self) {
        let mut store = self.store.borrow_mut();
        let id = next_condition_id();
        let name = format!("New Condition {}", id.0);
        store.conditions.insert(id, Condition {
            id,
            name: name.clone(),
            color: Color::WHITE,
            sequence: Vec::new(),
        });
        self.condition = Some(id);

        drop(store);
        self.update_sequence_values();
    }

    fn update_sequence_values(&mut self) {
        self.power_val.clear();
        self.feed_val.clear();
        self.passes_val.clear();

        if let Some(id) = self.condition {
            let mut store = self.store.borrow_mut();
            self.changed = true;

            let condition = store.conditions
                .get_mut(&id)
                .unwrap();

            for seq in condition.sequence.iter() {
                self.power_val.push(seq.power_string());
                self.feed_val.push(seq.feed_string());
                self.passes_val.push(seq.passes().to_string());
            }
        }
    }

    pub fn update(&mut self, msg: Message)->Task<Message> {
        match msg {
            // We handle this in MainProgram
            Message::CloseEditor=>{},
            Message::RecalcSheet=>{},

            Message::SelectCondition(id)=>{
                self.condition = Some(id);
                self.update_sequence_values();
            },
            Message::DefaultCondition(id)=>self.store.borrow_mut().default = Some(id),

            Message::NewCondition=>self.new_condition(),
            Message::DeleteCondition=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    store.conditions.shift_remove(&id);
                    self.condition = None;
                    drop(store);
                    self.update_sequence_values();
                }
            },
            Message::ChangeName(name)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    self.changed = true;

                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.name = name;
                }
            },
            Message::ChangeColorR(n)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.color.r = n;

                    return Task::done(Message::RecalcSheet);
                }
            },
            Message::ChangeColorG(n)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.color.g = n;

                    return Task::done(Message::RecalcSheet);
                }
            },
            Message::ChangeColorB(n)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.color.b = n;

                    return Task::done(Message::RecalcSheet);
                }
            },

            Message::NewSequence=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.sequence.push(Seq::GrblConst {
                        passes: 1,
                        power: 300,
                        feed: 1000,
                    });

                    drop(store);
                    self.update_sequence_values();
                }
            },
            Message::DeleteSequence(idx)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.sequence.remove(idx);

                    drop(store);
                    self.update_sequence_values();
                }
            },
            Message::ChangeFeed(idx, s)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();

                    match &mut condition.sequence[idx] {
                        Seq::GrblConst{feed, ..}|Seq::GrblDyn{feed, ..}=>{
                            if let Some(num) = crate::parse_u16(&s) {
                                *feed = num;
                                self.feed_val[idx] = s;
                            }
                        },
                        Seq::Custom{feed, ..}=>{
                            *feed = s.clone();
                            self.feed_val[idx] = s;
                        },
                    }
                }
            },
            Message::ChangePower(idx, s)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();

                    match &mut condition.sequence[idx] {
                        Seq::GrblConst{power, ..}|Seq::GrblDyn{power, ..}=>{
                            if let Some(num) = crate::parse_u16(&s) {
                                *power = num;
                                self.power_val[idx] = s;
                            }
                        },
                        Seq::Custom{power, ..}=>{
                            *power = s.clone();
                            self.power_val[idx] = s;
                        },
                    }
                }
            },
            Message::ChangePasses(idx, s)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    if let Some(num) = crate::parse_u16(&s) {
                        match &mut condition.sequence[idx] {
                            Seq::GrblConst{passes, ..}|Seq::GrblDyn{passes, ..}|Seq::Custom{passes, ..}=>{
                                *passes = num;
                            },
                        }
                        self.passes_val[idx] = s;
                    }
                }
            },
            Message::ChangeLaserOn(idx, s)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    match &mut condition.sequence[idx] {
                        Seq::Custom{laser_on, ..}=>{
                            *laser_on = s;
                        },
                        _=>{},
                    }
                }
            },
            Message::ChangeLaserOff(idx, s)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    match &mut condition.sequence[idx] {
                        Seq::Custom{laser_off, ..}=>{
                            *laser_off = s;
                        },
                        _=>{},
                    }
                }
            },
            Message::ChangeSeqItemType(idx, ty)=>{
                if let Some(id) = self.condition {
                    self.changed = true;

                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    match ty {
                        SeqItemType::GrblConst=>match condition.sequence[idx] {
                            Seq::Custom{..}=>condition.sequence[idx] = Seq::GrblConst {
                                passes: 1,
                                power: 1000,
                                feed: 1000,
                            },
                            Seq::GrblDyn{passes, power, feed}=>condition.sequence[idx] = Seq::GrblConst {passes, power, feed},
                            Seq::GrblConst{..}=>{},
                        },
                        SeqItemType::GrblDyn=>match condition.sequence[idx] {
                            Seq::Custom{..}=>condition.sequence[idx] = Seq::GrblDyn {
                                passes: 1,
                                power: 1000,
                                feed: 1000,
                            },
                            Seq::GrblConst{passes, power, feed}=>condition.sequence[idx] = Seq::GrblDyn {passes, power, feed},
                            Seq::GrblDyn{..}=>{},
                        },
                        SeqItemType::Custom=>match condition.sequence[idx] {
                            Seq::Custom{..}=>{},
                            Seq::GrblConst{passes, power, feed}=>condition.sequence[idx] = Seq::Custom {
                                passes,
                                power: format!("S{power}"),
                                feed: format!("F{feed}"),
                                laser_on: "M3".into(),
                                laser_off: "M5".into(),
                            },
                            Seq::GrblDyn{passes, power, feed}=>condition.sequence[idx] = Seq::Custom {
                                passes,
                                power: format!("S{power}"),
                                feed: format!("F{feed}"),
                                laser_on: "M4".into(),
                                laser_off: "M5".into(),
                            },
                        },
                    }

                    self.power_val[idx] = condition.sequence[idx].power_string();
                    self.feed_val[idx] = condition.sequence[idx].feed_string();
                    self.passes_val[idx] = condition.sequence[idx].passes().to_string();
                }
            },
        }

        return Task::none();
    }
}

/// A storage medium for laser conditions
#[derive(Deserialize, Serialize)]
pub struct ConditionStore {
    #[serde(default)]
    default: Option<ConditionId>,
    #[serde(default)]
    conditions: IndexMap<ConditionId, Condition>,
}
impl ConditionStore {
    pub fn get(&self, id: ConditionId)->&Condition {
        self.conditions.get(&id).unwrap()
    }

    pub fn iter(&self)->impl Iterator<Item = &Condition> {
        self.conditions.values()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ConditionId(usize);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub id: ConditionId,
    pub color: Color,
    pub name: String,
    pub sequence: Vec<SequenceItem>,
}
impl Condition {
    pub fn display(&self)->ConditionDisplay {
        ConditionDisplay {
            name: self.name.clone(),
            id: self.id,
        }
    }
}
impl PartialEq for Condition {
    fn eq(&self, other: &Self)->bool {
        self.id == other.id
    }
}
impl Display for Condition {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        self.name.fmt(f)
    }
}

#[derive(PartialEq, Clone)]
pub struct ConditionDisplay {
    pub id: ConditionId,
    name: String,
}
impl Display for ConditionDisplay {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        self.name.fmt(f)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
}
impl Color {
    pub const WHITE: Self = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    };
}
impl From<Color> for IcedColor {
    fn from(c: Color)->Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            a: 1.0,
        }
    }
}

pub struct ElementList<'a, M>(pub Vec<Element<'a, M>>);
impl<'a, M> ElementList<'a, M> {
    pub fn new()->Self {ElementList(Vec::new())}

    pub fn push<T: Into<Element<'a, M>>>(&mut self, item: T) {
        self.0.push(item.into());
    }
}


static CONDITION_COUNT: AtomicUsize = AtomicUsize::new(0);


/// Generate a new, per-execution unique condition ID
fn next_condition_id()->ConditionId {
    ConditionId(CONDITION_COUNT.fetch_add(1, Ordering::SeqCst))
}
