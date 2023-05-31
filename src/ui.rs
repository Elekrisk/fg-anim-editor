use std::{str::FromStr, sync::atomic::AtomicBool};

use bevy::{app::AppExit, prelude::*};
use bevy_egui::EguiContexts;
use egui::Context;

use crate::{Action, EditorState, InteractionLock, PendingFileDialog, Tool};

pub(crate) fn build_ui(commands: &mut Commands) {}
pub(crate) fn add_systems(app: &mut App) {
    app.insert_resource(UiState::default());
    app.add_systems((update_ui_state.before(ui), ui));
}

fn ui(
    mut commands: Commands,
    mut editor_state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut pending_file_dialog: NonSendMut<PendingFileDialog>,
    mut contexts: EguiContexts,
    mut interaction_lock: ResMut<InteractionLock>,
    assets: Res<Assets<Image>>,
) {
    let ctx = contexts.ctx_mut();
    save_confirmation_window(
        &mut commands,
        ctx,
        &mut editor_state,
        &mut ui_state,
        &mut interaction_lock,
        &mut pending_file_dialog,
        &assets,
    );

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.set_enabled(*interaction_lock <= InteractionLock::Playback);
        toolbar(ui, &mut editor_state);
    });

    egui::TopBottomPanel::bottom("timeline").show(ctx, |ui| {
        ui.set_enabled(*interaction_lock <= InteractionLock::Playback);
        timeline(&mut editor_state, ui);
    });
    egui::SidePanel::right("right_panel").show(ctx, |ui| {
        ui.set_enabled(*interaction_lock <= InteractionLock::None);
        frame_info(&mut editor_state, &mut ui_state, ui);
    });
}

fn save_confirmation_window(
    commands: &mut Commands,
    ctx: &mut Context,
    editor_state: &mut EditorState,
    ui_state: &mut UiState,
    interaction_lock: &mut InteractionLock,
    pending_file_dialog: &mut PendingFileDialog,
    assets: &Assets<Image>,
) {
    if ui_state.show_save_menu {
        egui::Window::new("Save?")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("You have unsaved changes. Do you want to save?");
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        editor_state.action_after_save = Some(Box::new(|es| es.exit_now = true));
                        editor_state.save(pending_file_dialog, assets);
                        ui_state.show_save_menu = false;
                        *interaction_lock = InteractionLock::None;
                    };
                    if ui.button("Don't save").clicked() {
                        if let Some(action) = editor_state.action_after_save.take() {
                            action(editor_state);
                        }
                        ui_state.show_save_menu = false;
                        *interaction_lock = InteractionLock::None;
                    };
                    if ui.button("Cancel").clicked() {
                        ui_state.show_save_menu = false;
                        *interaction_lock = InteractionLock::None;
                    };
                });
            });
    }
}

fn toolbar(ui: &mut egui::Ui, editor_state: &mut EditorState) {
    ui.horizontal_centered(|ui| {
        let mut button = |tool: Tool, msg: &str| {
            if ui
                .add_enabled(editor_state.selected_tool != tool, egui::Button::new(msg))
                .clicked()
            {
                editor_state.selected_tool = tool;
            }
        };

        button(Tool::Select, "Select");
        button(Tool::MoveAnchor, "Move Anchor");
        button(Tool::MoveRootMotion, "Move Root Motion");
        button(Tool::CreateHitbox, "Create Hitbox");
        button(Tool::CreateHurtbox, "Create Hurtbox");
    });
}

fn timeline(editor_state: &mut EditorState, ui: &mut egui::Ui) {
    ui.group(|ui| {
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for i in 0..editor_state.current_animation.timeline.frames.len() {
                    if i != 0 {
                        ui.separator();
                    }

                    ui.allocate_ui_with_layout(
                        egui::Vec2::new(30.0, 100.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            if ui
                                .add_enabled(
                                    i != editor_state.current_frame,
                                    egui::Button::new(format!("{}", i + 1))
                                        .min_size(egui::Vec2::new(30.0, 0.0)),
                                )
                                .clicked()
                            {
                                editor_state.current_frame = i;
                            };

                            ui.label(format!(
                                "[{}]",
                                editor_state.current_animation.timeline.frames[i].delay
                            ));
                        },
                    );
                }
            });
        });
    });
}

#[derive(Resource, Default)]
pub struct UiState {
    pub(crate) show_save_menu: bool,
    frame_delay: Cached<usize>,
    frame_offset_x: Cached<f32>,
    frame_offset_y: Cached<f32>,
}

struct Cached<T> {
    cache: T,
    val: String,
}

