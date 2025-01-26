use iced::{
    widget::{
        pane_grid::{
            State as PaneState,
            ResizeEvent,
            self,
        },
        button::Status as ButtonStatus,
        container::Style,
        column,
        row,
        text,
        self,
    },
    event::{
        Event,
        self,
    },
    Background,
    Border,
    Length,
    Element,
    Theme,
    Task,
    window,
};
use rfd::{
    AsyncFileDialog,
    FileHandle,
};
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
};
use sheet::*;
use model::*;
use laser::{
    ConditionEditor,
    Message as ConditionMessage,
    ConditionId,
};


mod model;
mod sheet;
mod gcode;
mod laser;
mod utils;


pub type Point = ultraviolet::DVec2;
pub type Vector = ultraviolet::DVec2;
/// Positive angles rotate clockwise, negative counter clockwise
pub type Rotation = ultraviolet::DRotor2;
pub type Transform = ultraviolet::DSimilarity2;
pub type Translation = ultraviolet::DVec2;


/// Main program state changes
#[derive(Debug, Clone)]
pub enum Message {
    Sheet(SheetMessage),
    Condition(ConditionMessage),
    Iced(Event),

    RenameSheet(String),
    SelectSheet(usize),
    NewSheet,
    DeleteSheet,
    ChangeSheetWidth(String),
    ChangeSheetHeight(String),

    AddModel(ModelHandle),

    ResizePane(ResizeEvent),

    ModelPaneState(ModelPaneState),

    OpenFilePicker,
    LoadModel(Option<Vec<FileHandle>>),

    OpenGcodeSaveDialog,
    SaveGcode(Option<FileHandle>),

    EntityParamsX(String),
    EntityParamsY(String),
    EntityParamsAngle(f64),
    EntityParamsAngleString(String),
    EntityParamsScale(String),
    EntityParamsFlip(bool),
    EntityParamsCondition(ConditionId),
    DeleteEntity,

    ToggleConditionEditor,
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProgramPane {
    Sheet,
    SheetList,
    ModelList,
    EntityParams,
    ConditionEditor,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ModelPaneState {
    ActiveModels,
    AllModels,
}
impl Display for ModelPaneState {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        match self {
            Self::ActiveModels=>write!(f, "Active Models"),
            Self::AllModels=>write!(f, "All Models"),
        }
    }
}


#[derive(Clone, PartialEq)]
pub struct SheetIndex {
    pub name: String,
    pub gcode: Option<String>,
    pub index: usize,
}
impl Display for SheetIndex {
    fn fmt(&self, f: &mut Formatter)->FmtResult {
        self.name.fmt(f)
    }
}

struct EntityParams {
    id: EntityId,
    x: String,
    y: String,
    angle: f64,
    angle_string: String,
    scale: String,
    flip: bool,
    laser_condition: ConditionId,
}

pub struct MainProgram {
    panes: PaneState<ProgramPane>,
    models: ModelStore,
    active_sheet: usize,
    sheets: Vec<Sheet>,
    sheet_settings: Vec<SheetIndex>,
    model_pane_state: ModelPaneState,
    entity_params: Option<EntityParams>,
    sheet_size: [String; 2],
    conditions: ConditionEditor,
}
impl MainProgram {
    pub fn view(&self)->Element<Message> {
        widget::pane_grid(
            &self.panes,
            |_pane, state, _is_maximized|{
                match state {
                    ProgramPane::ConditionEditor=>pane_grid::Content::new(self.conditions.view().map(Message::Condition))
                        .style(|theme|{
                            Style {
                                border: Border {
                                    color: theme.palette().primary,
                                    width: 1.0,
                                    ..Border::default()
                                },
                                ..Style::default()
                            }
                        }),
                    ProgramPane::Sheet=>pane_grid::Content::new(self.sheet_view())
                        .style(|theme|{
                            Style {
                                border: Border {
                                    color: theme.palette().primary,
                                    width: 1.0,
                                    ..Border::default()
                                },
                                ..Style::default()
                            }
                        }),
                    ProgramPane::SheetList=>pane_grid::Content::new(self.sheet_list_view())
                        .style(|theme|{
                            Style {
                                border: Border {
                                    color: theme.palette().primary,
                                    width: 1.0,
                                    ..Border::default()
                                },
                                ..Style::default()
                            }
                        })
                        .title_bar(
                            pane_grid::TitleBar::new(widget::center(text!("Sheets")).height(Length::Shrink))
                                .padding(5.0)
                        ),
                    ProgramPane::ModelList=>pane_grid::Content::new(self.model_list_view())
                        .style(|theme|{
                            Style {
                                border: Border {
                                    color: theme.palette().primary,
                                    width: 1.0,
                                    ..Border::default()
                                },
                                ..Style::default()
                            }
                        })
                        .title_bar(
                            pane_grid::TitleBar::new(widget::center(text!("Models")).height(Length::Shrink))
                                .padding(5.0)
                        ),
                    ProgramPane::EntityParams=>pane_grid::Content::new(self.entity_params_view())
                        .style(|theme|{
                            Style {
                                border: Border {
                                    color: theme.palette().primary,
                                    width: 1.0,
                                    ..Border::default()
                                },
                                ..Style::default()
                            }
                        })
                        .title_bar(
                            pane_grid::TitleBar::new(widget::center(text!("Entity Settings")).height(Length::Shrink))
                                .padding(5.0)
                        ),
                }
            },
        )
            .on_resize(10.0, Message::ResizePane)
            .into()
    }

