#![feature(default_free_fn)]
#![feature(let_chains)]
#![feature(type_alias_impl_trait)]
#![feature(int_roundings)]

mod ui;

use std::{
    default::default,
    future::Future,
    io::Cursor,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{
    app::AppExit,
    input::keyboard::KeyboardInput,
    prelude::*,
    render::render_resource::Extent3d,
    sprite::{Anchor, Sprite, SpriteBundle},
    text::TextStyle,
    ui::{JustifyContent, Size, Style, UiRect, Val},
    window::{PrimaryWindow, Window, WindowCloseRequested},
    DefaultPlugins,
};
use bevy_egui::EguiPlugin;
use bevy_prototype_lyon::prelude::*;
use futures::io::BufWriter;
use image::{DynamicImage, ImageFormat};
use leafwing_input_manager::{
    prelude::{ActionState, DualAxis, InputManagerPlugin, InputMap},
    user_input::{InputKind, Modifier},
    Actionlike, InputManagerBundle,
};
use rfd::FileHandle;
use serde::{Deserialize, Serialize};
use ui::UiState;

#[derive(Actionlike, Clone, Debug)]
enum Input2 {
    LeftClick,
    Pan,
    ToolSelect,
    ToolMoveAnchor,
    ToolCreateHitbox,
    ToolCreateHurtbox,
    ToolCreateCollisionbox,
    New,
    Open,
    Save,
    SaveAs,
    AddFrame,
    DeleteFrame,
    DeleteSelected,
    Undo,
    Redo,
    PrevFrame,
    NextFrame,
    TogglePlayback,
}

fn main() {
    println!("{}", InteractionLock::All < InteractionLock::Playback);

    let mut app = App::new();
    app.insert_resource(EditorState::new())
        .insert_non_send_resource(PendingFileDialog { action: None })
        .insert_resource(Msaa::Off)
        .insert_resource(InteractionLock::None)
        .insert_resource(LastMousePos(default()))
        .insert_resource(MouseDelta(default()))
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    close_when_requested: false,
                    ..default()
                }),
        )
        .add_plugin(EguiPlugin)
        .add_plugin(ShapePlugin)
        .add_plugin(InputManagerPlugin::<Input2>::default())
        .add_startup_system(start)
        .add_systems((
            mouse_delta.before(mouse_interaction),
            poll_pending_file_dialog,
            // interaction,
            mouse_interaction,
            keyboard_interaction,
            render.after(mouse_interaction),
            exit_system,
            on_close,
        ));
    app.get_schedule_mut(CoreSchedule::FixedUpdate)
        .unwrap()
        .add_system(animator);
    ui::add_systems(&mut app);

    app.run();
}

