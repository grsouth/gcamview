use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

use gstreamer as gst;
use gst::prelude::*;
use gst::bus::BusWatchGuard;

mod config;

fn main() -> glib::ExitCode {
    gst::init().expect("Failed to initialize GStreamer");

    let config = config::load_config();

    let mut keys: Vec<&String> = config.camera_urls.keys().collect();
    keys.sort();

    let cameras: Vec<(String, String)> = keys
        .into_iter()
        .map(|key| {
            let url = config
                .camera_urls
                .get(key)
                .expect("Camera key missing unexpectedly")
                .clone();
            (key.clone(), url)
        })
        .collect();

    if cameras.is_empty() {
        panic!("No cameras defined in config.toml");
    }

    let app = adw::Application::builder()
        .application_id("com.garrett.gcamview")
        .build();

    app.connect_activate(move |app| build_ui(app, cameras.clone()));

    app.run()
}

fn build_ui(app: &adw::Application, cameras: Vec<(String, String)>) {
    const CSS: &str = r#"
    .camera-button {
        background-color: rgba(20, 20, 20, 0.9);
        color: #f7f7f7;
        min-height: 52px;
        min-width: 120px;
        padding: 8px 14px;
        font-size: 16px;
        border-radius: 14px;
        border: 1px solid rgba(255, 255, 255, 0.15);
    }
    .camera-button:checked {
        background-color: rgba(35, 35, 35, 0.95);
    }
    "#;

    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let picture = gtk::Picture::new();
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_can_shrink(true);

    let (first_name, first_url) = cameras
        .first()
        .expect("No cameras defined in config.toml")
        .clone();

    let overlay_label = gtk::Label::new(Some(&format!("Loading {first_name}...")));
    overlay_label.set_margin_top(12);
    overlay_label.set_margin_start(12);
    overlay_label.set_halign(gtk::Align::Start);
    overlay_label.set_valign(gtk::Align::Start);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&picture));
    overlay.add_overlay(&overlay_label);

    let (pipeline, paintable) = build_pipeline(&first_url);
    let pipeline = Rc::new(RefCell::new(pipeline));
    let bus_watch_guard: Rc<RefCell<Option<BusWatchGuard>>> = Rc::new(RefCell::new(None));
    picture.set_paintable(Some(&paintable));

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.set_halign(gtk::Align::Center);
    controls.set_valign(gtk::Align::Start);
    controls.set_margin_top(12);
    controls.set_margin_start(12);
    controls.set_margin_end(12);

    let mut first_button: Option<gtk::ToggleButton> = None;
    for (name, url) in cameras.iter() {
        let button = gtk::ToggleButton::with_label(name);
        button.add_css_class("camera-button");

        if let Some(existing) = &first_button {
            button.set_group(Some(existing));
        } else {
            button.set_active(true);
            first_button = Some(button.clone());
        }

        let pipeline_for_button = pipeline.clone();
        let bus_watch_for_button = bus_watch_guard.clone();
        let picture_for_button = picture.clone();
        let overlay_label_for_button = overlay_label.clone();
        let name = name.clone();
        let url = url.clone();

        button.connect_toggled(move |btn| {
            if btn.is_active() {
                overlay_label_for_button.set_text(&format!("Loading {name}..."));
                overlay_label_for_button.set_visible(true);

                let _ = bus_watch_for_button.borrow_mut().take();

                {
                    let mut current = pipeline_for_button.borrow_mut();
                    let _ = current.set_state(gst::State::Null);
                }

                let (new_pipeline, new_paintable) = build_pipeline(&url);
                picture_for_button.set_paintable(Some(&new_paintable));

                {
                    let mut current = pipeline_for_button.borrow_mut();
                    *current = new_pipeline;
                }

                let guard = attach_bus_watch(
                    &pipeline_for_button.borrow(),
                    &overlay_label_for_button,
                );
                *bus_watch_for_button.borrow_mut() = Some(guard);

                pipeline_for_button
                    .borrow()
                    .set_state(gst::State::Playing)
                    .expect("Failed to set Playing");
            }
        });

        controls.append(&button);
    }

    overlay.add_overlay(&controls);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("gcamview")
        .content(&overlay)
        .build();
    window.fullscreen();

    // esc to quit
    install_quit_shortcut(app, &window);

    let guard = attach_bus_watch(&pipeline.borrow(), &overlay_label);
    *bus_watch_guard.borrow_mut() = Some(guard);

    pipeline
        .borrow()
        .set_state(gst::State::Playing)
        .expect("Failed to set Playing");

    let pipeline_for_close = pipeline.clone();
    window.connect_close_request(move |_| {
        let _ = pipeline_for_close.borrow().set_state(gst::State::Null);
        glib::Propagation::Proceed
    });

    window.present();
}

fn build_pipeline(uri: &str) -> (gst::Pipeline, gdk::Paintable) {
    let safe_uri = uri.replace('"', "%22");

    let desc = format!(
        r#"
        rtspsrc name=src location="{safe_uri}" protocols=tcp latency=100 drop-on-latency=true
            src. ! application/x-rtp,media=video !
                queue ! decodebin ! videoconvert !
                gtk4paintablesink name=sink sync=false
            src. ! application/x-rtp,media=audio !
                queue ! decodebin ! audioconvert ! audioresample !
                autoaudiosink
        "#
    );

    let element = gst::parse::launch(&desc).expect("Failed to parse GStreamer pipeline");
    let pipeline = element
        .downcast::<gst::Pipeline>()
        .expect("parse_launch did not return a gst::Pipeline");

    let sink = pipeline
        .by_name("sink")
        .expect("Pipeline missing element named 'sink' (gtk4paintablesink)");

    let paintable = sink.property::<gdk::Paintable>("paintable");
    (pipeline, paintable)
}

fn attach_bus_watch(
    pipeline: &gst::Pipeline,
    overlay_label: &gtk::Label,
) -> BusWatchGuard {
    let bus = pipeline
        .bus()
        .expect("Pipeline has no bus; cannot watch state");
    let overlay_label = overlay_label.clone();
    bus.add_watch_local(move |_, msg| {
        if let gst::MessageView::StateChanged(state) = msg.view() {
            if state.current() == gst::State::Playing {
                overlay_label.set_visible(false);
            }
        }
        glib::ControlFlow::Continue
    })
    .expect("Failed to add GStreamer bus watch")
}

fn install_quit_shortcut(app: &adw::Application, window: &adw::ApplicationWindow) {
    let quit_action = gio::SimpleAction::new("quit", None);
    let app_clone = app.clone();
    quit_action.connect_activate(move |_, _| {
        app_clone.quit();
    });
    app.add_action(&quit_action);

    let controller = gtk::ShortcutController::new();
    let trigger = gtk::KeyvalTrigger::new(gdk::Key::Escape, gdk::ModifierType::empty());
    let action = gtk::NamedAction::new("app.quit");
    let shortcut = gtk::Shortcut::new(Some(trigger), Some(action));
    controller.add_shortcut(shortcut);
    window.add_controller(controller);
}