impl<T: Default + ToString> Default for Cached<T> {
    fn default() -> Self {
        let cache = T::default();
        Self {
            val: cache.to_string(),
            cache,
        }
    }
}

impl<T: ToString + PartialEq + Clone> Cached<T> {
    fn update(&mut self, t: &T) {
        if self.cache != *t {
            self.cache = t.clone();
            self.val = t.to_string();
        }
    }
}

fn update_ui_state(editor_state: Res<EditorState>, mut ui_state: ResMut<UiState>) {
    if let Some(frame) = editor_state.get_frame(editor_state.current_frame) {
        ui_state.frame_delay.update(&frame.delay);
        ui_state.frame_offset_x.update(&frame.offset.x);
        ui_state.frame_offset_y.update(&frame.offset.y);
    }
}

fn cached_property_textbox<T: ToString + FromStr>(
    ui: &mut egui::Ui,
    property: &mut Cached<T>,
    action: impl FnOnce(&T, T),
) {
    if ui
        .add(
            egui::TextEdit::singleline(&mut property.val).min_size(egui::Vec2::new(50.0, 0.0)), // .desired_width(50.0),
        )
        .lost_focus()
    {
        if let Ok(new_val) = property.val.parse::<T>() {
            action(&property.cache, new_val);
        }
    };
}

fn frame_info(editor_state: &mut EditorState, ui_state: &mut UiState, ui: &mut egui::Ui) {
    let current_frame = editor_state.current_frame;
    if editor_state.get_frame(current_frame).is_none() {
        return;
    }

    egui::Grid::new("frame_info").num_columns(2).show(ui, |ui| {
        ui.label("Frame number");
        ui.label((current_frame + 1).to_string());
        ui.end_row();

        ui.label("Duration");
        cached_property_textbox(ui, &mut ui_state.frame_delay, |old_delay, new_delay| {
            editor_state.do_action(Action::ChangeDelay {
                index: current_frame,
                from: *old_delay,
                to: new_delay,
            });
        });
        ui.end_row();

        ui.label("Offset");

        egui::Grid::new("offset_grid")
            .num_columns(2)
            .min_col_width(0.0)
            .show(ui, |ui| {
                ui.label("X:");
                cached_property_textbox(ui, &mut ui_state.frame_offset_x, |_, new_x| {
                    let cur_offset = editor_state.frame(current_frame).offset;
                    editor_state.do_action(Action::MoveSprite {
                        frame_index: current_frame,
                        from: cur_offset,
                        to: Vec2::new(new_x, cur_offset.y),
                    });
                });
                ui.end_row();

                ui.label("Y:");
                cached_property_textbox(ui, &mut ui_state.frame_offset_y, |_, new_y| {
                    let cur_offset = editor_state.frame(current_frame).offset;
                    editor_state.do_action(Action::MoveSprite {
                        frame_index: current_frame,
                        from: cur_offset,
                        to: Vec2::new(cur_offset.x, new_y),
                    });
                });
                ui.end_row();
            });
        ui.end_row();

        ui.label("Root motion");

        egui::Grid::new("root_motion_grid")
            .num_columns(2)
            .min_col_width(0.0)
            .show(ui, |ui| {
                ui.label("X:");
                cached_property_textbox(ui, &mut ui_state.frame_offset_x, |_, new_x| {
                    let cur_offset = editor_state.frame(current_frame).offset;
                    editor_state.do_action(Action::MoveSprite {
                        frame_index: current_frame,
                        from: cur_offset,
                        to: Vec2::new(new_x, cur_offset.y),
                    });
                });
                ui.end_row();

                ui.label("Y:");
                cached_property_textbox(ui, &mut ui_state.frame_offset_y, |_, new_y| {
                    let cur_offset = editor_state.frame(current_frame).offset;
                    editor_state.do_action(Action::MoveSprite {
                        frame_index: current_frame,
                        from: cur_offset,
                        to: Vec2::new(cur_offset.x, new_y),
                    });
                });
                ui.end_row();
            });
        ui.end_row();

        ui.add_enabled_ui(current_frame > 0, |ui| {
            if ui.button("Move frame left").clicked() {
                let action = Action::SwapFrames {
                    a: current_frame,
                    b: current_frame - 1,
                };
                editor_state.do_action(action);
                editor_state.current_frame -= 1;
            };
        });
        ui.add_enabled_ui(
            current_frame + 1 < editor_state.current_animation.timeline.frames.len(),
            |ui| {
                if ui.button("Move frame right").clicked() {
                    let action = Action::SwapFrames {
                        a: current_frame,
                        b: current_frame + 1,
                    };
                    editor_state.do_action(action);
                    editor_state.current_frame += 1;
                };
            },
        );
        ui.end_row();
    });
}