fn start(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    asset_server: Res<AssetServer>,
) {
    let mut input_map = InputMap::default();
    input_map.insert(MouseButton::Left, Input2::LeftClick);
    input_map.insert(KeyCode::Space, Input2::Pan);
    input_map.insert(KeyCode::S, Input2::ToolSelect);
    input_map.insert(KeyCode::A, Input2::ToolMoveAnchor);
    input_map.insert(KeyCode::H, Input2::ToolCreateHitbox);
    input_map.insert(KeyCode::G, Input2::ToolCreateHurtbox);
    input_map.insert(KeyCode::C, Input2::ToolCreateCollisionbox);
    input_map.insert_modified(Modifier::Control, KeyCode::N, Input2::New);
    input_map.insert_modified(Modifier::Control, KeyCode::O, Input2::Open);
    input_map.insert_modified(Modifier::Control, KeyCode::S, Input2::Save);
    input_map.insert_chord(
        [
            InputKind::from(Modifier::Control),
            Modifier::Shift.into(),
            KeyCode::S.into(),
        ],
        Input2::SaveAs,
    );
    input_map.insert(KeyCode::F, Input2::AddFrame);
    input_map.insert(KeyCode::Delete, Input2::DeleteSelected);
    input_map.insert_modified(Modifier::Control, KeyCode::Delete, Input2::DeleteFrame);
    input_map.insert_modified(Modifier::Control, KeyCode::Z, Input2::Undo);
    input_map.insert_chord(
        [
            InputKind::from(Modifier::Control),
            Modifier::Shift.into(),
            KeyCode::Z.into(),
        ],
        Input2::Redo,
    );
    input_map.insert(KeyCode::Left, Input2::PrevFrame);
    input_map.insert(KeyCode::Right, Input2::NextFrame);
    input_map.insert(KeyCode::K, Input2::TogglePlayback);

    commands.spawn(InputManagerBundle::<Input2> {
        action_state: default(),
        input_map,
    });

    commands.spawn(Camera2dBundle {
        projection: OrthographicProjection {
            scale: 0.1,
            ..default()
        },
        ..default()
    });

    ui::build_ui(&mut commands);

    commands.spawn(SpriteBundle {
        texture: Handle::default(),
        sprite: Sprite {
            anchor: Anchor::TopLeft,
            ..default()
        },
        ..default()
    });

    let line = shapes::Line(
        Vec2 {
            x: -10000.0,
            y: 0.0,
        },
        Vec2 { x: 10000.0, y: 0.0 },
    );
    commands.spawn((
        ShapeBundle {
            path: GeometryBuilder::build_as(&line),
            ..default()
        },
        Stroke::new(Color::BLUE, 0.15),
    ));

    for i in -10..=10 {
        if i == 0 {
            continue;
        }
        let line = shapes::Line(
            Vec2 {
                x: -10000.0,
                y: i as f32 * 8.0,
            },
            Vec2 {
                x: 10000.0,
                y: i as f32 * 8.0,
            },
        );
        commands.spawn((
            ShapeBundle {
                path: GeometryBuilder::build_as(&line),
                ..default()
            },
            Stroke::new(Color::rgba(0.0, 0.0, 0.0, 0.5), 0.1),
        ));
    }

    let line = shapes::Line(
        Vec2 {
            x: 0.0,
            y: -10000.0,
        },
        Vec2 { x: 0.0, y: 10000.0 },
    );
    commands.spawn((
        ShapeBundle {
            path: GeometryBuilder::build_as(&line),
            ..default()
        },
        Stroke::new(Color::RED, 0.15),
    ));

    for i in -10..=10 {
        if i == 0 {
            continue;
        }
        let line = shapes::Line(
            Vec2 {
                x: i as f32 * 8.0,
                y: -10000.0,
            },
            Vec2 {
                x: i as f32 * 8.0,
                y: 10000.0,
            },
        );
        commands.spawn((
            ShapeBundle {
                path: GeometryBuilder::build_as(&line),
                ..default()
            },
            Stroke::new(Color::rgba(0.0, 0.0, 0.0, 0.5), 0.1),
        ));
    }
}

fn load(path: impl AsRef<Path>, assets: &mut Assets<Image>) -> Animation {
    let animation_file_data: AnimationFileData =
        serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();

    let cell_width = animation_file_data.spritesheet_info.cell_width as u32;
    let cell_height = animation_file_data.spritesheet_info.cell_height as u32;
    let cols = animation_file_data.spritesheet_info.columns as u32;
    let frame_count = animation_file_data.spritesheet_info.frame_count as u32;

    let image = image::load_from_memory(&animation_file_data.spritesheet).unwrap();

    let mut frames = vec![];

    for i in 0..frame_count {
        let x = i % cols;
        let y = i / cols;

        let handle = assets.add(Image::from_dynamic(
            image.crop_imm(x * cell_width, y * cell_height, cell_width, cell_height),
            true,
        ));
        let frame_info = &animation_file_data.spritesheet_info.frame_data[i as usize];
        println!("{}", frame_info.origin);
        let offset = frame_info.origin;
        println!("{}", offset);
        let delay = frame_info.delay;

        frames.push(Frame {
            image: handle,
            offset,
            root_motion: Vec2::ZERO,
            delay,
        });
    }

    Animation {
        timeline: Timeline { frames },
    }
}

#[derive(Serialize, Deserialize)]
struct ImageHandle {
    path: String,
    #[serde(skip)]
    image: Handle<Image>,
}

#[derive(Resource)]
struct EditorState {
    current_animation: Animation,
    current_basepath: Option<String>,
    current_frame: usize,
    action_list: Vec<Action>,
    undo_depth: usize,
    drag_starting_pos: Option<Vec2>,
    selected_tool: Tool,
    currently_selected_box: Option<usize>,
    has_saved: bool,
    action_after_save: Option<Box<dyn FnOnce(&mut EditorState) + Send + Sync>>,
    exit_now: bool,
    with_pfd: Option<Box<dyn FnOnce(&mut PendingFileDialog) + Send + Sync>>,
    animation_running: bool,
    frames_since_last_frame: usize,
}

impl EditorState {
    fn new() -> Self {
        Self {
            current_animation: Animation::new(),
            current_basepath: None,
            current_frame: 0,
            action_list: vec![],
            undo_depth: 0,
            drag_starting_pos: None,
            selected_tool: Tool::Select,
            currently_selected_box: None,
            has_saved: true,
            action_after_save: None,
            exit_now: false,
            with_pfd: None,
            animation_running: false,
            frames_since_last_frame: 0,
        }
    }