    fn sheet_view(&self)->Element<Message> {
        widget::container(
            self.sheets[self.active_sheet]
                .main_view()
                .map(|m|Message::Sheet(m))
        )
            .width(Length::FillPortion(3))
            .height(Length::Fill)
            .into()
    }

    fn sheet_list_view(&self)->Element<Message> {
        widget::scrollable(
            column![
                // sheet selector
                widget::pick_list(
                    self.sheet_settings.as_slice(),
                    Some(&self.sheet_settings[self.active_sheet]),
                    |named_sheet|Message::SelectSheet(named_sheet.index),
                ),

                widget::button("New sheet")
                    .on_press(Message::NewSheet),

                widget::Space::with_height(15.0),

                widget::button("Laser condition editor")
                    .on_press(Message::ToggleConditionEditor),

                widget::Space::with_height(5.0),

                widget::button("Delete sheet")
                    .style(danger_button)
                    .on_press(Message::DeleteSheet),

                widget::Space::with_height(5.0),

                row![
                    "Rename: ",
                    widget::text_input(
                        "Sheet name",
                        self.sheet_settings[self.active_sheet].name.as_str(),
                    )
                        .on_input(|s|Message::RenameSheet(s)),
                ],

                row![
                    "Width: ",
                    widget::text_input(
                        "Width",
                        &self.sheet_size[0],
                    )
                        .on_input(Message::ChangeSheetWidth),
                ],

                row![
                    "Height: ",
                    widget::text_input(
                        "Height",
                        &self.sheet_size[1],
                    )
                        .on_input(Message::ChangeSheetHeight),
                ],

                widget::button("Save GCODE")
                    .on_press(Message::OpenGcodeSaveDialog)
            ]
                .padding(5.0)
        )
            .width(Length::Fill)
            .into()
    }

    fn model_list_view(&self)->Element<Message> {
        let mut column_items = Vec::new();

        column_items.push(row![
            widget::button("Load new model")
                .on_press(Message::OpenFilePicker),
        ].into());

        column_items.push(widget::Space::with_height(10.0).into());

        column_items.push(widget::pick_list(
            [ModelPaneState::ActiveModels, ModelPaneState::AllModels],
            Some(self.model_pane_state),
            |state|Message::ModelPaneState(state),
        )
            .into());

        match self.model_pane_state {
            ModelPaneState::ActiveModels=>{
                let active_models = &self.sheets[self.active_sheet].active_models;

                // a list of active models
                for (model, _) in active_models.iter() {
                    column_items.push(widget::Space::with_height(10.0).into());

                    column_items.push(widget::button(model.name())
                        .on_press(Message::AddModel(model.clone()))
                        .into()
                    );
                }
            },
            ModelPaneState::AllModels=>{
                let all_models = self.models.iter();

                // a list of active models
                for handle in all_models {
                    column_items.push(widget::Space::with_height(10.0).into());

                    column_items.push(row![
                        widget::button(widget::text(handle.name().to_string()))
                            .on_press(Message::AddModel(handle)),
                    ].into());
                }
            },
        }

        widget::scrollable(
            widget::column(column_items)
                .padding(5.0)
        )
            .width(Length::Fill)
            .into()
    }

