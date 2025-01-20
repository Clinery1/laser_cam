use iced::{
    widget::{
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
    collections::HashMap,
    time::Instant,
};


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
}


pub struct ConditionEditor {
    store: Rc<RefCell<ConditionStore>>,
    condition: Option<ConditionId>,
    err: Option<(String, Instant)>,
}
impl Default for ConditionEditor {
    fn default()->Self {
        ConditionEditor {
            store: Rc::new(RefCell::new(ConditionStore {
                default: None,
                conditions: HashMap::new(),
                names: HashMap::new(),
            })),
            condition: None,
            err: None,
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

            let store: ConditionStore = ron::from_str(&s).unwrap();
            eprintln!("Loaded laser conditions");

            // update the condition count
            let mut max = 0;
            for id in store.conditions.keys() {
                max = max.max(id.0);
            }
            eprintln!("DEBUG: Next ConditionId = {}", max + 1);
            CONDITION_COUNT.store(max + 1, Ordering::Relaxed);

            return ConditionEditor {
                condition: store.default,
                store: Rc::new(RefCell::new(store)),
                ..Default::default()
            };
        }

        return Self::default();
    }

    pub fn save(&self) {
        let config_path = directories::BaseDirs::new()
            .unwrap()
            .config_dir()
            .to_path_buf()
            .join("laser_cam");
        std::fs::create_dir_all(&config_path).unwrap();
        let config_path = config_path.join("laser_conditions.ron");

        let s = ron::to_string(&*self.store.borrow()).unwrap();
        std::fs::write(config_path, s).expect("Could not write config file");

        eprintln!("Saved laser conditions");
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
                ).width(Length::FillPortion(5)),
                widget::button("New condition")
                    .width(Length::FillPortion(1))
                    .on_press(Message::NewCondition),
                widget::button("Close editor")
                    .width(Length::FillPortion(1))
                    .on_press(Message::CloseEditor),
                widget::Space::with_width(5.0),
                text!("Default condition: "),
                widget::pick_list(
                    condition_list,
                    default_condition,
                    |c|Message::DefaultCondition(c.id),
                ),
            ]
                .spacing(5.0)
                .align_y(VerticalAlign::Center)
                .height(Length::Shrink)
                .into()
        );

        if let Some((msg, _)) = &self.err {
            column.push(column![
                widget::Space::with_height(5.0),
                widget::text(msg),
                widget::Space::with_height(5.0),
            ].height(Length::Shrink).into());
        }

        if let Some(id) = self.condition {
            let condition = &store.conditions[&id];
            let color = condition.color.into();
            column.push(
                column![
                    widget::Space::with_height(5.0),
                    widget::center(widget::horizontal_rule(1.0)),
                    widget::Space::with_height(5.0),

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
                            .height(Length::Shrink),

                        widget::Space::with_width(20.0),

                        widget::center(widget::Space::with_width(10.0))
                            .style(move|_|widget::container::Style {
                                background: Some(Background::Color(color)),
                                ..Default::default()
                            })
                            .height(Length::Fill),

                        widget::Space::with_width(5.0),
                        widget::center(widget::vertical_rule(1.0)),
                        widget::Space::with_width(5.0),

                        widget::text_input(
                            "Condition name",
                            &condition.name.as_str(),
                        )
                            .on_input(Message::ChangeName),

                        widget::Space::with_width(10.0),
                        widget::center(widget::vertical_rule(5.0)),
                        widget::Space::with_width(10.0),

                        widget::button("Delete condition")
                            .style(crate::danger_button)
                            .on_press(Message::DeleteCondition),
                    ]
                        .align_y(VerticalAlign::Center)
                        .height(Length::Shrink),
                ]
                    .height(Length::Shrink)
                    .into()
            );

            let mut seq_column = Vec::new();

            seq_column.push(widget::Space::with_height(5.0).into());
            seq_column.push(widget::horizontal_rule(1.0).into());
            seq_column.push(widget::Space::with_height(5.0).into());

            seq_column.push(widget::button("New sequence item")
                .on_press(Message::NewSequence)
                .into()
            );

            seq_column.push(widget::Space::with_height(5.0).into());
            seq_column.push(widget::horizontal_rule(1.0).into());
            seq_column.push(widget::Space::with_height(5.0).into());

            for (i, seq) in condition.sequence.iter().enumerate() {
                seq_column.push(
                    row![
                        widget::center(text!("Passes: ")),
                        widget::text_input(
                            "Passes",
                            &seq.passes.to_string(),
                        )
                            .on_input(move|s|Message::ChangePasses(i, s)),

                        widget::Space::with_width(5.0),
                        widget::center(widget::vertical_rule(1.0)),
                        widget::Space::with_width(5.0),

                        widget::center(text!("Feed: ")),
                        widget::text_input(
                            "Feed",
                            &seq.feed.to_string(),
                        )
                            .on_input(move|s|Message::ChangeFeed(i, s)),

                        widget::Space::with_width(5.0),
                        widget::center(widget::vertical_rule(1.0)),
                        widget::Space::with_width(5.0),

                        widget::center(text!("Power: ")),
                        widget::text_input(
                            "Power",
                            &seq.power.to_string(),
                        )
                            .on_input(move|s|Message::ChangePower(i, s)),

                        widget::Space::with_width(10.0),
                        widget::center(widget::vertical_rule(5.0)),
                        widget::Space::with_width(10.0),

                        widget::button("Delete sequence item")
                            .style(crate::danger_button)
                            .on_press(Message::DeleteSequence(i)),
                    ]
                        .align_y(VerticalAlign::Center)
                        .height(Length::Shrink)
                        .into()
                );

                seq_column.push(widget::Space::with_height(5.0).into());
                seq_column.push(widget::horizontal_rule(1.0).into());
                seq_column.push(widget::Space::with_height(5.0).into());
            }

            column.push(widget::scrollable(widget::column(seq_column)).into());
        }


        widget::column(column)
            .align_x(HorizontalAlign::Center)
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
        store.names.insert(name, id);
    }

    pub fn update(&mut self, msg: Message)->Task<Message> {
        if let Some((_, start)) = &self.err {
            if start.elapsed().as_secs() > 5 {
                self.err = None;
            }
        }
        match msg {
            // We handle this in MainProgram
            Message::CloseEditor=>{},
            Message::RecalcSheet=>{},

            Message::SelectCondition(id)=>self.condition = Some(id),
            Message::DefaultCondition(id)=>self.store.borrow_mut().default = Some(id),

            Message::NewCondition=>self.new_condition(),
            Message::DeleteCondition=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .remove(&id)
                        .unwrap();
                    store.names.remove(&condition.name);
                }
            },
            Message::ChangeName(name)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    if let Some(name_id) = store.names.get(&name) {
                        if id != *name_id {
                            self.err = Some((
                                format!("There is already a condition with the name `{name}`"),
                                Instant::now(),
                            ));
                        }
                    } else {
                        let condition = store.conditions
                            .get_mut(&id)
                            .unwrap();
                        let old_name = std::mem::replace(&mut condition.name, name.clone());
                        store.names.remove(&old_name);

                        store.names.insert(name.clone(), id);
                    }
                }
            },
            Message::ChangeColorR(n)=>{
                if let Some(id) = self.condition {
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
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.sequence.push(SequenceItem {
                        passes: 1,
                        power: 300,
                        feed: 1000,
                    });
                }
            },
            Message::DeleteSequence(idx)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    condition.sequence.remove(idx);
                }
            },
            Message::ChangeFeed(idx, s)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    if let Some(num) = crate::parse_u16(&s) {
                        condition.sequence[idx].feed = num;
                    }
                }
            },
            Message::ChangePower(idx, s)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    if let Some(num) = crate::parse_u16(&s) {
                        condition.sequence[idx].power = num;
                    }
                }
            },
            Message::ChangePasses(idx, s)=>{
                if let Some(id) = self.condition {
                    let mut store = self.store.borrow_mut();
                    let condition = store.conditions
                        .get_mut(&id)
                        .unwrap();
                    if let Some(num) = crate::parse_u16(&s) {
                        condition.sequence[idx].passes = num;
                    }
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
    conditions: HashMap<ConditionId, Condition>,
    #[serde(default)]
    names: HashMap<String, ConditionId>,
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct SequenceItem {
    pub passes: u16,
    pub feed: u16,
    pub power: u16,
}


static CONDITION_COUNT: AtomicUsize = AtomicUsize::new(0);


/// Generate a new, per-execution unique condition ID
fn next_condition_id()->ConditionId {
    ConditionId(CONDITION_COUNT.fetch_add(1, Ordering::SeqCst))
}