    fn confirm_if_unsaved(
        &mut self,
        ui_state: &mut UiState,
        interaction_lock: &mut InteractionLock,
        action: impl FnOnce(&mut Self) + Send + Sync + 'static,
    ) {
        if self.has_saved {
            action(self);
        } else {
            self.animation_running = false;
            *interaction_lock = InteractionLock::All;
            ui_state.show_save_menu = true;
            self.action_after_save = Some(Box::new(action));
        }
    }

    fn save(&mut self, pending_file_dialog: &mut PendingFileDialog, assets: &Assets<Image>) {
        if let Some(path) = self.current_basepath.clone() {
            self.save_to(path, assets);
        } else {
            let future = rfd::AsyncFileDialog::new()
                .add_filter("anim", &["anim"])
                .save_file();
            pending_file_dialog.action = Some(FileAction::Save(Box::pin(future)));
        }
    }

    fn save_to(&mut self, path: impl AsRef<Path>, assets: &Assets<Image>) {
        let images = self
            .current_animation
            .timeline
            .frames
            .iter()
            .map(|ih| {
                let img = assets.get(&ih.image).unwrap();
                let img = img.clone().try_into_dynamic().unwrap();
                let offset = ih.offset;
                println!("{}", offset);
                (img, offset, ih.delay)
            })
            .collect::<Vec<_>>();

        let image_count = images.len();

        let pre_min_image_width = images.iter().map(|(img, _, _)| img.width()).min();
        let pre_max_image_width = images.iter().map(|(img, _, _)| img.width()).max();
        let pre_min_image_height = images.iter().map(|(img, _, _)| img.height()).min();
        let pre_max_image_height = images.iter().map(|(img, _, _)| img.height()).max();

        let mut image_bb_width = 0;
        let mut image_bb_height = 0;

        let mut cropped_images = vec![];

        for (image, offset, delay) in images {
            let pixels = image.as_rgba8().unwrap();

            let mut left = pixels.width();
            let mut right = 0;
            let mut top = pixels.height();
            let mut bottom = 0;

            for x in 0..pixels.width() {
                for y in 0..pixels.height() {
                    let has_pixel = pixels[(x, y)][3] != 0;

                    if has_pixel {
                        left = x.min(left);
                        right = x;
                        top = y.min(top);
                        bottom = y.max(bottom);
                    }
                }
            }

            let (width, height) = if right < left {
                (0, 0)
            } else {
                (right - left + 1, (bottom - top + 1))
            };

            println!("{width}, {height}");

            let cropped_image = image.crop_imm(left, top, width, height);

            let new_offset = Vec2::new(offset.x - left as f32, offset.y - top as f32);
            cropped_images.push((cropped_image, new_offset, delay));
            println!("{new_offset}");

            image_bb_width = image_bb_width.max(width);
            image_bb_height = image_bb_height.max(height);
        }

        let mut expanded_images = vec![];

        for (image, offset, delay) in cropped_images {
            let diff_x = image_bb_width - image.width();
            let diff_y = image_bb_height - image.height();

            let pad_left = diff_x / 2;
            let pad_right = diff_x - pad_left;
            let pad_top = diff_y / 2;
            let pad_bot = diff_y - pad_top;

            println!(
                "bb: {image_bb_width}, {image_bb_height} | width: {}, {}",
                image.width(),
                image.height()
            );
            println!("left: {pad_left}, right: {pad_right}, top: {pad_top}, bot: {pad_bot}");

            let mut expanded_image = DynamicImage::new_rgba8(image_bb_width, image_bb_height);
            let pixels = expanded_image.as_mut_rgba8().unwrap();
            let orig_pixels = image.as_rgba8().unwrap();

            for x in 0..image_bb_width {
                for y in 0..image_bb_height {
                    if x < pad_left
                        || image_bb_width - x - 1 < pad_right
                        || y < pad_top
                        || image_bb_height - y - 1 < pad_bot
                    {
                        pixels[(x, y)].0 = [0; 4];
                    } else {
                        pixels[(x, y)] = orig_pixels[(x - pad_left, y - pad_top)];
                    }
                }
            }

            expanded_images.push((
                expanded_image,
                offset + Vec2::new(pad_left as _, pad_top as _),
                delay,
            ));
        }

        // for (index, (img, offset, delay)) in expanded_images.iter().enumerate() {
        //     let mut path = PathBuf::from(path.as_ref());
        //     let file_name = path.file_name().unwrap();
        //     let new_file_name = format!("{}.{index}.png", file_name.to_string_lossy());
        //     path.set_file_name(new_file_name);
        //     img.save(path).unwrap();
        // }

        let mut cols = expanded_images.len();

        for c in (1..=expanded_images.len()).rev() {
            let r = expanded_images.len().div_ceil(c);

            let w = c * image_bb_width as usize;
            let h = r * image_bb_height as usize;

            if h > w {
                break;
            }
            cols = c;
        }

        let cols = cols as u32;
        let rows = expanded_images.len().div_ceil(cols as usize) as u32;

        let mut spritesheet =
            DynamicImage::new_rgba8(cols as u32 * image_bb_width, rows as u32 * image_bb_height);
        let spritesheet_pixels = spritesheet.as_mut_rgba8().unwrap();

        for ix in 0..cols {
            for iy in 0..rows {
                let index = (iy * cols + ix) as usize;
                if index as usize >= expanded_images.len() {
                    continue;
                }

                let original_pixels = expanded_images[index].0.as_rgba8().unwrap();
                for lx in 0..image_bb_width {
                    for ly in 0..image_bb_height {
                        let tx = ix * image_bb_width + lx;
                        let ty = iy * image_bb_height + ly;

                        spritesheet_pixels[(tx, ty)] = original_pixels[(lx, ly)];
                    }
                }
            }
        }

        // spritesheet
        //     .save(format!("{}.all.png", path.as_ref().to_string_lossy()))
        //     .unwrap();

        let frame_data = SpritesheetInfo {
            cell_width: image_bb_width as _,
            cell_height: image_bb_height as _,
            columns: cols as _,
            frame_count: expanded_images.len(),
            frame_data: expanded_images
                .into_iter()
                .map(|(_, offset, delay)| FrameData {
                    delay,
                    origin: offset,
                    root_motion: Vec2::ZERO,
                    hitboxes: (),
                })
                .collect(),
        };

        // serde_json::to_writer_pretty(
        //     std::fs::File::create(format!("{}.json", path.as_ref().to_string_lossy())).unwrap(),
        //     &frame_data,
        // )
        // .unwrap();

        let mut bytes = vec![];
        let mut cursor = Cursor::new(&mut bytes);
        spritesheet.write_to(&mut cursor, ImageFormat::Png).unwrap();

        let animation_file_data = AnimationFileData {
            spritesheet: bytes,
            spritesheet_info: frame_data,
        };

        serde_json::to_writer_pretty(
            std::fs::File::create(path.as_ref().to_string_lossy().as_ref()).unwrap(),
            &animation_file_data,
        )
        .unwrap();
        // std::fs::write(
        //     format!("{}.anim.bincode", path.as_ref().to_string_lossy()),
        //     bincode::serialize(&animation_file_data).unwrap(),
        // )
        // .unwrap();

        self.has_saved = true;

        if let Some(action) = self.action_after_save.take() {
            action(self);
        }
    }