    fn entity_params_view(&self)->Element<Message> {
        let params = self.entity_params.as_ref().unwrap();

        let store = self.conditions
            .get_store();
        let store = store.borrow();
        let conditions = store.iter()
            .map(|c|c.display())
            .collect::<Vec<_>>();
        let current_condition = store.get(params.laser_condition).display();
        drop(store);

        widget::scrollable(
            column![
                row![
                    text!("X: "),
                    widget::text_input(
                        "X",
                        &params.x,
                    )
                        .on_input(Message::EntityParamsX),
                ],

                row![
                    text!("Y: "),
                    widget::text_input(
                        "Y",
                        &params.y,
                    )
                        .on_input(Message::EntityParamsY),
                ],

                row![
                    text!("Angle: "),
                    column![
                        widget::slider(
                            0.0..=360.0,
                            params.angle,
                            Message::EntityParamsAngle,
                        ).step(1.0),
                        widget::TextInput::new(
                            "Angle",
                            params.angle_string.as_str(),
                        )
                            .on_input(Message::EntityParamsAngleString),
                    ],
                ],

                row![
                    text!("Scale: "),
                    widget::text_input(
                        "Scale",
                        &params.scale,
                    )
                        .on_input(Message::EntityParamsScale),
                ],

                row![
                    widget::checkbox(
                        "Flip: ",
                        params.flip,
                    )
                        .on_toggle(Message::EntityParamsFlip),
                ],

                widget::pick_list(
                    conditions,
                    Some(current_condition),
                    |c|Message::EntityParamsCondition(c.id),
                ),

                widget::Space::with_height(25.0),

                widget::button("Delete entity")
                    .style(danger_button)
                    .on_press(Message::DeleteEntity),
            ]
                .padding(5.0)
        )
            .width(Length::Fill)
            .into()
    }

