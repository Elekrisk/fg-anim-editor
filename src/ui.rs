use std::sync::atomic::AtomicI64;

use bevy::{
    ecs::system::EntityCommands,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
    window::PrimaryWindow,
};

use crate::EditorState;

pub fn build_ui(commands: &mut Commands, asset_server: &AssetServer) {
    let wrapper = commands
        .spawn(NodeBundle {
            style: Style {
                max_size: Size::all(Val::Percent(100.0)),
                ..default()
            },
            // background_color: Color::rgba(1.0, 1.0, 1.0, 0.0).into(),
            ..default()
        })
        .insert(Interaction::default())
        .id();

    scrollable(commands, wrapper, Direction::Vertical, |parent| {})
}

pub fn add_systems(app: &mut App) {
    app.add_systems((check_ui_interaction, mouse_scroll, mouse_grab_scrollbar, update_frame_list));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Horizontal,
    Vertical,
}

fn scrollable(
    commands: &mut Commands,
    parent: Entity,
    direction: Direction,
    children: impl FnOnce(&mut ChildBuilder),
) {
    let (dir, rev_dir) = match direction {
        Direction::Horizontal => (FlexDirection::Row, FlexDirection::Column),
        Direction::Vertical => (FlexDirection::Column, FlexDirection::Row),
    };

    // Scrollbar handle
    let handle = commands
        .spawn((
            NodeBundle {
                style: Style {
                    size: Size::all(Val::Px(20.0)),
                    ..default()
                },
                background_color: Color::BLACK.into(),
                ..default()
            },
            ScrollingHandle {
                direction,
                position: 0.0,
                last_mouse_pos: None,
                list: Entity::PLACEHOLDER,
            },
            Interaction::default(),
        ))
        .id();

    // Scrollbar
    let scrollbar = commands
        .spawn(NodeBundle {
            style: Style {
                size: match direction {
                    Direction::Horizontal => Size::height(Val::Px(20.0)),
                    Direction::Vertical => Size::width(Val::Px(20.0)),
                },
                align_self: AlignSelf::Stretch,
                ..default()
            },
            background_color: Color::FUCHSIA.into(),
            ..default()
        })
        .add_child(handle)
        .id();

    // Panel
    let panel = commands
        .spawn((
            NodeBundle {
                style: Style {
                    flex_direction: dir,
                    max_size: Size::all(Val::Undefined),
                    ..default()
                },
                background_color: Color::LIME_GREEN.into(),
                ..default()
            },
            ScrollingList {
                direction,
                position: 0.0,
                handle,
            },
        ))
        .with_children(children)
        .id();

    commands.add(move |world: &mut World| {
        world
            .entity_mut(handle)
            .get_mut::<ScrollingHandle>()
            .unwrap()
            .list = panel;
    });

    // Wrapper
    let wrapper = commands
        .spawn(NodeBundle {
            style: Style {
                flex_direction: dir,
                align_self: AlignSelf::Stretch,
                overflow: Overflow::Hidden,
                ..default()
            },
            background_color: Color::BISQUE.into(),
            ..default()
        })
        .add_child(panel)
        .id();

    // Container
    let container = commands
        .spawn(NodeBundle {
            style: Style {
                flex_direction: rev_dir,
                max_size: Size::all(Val::Percent(100.0)),
                ..default()
            },
            ..default()
        })
        .push_children(&[wrapper, scrollbar])
        .id();

    commands.add(move |world: &mut World| {
        world.entity_mut(parent).add_child(container);
    });
}

#[derive(Resource, Default)]
pub struct UiHandling {
    pub is_pointer_over_ui: bool,
}

fn check_ui_interaction(
    mut ui_handling: ResMut<UiHandling>,
    interaction_query: Query<&Interaction, (With<Node>, Changed<Interaction>)>,
) {
    if interaction_query.iter().next().is_none() {
        return;
    }
    ui_handling.is_pointer_over_ui = interaction_query
        .iter()
        .any(|i| matches!(i, Interaction::Clicked | Interaction::Hovered));
    println!("{}", ui_handling.is_pointer_over_ui);
}