    fn load(&mut self, path: impl AsRef<Path>, assets: &mut Assets<Image>) {
        self.current_animation = load(&path, assets);
        self.current_frame = 0;
        self.current_basepath = Some(path.as_ref().to_string_lossy().to_string());
        self.action_list = vec![];
        self.has_saved = true;
    }

    fn do_action(&mut self, action: Action) {
        for _ in 0..self.undo_depth {
            self.action_list.pop().unwrap();
        }
        self.undo_depth = 0;
        action.apply(self);
        self.action_list.push(action);

        self.has_saved = false;
    }

    fn undo(&mut self) {
        if self.undo_depth >= self.action_list.len() {
            return;
        }
        self.undo_depth += 1;
        let action = self.action_list[self.action_list.len() - self.undo_depth].clone();
        action.reverse(self);

        self.has_saved = false;
    }

    fn redo(&mut self) {
        if self.undo_depth == 0 {
            return;
        }

        let action = self.action_list[self.action_list.len() - self.undo_depth].clone();
        action.apply(self);
        self.undo_depth -= 1;

        self.has_saved = false;
    }

    fn get_frame(&self, index: usize) -> Option<&Frame> {
        self.current_animation.timeline.frames.get(index)
    }

    fn get_frame_mut(&mut self, index: usize) -> Option<&mut Frame> {
        self.current_animation.timeline.frames.get_mut(index)
    }

    fn frame(&self, index: usize) -> &Frame {
        &self.current_animation.timeline.frames[index]
    }

    fn frame_mut(&mut self, index: usize) -> &mut Frame {
        &mut self.current_animation.timeline.frames[index]
    }
}

fn exit_system(editor_state: Res<EditorState>, mut close_events: EventWriter<AppExit>) {
    if editor_state.exit_now {
        close_events.send(AppExit);
    }
}