    pub fn update(&mut self, msg: Message)->Task<Message> {
        match msg {
            Message::Sheet(msg)=>{
                match msg {
                    SheetMessage::Select(id)|SheetMessage::SelectMove(id, _)=>{
                        let mt = &self.sheets[self.active_sheet]
                            .entities[&id].1;
                        let rotation = mt.transform.rotation.normalized();
                        let mut vec = Vector::new(1.0, 0.0);
                        rotation.rotate_vec(&mut vec);
                        let mut angle = vec.y.atan2(vec.x).to_degrees();
                        if angle < 0.0 {
                            angle += 360.0;
                        }
                        self.entity_params = Some(EntityParams {
                            id,
                            x: mt.transform.translation.x.to_string(),
                            y: mt.transform.translation.y.to_string(),
                            angle,
                            angle_string: angle.to_string(),
                            scale: mt.transform.scale.to_string(),
                            flip: mt.flip,
                            laser_condition: mt.laser_condition,
                        });

                        self.close_entity_params();
                        self.open_entity_params();
                    },
                    SheetMessage::Deselect(_)=>{
                        self.entity_params = None;
                        self.close_entity_params();
                    },
                    SheetMessage::Move(..)=>{
                        if let Some(params) = &mut self.entity_params {
                            let entity = self.sheets[self.active_sheet]
                                .entities[&params.id].1;

                            params.x = entity.transform.translation.x.to_string();
                            params.y = entity.transform.translation.y.to_string();
                        }
                    },
                    _=>{},
                }
                return self.sheets[self.active_sheet]
                    .main_update(msg)
                    .map(|m|Message::Sheet(m));
            },
            Message::Condition(msg)=>{
                match msg {
                    ConditionMessage::CloseEditor=>{
                        self.close_condition_editor();
                    },
                    ConditionMessage::RecalcSheet=>{
                        self.sheets[self.active_sheet].recalc_paths();
                    },
                    _=>{},
                }

                return self.conditions.update(msg).map(Message::Condition);
            },
            Message::RenameSheet(name)=>self.sheet_settings[self.active_sheet].name = name,
            Message::NewSheet=>{
                self.active_sheet = self.sheets.len();
                self.sheet_settings.push(SheetIndex {
                    name: "New Sheet".into(),
                    gcode: None,
                    index: self.sheets.len(),
                });
                self.sheets.push(Sheet::new(self.models.clone(), self.conditions.get_store()));

                self.sheet_size = [
                    format!("{}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::DeleteSheet=>{
                // ensure there is at least 1 sheet so we don't have errors
                if self.sheets.len() == 1 {
                    self.sheets.clear();
                    self.sheet_settings.clear();

                    self.sheet_settings.push(SheetIndex {
                        name: "New Sheet".into(),
                        gcode: None,
                        index: self.sheets.len(),
                    });
                    self.sheets.push(Sheet::new(self.models.clone(), self.conditions.get_store()));
                } else {
                    self.sheets.remove(self.active_sheet);
                    self.sheet_settings.remove(self.active_sheet);
                    self.active_sheet = 0;
                }

                self.sheet_size = [
                    format!("{}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::SelectSheet(idx)=>{
                self.active_sheet = idx;

                self.sheet_size = [
                    format!("{}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::ResizePane(event)=>self.panes.resize(event.split, event.ratio),
            Message::AddModel(handle)=>{

                self.sheets[self.active_sheet]
                    .add_model_from_handle(handle, 1, self.conditions.default_condition());
            },
            Message::ModelPaneState(state)=>self.model_pane_state = state,
            Message::OpenFilePicker=>{
                let future = AsyncFileDialog::new()
                    .add_filter("DXF Files", &["dxf"])
                    .set_title("Load DXF files")
                    .pick_files();
                return Task::perform(future,Message::LoadModel);
            },
            Message::LoadModel(opt_files)=>if let Some(files) = opt_files {
                for file in files {
                    // TODO(error handling): Make this not crash when we have an error

                    let model = Model::load(file.path())
                        .expect("Could not load files");

                    let handle = self.models.add(model);
                    self.sheets[self.active_sheet]
                        .add_model_from_handle(handle, 1, self.conditions.default_condition());
                }
            },
            Message::EntityParamsX(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.x = val;
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .translation.x = f;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::EntityParamsY(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.y = val;
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .translation.y = f;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::EntityParamsAngle(val)=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                params.angle = val;
                params.angle_string = val.to_string();
                self.sheets[self.active_sheet]
                    .entities.get_mut(&params.id)
                    .unwrap().1
                    .transform
                    .rotation = Rotation::from_angle(val.to_radians());

                self.sheets[self.active_sheet].recalc_paths();
            },
            Message::EntityParamsAngleString(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.angle = f;
                    params.angle_string = val;
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .rotation = Rotation::from_angle(f.to_radians());

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::EntityParamsScale(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    if val.len() > 0 {
                        self.sheets[self.active_sheet]
                            .entities.get_mut(&params.id)
                            .unwrap().1
                            .transform
                            .scale = f;
                    }

                    params.scale = val;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::EntityParamsFlip(val)=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                params.flip = val;
                self.sheets[self.active_sheet]
                    .entities.get_mut(&params.id)
                    .unwrap().1
                    .flip = val;

                self.sheets[self.active_sheet].recalc_paths();
            },
            Message::EntityParamsCondition(id)=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                params.laser_condition = id;
                self.sheets[self.active_sheet]
                    .entities.get_mut(&params.id)
                    .unwrap().1
                    .laser_condition = id;

                self.sheets[self.active_sheet].recalc_paths();
            },
            Message::DeleteEntity=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                self.sheets[self.active_sheet]
                    .delete_entity(params.id);

                self.entity_params = None;
                self.close_entity_params();
            },
            Message::ChangeSheetWidth(val)=>{
                if let Some(f) = parse_float(&val) {
                    self.sheet_size[0] = val;

                    self.sheets[self.active_sheet].change_width(f);
                }
            },
            Message::ChangeSheetHeight(val)=>{
                if let Some(f) = parse_float(&val) {
                    self.sheet_size[1] = val;

                    self.sheets[self.active_sheet].change_height(f);
                }
            },
            Message::SaveGcode(opt_file)=>{
                if let Some(file) = opt_file {
                    let mut path = file.path().to_path_buf();

                    // ensure there is a file extension
                    if path.extension().is_none() {
                        path.set_extension(".gcode");
                    }

                    let gcode = self.sheet_settings[self.active_sheet]
                        .gcode
                        .take()
                        .unwrap_or(String::new());

                    match std::fs::write(path, gcode) {
                        Err(e)=>eprintln!("Error saving GCODE file: {e}"),
                        _=>eprintln!("Saved GCODE file"),
                    }
                }
            },
            Message::OpenGcodeSaveDialog=>{
                let start = std::time::Instant::now();

                let settings = &mut self.sheet_settings[self.active_sheet];
                let gcode = self.sheets[self.active_sheet]
                    .generate_gcode(settings.name.as_str());
                settings.gcode = Some(gcode);

                let elapsed = start.elapsed();
                eprintln!("GCODE Generated in {elapsed:?}");

                let future = AsyncFileDialog::new()
                    .add_filter("GCODE Files", &["gcode", "nc"])
                    .set_title("Save GCODE file")
                    .set_file_name(format!("{}.gcode", self.sheet_settings[self.active_sheet].name))
                    .save_file();
                return Task::perform(future, Message::SaveGcode);
            },
            Message::ToggleConditionEditor=>{
                if !self.open_condition_editor() {
                    self.close_condition_editor();
                }
            },
            Message::Iced(event)=>{
                if let Event::Window(window::Event::CloseRequested) = event {
                    self.conditions.save();
                    return window::get_latest().and_then(window::close);
                }
            }
        }

        return Task::none();
    }

    fn open_condition_editor(&mut self)->bool {
        let pane = self.panes.iter()
            .map(|(p,s)|(*p,*s))
            .find(|(_,state)|*state==ProgramPane::Sheet);
        if let Some((pane, _)) = pane {
            *self.panes
                .get_mut(pane)
                .unwrap() = ProgramPane::ConditionEditor;
            return true;
        }

        return false;
    }

    fn close_condition_editor(&mut self)->bool {
        let pane = self.panes.iter()
            .map(|(p,s)|(*p,*s))
            .find(|(_,state)|*state==ProgramPane::ConditionEditor);
        if let Some((pane, _)) = pane {
            *self.panes
                .get_mut(pane)
                .unwrap() = ProgramPane::Sheet;
            return true;
        }

        return false;
    }

    fn close_entity_params(&mut self) {
        let pane = self.panes.iter()
            .map(|(p,s)|(*p,*s))
            .find(|(_,state)|*state==ProgramPane::EntityParams);
        if let Some((pane, _)) = pane {
            *self.panes
                .get_mut(pane)
                .unwrap() = ProgramPane::ModelList;
        }
    }

    fn open_entity_params(&mut self) {
        let pane = self.panes.iter()
            .map(|(p,s)|(*p,*s))
            .find(|(_,state)|*state==ProgramPane::ModelList);
        if let Some((pane, _)) = pane {
            *self.panes
                .get_mut(pane)
                .unwrap() = ProgramPane::EntityParams;
        }
    }
}
impl Default for MainProgram {
    fn default()->Self {
        use pane_grid::{
            Configuration,
            Axis,
        };
        let conditions = ConditionEditor::load();
        let models = ModelStore::new();
        let sheet = Sheet::new(models.clone(), conditions.get_store());

        MainProgram {
            sheet_size: [
                format!("{}", sheet.sheet_size.x),
                format!("{}", sheet.sheet_size.y),
            ],
            panes: PaneState::with_configuration(Configuration::Split {
                axis: Axis::Vertical,
                ratio: 0.8,
                a: Box::new(Configuration::Pane(ProgramPane::Sheet)),
                b: Box::new(Configuration::Split {
                    axis: Axis::Horizontal,
                    ratio: 0.45,
                    a: Box::new(Configuration::Pane(ProgramPane::SheetList)),
                    b: Box::new(Configuration::Pane(ProgramPane::ModelList)),
                }),
            }),
            models,
            active_sheet: 0,
            sheets: vec![sheet],
            sheet_settings: vec![SheetIndex {
                name: "New Sheet".into(),
                gcode: None,
                index: 0,
            }],
            model_pane_state: ModelPaneState::AllModels,
            entity_params: None,
            conditions,
        }
    }
}


fn main()->iced::Result {
    iced::application(
        "LaserCAM",
        MainProgram::update,
        MainProgram::view,
    )
        .subscription(|_|event::listen().map(Message::Iced))
        .exit_on_close_request(false)
        .centered()
        .theme(|_|Theme::Dark)
        .run()
}

pub fn parse_float(s: &str)->Option<f64> {
    if s.len() == 0 {
        return Some(0.0);
    }

    s.parse().ok()
}

pub fn parse_u16(s: &str)->Option<u16> {
    if s.len() == 0 {
        return Some(0);
    }

    let num: Option<u32> = s.parse().ok();
    num.map(|n|if n > u16::MAX as u32 {u16::MAX} else {n as u16})
}

pub fn danger_button(theme: &Theme, status: ButtonStatus)->widget::button::Style {
    let palette = theme.extended_palette();
    let danger = palette.danger;
    match status {
        ButtonStatus::Active=>widget::button::Style {
            background: Some(Background::Color(danger.base.color)),
            text_color: danger.base.text,
            ..Default::default()
        },
        ButtonStatus::Hovered=>widget::button::Style {
            background: Some(Background::Color(danger.weak.color)),
            text_color: danger.weak.text,
            ..Default::default()
        },
        ButtonStatus::Pressed=>widget::button::Style {
            background: Some(Background::Color(danger.strong.color)),
            text_color: danger.strong.text,
            ..Default::default()
        },
        ButtonStatus::Disabled=>widget::button::Style {
            background: Some(Background::Color(danger.weak.color)),
            text_color: danger.weak.text,
            ..Default::default()
        },
    }
}
