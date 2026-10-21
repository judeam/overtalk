#![deny(clippy::undocumented_unsafe_blocks)]
mod autostart;
mod controller;
mod drafts;
mod engine;
mod motion;
pub mod notes;
mod perf;
mod playback;
mod preferences;
mod timeline;
mod transliteration;
mod waveform;
slint::include_modules!();
use slint::ComponentHandle;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_with_options(|_| {}, true)
}

/// Use the same application setup with a custom platform (for visual regression capture).
pub fn run_with_setup(setup: impl FnOnce(&AppWindow)) -> Result<(), Box<dyn std::error::Error>> {
    run_with_options(setup, false)
}

fn run_with_options(
    setup: impl FnOnce(&AppWindow),
    native_tray: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let _lock = preferences::lock().map_err(std::io::Error::other)?;
    let ui = AppWindow::new()?;
    let widget = if native_tray {
        Some(MeetingWidget::new()?)
    } else {
        None
    };
    if let Some(widget) = &widget {
        let main = ui.as_weak();
        widget.on_action(move |action| {
            if let Some(ui) = main.upgrade() {
                let state = ui.global::<State>();
                match action.as_str() {
                    "record" => {
                        state.set_recording_title(state.get_widget_title());
                        state.invoke_action("start-recording".into(), "".into());
                    }
                    "stop" => state.invoke_action("stop-recording".into(), "".into()),
                    "join" => state.invoke_action("hub-widget-join".into(), "".into()),
                    _ => {
                        let _ = ui.show();
                    }
                }
            }
        });
        let main = ui.as_weak();
        widget.window().on_close_requested(move || {
            if let Some(ui) = main.upgrade() {
                ui.global::<State>().set_widget_visible(false);
            }
            slint::CloseRequestResponse::HideWindow
        });
        let weak = widget.as_weak();
        let main = ui.as_weak();
        ui.global::<State>().on_widget_updated(move || {
            if let (Some(widget), Some(ui)) = (weak.upgrade(), main.upgrade()) {
                let state = ui.global::<State>();
                widget.set_meeting_title(state.get_widget_title());
                widget.set_description(state.get_widget_description());
                widget.set_recording(state.get_recording());
                widget.set_can_join(!state.get_widget_url().is_empty());
                widget.global::<Theme>().set_dark(state.get_dark());
                if state.get_widget_visible() {
                    let _ = widget.show();
                } else {
                    let _ = widget.hide();
                }
            }
        });
    }
    ui.global::<State>()
        .on_matches_setting(|label, query| label.to_lowercase().contains(&query.to_lowercase()));
    ui.global::<State>().on_note_group_contains(|members, id| {
        !id.is_empty()
            && serde_json::from_str::<Vec<String>>(&members)
                .is_ok_and(|members| members.iter().any(|member| member == id.as_str()))
    });
    perf::install(&ui);
    let preferences = preferences::load();
    ui.global::<State>().set_guide_read(
        std::rc::Rc::new(slint::VecModel::from(
            preferences
                .guide_read
                .iter()
                .map(|id| id.as_str().into())
                .collect::<Vec<slint::SharedString>>(),
        ))
        .into(),
    );
    ui.global::<State>().set_dark(preferences.dark);
    ui.global::<State>()
        .set_zoom(preferences.zoom.clamp(0.8, 1.5));
    ui.global::<State>().set_timeline_open(preferences.timeline);
    ui.global::<State>().set_notes_timeline_fraction(
        if preferences.notes_timeline_fraction == 0.0 {
            0.0
        } else {
            preferences.notes_timeline_fraction.clamp(0.14, 0.45)
        },
    );
    ui.global::<State>()
        .set_notes_highlights_open(preferences.notes_highlights_open);
    ui.global::<State>()
        .set_latin_devanagari(preferences.latin_devanagari);
    ui.global::<State>()
        .set_reading_size(preferences.reading_size.clamp(0, 2));
    ui.global::<State>()
        .set_motion_mode(preferences.motion_mode.clamp(0, 2));
    ui.global::<State>()
        .set_reduced_motion(motion::reduced(preferences.motion_mode, false));
    ui.global::<State>()
        .set_close_to_tray(preferences.close_to_tray);
    ui.global::<State>()
        .set_auto_summary(preferences.auto_summary);
    ui.global::<State>()
        .set_setup_audio_confirmed(preferences.setup_audio_confirmed);
    ui.global::<State>()
        .set_default_summary_language(preferences.default_summary_language.into());
    ui.global::<State>()
        .set_summary_template_id(preferences.summary_template_id.clone().into());
    let preference_writer = std::rc::Rc::new(preferences::Writer::new());
    let writer = preference_writer.clone();
    let weak_preferences = ui.as_weak();
    ui.global::<State>().on_preferences_changed(move || {
        if let Some(ui) = weak_preferences.upgrade() {
            writer.enqueue(preferences::snapshot(&ui));
        }
    });
    let tray = if native_tray {
        AppTray::new().ok()
    } else {
        None
    };
    let motion_ui = ui.as_weak();
    std::thread::spawn(move || {
        let system = motion::system_reduced();
        let _ = motion_ui.upgrade_in_event_loop(move |ui| {
            let state = ui.global::<State>();
            state.set_system_reduced_motion(system);
            state.set_reduced_motion(motion::reduced(state.get_motion_mode(), system));
        });
    });
    let weak = ui.as_weak();
    let engine = engine::Engine::start(&engine::service_path(), move |message| {
        let _ = weak.upgrade_in_event_loop(move |ui| {
            controller::with_controller(|controller| match message {
                overtalk_protocol::Message::Ready { .. } => {
                    ui.global::<State>().set_engine_ready(true);
                    controller.initialize(&ui);
                }
                overtalk_protocol::Message::Event { event, payload } => {
                    controller.event(&ui, &event, payload)
                }
                _ => {}
            });
        });
    })
    .map_err(std::io::Error::other)?;
    controller::install(controller::Controller::new(
        engine.clone(),
        preferences.summary_template_id,
    ));
    if let Some(tray) = &tray {
        let weak = ui.as_weak();
        tray.on_open_requested(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.show();
            }
        });
        let weak = ui.as_weak();
        tray.on_record_requested(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.show();
                ui.global::<State>().invoke_action("home".into(), "".into());
                if !ui.global::<State>().get_recording() {
                    ui.global::<State>()
                        .invoke_action("start-recording".into(), "".into());
                }
            }
        });
        let weak = ui.as_weak();
        tray.on_pause_requested(move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<State>()
                    .invoke_action("pause-recording".into(), "".into());
            }
        });
        let weak = ui.as_weak();
        tray.on_quit_requested(move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<State>().invoke_action("quit".into(), "".into());
            }
        });
        let weak = ui.as_weak();
        let weak_tray = tray.as_weak();
        ui.global::<State>().on_recording_state_changed(move || {
            if let (Some(ui), Some(tray)) = (weak.upgrade(), weak_tray.upgrade()) {
                tray.set_recording(ui.global::<State>().get_recording());
                tray.set_paused(ui.global::<State>().get_recording_paused());
            }
        });
    }
    let weak = ui.as_weak();
    let trace_actions = std::env::var_os("OVERTALK_TRACE_RENDER").is_some();
    ui.global::<State>().on_action(move |action, value| {
        if let Some(ui) = weak.upgrade() {
            controller::with_controller(|controller| controller.action(&ui, &action, &value));
            if trace_actions {
                eprintln!(
                    "NATIVE_ACTION action={action} view={}",
                    ui.global::<State>().get_view()
                );
            }
        }
    });
    let weak = ui.as_weak();
    ui.global::<State>().on_ui_style_changed(move || {
        let _ =
            weak.upgrade_in_event_loop(|ui| controller::with_controller(|c| c.refresh_style(&ui)));
    });
    let weak = ui.as_weak();
    ui.global::<State>().on_notes_edited(move |text| {
        if let Some(ui) = weak.upgrade() {
            controller::with_controller(|controller| controller.notes_edited(&ui, text.into()));
        }
    });
    let weak = ui.as_weak();
    ui.global::<State>().on_form_edited(move |key, value| {
        if let Some(ui) = weak.upgrade() {
            controller::with_controller(|controller| controller.form_edited(&ui, &key, &value));
        }
    });
    let weak = ui.as_weak();
    let has_tray = tray.is_some();
    ui.window().on_close_requested(move || {
        let mut close = true;
        if let Some(ui) = weak.upgrade() {
            if has_tray && ui.global::<State>().get_close_to_tray() {
                return slint::CloseRequestResponse::HideWindow;
            }
            controller::with_controller(|c| close = c.request_close(&ui));
        }
        if close {
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        } else {
            slint::CloseRequestResponse::KeepWindowShown
        }
    });
    setup(&ui);
    if has_tray {
        ui.show()?;
        slint::run_event_loop_until_quit()?;
    } else {
        ui.run()?;
    }
    preference_writer.enqueue(preferences::snapshot(&ui));
    ui.global::<State>().on_preferences_changed(|| {});
    drop(preference_writer);
    engine.shutdown();
    Ok(())
}