fn on_close(
    mut editor_state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut interaction_lock: ResMut<InteractionLock>,
    mut commands: Commands,
    primary_window: Query<With<PrimaryWindow>>,
    mut closed: EventReader<WindowCloseRequested>,
) {
    for event in closed.iter() {
        if primary_window.get(event.window).is_err() || editor_state.has_saved {
            commands.entity(event.window).despawn();
        } else {
            editor_state.animation_running = false;
            editor_state.action_after_save = Some(Box::new(|es| es.exit_now = true));
            ui_state.show_save_menu = true;
            *interaction_lock = InteractionLock::All;
        }
    }
}

#[derive(PartialEq)]
enum Tool {
    Select,
    MoveAnchor,
    MoveRootMotion,
    CreateHitbox,
    CreateHurtbox,
    MoveSelected,
}

#[derive(Clone)]
enum Action {
    RemoveFrame {
        frame: Frame,
        index: usize,
    },
    ChangeDelay {
        index: usize,
        from: usize,
        to: usize,
    },
    AddFrame {
        image: Handle<Image>,
    },
    MoveSprite {
        frame_index: usize,
        from: Vec2,
        to: Vec2,
    },
    SwapFrames {
        a: usize,
        b: usize,
    },
}

impl Action {
    fn apply(&self, state: &mut EditorState) {
        match self {
            Action::RemoveFrame { frame, index } => {
                let removed_frame = state.current_animation.timeline.frames.remove(*index);
                assert!(*frame == removed_frame);
                if *index < state.current_frame {
                    state.current_frame -= 1;
                }
                if state.current_frame >= state.current_animation.timeline.frames.len()
                    && state.current_frame != 0
                {
                    state.current_frame = state.current_animation.timeline.frames.len() - 1;
                }
            }
            Action::AddFrame { image } => state.current_animation.timeline.frames.push(Frame {
                image: image.clone(),
                offset: Vec2::ZERO,
                root_motion: Vec2::ZERO,
                delay: 1,
            }),
            Action::MoveSprite {
                frame_index,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*frame_index].offset = *to;
            }
            Action::ChangeDelay { index, from, to } => {
                state.current_animation.timeline.frames[*index].delay = *to;
            }
            Action::SwapFrames { a, b } => {
                state.current_animation.timeline.frames.swap(*a, *b);
            }
        }
    }

    fn reverse(&self, state: &mut EditorState) {
        match self {
            Action::RemoveFrame { frame, index } => {
                state
                    .current_animation
                    .timeline
                    .frames
                    .insert(*index, frame.clone());
                if state.current_frame >= *index
                    && state.current_animation.timeline.frames.len() != 1
                {
                    state.current_frame += 1;
                }
            }
            Action::AddFrame { image } => {
                let frame = state.current_animation.timeline.frames.pop().unwrap();
                assert!(frame.image == *image);
                if state.current_frame >= state.current_animation.timeline.frames.len()
                    && state.current_frame != 0
                {
                    state.current_frame = state.current_animation.timeline.frames.len() - 1;
                }
            }
            Action::MoveSprite {
                frame_index,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*frame_index].offset = *from;
            }
            Action::ChangeDelay { index, from, to } => {
                state.current_animation.timeline.frames[*index].delay = *from;
            }
            Action::SwapFrames { a, b } => {
                state.current_animation.timeline.frames.swap(*a, *b);
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AnimationFileData {
    #[serde(with = "seethe")]
    spritesheet: Vec<u8>,
    spritesheet_info: SpritesheetInfo,
}

mod seethe {
    use base64::Engine;
    use serde::{de::Visitor, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], mut s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes))
        } else {
            s.serialize_bytes(bytes)
        }
    }

    pub(super) fn deserialize<'d, D: Deserializer<'d>>(mut d: D) -> Result<Vec<u8>, D::Error> {
        struct V;

        impl Visitor<'_> for V {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("data")
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v)
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.into())
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(base64::engine::general_purpose::STANDARD_NO_PAD
                    .decode(v)
                    .unwrap())
            }
        }

        if d.is_human_readable() {
            d.deserialize_str(V)
        } else {
            d.deserialize_byte_buf(V)
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SpritesheetInfo {
    cell_width: usize,
    cell_height: usize,
    columns: usize,
    frame_count: usize,
    frame_data: Vec<FrameData>,
}

#[derive(Serialize, Deserialize)]
struct FrameData {
    delay: usize,
    origin: Vec2,
    root_motion: Vec2,
    hitboxes: (),
}

struct Animation {
    timeline: Timeline,
}

impl Animation {
    fn new() -> Self {
        Self {
            timeline: Timeline { frames: vec![] },
        }
    }
}

#[derive(PartialEq, Clone)]
struct Frame {
    image: Handle<Image>,
    offset: Vec2,
    root_motion: Vec2,
    delay: usize,
}

struct Timeline {
    frames: Vec<Frame>,
}

struct Hitbox {
    pos: Vec2,
    size: Vec2,
}

#[derive(Resource)]
struct LastMousePos(Vec2);
#[derive(Resource)]
struct MouseDelta(Vec2);

fn mouse_delta(
    mut last_mouse_pos: ResMut<LastMousePos>,
    mut mouse_delta: ResMut<MouseDelta>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(cur_mouse_pos) = primary_window.single().cursor_position() else {
        mouse_delta.0 = Vec2::splat(0.0);
        return;
    };

    mouse_delta.0 = cur_mouse_pos - last_mouse_pos.0;
    last_mouse_pos.0 = cur_mouse_pos;
}

struct PendingFileDialog {
    action: Option<FileAction>,
}

enum FileAction {
    LoadFrame(Pin<Box<dyn Future<Output = Option<Vec<FileHandle>>>>>),
    Save(Pin<Box<dyn Future<Output = Option<FileHandle>>>>),
    Open(Pin<Box<dyn Future<Output = Option<FileHandle>>>>),
}

fn poll_pending_file_dialog(
    mut editor_state: ResMut<EditorState>,
    mut pending_file_dialog: NonSendMut<PendingFileDialog>,
    mut assets: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    if pending_file_dialog.action.is_none() {
        return;
    }
    let waker = futures::task::noop_waker_ref();
    let ctx = &mut Context::from_waker(waker);

    match pending_file_dialog.action.as_mut().unwrap() {
        FileAction::LoadFrame(fut) => match fut.as_mut().poll(ctx) {
            Poll::Pending => {}
            Poll::Ready(None) => {
                pending_file_dialog.action = None;
            }
            Poll::Ready(Some(val)) => {
                pending_file_dialog.action = None;
                for filename in val {
                    let img =
                        image::load_from_memory(&std::fs::read(filename.path()).unwrap()).unwrap();
                    let handle = assets.add(Image::from_dynamic(img, true));
                    let action = Action::AddFrame { image: handle };
                    editor_state.do_action(action);
                }
            }
        },
        FileAction::Save(fut) => match fut.as_mut().poll(ctx) {
            Poll::Pending => {}
            Poll::Ready(None) => {
                pending_file_dialog.action = None;
            }
            Poll::Ready(Some(val)) => {
                pending_file_dialog.action = None;
                let filename = val;
                editor_state.save_to(filename.path(), &assets);
            }
        },
        FileAction::Open(fut) => match fut.as_mut().poll(ctx) {
            Poll::Pending => {}
            Poll::Ready(None) => {
                pending_file_dialog.action = None;
            }
            Poll::Ready(Some(val)) => {
                pending_file_dialog.action = None;
                let filename = val;
                editor_state.load(filename.path(), &mut assets);
            }
        },
    }
}

#[derive(Resource, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InteractionLock {
    None,
    Playback,
    All,
}