#[derive(Component)]
pub struct ScrollingList {
    direction: Direction,
    position: f32,
    handle: Entity,
}

#[derive(Component)]
struct ScrollingHandle {
    direction: Direction,
    position: f32,
    last_mouse_pos: Option<Vec2>,
    list: Entity,
}

fn mouse_scroll(
    mut mouse_wheel_events: EventReader<MouseWheel>,
    mut query_list: Query<(&mut ScrollingList, &mut Style, &Parent, &Node)>,
    mut query_handle: Query<(&mut Style, &mut ScrollingHandle, &Node), Without<ScrollingList>>,
    query_node: Query<&Node>,
) {
    for mouse_wheel_event in mouse_wheel_events.iter() {
        for (mut scrolling_list, mut style, parent, list_node) in &mut query_list {
            let items_size = list_node.size();
            let container_size = query_node.get(parent.get()).unwrap().size();

            let dy = match mouse_wheel_event.unit {
                MouseScrollUnit::Line => mouse_wheel_event.y * 20.,
                MouseScrollUnit::Pixel => mouse_wheel_event.y,
            };

            let (mut handle_style, mut handle, handle_node) =
                query_handle.get_mut(scrolling_list.handle).unwrap();
            let handle_size = handle_node.size();

            scroll_panel_by(
                scrolling_list.direction,
                dy,
                container_size,
                &mut scrolling_list,
                &mut style,
                items_size,
                &mut handle,
                &mut handle_style,
                handle_size,
            )
        }
    }
}

fn scroll_panel_by(
    direction: Direction,
    dy: f32,
    container_size: Vec2,
    panel: &mut ScrollingList,
    panel_style: &mut Style,
    panel_size: Vec2,
    handle: &mut ScrollingHandle,
    handle_style: &mut Style,
    handle_size: Vec2,
) {
    let percentage = match direction {
        Direction::Horizontal => panel.position - dy / panel_size.x,
        Direction::Vertical => panel.position - dy / panel_size.y,
    }
    .clamp(0.0, 1.0);
    //println!("{dy} : {} : {percentage}", panel_size.x);

    scroll_panel_to(
        direction,
        percentage,
        container_size,
        panel,
        panel_style,
        panel_size,
        handle,
        handle_style,
        handle_size,
    )
}

fn scroll_panel_to(
    direction: Direction,
    percentage: f32,
    container_size: Vec2,
    panel: &mut ScrollingList,
    panel_style: &mut Style,
    panel_size: Vec2,
    handle: &mut ScrollingHandle,
    handle_style: &mut Style,
    handle_size: Vec2,
) {
    let axis = |v: Vec2| match direction {
        Direction::Horizontal => v.x,
        Direction::Vertical => v.y,
    };

    panel.position = percentage;
    handle.position = percentage;

    let percentage = percentage.clamp(0.0, 1.0);

    let max_panel_scroll = (axis(panel_size) - axis(container_size)).max(0.0);
    let max_handle_scroll = (axis(container_size) - axis(handle_size)).max(0.0);

    let panel_pixel_scroll = max_panel_scroll * percentage;
    let handle_pixel_scroll = max_handle_scroll * percentage;

    match direction {
        Direction::Horizontal => {
            panel_style.position.left = Val::Px(-panel_pixel_scroll);
            handle_style.position.left = Val::Px(handle_pixel_scroll);
        }
        Direction::Vertical => {
            panel_style.position.top = Val::Px(-panel_pixel_scroll);
            handle_style.position.top = Val::Px(handle_pixel_scroll);
        }
    }
}

