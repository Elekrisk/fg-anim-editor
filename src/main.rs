#![feature(default_free_fn)]
#![feature(let_chains)]

mod ui;

use std::{default::default, path::Path};

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
use serde::{Deserialize, Serialize};

use ui::{build_ui, ScrollingList, UiHandling};

fn main() {
    let mut app = App::new();
    app.insert_resource(EditorState::new())
        .insert_resource(LastMousePos(default()))
        .insert_resource(MouseDelta(default()))
        .insert_resource(UiHandling::default())
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugin(ShapePlugin)
        .add_startup_system(start)
        .add_systems((mouse_delta, interaction, render));
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
    current_frame: usize,
}

impl EditorState {
    fn new() -> Self {
        Self {
            current_animation: Animation::new(),
            current_frame: 0,
        }
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

fn interaction(
    mut editor_state: ResMut<EditorState>,
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
        if let Some(files) = rfd::FileDialog::new().pick_files() && !files.is_empty() {
            let len = editor_state.current_animation.timeline.frames.len();

            for file in files {
                let img = image::load_from_memory(&std::fs::read(&file).unwrap()).unwrap();
                let handle = assets.add(Image::from_dynamic(img, false));
                add_frame(&mut commands, handle);
            }
        }
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
