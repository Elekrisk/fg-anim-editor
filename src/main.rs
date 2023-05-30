#![feature(default_free_fn)]
#![feature(let_chains)]
#![feature(type_alias_impl_trait)]
#![feature(int_roundings)]

mod ui;

use std::{
    default::default,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{
    input::keyboard::KeyboardInput,
    prelude::*,
    render::render_resource::Extent3d,
    sprite::{Sprite, SpriteBundle},
    text::TextStyle,
    ui::{JustifyContent, Size, Style, UiRect, Val},
    window::{PrimaryWindow, Window},
    DefaultPlugins,
};
use bevy_prototype_lyon::prelude::*;
use image::DynamicImage;
use rfd::FileHandle;
use serde::{Deserialize, Serialize};

use ui::{build_ui, ScrollingList, UiHandling};

fn main() {
    let mut app = App::new();
    app.insert_resource(EditorState::new())
        .insert_non_send_resource(PendingFileDialog { action: None })
        .insert_resource(Msaa::Off)
        .insert_resource(LastMousePos(default()))
        .insert_resource(MouseDelta(default()))
        .insert_resource(UiHandling::default())
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugin(ShapePlugin)
        .add_startup_system(start)
        .add_systems((mouse_delta, poll_pending_file_dialog, interaction, render));
    ui::add_systems(&mut app);

    app.run();
}

fn start(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    asset_server: Res<AssetServer>,
) {
    commands.spawn(Camera2dBundle {
        projection: OrthographicProjection {
            scale: 0.1,
            ..default()
        },
        ..default()
    });

    build_ui(&mut commands, &asset_server);

    commands.spawn(SpriteBundle { ..default() });

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

fn load(path: impl AsRef<Path>) -> Animation {
    serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap()
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
}

impl EditorState {
    fn new() -> Self {
        Self {
            current_animation: Animation::new(),
            current_basepath: None,
            current_frame: 0,
        }
    }

    fn save(&self, path: impl AsRef<Path>, assets: &Assets<Image>) {
        let images = self.current_animation.timeline.frames.iter().map(|ih| {
            let img = assets.get(&ih.image.image).unwrap();
            let img = img.clone().try_into_dynamic().unwrap();
            let offset = ih.offset + Vec2::new(img.width() as _, img.height() as _) / 2.0;
            println!("{}", offset);
            (img, offset)
        }).collect::<Vec<_>>();

        let image_count = images.len();

        let pre_min_image_width = images.iter().map(|(img, _)| img.width()).min();
        let pre_max_image_width = images.iter().map(|(img, _)| img.width()).max();
        let pre_min_image_height = images.iter().map(|(img, _)| img.height()).min();
        let pre_max_image_height = images.iter().map(|(img, _)| img.height()).max();

        let mut image_bb_width = 0;
        let mut image_bb_height = 0;

        let mut cropped_images = vec![];

        for (image, offset) in images {
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
            cropped_images.push((cropped_image, new_offset));
            println!("{new_offset}");

            image_bb_width = image_bb_width.max(width);
            image_bb_height = image_bb_height.max(height);
        }

        let mut expanded_images = vec![];

        for (image, offset) in cropped_images {
            let diff_x = image_bb_width - image.width();
            let diff_y = image_bb_height - image.height();

            let pad_left = diff_x / 2;
            let pad_right = diff_x - pad_left;
            let pad_top = diff_y / 2;
            let pad_bot = diff_y - pad_top;

            println!("bb: {image_bb_width}, {image_bb_height} | width: {}, {}", image.width(), image.height());
            println!("left: {pad_left}, right: {pad_right}, top: {pad_top}, bot: {pad_bot}");

            let mut expanded_image = DynamicImage::new_rgba8(image_bb_width, image_bb_height);
            let pixels = expanded_image.as_mut_rgba8().unwrap();
            let orig_pixels = image.as_rgba8().unwrap();

            for x in 0..image_bb_width {
                for y in 0..image_bb_height {
                    if x < pad_left || image_bb_width - x - 1 < pad_right || y < pad_top || image_bb_height - y - 1 < pad_bot {
                        pixels[(x, y)].0 = [0; 4];
                    } else {
                        pixels[(x, y)] = orig_pixels[(x - pad_left, y - pad_top)];
                    }
                }
            }

            expanded_images.push((expanded_image, offset + Vec2::new(pad_left as _, pad_top as _)));
        }

        for (index, (img, offset)) in expanded_images.iter().enumerate() {
            let mut path = PathBuf::from(path.as_ref());
            let file_name = path.file_name().unwrap();
            let new_file_name = format!("{}.{index}.png", file_name.to_string_lossy());
            path.set_file_name(new_file_name);
            img.save(path).unwrap();
        }

        let mut cols = expanded_images.len();
        
        for c in (1..=expanded_images.len()).rev() {
            let r = expanded_images.len().div_ceil(c);

            let w = c * image_bb_width as usize;
            let h =  r * image_bb_height as usize;

            if h > w {
                break;
            }
            cols = c;
        }

        let cols = cols as u32;
        let rows = expanded_images.len().div_ceil(cols as usize) as u32;

        let mut spritesheet = DynamicImage::new_rgba8(cols as u32 * image_bb_width, rows as u32 * image_bb_height);
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

        spritesheet.save(format!("{}.all.png", path.as_ref().to_string_lossy())).unwrap();

        let value = serde_json::Value::Array(expanded_images.into_iter().map(|a| serde_json::Value::Object([("x".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(a.1.x as _).unwrap())), ("y".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(a.1.y as _).unwrap()))].into_iter().collect())).collect());
        std::fs::write(format!("{}.json", path.as_ref().to_string_lossy()), serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
struct Frame {
    image: ImageHandle,
    offset: Vec2,
    delay_since_last: usize,
}

#[derive(Serialize, Deserialize)]
struct Timeline {
    frames: Vec<Frame>,
}

#[derive(Serialize, Deserialize)]
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

    match pending_file_dialog.action.as_mut().unwrap() {
        FileAction::LoadFrame(fut) => {
            let waker = futures::task::noop_waker_ref();
            match fut.as_mut().poll(&mut Context::from_waker(waker)) {
                Poll::Pending => {}
                Poll::Ready(None) => {
                    pending_file_dialog.action = None;
                } 
                Poll::Ready(Some(val)) => {
                    pending_file_dialog.action = None;
                    for filename in val {
                        let img = image::load_from_memory(&std::fs::read(filename.path()).unwrap()).unwrap();
                        let handle = assets.add(Image::from_dynamic(img, true));
                        add_frame(&mut commands, handle);
                    }
                }
            }
        },
        FileAction::Save(fut) => {
            struct DummyCtx;

            let waker = futures::task::noop_waker_ref();
            match fut.as_mut().poll(&mut Context::from_waker(waker)) {
                Poll::Pending => {}
                Poll::Ready(None) => {
                    pending_file_dialog.action = None;
                }
                Poll::Ready(Some(val)) => {
                    pending_file_dialog.action = None;
                    let filename = val;
                    editor_state.save(filename.path(), &assets);
                }
            }
        }
    }
}

fn interaction(
    mut editor_state: ResMut<EditorState>,
    mut pending_file_dialog: NonSendMut<PendingFileDialog>,
    mut assets: ResMut<Assets<Image>>,
    ui_handling: Res<UiHandling>,
    mouse_delta: Res<MouseDelta>,
    input: Res<Input<KeyCode>>,
    mouse_input: Res<Input<MouseButton>>,
    mut query_camera: Query<(&mut Transform, &OrthographicProjection), With<Camera2d>>,
    mut commands: Commands,
    list_query: Query<&Children, With<ScrollingList>>,
) {
    // println!("{}", ui_handling.is_pointer_over_ui);

    if pending_file_dialog.action.is_some() {
        return;
    }

    if ui_handling.is_pointer_over_ui {
        return;
    }

    let delta = mouse_delta.0;

    let current_frame = editor_state.current_frame;
    let frame = editor_state
        .current_animation
        .timeline
        .frames
        .get_mut(current_frame);

    let (mut camera_transform, proj) = query_camera.single_mut();
    if input.pressed(KeyCode::Space) && delta.length_squared() != 0.0 {
        camera_transform.translation.x -= delta.x * proj.scale;
        camera_transform.translation.y -= delta.y * proj.scale;
    } else if let Some(frame) = frame {
        if mouse_input.pressed(MouseButton::Left) && delta.length_squared() != 0.0 {
            frame.offset += delta * proj.scale;
        } else if mouse_input.just_released(MouseButton::Left) {
            frame.offset.x = frame.offset.x.round();
            frame.offset.y = frame.offset.y.round();
        }
    }

    if input.pressed(KeyCode::LControl) && input.just_pressed(KeyCode::F) {
        pending_file_dialog.action = Some(FileAction::LoadFrame(Box::pin(rfd::AsyncFileDialog::new().pick_files())));
    }

    if input.just_pressed(KeyCode::Left) {
        let frame = editor_state.current_frame;
        switch_to_frame(&mut commands, frame.saturating_sub(1));
    }
    if input.just_pressed(KeyCode::Right) {
        let frame = editor_state.current_frame;
        switch_to_frame(&mut commands, frame + 1);
    }

    if input.pressed(KeyCode::LControl) && input.just_pressed(KeyCode::Delete) {
        let frame = editor_state.current_frame;
        delete_frame(&mut commands, frame);
    }

    if input.just_pressed(KeyCode::Plus) || input.just_pressed(KeyCode::NumpadAdd) {
        increase_delay(&mut commands);
    }
    if input.just_pressed(KeyCode::Minus) || input.just_pressed(KeyCode::NumpadSubtract) {
        decrease_delay(&mut commands);
    }

    if input.pressed(KeyCode::LControl) && input.just_pressed(KeyCode::S) {
        let future = rfd::AsyncFileDialog::new()
            .add_filter("json", &["json"])
            .save_file();
        pending_file_dialog.action = Some(FileAction::Save(Box::pin(future)));
    }
}

fn add_frame(commands: &mut Commands, image: Handle<Image>) {
    commands.add(|world: &mut World| {
        let mut editor_state = world.resource_mut::<EditorState>();
        let len = editor_state.current_animation.timeline.frames.len();
        editor_state.current_animation.timeline.frames.push(Frame {
            image: ImageHandle {
                path: "".into(),
                image,
            },
            offset: Vec2::ZERO,
            delay_since_last: 1,
        });
        // let mut q = world.query_filtered::<Entity, With<ScrollingList>>();
        // let e = q.single(world);

        // let frame_text = world
        //     .spawn(
        //         TextBundle::from_section(
        //             format!("Frame {}", len + 1),
        //             TextStyle {
        //                 font: world
        //                     .resource::<AssetServer>()
        //                     .load("fonts/VT323-Regular.ttf"),
        //                 font_size: 20.0,
        //                 color: Color::WHITE,
        //             },
        //         )
        //         .with_background_color(Color::GRAY),
        //     )
        //     .id();

        // world.entity_mut(e).add_child(frame_text);
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

    let mut c = world.query_filtered::<&Children, With<ScrollingList>>();
    let c = c.single(world);

    let old = c[old_frame];
    let new = c[new_frame];

    world
        .entity_mut(old)
        .get_mut::<BackgroundColor>()
        .unwrap()
        .0 = Color::GRAY;
    world
        .entity_mut(new)
        .get_mut::<BackgroundColor>()
        .unwrap()
        .0 = Color::DARK_GRAY;
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

        editor_state.current_animation.timeline.frames[frame].delay_since_last += 1;
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

        editor_state.current_animation.timeline.frames[frame].delay_since_last =
            editor_state.current_animation.timeline.frames[frame]
                .delay_since_last
                .saturating_sub(1);
    });
}

fn render(
    mut editor_state: ResMut<EditorState>,
    mut sprite_query: Query<(&mut Transform, &mut Handle<Image>)>,
) {
    let current_frame = editor_state.current_frame;
    let frame = editor_state
        .current_animation
        .timeline
        .frames
        .get_mut(current_frame);
    let (mut transform, mut img) = sprite_query.single_mut();
    if let Some(frame) = frame {
        transform.translation.x = frame.offset.x;
        transform.translation.y = frame.offset.y;
        if *img != frame.image.image {
            *img = frame.image.image.clone();
        }
    } else {
        if *img != Handle::default() {
            *img = Handle::default();
        }
    }
}