fn mouse_interaction(
    delta: Res<MouseDelta>,
    interaction_lock: Res<InteractionLock>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    input: Query<&ActionState<Input2>>,
    mut editor_state: ResMut<EditorState>,
    mut query_camera: Query<(&mut Transform, &OrthographicProjection), With<Camera2d>>,
) {
    if *interaction_lock == InteractionLock::All {
        return;
    }

    let input = input.single();
    let mouse_pos = primary_window.single().cursor_position();

    let delta = delta.0;
    let index = editor_state.current_frame;

    let (mut camera, mut proj) = query_camera.single_mut();

    if input.pressed(Input2::Pan) {
        camera.translation.x -= delta.x * proj.scale;
        camera.translation.y -= delta.y * proj.scale;
    }

    if *interaction_lock >= InteractionLock::Playback {
        return;
    }

    if editor_state.get_frame(index).is_some() {
        if input.just_pressed(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {}
                Tool::MoveAnchor => {
                    editor_state.drag_starting_pos = Some(editor_state.frame(index).offset);
                }
                Tool::MoveRootMotion => {}
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        } else if input.pressed(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {}
                Tool::MoveAnchor => {
                    if editor_state.drag_starting_pos.is_some() {
                        editor_state.frame_mut(index).offset +=
                            delta * proj.scale * Vec2::new(-1.0, 1.0);
                    }
                }
                Tool::MoveRootMotion => {}
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        } else if input.just_released(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {}
                Tool::MoveAnchor => {
                    if let Some(from) = editor_state.drag_starting_pos {
                        let action = Action::MoveSprite {
                            frame_index: index,
                            from,
                            to: editor_state.frame(index).offset.round(),
                        };
                        editor_state.do_action(action);
                    }
                }
                Tool::MoveRootMotion => {}
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        }
    }
}

