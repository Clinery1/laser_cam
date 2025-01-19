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
    Background,
    Border,
    Length,
    Element,
    Theme,
    Task,
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


mod model;
mod sheet;
mod gcode;


pub type Point = ultraviolet::DVec2;
pub type Vector = ultraviolet::DVec2;
pub type Rotation = ultraviolet::DRotor2;
pub type Transform = ultraviolet::DSimilarity2;
pub type Translation = ultraviolet::DVec2;


/// Main program state changes
#[derive(Debug, Clone)]
pub enum Message {
    Sheet(SheetMessage),

    RenameSheet(String),
    SelectSheet(usize),
    NewSheet,
    DeleteSheet,
    ChangeSheetWidth(String),
    ChangeSheetHeight(String),
    ChangeSheetFeed(String),
    ChangeSheetPower(String),

    AddModel(ModelHandle),

    ResizePane(ResizeEvent),

    ModelPaneState(ModelPaneState),

    OpenFilePicker,
    LoadModel(Option<Vec<FileHandle>>),

    OpenGcodeSaveDialog,
    SaveGcode(Option<FileHandle>),

    ModelParamsX(String),
    ModelParamsY(String),
    ModelParamsAngle(f64),
    ModelParamsAngleString(String),
    ModelParamsScale(String),
    ModelParamsFlip(bool),
    DeleteEntity,
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProgramPane {
    Sheet,
    SheetList,
    ModelList,
    ModelParams,
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
    pub feedrate: u16,
    pub laser_power: u16,
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
    rotation: f64,
    scale: String,
    flip: bool,
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
}
impl MainProgram {
    pub fn view(&self)->Element<Message> {
        widget::pane_grid(
            &self.panes,
            |_pane, state, _is_maximized|{
                match state {
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
                    ProgramPane::ModelParams=>pane_grid::Content::new(self.entity_params_view())
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
                row![
                    // sheet selector
                    widget::pick_list(
                        self.sheet_settings.as_slice(),
                        Some(&self.sheet_settings[self.active_sheet]),
                        |named_sheet|Message::SelectSheet(named_sheet.index),
                    ),

                    widget::button("New sheet")
                        .on_press(Message::NewSheet),
                ],

                widget::Space::with_height(15.0),

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

                row![
                    "Feed: ",
                    widget::text_input(
                        "Feed",
                        &self.sheet_settings[self.active_sheet].feedrate.to_string(),
                    )
                        .on_input(Message::ChangeSheetFeed),
                ],

                row![
                    "Laser Power: ",
                    widget::text_input(
                        "Power",
                        &self.sheet_settings[self.active_sheet].laser_power.to_string(),
                    )
                        .on_input(Message::ChangeSheetPower),
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

        widget::scrollable(
            column![
                row![
                    text!("X: "),
                    widget::text_input(
                        "X",
                        &params.x,
                    )
                        .on_input(Message::ModelParamsX),
                ],

                row![
                    text!("Y: "),
                    widget::text_input(
                        "Y",
                        &params.y,
                    )
                        .on_input(Message::ModelParamsY),
                ],

                row![
                    text!("Angle: "),
                    column![
                        widget::slider(
                            0.0..=360.0,
                            params.rotation,
                            Message::ModelParamsAngle,
                        ),
                        widget::TextInput::new(
                            "Angle",
                            format!("{:.6}", params.rotation).as_str(),
                        )
                            .on_input(Message::ModelParamsAngleString),
                    ],
                ],

                row![
                    text!("Scale: "),
                    widget::text_input(
                        "Scale",
                        &params.scale,
                    )
                        .on_input(Message::ModelParamsScale),
                ],

                row![
                    widget::checkbox(
                        "Flip: ",
                        params.flip,
                    )
                        .on_toggle(Message::ModelParamsFlip),
                ],

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
                    SheetMessage::Select(id)=>{
                        let mt = &self.sheets[self.active_sheet]
                            .entities[&id].1;
                        let rotation = mt.transform.rotation.normalized();
                        let mut vec = Vector::new(1.0, 0.0);
                        rotation.rotate_vec(&mut vec);
                        let angle = vec.y.atan2(vec.x).to_degrees();
                        self.entity_params = Some(EntityParams {
                            id,
                            x: format!("{:.6}", mt.transform.translation.x),
                            y: format!("{:.6}", mt.transform.translation.y),
                            rotation: angle,
                            scale: format!("{:.6}", mt.transform.scale),
                            flip: mt.flip,
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

                            params.x = format!("{:.6}", entity.transform.translation.x);
                            params.y = format!("{:.6}", entity.transform.translation.y);
                        }
                    },
                    _=>{},
                }
                return self.sheets[self.active_sheet]
                    .main_update(msg)
                    .map(|m|Message::Sheet(m));
            },
            Message::RenameSheet(name)=>self.sheet_settings[self.active_sheet].name = name,
            Message::NewSheet=>{
                self.active_sheet = self.sheets.len();
                self.sheet_settings.push(SheetIndex {
                    name: "New Sheet".into(),
                    feedrate: 1000,
                    laser_power: 100,
                    gcode: None,
                    index: self.sheets.len(),
                });
                self.sheets.push(Sheet::new(self.models.clone()));

                self.sheet_size = [
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::DeleteSheet=>{
                // ensure there is at least 1 sheet so we don't have errors
                if self.sheets.len() == 1 {
                    self.sheets.clear();
                    self.sheet_settings.clear();

                    self.sheet_settings.push(SheetIndex {
                        name: "New Sheet".into(),
                        feedrate: 1000,
                        laser_power: 100,
                        gcode: None,
                        index: self.sheets.len(),
                    });
                    self.sheets.push(Sheet::new(self.models.clone()));
                } else {
                    self.sheets.remove(self.active_sheet);
                    self.sheet_settings.remove(self.active_sheet);
                    self.active_sheet = 0;
                }

                self.sheet_size = [
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::SelectSheet(idx)=>{
                self.active_sheet = idx;

                self.sheet_size = [
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.x),
                    format!("{:.6}", self.sheets[self.active_sheet].sheet_size.y),
                ];
            },
            Message::ResizePane(event)=>self.panes.resize(event.split, event.ratio),
            Message::AddModel(handle)=>{
                self.sheets[self.active_sheet]
                    .add_model_from_handle(handle, 1);
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
                        .add_model_from_handle(handle, 1);
                }
            },
            Message::ModelParamsX(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.x = format!("{:.6}", f);
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .translation.x = f;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ModelParamsY(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.y = format!("{:.6}", f);
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .translation.y = f;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ModelParamsAngle(val)=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                params.rotation = val;
                self.sheets[self.active_sheet]
                    .entities.get_mut(&params.id)
                    .unwrap().1
                    .transform
                    .rotation = Rotation::from_angle(val.to_radians());

                self.sheets[self.active_sheet].recalc_paths();
            },
            Message::ModelParamsAngleString(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.rotation = f;
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .rotation = Rotation::from_angle(f.to_radians());

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ModelParamsScale(val)=>{
                if let Some(f) = parse_float(&val) {
                    let Some(params) = self.entity_params
                        .as_mut() else {return Task::none()};

                    params.scale = format!("{:.6}", f);
                    self.sheets[self.active_sheet]
                        .entities.get_mut(&params.id)
                        .unwrap().1
                        .transform
                        .scale = f;

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ModelParamsFlip(val)=>{
                let Some(params) = self.entity_params
                    .as_mut() else {return Task::none()};

                params.flip = val;
                self.sheets[self.active_sheet]
                    .entities.get_mut(&params.id)
                    .unwrap().1
                    .flip = val;

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
                    self.sheets[self.active_sheet]
                        .sheet_size.x = f;

                    self.sheet_size[0] = format!("{:.6}", f);

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ChangeSheetHeight(val)=>{
                if let Some(f) = parse_float(&val) {
                    self.sheets[self.active_sheet]
                        .sheet_size.y = f;

                    self.sheet_size[1] = format!("{:.6}", f);

                    self.sheets[self.active_sheet].recalc_paths();
                }
            },
            Message::ChangeSheetFeed(val)=>{
                if let Some(n) = parse_num(&val) {
                    self.sheet_settings[self.active_sheet]
                        .feedrate = n;
                }
            },
            Message::ChangeSheetPower(val)=>{
                if let Some(n) = parse_num(&val) {
                    self.sheet_settings[self.active_sheet]
                        .laser_power = n;
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
                    .generate_gcode(settings.laser_power, settings.feedrate, settings.name.as_str());
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
        }

        return Task::none();
    }

    fn close_entity_params(&mut self) {
        let pane = self.panes.iter()
            .map(|(p,s)|(*p,*s))
            .find(|(_,state)|*state==ProgramPane::ModelParams);
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
                .unwrap() = ProgramPane::ModelParams;
        }
    }
}
impl Default for MainProgram {
    fn default()->Self {
        let models = ModelStore::new();
        let sheet = Sheet::new(models.clone());

        MainProgram {
            sheet_size: [
                format!("{:.6}", sheet.sheet_size.x),
                format!("{:.6}", sheet.sheet_size.y),
            ],
            panes: PaneState::with_configuration(pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.8,
                a: Box::new(pane_grid::Configuration::Pane(ProgramPane::Sheet)),
                b: Box::new(pane_grid::Configuration::Split {
                    axis: pane_grid::Axis::Horizontal,
                    ratio: 0.45,
                    a: Box::new(pane_grid::Configuration::Pane(ProgramPane::SheetList)),
                    b: Box::new(pane_grid::Configuration::Pane(ProgramPane::ModelList)),
                }),
            }),
            models,
            active_sheet: 0,
            sheets: vec![sheet],
            sheet_settings: vec![SheetIndex {
                name: "New Sheet".into(),
                feedrate: 1000,
                laser_power: 100,
                gcode: None,
                index: 0,
            }],
            model_pane_state: ModelPaneState::AllModels,
            entity_params: None,
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

pub fn p_conv(uv: Point)->iced::Point {
    iced::Point {
        x: uv.x as f32,
        y: uv.y as f32,
    }
}

fn parse_float(s: &str)->Option<f64> {
    if s.len() == 0 {
        return Some(0.0);
    }

    s.parse().ok()
}

fn parse_num(s: &str)->Option<u16> {
    if s.len() == 0 {
        return Some(0);
    }

    let num: Option<u32> = s.parse().ok();
    num.map(|n|if n > u16::MAX as u32 {u16::MAX} else {n as u16})
}

fn danger_button(theme: &Theme, status: ButtonStatus)->widget::button::Style {
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