fn mouse_grab_scrollbar(
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mut query_handle: Query<(
        &mut Style,
        &mut ScrollingHandle,
        &Interaction,
        &Parent,
        &Node,
    )>,
    query_node: Query<&Node>,
    mut query_list: Query<(&mut Style, &mut ScrollingList, &Node), Without<ScrollingHandle>>,
) {
    static Y: AtomicI64 = AtomicI64::new(0);

    let window = primary_window.single();
    let Some(mouse_pos) = window.cursor_position() else {
        return;
    };

    for (mut handle_style, mut handle, interaction, parent, node) in &mut query_handle {
        if *interaction != Interaction::Clicked {
            handle.last_mouse_pos = None;
            handle.position = handle.position.clamp(0.0, 1.0);
            continue;
        }

        let Some(last_mouse_pos) = handle.last_mouse_pos else {
            handle.last_mouse_pos = Some(mouse_pos);
            return;
        };

        let mut delta = mouse_pos - last_mouse_pos;
        if delta.length_squared() == 0.0 {
            continue;
        }
        delta.y *= -1.0;
        handle.last_mouse_pos = Some(mouse_pos);

        let y = Y.fetch_add(delta.y as i64, std::sync::atomic::Ordering::Relaxed);
        // //println!("Y kept track of: {y}");

        let handle_size = node.size();
        let container_size = query_node.get(parent.get()).unwrap().size();
        //println!("{container_size}");
        let change_in_percent = match handle.direction {
            Direction::Horizontal => delta.x / (container_size.x - handle_size.x),
            Direction::Vertical => delta.y / (container_size.y - handle_size.y),
        };

        //println!("{} / {} = {}", delta.x, container_size.x, change_in_percent);

        let new_pos = handle.position + change_in_percent;
        //println!("{new_pos}");

        let (mut panel_style, mut panel, panel_node) = query_list.get_mut(handle.list).unwrap();
        let panel_size = panel_node.size();

        scroll_panel_to(
            handle.direction,
            new_pos,
            container_size,
            &mut panel,
            &mut panel_style,
            panel_size,
            &mut handle,
            &mut handle_style,
            handle_size,
        )
    }
}

#[derive(Component)]
pub struct FrameInfo {
    pub frame: usize,
    pub delay: usize,
}

fn update_frame_list(
    editor_state: Res<EditorState>,
    list_query: Query<(Option<&Children>, Entity), With<ScrollingList>>,
    mut entry_query: Query<(&mut FrameInfo, &mut Text)>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    if !editor_state.is_changed() {
        println!("Editor state unchanged; skipping...");
        return;
    }

    let (children, list) = list_query.single();
    let children_len = children.map(|c| c.len()).unwrap_or(0);
    let len = editor_state.current_animation.timeline.frames.len();

    if children_len > len {
        let children = children.unwrap();
        commands
            .get_entity(list)
            .unwrap()
            .remove_children(&children[len..]);
        let to_remove = children[len..].to_vec();
        commands.add(move |world: &mut World| {
            for e in to_remove {
                world.despawn(e);
            }
        });
    }
    if children_len < len {
        for i in children_len..len {
            let child = commands.spawn((TextBundle::from_section(
                format!(
                    "Frame {} [{}]",
                    i + 1,
                    editor_state.current_animation.timeline.frames[i].delay
                ),
                TextStyle {
                    font: asset_server.load("fonts/VT323-Regular.ttf"),
                    font_size: 20.0,
                    color: Color::WHITE,
                },
            ), FrameInfo {
                frame: editor_state.current_frame,
                delay: editor_state.current_animation.timeline.frames[i].delay,
            })).id();
            commands.entity(list).add_child(child);
        }
    }

    let ml = len.min(children_len);

    for i in 0..ml {
        let children = children.unwrap();
        let e = children[i];
        let (mut frameinfo, mut text) = entry_query.get_mut(e).unwrap();
        if frameinfo.frame != i + 1 || frameinfo.delay != editor_state.current_animation.timeline.frames[i].delay {
            frameinfo.frame = i + 1;
            frameinfo.delay = editor_state.current_animation.timeline.frames[i].delay;
            text.sections[0].value = format!(
                "Frame {} [{}]",
                i + 1,
                editor_state.current_animation.timeline.frames[i].delay
            );
        }
    }
}