fn keyboard_interaction(
    mut interaction_lock: ResMut<InteractionLock>,
    input: Query<&ActionState<Input2>>,
    mut editor_state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut pending_file_dialog: NonSendMut<PendingFileDialog>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    assets: Res<Assets<Image>>,
) {
    if *interaction_lock == InteractionLock::All {
        return;
    }

    let input = input.single();

    if let Some(action) = editor_state.with_pfd.take() {
        action(&mut pending_file_dialog);
    }

    if input.just_pressed(Input2::New) {
        editor_state.confirm_if_unsaved(&mut ui_state, &mut interaction_lock, |es| {
            es.current_animation = Animation::new();
            es.current_frame = 0;
            es.has_saved = true;
            es.action_list = vec![];
            es.undo_depth = 0;
            es.action_after_save = None;
            es.current_basepath = None;
            es.currently_selected_box = None;
            es.drag_starting_pos = None;
        });
    }
    if input.just_pressed(Input2::Open) {
        editor_state.confirm_if_unsaved(&mut ui_state, &mut interaction_lock, |es| {
            es.with_pfd = Some(Box::new(|pfd: &mut PendingFileDialog| {
                pfd.action = Some(FileAction::Open(Box::pin(
                    rfd::AsyncFileDialog::new().pick_file(),
                )));
            }));
        });
    }
    if input.just_pressed(Input2::Save) {
        editor_state.save(&mut pending_file_dialog, &assets);
    }
    if input.just_pressed(Input2::SaveAs) {
        let future = rfd::AsyncFileDialog::new()
            .add_filter("anim", &["anim"])
            .save_file();
        pending_file_dialog.action = Some(FileAction::Save(Box::pin(future)));
    }
    if input.just_pressed(Input2::ToolSelect) {
        editor_state.selected_tool = Tool::Select;
    }
    if input.just_pressed(Input2::ToolMoveAnchor) {
        editor_state.selected_tool = Tool::MoveAnchor;
    }

    if input.just_pressed(Input2::TogglePlayback) {
        editor_state.animation_running = !editor_state.animation_running;
        editor_state.frames_since_last_frame = 0;
        if editor_state.animation_running {
            *interaction_lock = InteractionLock::Playback;
        } else {
            *interaction_lock = InteractionLock::None;
        }
    }

    if *interaction_lock >= InteractionLock::Playback {
        return;
    }

    if input.just_pressed(Input2::AddFrame) {
        pending_file_dialog.action = Some(FileAction::LoadFrame(Box::pin(
            rfd::AsyncFileDialog::new().pick_files(),
        )));
    }
    if input.just_pressed(Input2::DeleteFrame) {
        if let Some(frame) = editor_state.get_frame(editor_state.current_frame) {
            let action = Action::RemoveFrame {
                frame: frame.clone(),
                index: editor_state.current_frame,
            };
            editor_state.do_action(action);
        }
    }
    if input.just_pressed(Input2::Undo) {
        editor_state.undo();
    }
    if input.just_pressed(Input2::Redo) {
        editor_state.redo();
    }

    if input.just_pressed(Input2::PrevFrame) {
        if editor_state.current_frame > 0 {
            editor_state.current_frame -= 1;
        }
    }

    if input.just_pressed(Input2::NextFrame) {
        if editor_state.current_frame + 1 < editor_state.current_animation.timeline.frames.len() {
            editor_state.current_frame += 1;
        }
    }
}

fn add_frame(commands: &mut Commands, image: Handle<Image>) {
    commands.add(|world: &mut World| {
        let mut editor_state = world.resource_mut::<EditorState>();
        let len = editor_state.current_animation.timeline.frames.len();
        editor_state.current_animation.timeline.frames.push(Frame {
            image,
            offset: Vec2::ZERO,
            root_motion: Vec2::ZERO,
            delay: 1,
        });
    });
}

fn switch_to_frame(commands: &mut Commands, new_frame: usize) {
    commands.add(move |world: &mut World| {
        switch_to_frame_internal(world, new_frame);
    });
}

