use adw::prelude::*;
use gtk::{gdk, glib};

use gstreamer as gst;
use gst::prelude::*;

mod config;

fn main() -> glib::ExitCode {
    // initialize gstreamer
    gst::init().expect("Failed to initialize GStreamer");

    // Load secret config values (url with username/password)
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

    // Move cameras into the activate handler
    app.connect_activate(move |app| build_ui(app, cameras.clone()));

    app.run()
}

fn build_ui(app: &adw::Application, cameras: Vec<(String, String)>) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .camera-button {
            background-color: rgba(20, 20, 20, 0.9);
            color: #f7f7f7;
            min-height: 64px;
            min-width: 140px;
            padding: 10px 18px;
            font-size: 18px;
            border-radius: 16px;
            border: 1px solid rgba(255, 255, 255, 0.15);
        }
        .camera-button:checked {
            background-color: rgba(35, 35, 35, 0.95);
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("No display available"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let picture = gtk::Picture::new();
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_can_shrink(true);

    let (first_name, first_camera_url) = cameras
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

    let (playbin, paintable) = build_player(&first_camera_url);
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

        let playbin_for_button = playbin.clone();
        let overlay_label_for_button = overlay_label.clone();
        let name = name.clone();
        let url = url.clone();
        button.connect_toggled(move |btn| {
            if btn.is_active() {
                overlay_label_for_button.set_text(&format!("Loading {name}..."));
                overlay_label_for_button.set_visible(true);
                let _ = playbin_for_button.set_state(gst::State::Null);
                playbin_for_button.set_property("uri", &url);
                let _ = playbin_for_button.set_state(gst::State::Playing);
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

    let bus = playbin
        .bus()
        .expect("Playbin has no bus; cannot watch state");
    let playbin_obj = playbin.clone().upcast::<gst::Object>();
    let overlay_label_for_bus = overlay_label.clone();
    let bus_watch = bus
        .add_watch_local(move |_, msg| {
            if let gst::MessageView::StateChanged(state) = msg.view() {
                if msg
                    .src()
                    .as_ref()
                    .map(|src| src.as_ptr() == playbin_obj.as_ptr())
                    .unwrap_or(false)
                    && state.current() == gst::State::Playing
                {
                    overlay_label_for_bus.set_visible(false);
                }
            }
            glib::ControlFlow::Continue
        })
        .expect("Failed to add bus watch");
    // Keep bus watch alive for the window lifetime.
    unsafe {
        window.set_data("bus-watch", bus_watch);
    }

    playbin
        .set_state(gst::State::Playing)
        .expect("Failed to set Playing");

    let playbin_for_close = playbin.clone();
    window.connect_close_request(move |_| {
        let _ = playbin_for_close.set_state(gst::State::Null);
        glib::Propagation::Proceed
    });
    window.present();
}

fn build_player(uri: &str) -> (gst::Element, gdk::Paintable) {
    let sink = gst::ElementFactory::make("gtk4paintablesink")
        .build()
        .expect("Missing gtk4paintablesink. Install 'gst-plugin-gtk4' and verify with gst-inspect-1.0 gtk4paintablesink");

    let playbin = gst::ElementFactory::make("playbin3")
        .build()
        .or_else(|_| gst::ElementFactory::make("playbin").build())
        .expect("Missing playbin/playbin3 element");

    playbin.set_property("uri", uri);
    playbin.set_property("video-sink", &sink);

    let paintable = sink.property::<gdk::Paintable>("paintable");
    (playbin, paintable)
}
