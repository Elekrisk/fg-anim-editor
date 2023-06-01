#![feature(default_free_fn)]
#![feature(let_chains)]
#![feature(type_alias_impl_trait)]
#![feature(int_roundings)]
#![feature(hash_drain_filter)]

mod ui;

use std::{
    collections::HashMap,
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

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
enum Stages {
    Logic,
    Ui,
}

#[derive(Actionlike, Clone, Debug)]
enum Input2 {
    LeftClick,
    ShiftLeftClick,
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
        .configure_set(Stages::Logic.before(Stages::Ui))
        .add_startup_system(start)
        .add_systems(
            (
                mouse_delta.before(mouse_interaction),
                poll_pending_file_dialog,
                mouse_interaction,
                keyboard_interaction,
                render.after(mouse_interaction),
                exit_system,
                on_close,
            )
                .in_set(Stages::Logic),
        );
    app.get_schedule_mut(CoreSchedule::FixedUpdate)
        .unwrap()
        .add_system(animator);
    ui::add_systems(&mut app);

    app.run();
}

#[derive(Component)]
struct MotionMarker;

fn start(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    asset_server: Res<AssetServer>,
) {
    let mut input_map = InputMap::default();
    input_map.insert(MouseButton::Left, Input2::LeftClick);
    input_map.insert_modified(Modifier::Shift, MouseButton::Left, Input2::ShiftLeftClick);
    input_map.insert(KeyCode::Space, Input2::Pan);
    input_map.insert(KeyCode::Q, Input2::ToolSelect);
    input_map.insert(KeyCode::W, Input2::ToolMoveAnchor);
    input_map.insert(KeyCode::E, Input2::ToolCreateHitbox);
    input_map.insert(KeyCode::R, Input2::ToolCreateHurtbox);
    input_map.insert(KeyCode::T, Input2::ToolCreateCollisionbox);
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
    input_map.insert(KeyCode::A, Input2::PrevFrame);
    input_map.insert(KeyCode::D, Input2::NextFrame);
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

    let mut shape = shapes::Polygon::default();
    shape.points = vec![
        Vec2::new(0.0, 1.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(-1.0, 0.0),
    ];
    shape.closed = true;

    commands.spawn((
        ShapeBundle {
            path: GeometryBuilder::build_as(&shape),
            transform: Transform {
                translation: Vec3 {
                    z: 1.0,
                    ..default()
                },
                ..default()
            },
            ..default()
        },
        Fill::color(Color::YELLOW.with_a(0.5)),
        MotionMarker,
    ));

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

    let cell_width = animation_file_data.info.cell_width as u32;
    let cell_height = animation_file_data.info.cell_height as u32;
    let cols = animation_file_data.info.columns as u32;
    let frame_count = animation_file_data.info.frame_count as u32;

    let image = image::load_from_memory(&animation_file_data.spritesheet).unwrap();

    let mut frames = vec![];

    for i in 0..frame_count {
        let x = i % cols;
        let y = i / cols;

        let handle = assets.add(Image::from_dynamic(
            image.crop_imm(x * cell_width, y * cell_height, cell_width, cell_height),
            true,
        ));
        let frame_info = &animation_file_data.info.frame_data[i as usize];
        println!("{}", frame_info.origin);
        let offset = frame_info.origin;
        println!("{}", offset);
        let root_motion = frame_info.root_motion;
        let hitboxes = frame_info.hitboxes.clone();
        let delay = frame_info.delay;

        frames.push(Frame {
            image: handle,
            offset,
            root_motion,
            delay,
            hitboxes,
        });
    }

    Animation {
        timeline: Timeline { frames },
        hitboxes: animation_file_data.info.hitboxes.clone(),
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
    interaction_lock: InteractionLock,
    always_show_root_motion: bool,
    show_hitboxes: bool,
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
            interaction_lock: InteractionLock::None,
            always_show_root_motion: false,
            show_hitboxes: true,
        }
    }

    fn confirm_if_unsaved(
        &mut self,
        ui_state: &mut UiState,
        action: impl FnOnce(&mut Self) + Send + Sync + 'static,
        unlock_on_non_cancel: bool,
    ) {
        if self.has_saved {
            action(self);
        } else {
            self.animation_running = false;
            self.frames_since_last_frame = 0;
            self.interaction_lock.lock_all();
            ui_state.show_save_menu = true;
            ui_state.save_menu_unlock_on_non_cancel = unlock_on_non_cancel;
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
            self.interaction_lock.lock_all();
            self.animation_running = false;
            self.frames_since_last_frame = 0;
            pending_file_dialog.action = Some(FileAction::Save(Box::pin(future)));
        }
    }

    fn save_to(&mut self, path: impl AsRef<Path>, assets: &Assets<Image>) {
        let mut images = self
            .current_animation
            .timeline
            .frames
            .iter()
            .map(|ih| {
                let img = assets.get(&ih.image).unwrap();
                let img = img.clone().try_into_dynamic().unwrap();
                let offset = ih.offset;
                let root_motion = ih.root_motion;
                let hitboxes = ih.hitboxes.clone();
                println!("{}", offset);
                (img, offset, root_motion, hitboxes, ih.delay)
            })
            .collect::<Vec<_>>();

        let image_count = images.len();

        let mut image_bb_width = 0;
        let mut image_bb_height = 0;

        for (image, offset, _, _, _) in &mut images {
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

            *image = image.crop_imm(left, top, width, height);

            *offset = Vec2::new(offset.x - left as f32, offset.y - top as f32);
            println!("{offset}");

            image_bb_width = image_bb_width.max(width);
            image_bb_height = image_bb_height.max(height);
        }
        for (image, offset, _, _, _) in &mut images {
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

            *image = expanded_image;
            *offset += Vec2::new(pad_left as _, pad_top as _);
        }

        // for (index, (img, offset, delay)) in expanded_images.iter().enumerate() {
        //     let mut path = PathBuf::from(path.as_ref());
        //     let file_name = path.file_name().unwrap();
        //     let new_file_name = format!("{}.{index}.png", file_name.to_string_lossy());
        //     path.set_file_name(new_file_name);
        //     img.save(path).unwrap();
        // }

        let mut cols = images.len();

        for c in (1..=images.len()).rev() {
            let r = images.len().div_ceil(c);

            let w = c * image_bb_width as usize;
            let h = r * image_bb_height as usize;

            if h > w {
                break;
            }
            cols = c;
        }

        let cols = cols as u32;
        let rows = images.len().div_ceil(cols as usize) as u32;

        let mut spritesheet =
            DynamicImage::new_rgba8(cols as u32 * image_bb_width, rows as u32 * image_bb_height);
        let spritesheet_pixels = spritesheet.as_mut_rgba8().unwrap();

        for ix in 0..cols {
            for iy in 0..rows {
                let index = (iy * cols + ix) as usize;
                if index as usize >= images.len() {
                    continue;
                }

                let original_pixels = images[index].0.as_rgba8().unwrap();
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

        let frame_data = Info {
            cell_width: image_bb_width as _,
            cell_height: image_bb_height as _,
            columns: cols as _,
            frame_count: images.len(),
            frame_data: images
                .into_iter()
                .map(|(_, offset, root_motion, hitboxes, delay)| FrameData {
                    delay,
                    origin: offset,
                    root_motion,
                    hitboxes,
                })
                .collect(),
            hitboxes: self.current_animation.hitboxes.clone(),
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
            info: frame_data,
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
        if action.warrants_action() {
            for _ in 0..self.undo_depth {
                self.action_list.pop().unwrap();
            }
            self.undo_depth = 0;
            action.apply(self);
            self.action_list.push(action);

            self.has_saved = false;
        }
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
            editor_state.interaction_lock.lock_all();
        }
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
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
    SetMotionOffset {
        frame_index: usize,
        from: Vec2,
        to: Vec2,
    },
    SwapFrames {
        a: usize,
        b: usize,
    },
    CreateHitbox {
        id: usize,
        desc: String,
    },
    MoveHitbox {
        frame_index: usize,
        id: usize,
        from: Vec2,
        to: Vec2,
    },
    ResizeHitbox {
        frame_index: usize,
        id: usize,
        from: Vec2,
        to: Vec2,
    },
    ToggleHitboxEnabled {
        frame_index: usize,
        id: usize,
    }
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
                hitboxes: HashMap::new(),
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
            Action::SetMotionOffset {
                frame_index,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*frame_index].root_motion = *to;
            }
            Action::CreateHitbox { id, desc } => {
                state.current_animation.hitboxes.insert(
                    *id,
                    Hitbox {
                        id: *id,
                        desc: desc.clone(),
                        is_hurtbox: false,
                    },
                );
            }
            Action::MoveHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*index]
                    .hitbox_mut(*id)
                    .pos = *to;
            }
            Action::ResizeHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*index]
                    .hitbox_mut(*id)
                    .size = *to;
            }
            Action::ToggleHitboxEnabled { frame_index, id } => {
                state.current_animation.timeline.frames[*frame_index].hitbox_mut(*id).enabled.toggle();
            },
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
            Action::SetMotionOffset {
                frame_index,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*frame_index].root_motion = *from;
            }
            Action::CreateHitbox { id, desc } => {
                state.current_animation.hitboxes.remove(id);
            }
            Action::MoveHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*index]
                    .hitbox_mut(*id)
                    .pos = *from;
            }
            Action::ResizeHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => {
                state.current_animation.timeline.frames[*index]
                    .hitbox_mut(*id)
                    .size = *from;
            }
            Action::ToggleHitboxEnabled { frame_index, id } => {
                state.current_animation.timeline.frames[*frame_index].hitbox_mut(*id).enabled.toggle();
            },
        }
    }

    fn warrants_action(&self) -> bool {
        match self {
            Action::RemoveFrame { frame, index } => true,
            Action::ChangeDelay { index, from, to } => from != to,
            Action::AddFrame { image } => true,
            Action::MoveSprite {
                frame_index,
                from,
                to,
            } => from != to,
            Action::SetMotionOffset {
                frame_index,
                from,
                to,
            } => from != to,
            Action::SwapFrames { a, b } => a != b,
            Action::CreateHitbox { id, desc } => true,
            Action::MoveHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => from != to,
            Action::ResizeHitbox {
                frame_index: index,
                id,
                from,
                to,
            } => from != to,
            Action::ToggleHitboxEnabled { frame_index, id } => true,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AnimationFileData {
    #[serde(with = "seethe")]
    spritesheet: Vec<u8>,
    info: Info,
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
struct Info {
    cell_width: usize,
    cell_height: usize,
    columns: usize,
    frame_count: usize,
    frame_data: Vec<FrameData>,
    hitboxes: HashMap<usize, Hitbox>,
}

#[derive(Serialize, Deserialize)]
struct FrameData {
    delay: usize,
    origin: Vec2,
    root_motion: Vec2,
    hitboxes: HashMap<usize, HitboxPos>,
}

struct Animation {
    timeline: Timeline,
    hitboxes: HashMap<usize, Hitbox>,
}

impl Animation {
    fn new() -> Self {
        Self {
            timeline: Timeline { frames: vec![] },
            hitboxes: HashMap::new(),
        }
    }
}

#[derive(PartialEq, Clone)]
struct Frame {
    image: Handle<Image>,
    offset: Vec2,
    root_motion: Vec2,
    delay: usize,
    hitboxes: HashMap<usize, HitboxPos>,
}

impl Frame {
    fn has_hitbox(&self, id: usize) -> bool {
        self.hitboxes.contains_key(&id)
    }

    fn hitbox(&self, id: usize) -> &HitboxPos {
        self.hitboxes.get(&id).unwrap()
    }

    fn hitbox_mut(&mut self, id: usize) -> &mut HitboxPos {
        self.hitboxes.get_mut(&id).unwrap()
    }

    fn get_hitbox(&self, id: usize) -> Option<&HitboxPos> {
        self.hitboxes.get(&id)
    }

    fn get_hitbox_mut(&mut self, id: usize) -> Option<&mut HitboxPos> {
        self.hitboxes.get_mut(&id)
    }

    fn is_hitbox_enabled(&self, id: usize) -> bool {
        self.get_hitbox(id).is_some_and(|hp| hp.enabled)
    }
}

struct Timeline {
    frames: Vec<Frame>,
}

#[derive(Deserialize, Serialize, Clone)]
struct Hitbox {
    id: usize,
    desc: String,
    is_hurtbox: bool,
}

#[derive(PartialEq, Clone, Deserialize, Serialize)]
struct HitboxPos {
    id: usize,
    pos: Vec2,
    size: Vec2,
    enabled: bool,
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
                editor_state.interaction_lock.release();
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
                editor_state.interaction_lock.release();
            }
        },
        FileAction::Save(fut) => match fut.as_mut().poll(ctx) {
            Poll::Pending => {}
            Poll::Ready(None) => {
                pending_file_dialog.action = None;
                editor_state.interaction_lock.release();
            }
            Poll::Ready(Some(val)) => {
                pending_file_dialog.action = None;
                let filename = val;
                editor_state.save_to(filename.path(), &assets);
                editor_state.interaction_lock.release();
            }
        },
        FileAction::Open(fut) => match fut.as_mut().poll(ctx) {
            Poll::Pending => {}
            Poll::Ready(None) => {
                pending_file_dialog.action = None;
                editor_state.interaction_lock.release();
            }
            Poll::Ready(Some(val)) => {
                pending_file_dialog.action = None;
                let filename = val;
                editor_state.load(filename.path(), &mut assets);
                editor_state.interaction_lock.release();
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

impl InteractionLock {
    fn lock_all(&mut self) {
        *self = InteractionLock::All;
    }

    fn lock_playback(&mut self) {
        *self = InteractionLock::Playback;
    }

    fn release(&mut self) {
        *self = InteractionLock::None;
    }
}

fn mouse_interaction(
    delta: Res<MouseDelta>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    input: Query<&ActionState<Input2>>,
    mut editor_state: ResMut<EditorState>,
    mut query_camera: Query<
        (
            &mut Transform,
            &Camera,
            &GlobalTransform,
            &OrthographicProjection,
        ),
        With<Camera2d>,
    >,
) {
    if editor_state.interaction_lock == InteractionLock::All {
        return;
    }

    let input = input.single();
    let mouse_pos = primary_window.single().cursor_position();

    let delta = delta.0;
    let index = editor_state.current_frame;

    let (mut camera, actual_camera, global_camera, mut proj) = query_camera.single_mut();

    let world_pos = mouse_pos.and_then(|mp| actual_camera.viewport_to_world_2d(&global_camera, mp));

    if input.pressed(Input2::Pan) {
        camera.translation.x -= delta.x * proj.scale;
        camera.translation.y -= delta.y * proj.scale;
    }

    if editor_state.interaction_lock >= InteractionLock::Playback {
        return;
    }

    if editor_state.get_frame(index).is_some() {
        if input.just_pressed(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if let Some(wp) = world_pos {
                            let mut selected = false;
                            for hp in editor_state.frame(index).hitboxes.values() {
                                if wp.x >= hp.pos.x
                                    && wp.x <= hp.pos.x + hp.size.x
                                    && wp.y <= hp.pos.y
                                    && wp.y >= hp.pos.y - hp.size.y
                                {
                                    editor_state.currently_selected_box = Some(hp.id);
                                    selected = true;
                                    break;
                                }
                            }

                            if !selected {
                                editor_state.currently_selected_box = None;
                            } else {
                                editor_state.drag_starting_pos = Some(
                                    editor_state
                                        .frame(index)
                                        .hitbox(editor_state.currently_selected_box.unwrap())
                                        .pos,
                                );
                            }
                        }
                    }
                }
                Tool::MoveAnchor => {
                    editor_state.drag_starting_pos = Some(editor_state.frame(index).offset);
                }
                Tool::MoveRootMotion => {
                    editor_state.drag_starting_pos = Some(editor_state.frame(index).root_motion);
                }
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        } else if input.pressed(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if editor_state.drag_starting_pos.is_some() && let Some(id) = editor_state.currently_selected_box {
                            editor_state.frame_mut(index).hitbox_mut(id).pos += delta * proj.scale;
                        }
                    }
                }
                Tool::MoveAnchor => {
                    if editor_state.drag_starting_pos.is_some() {
                        editor_state.frame_mut(index).offset +=
                            delta * proj.scale * Vec2::new(-1.0, 1.0);
                    }
                }
                Tool::MoveRootMotion => {
                    if editor_state.drag_starting_pos.is_some() {
                        editor_state.frame_mut(index).root_motion += delta * proj.scale;
                    }
                }
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        } else if input.just_released(Input2::LeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if let Some(from) = editor_state.drag_starting_pos && let Some(id) = editor_state.currently_selected_box {
                            let action = Action::MoveHitbox {
                                frame_index: index,
                                id,
                                from,
                                to: editor_state.frame(index).hitbox(id).pos.round(),
                            };
                            editor_state.do_action(action);
                        }
                    }
                }
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
                Tool::MoveRootMotion => {
                    if let Some(from) = editor_state.drag_starting_pos {
                        let action = Action::SetMotionOffset {
                            frame_index: index,
                            from,
                            to: editor_state.frame(index).root_motion.round(),
                        };
                        editor_state.do_action(action);
                    }
                }
                Tool::CreateHitbox => {}
                Tool::CreateHurtbox => {}
                Tool::MoveSelected => {}
            }
        } else if input.just_pressed(Input2::ShiftLeftClick) {
            println!("{:?}", editor_state.selected_tool);
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if let Some(wp) = world_pos {
                            let mut selected = false;
                            for hp in editor_state.frame(index).hitboxes.values() {
                                if wp.x >= hp.pos.x
                                    && wp.x <= hp.pos.x + hp.size.x
                                    && wp.y <= hp.pos.y
                                    && wp.y >= hp.pos.y - hp.size.y
                                {
                                    editor_state.currently_selected_box = Some(hp.id);
                                    selected = true;
                                    break;
                                }
                            }

                            if !selected {
                                editor_state.currently_selected_box = None;
                            } else {
                                editor_state.drag_starting_pos = Some(
                                    editor_state
                                        .frame(index)
                                        .hitbox(editor_state.currently_selected_box.unwrap())
                                        .size,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        } else if input.pressed(Input2::ShiftLeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if editor_state.drag_starting_pos.is_some() && let Some(id) = editor_state.currently_selected_box {
                        editor_state.frame_mut(index).hitbox_mut(id).size += delta * proj.scale * Vec2::new(1.0, -1.0);
                    }
                    }
                }
                _ => {}
            }
        } else if input.just_released(Input2::ShiftLeftClick) {
            match editor_state.selected_tool {
                Tool::Select => {
                    if editor_state.show_hitboxes {
                        if let Some(from) = editor_state.drag_starting_pos && let Some(id) = editor_state.currently_selected_box {
                        let action = Action::ResizeHitbox {
                            frame_index: index,
                            id,
                            from,
                            to: editor_state.frame(index).hitbox(id).size.round(),
                        };
                        editor_state.do_action(action);
                    }
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Component)]
struct HitboxId(usize);

fn keyboard_interaction(
    input: Query<&ActionState<Input2>>,
    mut editor_state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut pending_file_dialog: NonSendMut<PendingFileDialog>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    assets: Res<Assets<Image>>,
) {
    if let Some(action) = editor_state.with_pfd.take() {
        action(&mut pending_file_dialog);
    }

    if editor_state.interaction_lock == InteractionLock::All {
        return;
    }

    let input = input.single();

    if input.just_pressed(Input2::New) {
        editor_state.confirm_if_unsaved(
            &mut ui_state,
            |es| {
                es.current_animation = Animation::new();
                es.current_frame = 0;
                es.has_saved = true;
                es.action_list = vec![];
                es.undo_depth = 0;
                es.action_after_save = None;
                es.current_basepath = None;
                es.currently_selected_box = None;
                es.drag_starting_pos = None;
            },
            true,
        );
    }
    if input.just_pressed(Input2::Open) {
        editor_state.confirm_if_unsaved(
            &mut ui_state,
            |es| {
                es.interaction_lock.lock_all();
                es.with_pfd = Some(Box::new(|pfd: &mut PendingFileDialog| {
                    pfd.action = Some(FileAction::Open(Box::pin(
                        rfd::AsyncFileDialog::new().pick_file(),
                    )));
                }));
            },
            false,
        );
    }
    if input.just_pressed(Input2::Save) {
        editor_state.save(&mut pending_file_dialog, &assets);
    }
    if input.just_pressed(Input2::SaveAs) {
        let future = rfd::AsyncFileDialog::new()
            .add_filter("anim", &["anim"])
            .save_file();
        editor_state.animation_running = false;
        editor_state.frames_since_last_frame = 0;
        editor_state.interaction_lock.lock_all();
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
            editor_state.interaction_lock = InteractionLock::Playback;
        } else {
            editor_state.interaction_lock = InteractionLock::None;
        }
    }

    if editor_state.interaction_lock >= InteractionLock::Playback {
        return;
    }

    if input.just_pressed(Input2::AddFrame) {
        editor_state.interaction_lock.lock_all();
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

fn render(
    mut editor_state: ResMut<EditorState>,
    mut sprite_query: Query<(&mut Transform, &mut Handle<Image>, &mut Sprite)>,
    mut marker_query: Query<&mut Transform, (With<MotionMarker>, Without<Sprite>)>,
    mut hitbox_shapes: Query<
        (
            Entity,
            &mut Transform,
            &mut bevy_prototype_lyon::prelude::Path,
            &mut HitboxId,
        ),
        (Without<MotionMarker>, Without<Sprite>),
    >,
    mut commands: Commands,
    assets: Res<Assets<Image>>,
) {
    let current_tool = editor_state.selected_tool;
    let current_frame = editor_state.current_frame;
    let always_show_root_motion = editor_state.always_show_root_motion;
    let show_hitboxes = editor_state.show_hitboxes;
    let frame = editor_state
        .current_animation
        .timeline
        .frames
        .get_mut(current_frame);
    let mut marker_transform = marker_query.single_mut();
    let (mut transform, mut img, mut sprite) = sprite_query.single_mut();
    if let Some(frame) = frame {
        if current_tool == Tool::MoveRootMotion || always_show_root_motion {
            transform.translation.x = frame.root_motion.x;
            transform.translation.y = frame.root_motion.y;
            marker_transform.translation.x = frame.root_motion.x;
            marker_transform.translation.y = frame.root_motion.y;
        } else {
            transform.translation.x = 0.0;
            transform.translation.y = 0.0;
            marker_transform.translation.x = 0.0;
            marker_transform.translation.y = 0.0;
        }

        let mut drawn_hitboxes = vec![];

        for (e, mut hitbox_transform, mut shape, mut id) in hitbox_shapes.iter_mut() {
            if let Some(hp) = frame.get_hitbox(id.0) && hp.enabled && show_hitboxes {
                hitbox_transform.translation.x = hp.pos.x;
                hitbox_transform.translation.y = hp.pos.y;
                if current_tool == Tool::MoveRootMotion || always_show_root_motion {
                    hitbox_transform.translation.x += frame.root_motion.x;
                    hitbox_transform.translation.y += frame.root_motion.y;
                }
                *shape = GeometryBuilder::build_as(&{
                    let mut rect = shapes::Rectangle::default();
                    rect.origin = RectangleOrigin::TopLeft;
                    rect.extents = hp.size;
                    rect
                });
                drawn_hitboxes.push(id.0.clone());
            } else {
                commands.entity(e).despawn();
            }
        }

        if show_hitboxes {
            commands.spawn_batch(
                frame
                    .hitboxes
                    .values()
                    .filter(|hp| hp.enabled && !drawn_hitboxes.contains(&hp.id))
                    .map(|hp| {
                        (
                            ShapeBundle {
                                path: GeometryBuilder::build_as(&{
                                    let mut rect = shapes::Rectangle::default();
                                    rect.origin = RectangleOrigin::TopLeft;
                                    rect.extents = hp.size;
                                    rect
                                }),
                                transform: Transform {
                                    translation: Vec3::new(hp.pos.x, hp.pos.y, 0.5),
                                    ..default()
                                },
                                ..default()
                            },
                            Fill::color(Color::GREEN.with_a(0.2)),
                            HitboxId(hp.id),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }

        if let Some(image) = assets.get(&img) {
            let image_size = image.size();
            sprite.anchor = Anchor::Custom(
                ((frame.offset / image_size) - Vec2::new(0.5, 0.5)) * Vec2::new(1.0, -1.0),
            );
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

trait Toggle {
    fn toggle(&mut self);
}

impl Toggle for bool {
    fn toggle(&mut self) {
        *self = !*self;
    }
}