fn switch_to_frame_internal(world: &mut World, new_frame: usize) {
    let mut editor_state = world.resource_mut::<EditorState>();
    let len = editor_state.current_animation.timeline.frames.len();
    if len == 0 {
        editor_state.current_frame = 0;
        return;
    }
    let cur_frame = editor_state.current_frame;
    let old_frame = cur_frame.min(len - 1);
    let new_frame = new_frame.min(len - 1);
    editor_state.current_frame = new_frame;

    // let mut c = world.query_filtered::<&Children, With<ScrollingList>>();
    // let c = c.single(world);

    // let old = c[old_frame];
    // let new = c[new_frame];

    // world
    //     .entity_mut(old)
    //     .get_mut::<BackgroundColor>()
    //     .unwrap()
    //     .0 = Color::GRAY;
    // world
    //     .entity_mut(new)
    //     .get_mut::<BackgroundColor>()
    //     .unwrap()
    //     .0 = Color::DARK_GRAY;
}

fn delete_frame(commands: &mut Commands, frame: usize) {
    commands.add(move |world: &mut World| {
        let mut editor_state = world.resource_mut::<EditorState>();
        let len = editor_state.current_animation.timeline.frames.len();
        if len == 0 {
            return;
        }
        let cur_frame = editor_state.current_frame;
        let old_frame = cur_frame.min(len - 1);
        if frame >= len {
            return;
        }
        editor_state.current_animation.timeline.frames.remove(frame);

        if frame == len - 1 && frame != 0 {
            switch_to_frame_internal(world, frame - 1);
        } else if frame == old_frame && frame != 0 {
            switch_to_frame_internal(world, frame);
        }

        // let mut q = world.query_filtered::<Entity, With<ScrollingList>>();
        // let e = q.single(world);
        // let c: Vec<_> = world
        //     .entity(e)
        //     .get::<Children>()
        //     .unwrap()
        //     .iter()
        //     .copied()
        //     .collect();
        // for i in frame + 1..len {
        //     let e = c[i];
        //     world.entity_mut(e).get_mut::<Text>().unwrap().sections[0].value = format!("Frame {i}");
        // }
        // world.entity_mut(e).remove_children(&[c[frame]]);
        // world.despawn(c[frame]);
    });
}

fn increase_delay(commands: &mut Commands) {
    commands.add(move |world: &mut World| {
        let mut editor_state = world.resource_mut::<EditorState>();
        let len = editor_state.current_animation.timeline.frames.len();
        let frame = editor_state.current_frame;
        if frame >= len {
            return;
        }

        editor_state.current_animation.timeline.frames[frame].delay += 1;
    });
}

fn decrease_delay(commands: &mut Commands) {
    commands.add(move |world: &mut World| {
        let mut editor_state = world.resource_mut::<EditorState>();
        let len = editor_state.current_animation.timeline.frames.len();
        let frame = editor_state.current_frame;
        if frame >= len {
            return;
        }

        editor_state.current_animation.timeline.frames[frame].delay =
            editor_state.current_animation.timeline.frames[frame]
                .delay
                .saturating_sub(1);
    });
}

fn render(
    mut editor_state: ResMut<EditorState>,
    mut sprite_query: Query<(&mut Transform, &mut Handle<Image>, &mut Sprite)>,
    assets: Res<Assets<Image>>,
) {
    let current_frame = editor_state.current_frame;
    let frame = editor_state
        .current_animation
        .timeline
        .frames
        .get_mut(current_frame);
    let (mut transform, mut img, mut sprite) = sprite_query.single_mut();
    if let Some(frame) = frame {
        // transform.translation.x = frame.offset.x;
        // transform.translation.y = frame.offset.y;
        if let Some(image) = assets.get(&img) {
            let image_size = image.size();
            sprite.anchor = Anchor::Custom(
                ((frame.offset / image_size) - Vec2::new(0.5, 0.5)) * Vec2::new(1.0, -1.0),
            );
            // println!("{:?}", sprite.anchor);
        } else {
            // println!("No");
        }
        if *img != frame.image {
            *img = frame.image.clone();
        }
    } else {
        if *img != Handle::default() {
            *img = Handle::default();
        }
    }
}

fn animator(mut editor_state: ResMut<EditorState>) {
    if !editor_state.animation_running {
        return;
    }

    if editor_state.current_animation.timeline.frames.is_empty() {
        return;
    }

    if editor_state.get_frame(editor_state.current_frame).is_none() {
        editor_state.current_frame = 0;
    }

    editor_state.frames_since_last_frame += 1;

    let index = editor_state.current_frame;

    let frame = editor_state.frame(index);

    if editor_state.frames_since_last_frame >= frame.delay {
        let mut new_index = index + 1;
        if new_index >= editor_state.current_animation.timeline.frames.len() {
            new_index = 0;
        }
        editor_state.current_frame = new_index;
        editor_state.frames_since_last_frame = 0;
    }
}
